//! The keyset-pagination cursor codec shared by every store backend.
//!
//! This lives here, rather than in a backend, because **all backends must produce
//! interchangeable cursors**. A cursor minted by the sqlite store has to decode in the
//! postgres store and vice versa; two private copies of one wire format is how they
//! silently stop agreeing — which is exactly what #187 found in `acdp-registry-sqlite`
//! and `acdp-registry-pg`, byte-for-byte duplicated, and what this module replaces.
//!
//! # Wire format
//!
//! `STANDARD_BASE64("{mint_ms}:{anchor_ms}:{ctx_id}")`, where:
//!
//! - `mint_ms` — when the cursor was issued (Unix ms). Drives expiry, and is the *only*
//!   thing expiry looks at, so a cursor cannot be refreshed by replaying it.
//! - `anchor_ms` — the keyset position: the `created_at` of the last row on the page.
//! - `ctx_id` — the tiebreaker for rows sharing an `anchor_ms`. Split with `splitn(3, ':')`
//!   so a `ctx_id` containing `:` (every `acdp://` URI does) survives intact.
//!
//! Cursors are **unsigned plaintext**, and deliberately not a confidentiality boundary.
//! RFC-ACDP-0005 §2.5.4's requirement is that a cursor carry no *client-decodable
//! visibility information*, which this format satisfies by carrying no visibility
//! information at all: visibility is recomputed per page from the current requester, never
//! remembered in the cursor.
//!
//! # What the anchor can name, and what it cannot
//!
//! This module used to claim outright that everything inside a cursor is "either a
//! timestamp or an identifier the requester was already shown". **That is not true in
//! general, and the exception is the whole reason this section exists.** The anchor is the
//! last row the SQL scan *touched*, not the last row the caller was *served* — a
//! distinction `acdp::pagination` makes on purpose, so that a page whose rows are all
//! removed by a post-SQL filter cannot halt pagination early. So whether the claim holds
//! depends entirely on whether the filter that matters ran **in the scan** or **after it**:
//!
//! - **§4.5 disclosure (visibility): in SQL, on both backends.** The scan never touches a
//!   restricted or private row the requester may not see, so the anchor cannot name one.
//!   The original claim holds for this dimension.
//! - **Tenancy: in SQL only for callers of
//!   [`ExtendedRegistryStore::search_in_tenant`](crate::ExtendedRegistryStore::search_in_tenant).**
//!   For those callers the anchor is necessarily one of the caller's own rows. For callers
//!   of the protocol-level `RegistryStore::search` — which carries no tenancy, and which
//!   the HTTP search handler still uses today — a tenant filter applied to the *result set*
//!   leaves the anchor free to name a foreign tenant's row, disclosing its
//!   `(created_at, ctx_id)`. That is an ordering and existence oracle, walkable one page at
//!   a time, and it is tracked as SECURITY follow-up #14.
//!
//! The honest summary: a cursor discloses nothing beyond what the *scan that produced it*
//! was allowed to see. Narrowing a result set after the scan does not narrow the cursor,
//! so any future filter that must not leak positions belongs in the query, not in Rust.
//!
//! Found by lane-1 while working on a neighbouring unit, and flagged rather than quietly
//! reworded here.
//!
//! # Expiry
//!
//! Cursors live `CURSOR_TTL_SECS` (1 hour), matching RFC-ACDP-0005 §2.5.4's "MUST remain
//! valid across a single iteration session of at most 1 hour". Past that,
//! [`decode_cursor`] returns `AcdpError::CursorExpired` — distinct from the malformed case,
//! which returns `AcdpError::InvalidCursor`. Keeping those two apart is a spec requirement
//! and is covered by the `cur` conformance family; do not collapse them.

use acdp::error::AcdpError;
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use chrono::{DateTime, Utc};

/// How long an issued cursor stays usable, per RFC-ACDP-0005 §2.5.4.
const CURSOR_TTL_SECS: i64 = 3600;

/// The single payload every parse failure carries.
///
/// `AcdpError::InvalidCursor` renders as `#[error("invalid cursor: {0}")]`, so the wire
/// message is exactly **`invalid cursor: malformed`** — it names no parse step. That is the
/// point: `cur-002`'s fixture rationale asks a registry not to "leak why a cursor failed to
/// parse beyond the registered code", and the previous per-arm strings named the cursor's
/// internal field layout — which field was absent, which one failed to parse as an integer. Nothing is lost operationally — `error.code` still discriminates
/// `invalid_cursor` from `cursor_expired`, which is the distinction callers actually branch
/// on, and a client already knows the cursor it sent.
const CURSOR_MALFORMED: &str = "malformed";

/// Mint a cursor for the last row of a page.
///
/// `created_at_ms` is that row's `created_at`; the mint stamp is taken from the clock here,
/// so every issued cursor starts a fresh TTL window.
pub fn encode_cursor(created_at_ms: i64, ctx_id: &str) -> String {
    let mint_ms = Utc::now().timestamp_millis();
    B64.encode(format!("{mint_ms}:{created_at_ms}:{ctx_id}"))
}

/// Decode a cursor into its `(anchor, ctx_id)` keyset position.
///
/// Returns `AcdpError::CursorExpired` for a well-formed cursor past its TTL, and
/// `AcdpError::InvalidCursor` for anything unparseable. The two are separate wire codes
/// (`cursor_expired` / `invalid_cursor`, both HTTP 400) and callers rely on the distinction.
///
/// The `Option` in the return type is vestigial: on the success path this **always** yields
/// `Ok(Some(..))`, never `Ok(None)`. It survives because both callers pass an
/// `Option<&str>` through `.map(decode_cursor).transpose()?.flatten()`, and flattening one
/// level there is cheaper than a wrapper. Do not write code that branches on `Ok(None)`
/// expecting it to mean anything.
pub fn decode_cursor(s: &str) -> Result<Option<(DateTime<Utc>, String)>, AcdpError> {
    let malformed = || AcdpError::InvalidCursor(CURSOR_MALFORMED.into());

    let bytes = B64.decode(s).map_err(|_| malformed())?;
    let decoded = String::from_utf8(bytes).map_err(|_| malformed())?;
    let mut parts = decoded.splitn(3, ':');
    // `splitn` always yields at least one element -- even for the empty string -- so the
    // first `next()` cannot be `None`. It previously carried its own missing-mint arm, which
    // was therefore unreachable; an empty first field falls through to the parse below
    // instead. `unwrap_or("")` states that rather than pretending the branch exists.
    let mint = parts.next().unwrap_or("");
    let anchor = parts.next().ok_or_else(malformed)?;
    let ctx_id = parts.next().ok_or_else(malformed)?;
    let mint_ms: i64 = mint.parse().map_err(|_| malformed())?;
    let anchor_ms: i64 = anchor.parse().map_err(|_| malformed())?;
    let now = Utc::now().timestamp_millis();
    if now.saturating_sub(mint_ms) > CURSOR_TTL_SECS * 1000 {
        return Err(AcdpError::CursorExpired);
    }
    let anchor_ts = DateTime::<Utc>::from_timestamp_millis(anchor_ms).ok_or_else(malformed)?;
    Ok(Some((anchor_ts, ctx_id.to_string())))
}

#[cfg(test)]
mod tests {
    use super::{decode_cursor, encode_cursor, B64, CURSOR_TTL_SECS};
    use acdp::error::AcdpError;
    use base64::Engine as _;
    use chrono::Utc;

    #[test]
    fn cursor_round_trips_anchor_and_ctx_id() {
        let anchor_ms = 1_700_000_123_456_i64;
        let cur = encode_cursor(anchor_ms, "acdp://reg/ctx-1");
        let (ts, ctx_id) = decode_cursor(&cur)
            .expect("decode ok")
            .expect("cursor present");
        assert_eq!(ts.timestamp_millis(), anchor_ms);
        assert_eq!(ctx_id, "acdp://reg/ctx-1");
    }

    /// A `ctx_id` containing `:` must survive the round trip — every `acdp://` URI has
    /// several, so a `split(':')` here instead of `splitn(3, ':')` would truncate real ids.
    #[test]
    fn cursor_preserves_colons_in_ctx_id() {
        let anchor_ms = 1_700_000_123_456_i64;
        let ctx = "acdp://registry.example.com/12345678-1234-4321-8123-123456781234";
        let cur = encode_cursor(anchor_ms, ctx);
        let (_, decoded) = decode_cursor(&cur)
            .expect("decode ok")
            .expect("cursor present");
        assert_eq!(decoded, ctx);
    }

    #[test]
    fn cursor_rejects_malformed_input() {
        // Not base64.
        assert!(matches!(
            decode_cursor("!!!not-base64!!!"),
            Err(AcdpError::InvalidCursor(_))
        ));
        // Valid base64 but missing the ctx_id field.
        let truncated = B64.encode("123:456");
        assert!(matches!(
            decode_cursor(&truncated),
            Err(AcdpError::InvalidCursor(_))
        ));
    }

    /// The remaining parse-failure arms.
    ///
    /// Deliberately one test rather than five: they share setup and a failure in any arm is
    /// equally a bug. Note the consequence, though — this is fail-fast, so a break in the
    /// first arm masks the other four until it is fixed.
    ///
    /// The pre-existing coverage reached only two of them (not-base64, missing ctx_id) while
    /// the codec has seven live arms. Asserting the **variant** rather than the message text
    /// is deliberate: the payload is an implementation detail that has already changed once,
    /// and a test pinned to it would have to be rewritten every time it does.
    #[test]
    fn cursor_rejects_each_parse_failure_arm() {
        // Valid base64, invalid UTF-8.
        let not_utf8 = B64.encode([0xff, 0xfe, 0xfd]);
        assert!(
            matches!(decode_cursor(&not_utf8), Err(AcdpError::InvalidCursor(_))),
            "non-utf8 payload must be rejected"
        );

        // No separator at all -> the anchor field is absent.
        let no_sep = B64.encode("abc");
        assert!(
            matches!(decode_cursor(&no_sep), Err(AcdpError::InvalidCursor(_))),
            "a payload with no ':' has no anchor and must be rejected"
        );

        // Non-integer mint.
        let bad_mint = B64.encode("notanint:456:acdp://reg/ctx-1");
        assert!(
            matches!(decode_cursor(&bad_mint), Err(AcdpError::InvalidCursor(_))),
            "a non-integer mint must be rejected"
        );

        // Non-integer anchor.
        let now_ms = Utc::now().timestamp_millis();
        let bad_anchor = B64.encode(format!("{now_ms}:notanint:acdp://reg/ctx-1"));
        assert!(
            matches!(decode_cursor(&bad_anchor), Err(AcdpError::InvalidCursor(_))),
            "a non-integer anchor must be rejected"
        );

        // Anchor integer that is not a representable timestamp.
        let out_of_range = B64.encode(format!("{now_ms}:{}:acdp://reg/ctx-1", i64::MAX));
        assert!(
            matches!(
                decode_cursor(&out_of_range),
                Err(AcdpError::InvalidCursor(_))
            ),
            "an anchor outside the representable range must be rejected"
        );
    }

    /// Pins the TTL **duration**, which nothing pinned before.
    ///
    /// The Phase 1 verification round found that `CURSOR_TTL_SECS: 3600 -> 60`, and dropping
    /// the `* 1000` so the window becomes 3.6 *seconds*, both left every test green: the suite
    /// proved "expired is rejected" and "just-minted is not" with a ~3600x gap between them,
    /// and every test imports the constant from `super::` so encode and decode move together.
    /// The module doc now publishes "1 hour" as a contract, so that gap became load-bearing.
    /// Bracketing the boundary to within ten seconds closes it.
    #[test]
    fn cursor_ttl_is_one_hour_of_milliseconds() {
        assert_eq!(
            CURSOR_TTL_SECS, 3600,
            "the module doc publishes a one-hour TTL as a contract (RFC-ACDP-0005 §2.5.4)"
        );
        let now_ms = Utc::now().timestamp_millis();
        let inside = B64.encode(format!("{}:{now_ms}:acdp://reg/ctx-1", now_ms - 3_595_000));
        assert!(
            decode_cursor(&inside).is_ok(),
            "a cursor minted 3595s ago is still inside the 1h window -- if this fails the TTL \
             is too short, e.g. the `* 1000` was dropped and the window is 3.6 seconds"
        );
        let outside = B64.encode(format!("{}:{now_ms}:acdp://reg/ctx-1", now_ms - 3_605_000));
        assert!(
            matches!(decode_cursor(&outside), Err(AcdpError::CursorExpired)),
            "a cursor minted 3605s ago is outside the 1h window -- if this fails the TTL is \
             too long"
        );
    }

    /// Pins the base64 **alphabet**, which nothing pinned before.
    ///
    /// Interchangeability across backends is the whole reason this module exists, and the
    /// alphabet is part of that contract — but every other test reaches `B64` through
    /// `super::`, so swapping the engine moves encode and decode together and stays invisible.
    /// The Phase 1 verification round confirmed it: `STANDARD -> URL_SAFE_NO_PAD` left all
    /// tests green. This one decodes with an engine named right here instead.
    #[test]
    fn cursor_uses_the_standard_base64_alphabet() {
        use base64::engine::general_purpose::STANDARD as PINNED_STANDARD;
        let ctx = "acdp://reg/ctx-1";
        let cur = encode_cursor(1_700_000_123_456_i64, ctx);
        let raw = PINNED_STANDARD
            .decode(&cur)
            .expect("cursors must be STANDARD base64 so another backend can decode them");
        let text = String::from_utf8(raw).expect("payload is utf-8");
        assert!(
            text.ends_with(":1700000123456:acdp://reg/ctx-1"),
            "decoded under an explicitly-STANDARD engine, got {text:?}"
        );
        assert!(
            !cur.contains('-') && !cur.contains('_'),
            "'-' and '_' belong to the URL-safe alphabet, not STANDARD: {cur:?}"
        );
    }

    /// The wire message must name no parse step.
    ///
    /// Asserts the exact rendered string rather than a `contains`, because the whole point is
    /// what is *absent*: a `contains("invalid cursor")` would still pass if a parse reason
    /// were appended. `AcdpError::InvalidCursor` renders as `invalid cursor: {payload}`, so a
    /// bare payload is not achievable and `"malformed"` is the chosen filler.
    #[test]
    fn malformed_cursor_message_names_no_parse_step() {
        // The out-of-range-anchor arm needs a FRESH mint: `decode_cursor` checks the TTL
        // before it converts the anchor, so a stale mint short-circuits to `CursorExpired`
        // and never reaches the arm under test. Observed, not assumed -- a hardcoded 2023
        // mint failed here with `got Err(CursorExpired)`.
        let fresh = Utc::now().timestamp_millis();
        for bad in [
            "!!!not-base64!!!",
            &B64.encode([0xff, 0xfe, 0xfd]),
            &B64.encode("abc"),
            &B64.encode("123:456"),
            &B64.encode("notanint:456:acdp://reg/ctx-1"),
            &B64.encode(format!("{fresh}:notanint:acdp://reg/ctx-1")),
            &B64.encode(format!("{fresh}:{}:acdp://reg/ctx-1", i64::MAX)),
        ] {
            match decode_cursor(bad) {
                Err(AcdpError::InvalidCursor(payload)) => {
                    assert_eq!(payload, "malformed", "payload must be the single constant");
                    let rendered = AcdpError::InvalidCursor(payload).to_string();
                    assert_eq!(rendered, "invalid cursor: malformed");
                }
                other => panic!("expected InvalidCursor for {bad:?}, got {other:?}"),
            }
        }
    }

    /// Every `InvalidCursor` this module can construct must carry `CURSOR_MALFORMED`.
    ///
    /// The by-example test above enumerates the seven arms that exist *today*, so it cannot
    /// see a NEW arm added later. A verification pass demonstrated the hole concretely: an
    /// eighth arm added at the top of `decode_cursor` --
    /// `if s.len() > 512 { return Err(AcdpError::InvalidCursor("cursor too long".into())) }`
    /// -- leaks a parse step while every by-example input stays short and BOTH the unit
    /// suite and the conformance suite stay green.
    ///
    /// So this checks the property structurally instead of by example: it reads this file's
    /// own source and requires every `AcdpError::InvalidCursor(` construction outside the
    /// test module to be `CURSOR_MALFORMED`. Writing the new arm the idiomatic way (reusing
    /// the local `malformed` closure) satisfies it; spelling out a bespoke string does not.
    #[test]
    fn every_invalid_cursor_construction_uses_the_shared_payload() {
        const SRC: &str = include_str!("cursor.rs");
        const NEEDLE: &str = "AcdpError::InvalidCursor(";

        // Scan production code only. The test module legitimately names the variant in
        // match patterns and in one `to_string()` round-trip.
        let prod = SRC
            .split_once("\n#[cfg(test)]")
            .expect("cursor.rs must contain a `#[cfg(test)]` module for this test to scope itself")
            .0;

        let mut found = 0;
        for (i, _) in prod.match_indices(NEEDLE) {
            let arg = &prod[i + NEEDLE.len()..];
            assert!(
                arg.starts_with("CURSOR_MALFORMED"),
                "cursor.rs constructs an InvalidCursor with something other than \
                 CURSOR_MALFORMED, which leaks a parse step onto the wire (see #187). \
                 Offending construction begins: {:?}",
                &arg[..arg.len().min(60)]
            );
            found += 1;
        }

        // Anti-vacuity: if the needle stopped matching -- renamed variant, reformatted call
        // -- the loop above would pass while checking nothing at all.
        assert_eq!(
            found, 1,
            "expected exactly one InvalidCursor construction in production code (the \
             `malformed` closure); found {found}. If an arm was legitimately added, it \
             should reuse that closure rather than construct the error itself."
        );
    }

    #[test]
    fn expired_cursor_is_rejected() {
        // Craft a cursor minted just past the TTL window. Note this hand-mints the plaintext
        // rather than calling `encode_cursor`, which always stamps `now` — backdating the
        // mint is the only way to exercise the real clock comparison without mocking it.
        let now_ms = Utc::now().timestamp_millis();
        let stale_mint = now_ms - (CURSOR_TTL_SECS * 1000 + 5_000);
        let raw = B64.encode(format!("{stale_mint}:{now_ms}:acdp://reg/ctx-1"));
        assert!(
            matches!(decode_cursor(&raw), Err(AcdpError::CursorExpired)),
            "a cursor older than the TTL must be rejected so stale pages can't be replayed"
        );
    }

    /// Expiry and malformedness are separate wire codes (`cursor_expired` vs
    /// `invalid_cursor`, RFC-ACDP-0005 §2.5.4) and the `cur` conformance family asserts the
    /// distinction. A refactor that collapsed them would still pass every test above.
    #[test]
    fn expired_and_malformed_are_distinct_variants() {
        let now_ms = Utc::now().timestamp_millis();
        let stale = B64.encode(format!(
            "{}:{now_ms}:acdp://reg/ctx-1",
            now_ms - (CURSOR_TTL_SECS * 1000 + 5_000)
        ));
        assert!(matches!(
            decode_cursor(&stale),
            Err(AcdpError::CursorExpired)
        ));
        assert!(matches!(
            decode_cursor("!!!not-base64!!!"),
            Err(AcdpError::InvalidCursor(_))
        ));
    }

    /// A cursor minted inside the window must still decode — otherwise
    /// `expired_cursor_is_rejected` would pass against a codec that rejects everything.
    #[test]
    fn fresh_cursor_is_not_expired() {
        let cur = encode_cursor(1_700_000_123_456_i64, "acdp://reg/ctx-1");
        assert!(
            decode_cursor(&cur).is_ok(),
            "a just-minted cursor must decode"
        );
    }
}
