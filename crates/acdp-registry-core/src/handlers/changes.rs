//! The change feed: `GET /changes` (RFC-ACDP-0018 §5, ACDP 0.5.0).
//!
//! Always mounted. A registry without `changes.enabled` (no cursor key in
//! [`AppState::changes`]) or on a backend without a feed answers
//! `not_implemented` (501), as the profile registry requires of
//! registries that do not advertise `acdp-registry-changes`.
//!
//! Error semantics (§5.1, §5.4):
//! - too many filter values, an unknown or empty `kind`, a malformed id,
//!   a repeated `since` / `limit` → `schema_violation` (400);
//! - a `limit` that is not a positive integer → `schema_violation` with
//!   `details.field = "limit"`; one above `limits.max_changes_page` is
//!   clamped to it (§5.1 only requires accepting values up to the max;
//!   `next_cursor` / `has_more` carry the rest, as on search);
//! - a cursor that does not decode, or whose tag fails under a known key
//!   → `invalid_cursor` (400);
//! - a cursor under an unknown key, above the head, below the oldest
//!   retained entry, or issued before a sequence reset → `cursor_expired`
//!   (400). Every cursor carries the head it was issued at and that row's
//!   fingerprint ([`CursorAnchor`]); a restore that rewinds the feed
//!   leaves that anchor above the head or on a different row, so **every**
//!   cursor that could skip entries expires, including one whose position
//!   the restored feed has since reached again (§5.3, rec-005 F). A cursor
//!   issued before the backup point still names rows the restore put back,
//!   so the restore runbook also raises `[changes] cursor_epoch`, which
//!   expires every cursor of the old epoch. A version-1 cursor, which has
//!   no anchor, expires the same way. The
//!   consumer recovers exactly as from a pruned cursor (§7: re-verify,
//!   resume from `since=head`).
//!
//! Visibility (§5.5) is a post-filter over the store's matching rows, one
//! batched `visible_ctx_ids` per scanned page. A hidden entry never ends a
//! page early (§5.2): the handler keeps scanning up to
//! [`REFILL_MAX_SCANS`] pages, and when the budget runs out it returns the
//! position it scanned through with `has_more: true`. Cursors name feed
//! positions, never a requester's view, so they leak nothing (§5.4).

use std::collections::HashSet;
use std::sync::Arc;

use acdp::error::AcdpError;
use acdp::types::change::{ChangeKind, ChangesResponse, WatchFilter, MAX_CURSOR_LEN};
use acdp::types::primitives::{AgentDid, CtxId, LineageId};
use acdp_registry_store::changes::{FeedFingerprint, FeedHead, FeedQuery, FeedRow, FEED_ORIGIN_FP};
use acdp_registry_store::ExtendedRegistryStore;
use acdp_registry_types::RegistryError;
use axum::extract::{RawQuery, State};
use axum::http::HeaderMap;
use axum::Json;

use super::context::tenant_for_principal;
use crate::changes_key::{ChangesKey, CursorAnchor, CursorError, DecodedCursor};
use crate::read_gate::ReadPrincipal;
use crate::state::AppState;
use crate::tenant_trust::PeerIp;

/// Store pages one request may scan while refilling past hidden entries.
pub const REFILL_MAX_SCANS: usize = 6;

/// The smallest store page a scan asks for, so a small `limit` behind many
/// hidden rows does not cost one round trip per visible entry. A test may
/// lower it with
/// [`AppStateInner::with_changes_min_scan_rows`](crate::state::AppStateInner::with_changes_min_scan_rows)
/// to reach the scan budget with a dozen rows instead of hundreds.
pub const MIN_SCAN_ROWS: u32 = 100;

/// The `limit` used when the request has none (§5.1).
const DEFAULT_LIMIT: u64 = 100;

/// Where a request starts reading (§5.1 `since`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Since {
    /// No `since`: the oldest retained entry.
    Oldest,
    /// `since=head`: no entries, the head cursor.
    Head,
    /// A cursor the request presented: checked against the feed (§5.3).
    Cursor(DecodedCursor),
    /// A position the registry itself recorded (the hint worker's
    /// progress), read as-is.
    Position(u64),
}

/// The anchor for a cursor issued while the feed head was `seq`, whose
/// fingerprint is `fp` ([`FeedHead::head_fp`]).
///
/// With no fingerprint (retention pruned through the head before migration
/// 020 recorded one) the cursor anchors on position 0, which every feed
/// holds: it is still served, without reset detection (only the restore
/// runbook's epoch covers it), until the next append gives the head a
/// fingerprint.
pub fn anchor_at(seq: u64, fp: Option<FeedFingerprint>) -> CursorAnchor {
    match fp {
        Some(fp) => CursorAnchor { seq, fp },
        None => CursorAnchor {
            seq: 0,
            fp: FEED_ORIGIN_FP,
        },
    }
}

/// The cursor at the feed head `head`: what `since=head` answers, and what
/// a change hint carries.
pub fn head_cursor(key: &ChangesKey, head: &FeedHead) -> String {
    key.encode_cursor(head.head, anchor_at(head.head, head.head_fp))
}

/// Whether `anchor` still names the row it was issued on, given the feed
/// markers `start`. An anchor above the head, on a pruned row other than
/// the marker, or on a row with another fingerprint was issued before a
/// sequence reset (§5.3) or cannot be placed (§5.4).
async fn anchor_holds<S: ExtendedRegistryStore + ?Sized>(
    store: &S,
    start: &FeedHead,
    anchor: CursorAnchor,
) -> Result<bool, AcdpError> {
    if anchor.seq > start.head {
        return Ok(false);
    }
    let stored = if anchor.seq == 0 {
        Some(FEED_ORIGIN_FP)
    } else if anchor.seq == start.head {
        start.head_fp
    } else {
        // One snapshot for the row and the markers, so a prune between
        // the two reads cannot make a live anchor look missing.
        let page = store
            .changes_page(&FeedQuery {
                after: anchor.seq - 1,
                scan_limit: 1,
                ..FeedQuery::default()
            })
            .await?;
        if anchor.seq == page.pruned_through {
            page.pruned_through_fp
        } else if anchor.seq < page.pruned_through {
            None
        } else {
            page.rows
                .first()
                .filter(|r| r.seq == anchor.seq)
                .map(FeedRow::fingerprint)
        }
    };
    Ok(stored == Some(anchor.fp))
}

/// A parsed `GET /changes` request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangesRequest {
    pub since: Since,
    pub limit: u64,
    pub filter: WatchFilter,
}

fn schema(msg: impl Into<String>) -> RegistryError {
    RegistryError::Acdp(AcdpError::SchemaViolation(msg.into()))
}

fn limit_violation(max: u64, raw: &str) -> RegistryError {
    RegistryError::FieldViolation {
        field: "limit",
        message: format!(
            "limit must be a positive integer, at most limits.max_changes_page {max} \
             is served (got '{raw}') (RFC-ACDP-0018 §5.1)"
        ),
    }
}

/// Parse the raw query string. Unknown parameters are ignored, as on
/// search. `since` and `limit` may appear at most once; the filter members
/// repeat (§5.1).
pub fn parse_request(
    raw: Option<&str>,
    key: &ChangesKey,
    max_change_filters: u64,
    max_changes_page: u64,
) -> Result<ChangesRequest, RegistryError> {
    let mut since: Option<String> = None;
    let mut limit: Option<String> = None;
    let mut filter = WatchFilter::new();
    for (k, v) in url::form_urlencoded::parse(raw.unwrap_or("").as_bytes()) {
        match k.as_ref() {
            "since" | "limit" => {
                let slot = if k == "since" { &mut since } else { &mut limit };
                if slot.replace(v.into_owned()).is_some() {
                    return Err(schema(format!(
                        "query parameter '{k}' given more than once"
                    )));
                }
            }
            "ctx_id" => filter.ctx_id.push(CtxId::parse(v.into_owned())?),
            "lineage_id" => filter.lineage_id.push(LineageId::parse(v.into_owned())?),
            "agent_id" => filter.agent_id.push(AgentDid::parse(v.into_owned())?),
            "kind" => filter.kind.push(ChangeKind::parse(&v)?),
            _ => {}
        }
    }
    filter.validate(Some(max_change_filters))?;
    let limit = match limit {
        None => DEFAULT_LIMIT.min(max_changes_page),
        Some(raw) => match raw.parse::<u64>() {
            Ok(n) if n >= 1 => n.min(max_changes_page),
            Err(e) if *e.kind() == std::num::IntErrorKind::PosOverflow => max_changes_page,
            _ => return Err(limit_violation(max_changes_page, &raw)),
        },
    };
    let since = match since.as_deref() {
        None => Since::Oldest,
        Some("head") => Since::Head,
        Some(c) if c.is_empty() || c.chars().count() > MAX_CURSOR_LEN => {
            return Err(RegistryError::Acdp(AcdpError::InvalidCursor(
                "since is not a cursor this registry issued".into(),
            )))
        }
        Some(c) => match key.decode_cursor(c) {
            Ok(cursor) => Since::Cursor(cursor),
            Err(CursorError::Invalid) => {
                return Err(RegistryError::Acdp(AcdpError::InvalidCursor(
                    "since is not a cursor this registry issued".into(),
                )))
            }
            Err(CursorError::UnknownKey | CursorError::Stale) => {
                return Err(RegistryError::Acdp(AcdpError::CursorExpired))
            }
        },
    };
    Ok(ChangesRequest {
        since,
        limit,
        filter,
    })
}

/// The id a row's visibility turns on (§5.5): the affected context for
/// `lifecycle`, the new version for `superseded`, none for
/// `key_revocation` (revocation contexts are public).
fn visibility_subject(row: &FeedRow) -> Option<&str> {
    match row.kind {
        ChangeKind::Lifecycle => Some(&row.ctx_id),
        ChangeKind::Superseded => Some(row.superseded_by.as_deref().unwrap_or("")),
        ChangeKind::KeyRevocation => None,
    }
}

/// Build the `/changes` response for `req` as seen by `requester` /
/// `tenant`. Also returns the number of feed rows scanned (for metrics).
/// Shared with the change-hint worker, which must hint only entries the
/// subscription owner may see.
pub async fn evaluate_changes<S: ExtendedRegistryStore + 'static>(
    state: &AppState<S>,
    key: &ChangesKey,
    requester: Option<&AgentDid>,
    tenant: Option<&str>,
    req: &ChangesRequest,
) -> Result<(ChangesResponse, u64), RegistryError> {
    let store = state.server.store();
    let public_arm_open = state.server.capabilities().anonymous_public_reads;
    let start = store.changes_head().await?;
    let mut pos = match req.since {
        Since::Head => {
            let resp = ChangesResponse {
                changes: Vec::new(),
                next_cursor: head_cursor(key, &start),
                has_more: false,
            };
            return Ok((resp, 0));
        }
        Since::Oldest => start.pruned_through,
        Since::Position(p) | Since::Cursor(DecodedCursor { pos: p, .. })
            if p > start.head || p < start.pruned_through =>
        {
            return Err(RegistryError::Acdp(AcdpError::CursorExpired));
        }
        Since::Cursor(c) => {
            if !anchor_holds(store, &start, c.anchor).await? {
                return Err(RegistryError::Acdp(AcdpError::CursorExpired));
            }
            c.pos
        }
        Since::Position(p) => p,
    };

    // §6: a `ctx_id` watch also matches its producer's key revocations —
    // resolved only for watched contexts the requester may see, so a
    // hidden context's producer is never disclosed through the feed.
    let watched: Vec<&str> = req.filter.ctx_id.iter().map(CtxId::as_str).collect();
    let mut revocation_agents = Vec::new();
    if !watched.is_empty() {
        let visible = store
            .visible_ctx_ids(&watched, requester, tenant, public_arm_open)
            .await?;
        let visible: Vec<&str> = watched
            .iter()
            .copied()
            .filter(|c| visible.contains(*c))
            .collect();
        if !visible.is_empty() {
            let mut agents: Vec<String> =
                store.producers_of(&visible).await?.into_values().collect();
            agents.sort();
            agents.dedup();
            revocation_agents = agents;
        }
    }
    let base = FeedQuery {
        after: pos,
        ctx_ids: watched.iter().map(|s| (*s).to_string()).collect(),
        lineage_ids: req
            .filter
            .lineage_id
            .iter()
            .map(|l| l.as_str().to_string())
            .collect(),
        agent_ids: req
            .filter
            .agent_id
            .iter()
            .map(|a| a.as_str().to_string())
            .collect(),
        revocation_agents,
        kinds: req.filter.kind.clone(),
        scan_limit: u32::try_from(req.limit)
            .unwrap_or(u32::MAX)
            .max(state.changes_min_scan_rows),
    };

    let target = usize::try_from(req.limit).unwrap_or(usize::MAX);
    let check_expiry = matches!(req.since, Since::Cursor(_) | Since::Position(_));
    let mut changes = Vec::new();
    let mut head = start.head;
    let mut head_fp = start.head_fp;
    let mut scanned = 0u64;
    'scan: for _ in 0..REFILL_MAX_SCANS {
        let page = store
            .changes_page(&FeedQuery {
                after: pos,
                ..base.clone()
            })
            .await?;
        if page.head >= head {
            (head, head_fp) = (page.head, page.head_fp);
        }
        // A prune that overtook the read position deleted rows this
        // cursor had not yet been served (§5.4). Without a cursor the
        // request asked only for the oldest retained entry onward.
        if check_expiry && page.pruned_through > pos {
            return Err(RegistryError::Acdp(AcdpError::CursorExpired));
        }
        scanned += page.rows.len() as u64;
        let subjects: Vec<&str> = page
            .rows
            .iter()
            .filter_map(visibility_subject)
            .filter(|s| !s.is_empty())
            .collect();
        let visible: HashSet<String> = if subjects.is_empty() {
            HashSet::new()
        } else {
            store
                .visible_ctx_ids(&subjects, requester, tenant, public_arm_open)
                .await?
        };
        for row in &page.rows {
            if changes.len() == target {
                break 'scan;
            }
            if visibility_subject(row).is_none_or(|id| visible.contains(id)) {
                changes.push(row.to_change()?);
            }
            pos = row.seq;
        }
        pos = pos.max(page.last_scanned);
        if page.reached_head || changes.len() == target {
            break;
        }
    }

    let resp = ChangesResponse {
        changes,
        next_cursor: key.encode_cursor(pos, anchor_at(head, head_fp)),
        has_more: pos < head,
    };
    resp.validate().map_err(|e| {
        RegistryError::Internal(format!("changes response failed its own schema: {e}"))
    })?;
    Ok((resp, scanned))
}

/// `GET /changes` (RFC-ACDP-0018 §5).
pub async fn changes<S: ExtendedRegistryStore + 'static>(
    State(state): State<Arc<AppState<S>>>,
    headers: HeaderMap,
    PeerIp(peer): PeerIp,
    principal: ReadPrincipal,
    RawQuery(raw): RawQuery,
) -> Result<Json<ChangesResponse>, RegistryError> {
    let result = serve(&state, &headers, peer, &principal, raw.as_deref()).await;
    match &result {
        Ok((_, scanned)) => crate::metrics::record_changes_request("ok", *scanned),
        Err(e) => crate::metrics::record_changes_request(e.wire_code(), 0),
    }
    result.map(|(resp, _)| Json(resp))
}

async fn serve<S: ExtendedRegistryStore + 'static>(
    state: &Arc<AppState<S>>,
    headers: &HeaderMap,
    peer: Option<std::net::IpAddr>,
    principal: &ReadPrincipal,
    raw: Option<&str>,
) -> Result<(ChangesResponse, u64), RegistryError> {
    let key = state.changes.as_ref().ok_or_else(|| {
        RegistryError::Acdp(AcdpError::NotImplemented(
            "this registry does not advertise acdp-registry-changes \
             (RFC-ACDP-0018: GET /changes is not implemented)"
                .into(),
        ))
    })?;
    let cfg = &state.config.changes;
    let req = parse_request(raw, key, cfg.max_change_filters, cfg.max_changes_page)?;
    let requester = principal.requester();
    // As on search: with the public arm closed an anonymous reader has no
    // visible entries but key revocations, and is refused rather than
    // served a feed that discloses only its head position.
    if requester.is_none() && !state.server.capabilities().anonymous_public_reads {
        return Err(RegistryError::Acdp(AcdpError::NotAuthorized(
            "anonymous /changes requires authentication \
             (registry caps: anonymous_public_reads=false)"
                .into(),
        )));
    }
    let tenant = tenant_for_principal(state, headers, peer, principal)?;
    evaluate_changes(state, key, requester.as_ref(), tenant.as_deref(), &req).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> ChangesKey {
        ChangesKey::from_master(&[7u8; 32], 1).unwrap()
    }

    fn parse(q: &str) -> Result<ChangesRequest, RegistryError> {
        parse_request(Some(q), &key(), 100, 100)
    }

    fn code(r: Result<ChangesRequest, RegistryError>) -> &'static str {
        r.expect_err("must be refused").wire_code()
    }

    #[test]
    fn defaults_are_oldest_and_100() {
        let r = parse_request(None, &key(), 100, 100).unwrap();
        assert_eq!(r.since, Since::Oldest);
        assert_eq!(r.limit, 100);
        assert!(r.filter.is_empty());
    }

    #[test]
    fn since_head_and_cursor_round_trip() {
        assert_eq!(parse("since=head").unwrap().since, Since::Head);
        let anchor = CursorAnchor {
            seq: 50,
            fp: [3; 8],
        };
        let c = key().encode_cursor(42, anchor);
        assert_eq!(
            parse(&format!("since={c}")).unwrap().since,
            Since::Cursor(DecodedCursor { pos: 42, anchor })
        );
    }

    #[test]
    fn an_anchor_without_a_fingerprint_falls_back_to_the_origin() {
        assert_eq!(
            anchor_at(7, Some([1; 8])),
            CursorAnchor { seq: 7, fp: [1; 8] }
        );
        assert_eq!(
            anchor_at(7, None),
            CursorAnchor {
                seq: 0,
                fp: FEED_ORIGIN_FP
            }
        );
    }

    #[test]
    fn cursor_errors_split_invalid_from_expired() {
        assert_eq!(code(parse("since=not-a-cursor")), "invalid_cursor");
        assert_eq!(code(parse("since=")), "invalid_cursor");
        let long = "a".repeat(MAX_CURSOR_LEN + 1);
        assert_eq!(code(parse(&format!("since={long}"))), "invalid_cursor");
        let other = ChangesKey::from_master(&[7u8; 32], 2)
            .unwrap()
            .encode_cursor(5, anchor_at(5, Some([0; 8])));
        assert_eq!(code(parse(&format!("since={other}"))), "cursor_expired");
        // Issued before the restore runbook raised `cursor_epoch`.
        let old_epoch = key()
            .with_epoch(3)
            .encode_cursor(5, anchor_at(5, Some([0; 8])));
        assert_eq!(code(parse(&format!("since={old_epoch}"))), "cursor_expired");
    }

    #[test]
    fn limit_bounds_name_the_field() {
        for bad in ["0", "-1", "1.5", "x", "", "+-1"] {
            let e = parse(&format!("limit={bad}")).expect_err(bad);
            assert_eq!(e.wire_code(), "schema_violation", "{bad}");
            assert!(
                matches!(e, RegistryError::FieldViolation { field: "limit", .. }),
                "{bad}"
            );
        }
        assert_eq!(parse("limit=1").unwrap().limit, 1);
        assert_eq!(parse("limit=100").unwrap().limit, 100);
        // Above the page maximum (100 in these tests): clamped, not refused.
        assert_eq!(parse("limit=101").unwrap().limit, 100);
        assert_eq!(parse("limit=99999999999999999999999").unwrap().limit, 100);
    }

    #[test]
    fn kinds_repeat_and_unknown_kinds_are_refused() {
        let r = parse("kind=lifecycle&kind=lifecycle&kind=key_revocation").unwrap();
        assert_eq!(r.filter.kind.len(), 3);
        assert_eq!(code(parse("kind=retraction")), "schema_violation");
        assert_eq!(code(parse("kind=")), "schema_violation");
    }

    #[test]
    fn filter_value_count_is_bounded() {
        let q = (0..101)
            .map(|_| "kind=lifecycle")
            .collect::<Vec<_>>()
            .join("&");
        assert_eq!(code(parse(&q)), "schema_violation");
        let q = (0..100)
            .map(|_| "kind=lifecycle")
            .collect::<Vec<_>>()
            .join("&");
        assert!(parse(&q).is_ok());
    }

    #[test]
    fn malformed_ids_and_repeated_singletons_are_refused() {
        assert_eq!(code(parse("ctx_id=nope")), "schema_violation");
        assert_eq!(code(parse("agent_id=nope")), "schema_violation");
        assert_eq!(code(parse("lineage_id=nope")), "schema_violation");
        assert_eq!(code(parse("since=head&since=head")), "schema_violation");
        assert_eq!(code(parse("limit=1&limit=2")), "schema_violation");
    }

    #[test]
    fn unknown_parameters_are_ignored() {
        assert!(parse("cursor=x&q=y").is_ok());
    }
}
