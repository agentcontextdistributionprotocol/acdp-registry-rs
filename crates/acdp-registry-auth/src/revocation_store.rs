//! JWT revocation list — keyed by the `jti` minted on token issuance.
//!
//! Without this layer a bearer token issued to a compromised DID key lives
//! until its `exp` (default 1 hour) and the only recourse is rotating the
//! JWT secret, which invalidates every other live token. The trait is
//! consulted on every `JwtSigner::validate` call. Backends ship for the
//! same three flavors as [`crate::ChallengeStore`].

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::AuthError;

/// The instant at which a revocation tombstone stops being needed.
///
/// **A tombstone must outlive the validator's acceptance window, not merely the
/// token's `exp`.** [`crate::JwtSigner::validate`] applies `leeway_seconds` of
/// clock-skew tolerance, so a token is still decoded and accepted until
/// `exp + leeway`. Comparing a tombstone against a bare `now` therefore retires
/// it `leeway` seconds too early, and in the window `(exp, exp + leeway]` a
/// **revoked token is accepted again after its own expiry** — revocation
/// un-revokes.
///
/// Both halves of the lifecycle must use this: the check
/// ([`RevocationStore::is_revoked`]) and the eviction
/// ([`RevocationStore::evict_expired`]). Widening only the check leaves eviction
/// deleting the row at `expires_at`, which defeats the fix through the other
/// door while a check-only test still passes.
pub fn tombstone_cutoff(now: DateTime<Utc>, leeway_seconds: u64) -> DateTime<Utc> {
    now - chrono::Duration::seconds(leeway_seconds as i64)
}

/// Tombstone record. The signer rejects any presented token whose `jti`
/// has a row here (revoked = true) and whose `expires_at` has not yet
/// elapsed (expired tokens are harmless and don't need to live forever).
#[derive(Debug, Clone)]
pub struct RevocationRecord {
    pub jti: String,
    pub agent_did: String,
    pub expires_at: DateTime<Utc>,
}

/// Persistent revocation index.
///
/// Two writes happen per token:
/// - [`Self::record_issued`] inserts a row with `revoked = false` when a
///   JWT is minted. Without this the revocation endpoint cannot look up
///   `agent_did` ownership and every revocation attempt fails with
///   "no record for jti" (the bug fixed in commit-after-798cb34).
/// - [`Self::revoke`] flips the row to `revoked = true`. Subsequent
///   `is_revoked` checks observe the flip and `JwtSigner::validate`
///   rejects the token.
#[async_trait]
pub trait RevocationStore: Send + Sync {
    /// Persist a freshly-minted JWT. Backends MUST set `revoked = false`
    /// for the inserted row and MUST NOT downgrade an existing
    /// `revoked = true` row to `revoked = false` if the same `jti` is
    /// re-inserted (which would let a revoked token come back to life
    /// via a contrived re-issuance race).
    async fn record_issued(&self, record: RevocationRecord) -> Result<(), AuthError>;

    /// Mark `jti` as revoked. Idempotent — calling twice is harmless.
    async fn revoke(&self, record: RevocationRecord) -> Result<(), AuthError>;

    /// Synchronous reachability check used inside `JwtSigner::validate`
    /// (which itself is sync). DB-backed implementations bridge via
    /// `block_in_place + Handle::block_on(...)`, matching the storage
    /// layer pattern elsewhere in this workspace.
    ///
    /// `cutoff` is the instant returned by [`tombstone_cutoff`] — `now` less the
    /// validator's leeway, NOT a bare `now`. A tombstone counts as live while
    /// `expires_at > cutoff`, so it outlives every token the validator would
    /// still accept. Passing a bare `now` here reopens the window this parameter
    /// exists to close.
    fn is_revoked(&self, jti: &str, cutoff: DateTime<Utc>) -> Result<bool, AuthError>;

    /// Whether a stored revocation belongs to `agent_did`. Used by the
    /// revocation endpoint to forbid cross-agent revocations.
    async fn owner_of(&self, jti: &str) -> Result<Option<String>, AuthError>;

    /// Drop tombstones that are no longer needed. Bounded background task —
    /// keeps the table from growing forever.
    ///
    /// `cutoff` MUST be [`tombstone_cutoff`]'s value, not a bare `now`: a row is
    /// still needed while the validator would still accept a token bearing that
    /// `jti`, which is `leeway` seconds past `expires_at`. This is the eviction
    /// half of the pair described on [`tombstone_cutoff`]; getting it wrong
    /// deletes the evidence that [`Self::is_revoked`] is about to look for.
    async fn evict_expired(&self, cutoff: DateTime<Utc>) -> Result<(), AuthError>;

    /// Read the persisted poll cursor for a federated `issuer`.
    ///
    /// `None` means the registry has never polled this peer (or the
    /// row was never written) — the poller should start at 0. The
    /// in-memory implementation always returns `None`, matching the
    /// pre-§5 "refetch from 0 on restart" behavior.
    ///
    /// Plan §5.
    async fn get_revocation_cursor(&self, issuer: &str) -> Result<Option<i64>, AuthError>;

    /// Persist the poll cursor for a federated `issuer`. Idempotent —
    /// callers write the cursor only after the corresponding batch
    /// has been applied locally, so a partial-batch failure leaves
    /// the prior cursor in place and the entries retry next interval.
    ///
    /// Plan §5.
    async fn set_revocation_cursor(&self, issuer: &str, cursor_ms: i64) -> Result<(), AuthError>;
}

// ── In-memory ────────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct InMemoryRevocationStore {
    inner: Mutex<HashMap<String, InMemoryEntry>>,
    /// Per-issuer revocation-feed cursor cache (plan §5). In-memory
    /// only — disappears on restart, matching the pre-§5 behavior. Use
    /// the SQLite or Postgres backends in production.
    cursors: Mutex<HashMap<String, i64>>,
}

impl InMemoryRevocationStore {
    pub fn new() -> Self {
        Self::default()
    }
}

// `revoked` is tracked alongside the record so the in-memory store
// matches the DB schema semantics (`record_issued` inserts unrevoked,
// `revoke` flips the flag).
struct InMemoryEntry {
    record: RevocationRecord,
    revoked: bool,
}

#[async_trait]
impl RevocationStore for InMemoryRevocationStore {
    async fn record_issued(&self, record: RevocationRecord) -> Result<(), AuthError> {
        let mut g = self
            .inner
            .lock()
            .map_err(|_| AuthError::Internal("lock poisoned".into()))?;
        // Do NOT downgrade `revoked = true` rows. A future `issue_token`
        // call that happens to draw the same UUIDv4 (effectively never)
        // or any operator-triggered double-issue must not resurrect a
        // tombstoned token.
        let entry = g.entry(record.jti.clone()).or_insert(InMemoryEntry {
            record: record.clone(),
            revoked: false,
        });
        if !entry.revoked {
            entry.record = record;
        }
        Ok(())
    }

    async fn revoke(&self, record: RevocationRecord) -> Result<(), AuthError> {
        let mut g = self
            .inner
            .lock()
            .map_err(|_| AuthError::Internal("lock poisoned".into()))?;
        g.entry(record.jti.clone())
            .and_modify(|e| e.revoked = true)
            .or_insert(InMemoryEntry {
                record,
                revoked: true,
            });
        Ok(())
    }

    fn is_revoked(&self, jti: &str, cutoff: DateTime<Utc>) -> Result<bool, AuthError> {
        let g = self
            .inner
            .lock()
            .map_err(|_| AuthError::Internal("lock poisoned".into()))?;
        Ok(g.get(jti)
            .is_some_and(|e| e.revoked && e.record.expires_at > cutoff))
    }

    async fn owner_of(&self, jti: &str) -> Result<Option<String>, AuthError> {
        let g = self
            .inner
            .lock()
            .map_err(|_| AuthError::Internal("lock poisoned".into()))?;
        Ok(g.get(jti).map(|e| e.record.agent_did.clone()))
    }

    async fn evict_expired(&self, cutoff: DateTime<Utc>) -> Result<(), AuthError> {
        let mut g = self
            .inner
            .lock()
            .map_err(|_| AuthError::Internal("lock poisoned".into()))?;
        g.retain(|_, e| e.record.expires_at > cutoff);
        Ok(())
    }

    async fn get_revocation_cursor(&self, issuer: &str) -> Result<Option<i64>, AuthError> {
        let g = self
            .cursors
            .lock()
            .map_err(|_| AuthError::Internal("cursor lock poisoned".into()))?;
        Ok(g.get(issuer).copied())
    }

    async fn set_revocation_cursor(&self, issuer: &str, cursor_ms: i64) -> Result<(), AuthError> {
        let mut g = self
            .cursors
            .lock()
            .map_err(|_| AuthError::Internal("cursor lock poisoned".into()))?;
        g.insert(issuer.to_string(), cursor_ms);
        Ok(())
    }
}

// ── SQLite ───────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct SqliteRevocationStore {
    pool: sqlx::SqlitePool,
}

impl SqliteRevocationStore {
    pub fn new(pool: sqlx::SqlitePool) -> Self {
        Self { pool }
    }

    fn block_on<F: std::future::Future<Output = T>, T>(&self, fut: F) -> T {
        tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(fut))
    }
}

#[async_trait]
impl RevocationStore for SqliteRevocationStore {
    async fn record_issued(&self, record: RevocationRecord) -> Result<(), AuthError> {
        // `DO NOTHING` rather than `DO UPDATE` preserves an existing
        // `revoked = 1` row — a duplicate jti (vanishingly unlikely with
        // UUIDv4 but possible under operator error) must not resurrect a
        // revoked token.
        sqlx::query(
            "INSERT INTO issued_tokens (jti, agent_did, expires_at, revoked) \
             VALUES (?, ?, ?, 0) \
             ON CONFLICT(jti) DO NOTHING",
        )
        .bind(&record.jti)
        .bind(&record.agent_did)
        .bind(record.expires_at.to_rfc3339())
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|e| AuthError::Storage(e.to_string()))
    }

    async fn revoke(&self, record: RevocationRecord) -> Result<(), AuthError> {
        sqlx::query(
            "INSERT INTO issued_tokens (jti, agent_did, expires_at, revoked) \
             VALUES (?, ?, ?, 1) \
             ON CONFLICT(jti) DO UPDATE SET revoked = 1",
        )
        .bind(&record.jti)
        .bind(&record.agent_did)
        .bind(record.expires_at.to_rfc3339())
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|e| AuthError::Storage(e.to_string()))
    }

    fn is_revoked(&self, jti: &str, cutoff: DateTime<Utc>) -> Result<bool, AuthError> {
        let jti = jti.to_string();
        self.block_on(async move {
            use sqlx::Row;
            let row = sqlx::query(
                "SELECT revoked, expires_at FROM issued_tokens WHERE jti = ? AND revoked = 1",
            )
            .bind(&jti)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| AuthError::Storage(e.to_string()))?;
            let Some(row) = row else {
                return Ok(false);
            };
            let exp: String = row
                .try_get("expires_at")
                .map_err(|e| AuthError::Storage(e.to_string()))?;
            let exp = chrono::DateTime::parse_from_rfc3339(&exp)
                .map_err(|e| AuthError::Storage(e.to_string()))?
                .with_timezone(&Utc);
            Ok(exp > cutoff)
        })
    }

    async fn owner_of(&self, jti: &str) -> Result<Option<String>, AuthError> {
        use sqlx::Row;
        let row = sqlx::query("SELECT agent_did FROM issued_tokens WHERE jti = ?")
            .bind(jti)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| AuthError::Storage(e.to_string()))?;
        Ok(row.and_then(|r| r.try_get::<String, _>("agent_did").ok()))
    }

    async fn evict_expired(&self, cutoff: DateTime<Utc>) -> Result<(), AuthError> {
        sqlx::query("DELETE FROM issued_tokens WHERE expires_at <= ?")
            .bind(cutoff.to_rfc3339())
            .execute(&self.pool)
            .await
            .map(|_| ())
            .map_err(|e| AuthError::Storage(e.to_string()))
    }

    async fn get_revocation_cursor(&self, issuer: &str) -> Result<Option<i64>, AuthError> {
        use sqlx::Row;
        let row =
            sqlx::query("SELECT cursor_ms FROM auth_revocation_poll_cursors WHERE issuer = ?")
                .bind(issuer)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| AuthError::Storage(e.to_string()))?;
        Ok(row.and_then(|r| r.try_get::<i64, _>("cursor_ms").ok()))
    }

    async fn set_revocation_cursor(&self, issuer: &str, cursor_ms: i64) -> Result<(), AuthError> {
        sqlx::query(
            "INSERT INTO auth_revocation_poll_cursors (issuer, cursor_ms, updated_at) \
             VALUES (?, ?, ?) \
             ON CONFLICT(issuer) DO UPDATE SET cursor_ms = excluded.cursor_ms, \
                                               updated_at = excluded.updated_at",
        )
        .bind(issuer)
        .bind(cursor_ms)
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|e| AuthError::Storage(e.to_string()))
    }
}

// ── Postgres ─────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct PgRevocationStore {
    pool: sqlx::PgPool,
}

impl PgRevocationStore {
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }

    fn block_on<F: std::future::Future<Output = T>, T>(&self, fut: F) -> T {
        tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(fut))
    }
}

#[async_trait]
impl RevocationStore for PgRevocationStore {
    async fn record_issued(&self, record: RevocationRecord) -> Result<(), AuthError> {
        sqlx::query(
            "INSERT INTO issued_tokens (jti, agent_did, expires_at, revoked) \
             VALUES ($1, $2, $3, false) \
             ON CONFLICT (jti) DO NOTHING",
        )
        .bind(&record.jti)
        .bind(&record.agent_did)
        .bind(record.expires_at)
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|e| AuthError::Storage(e.to_string()))
    }

    async fn revoke(&self, record: RevocationRecord) -> Result<(), AuthError> {
        sqlx::query(
            "INSERT INTO issued_tokens (jti, agent_did, expires_at, revoked) \
             VALUES ($1, $2, $3, true) \
             ON CONFLICT (jti) DO UPDATE SET revoked = true",
        )
        .bind(&record.jti)
        .bind(&record.agent_did)
        .bind(record.expires_at)
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|e| AuthError::Storage(e.to_string()))
    }

    fn is_revoked(&self, jti: &str, cutoff: DateTime<Utc>) -> Result<bool, AuthError> {
        let jti = jti.to_string();
        self.block_on(async move {
            use sqlx::Row;
            let row = sqlx::query(
                "SELECT expires_at FROM issued_tokens WHERE jti = $1 AND revoked = true",
            )
            .bind(&jti)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| AuthError::Storage(e.to_string()))?;
            let Some(row) = row else {
                return Ok(false);
            };
            let exp: DateTime<Utc> = row
                .try_get("expires_at")
                .map_err(|e| AuthError::Storage(e.to_string()))?;
            Ok(exp > cutoff)
        })
    }

    async fn owner_of(&self, jti: &str) -> Result<Option<String>, AuthError> {
        use sqlx::Row;
        let row = sqlx::query("SELECT agent_did FROM issued_tokens WHERE jti = $1")
            .bind(jti)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| AuthError::Storage(e.to_string()))?;
        Ok(row.and_then(|r| r.try_get::<String, _>("agent_did").ok()))
    }

    async fn evict_expired(&self, cutoff: DateTime<Utc>) -> Result<(), AuthError> {
        sqlx::query("DELETE FROM issued_tokens WHERE expires_at <= $1")
            .bind(cutoff)
            .execute(&self.pool)
            .await
            .map(|_| ())
            .map_err(|e| AuthError::Storage(e.to_string()))
    }

    async fn get_revocation_cursor(&self, issuer: &str) -> Result<Option<i64>, AuthError> {
        use sqlx::Row;
        let row =
            sqlx::query("SELECT cursor_ms FROM auth_revocation_poll_cursors WHERE issuer = $1")
                .bind(issuer)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| AuthError::Storage(e.to_string()))?;
        Ok(row.and_then(|r| r.try_get::<i64, _>("cursor_ms").ok()))
    }

    async fn set_revocation_cursor(&self, issuer: &str, cursor_ms: i64) -> Result<(), AuthError> {
        sqlx::query(
            "INSERT INTO auth_revocation_poll_cursors (issuer, cursor_ms, updated_at) \
             VALUES ($1, $2, $3) \
             ON CONFLICT (issuer) DO UPDATE SET cursor_ms = EXCLUDED.cursor_ms, \
                                                updated_at = EXCLUDED.updated_at",
        )
        .bind(issuer)
        .bind(cursor_ms)
        .bind(Utc::now())
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|e| AuthError::Storage(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(jti: &str) -> RevocationRecord {
        RevocationRecord {
            jti: jti.into(),
            agent_did: "did:web:agent.example".into(),
            expires_at: Utc::now() + chrono::Duration::seconds(60),
        }
    }

    #[tokio::test]
    async fn in_memory_record_issued_then_revoke_lifecycle() {
        let s = InMemoryRevocationStore::new();
        // 1. record_issued — owner_of returns Some, is_revoked is false.
        s.record_issued(rec("t1")).await.unwrap();
        assert_eq!(
            s.owner_of("t1").await.unwrap().as_deref(),
            Some("did:web:agent.example"),
            "the issuance must be observable by the revocation endpoint"
        );
        assert!(
            !s.is_revoked("t1", crate::tombstone_cutoff(Utc::now(), 0))
                .unwrap(),
            "fresh token is not revoked"
        );

        // 2. revoke — is_revoked flips to true. owner_of still works
        //    so a subsequent attempt to re-revoke is allowed.
        s.revoke(rec("t1")).await.unwrap();
        assert!(s
            .is_revoked("t1", crate::tombstone_cutoff(Utc::now(), 0))
            .unwrap());
        assert_eq!(
            s.owner_of("t1").await.unwrap().as_deref(),
            Some("did:web:agent.example")
        );

        // 3. record_issued on a revoked jti MUST NOT resurrect it —
        //    a hostile or buggy re-issuance with a colliding uuid must
        //    not make a tombstoned token usable again.
        s.record_issued(rec("t1")).await.unwrap();
        assert!(
            s.is_revoked("t1", crate::tombstone_cutoff(Utc::now(), 0))
                .unwrap(),
            "re-recording an issued jti must not clear the tombstone"
        );
    }

    #[tokio::test]
    async fn in_memory_owner_of_is_none_for_unknown_jti() {
        // The revocation endpoint relies on owner_of(None) to mean "no such
        // token" so an attacker can't probe for existence via the error shape.
        let s = InMemoryRevocationStore::new();
        assert!(s.owner_of("never-issued").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn in_memory_is_revoked_false_for_expired_tombstone() {
        // Past the CUTOFF a tombstone is genuinely spent and may go cold.
        // NOTE the justification, which used to read "validate already rejects
        // it on exp" and was false: with leeway, validate does NOT reject at
        // `exp` — it accepts until `exp + leeway`, which is the whole reason
        // `is_revoked` takes a cutoff instead of comparing against `now`. This
        // test passes a ZERO-leeway cutoff, so `cutoff == now` and the old
        // boundary is what is being asserted here.
        let s = InMemoryRevocationStore::new();
        s.revoke(RevocationRecord {
            jti: "expired".into(),
            agent_did: "did:web:agent.example".into(),
            expires_at: Utc::now() - chrono::Duration::seconds(1),
        })
        .await
        .unwrap();
        assert!(
            !s.is_revoked("expired", crate::tombstone_cutoff(Utc::now(), 0))
                .unwrap(),
            "expired tombstone must not count as an active revocation"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn sqlite_revoke_unknown_jti_creates_tombstone() {
        // Parity with the in-memory store: a direct revoke (no prior
        // record_issued) must still produce an active tombstone via the
        // INSERT … ON CONFLICT DO UPDATE path.
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::query(
            "CREATE TABLE issued_tokens (\
                jti TEXT PRIMARY KEY, \
                agent_did TEXT NOT NULL, \
                expires_at TEXT NOT NULL, \
                revoked INTEGER NOT NULL DEFAULT 0\
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        let s = SqliteRevocationStore::new(pool);
        assert!(
            !s.is_revoked("t9", crate::tombstone_cutoff(Utc::now(), 0))
                .unwrap(),
            "unknown jti is not revoked"
        );
        s.revoke(rec("t9")).await.unwrap();
        assert!(s
            .is_revoked("t9", crate::tombstone_cutoff(Utc::now(), 0))
            .unwrap());
        assert_eq!(
            s.owner_of("t9").await.unwrap().as_deref(),
            Some("did:web:agent.example")
        );
    }

    #[tokio::test]
    async fn in_memory_revoke_unknown_jti_creates_tombstone() {
        // Direct `revoke` (without prior record_issued) still tombstones.
        // The AuthService policy enforces caller==owner before calling this.
        let s = InMemoryRevocationStore::new();
        s.revoke(rec("t2")).await.unwrap();
        assert!(s
            .is_revoked("t2", crate::tombstone_cutoff(Utc::now(), 0))
            .unwrap());
    }

    #[tokio::test]
    async fn in_memory_cursor_round_trip() {
        let s = InMemoryRevocationStore::new();
        assert_eq!(s.get_revocation_cursor("cp.local").await.unwrap(), None);
        s.set_revocation_cursor("cp.local", 1_700_000_000_000)
            .await
            .unwrap();
        assert_eq!(
            s.get_revocation_cursor("cp.local").await.unwrap(),
            Some(1_700_000_000_000)
        );
        // Update for the same issuer overwrites.
        s.set_revocation_cursor("cp.local", 1_700_000_005_000)
            .await
            .unwrap();
        assert_eq!(
            s.get_revocation_cursor("cp.local").await.unwrap(),
            Some(1_700_000_005_000)
        );
        // A different issuer has its own cell.
        assert_eq!(s.get_revocation_cursor("other.local").await.unwrap(), None);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn sqlite_cursor_round_trip() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::query(
            "CREATE TABLE auth_revocation_poll_cursors (
                issuer TEXT NOT NULL PRIMARY KEY,
                cursor_ms INTEGER NOT NULL,
                updated_at TEXT NOT NULL
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        let s = SqliteRevocationStore::new(pool);
        assert_eq!(s.get_revocation_cursor("cp.local").await.unwrap(), None);
        s.set_revocation_cursor("cp.local", 1_700_000_000_000)
            .await
            .unwrap();
        assert_eq!(
            s.get_revocation_cursor("cp.local").await.unwrap(),
            Some(1_700_000_000_000)
        );
        // Upsert path.
        s.set_revocation_cursor("cp.local", 1_700_000_005_000)
            .await
            .unwrap();
        assert_eq!(
            s.get_revocation_cursor("cp.local").await.unwrap(),
            Some(1_700_000_005_000)
        );
    }

    #[tokio::test]
    async fn in_memory_evict_drops_expired_rows() {
        let s = InMemoryRevocationStore::new();
        let past = RevocationRecord {
            jti: "old".into(),
            agent_did: "did:web:agent.example".into(),
            expires_at: Utc::now() - chrono::Duration::seconds(1),
        };
        s.record_issued(past).await.unwrap();
        s.evict_expired(Utc::now()).await.unwrap();
        assert!(
            s.owner_of("old").await.unwrap().is_none(),
            "expired rows must be evicted to keep the table bounded"
        );
    }

    // `SqliteRevocationStore::is_revoked` bridges to async via
    // `block_in_place`, which is only valid in a multi-threaded runtime.
    // Match production: `#[tokio::main]` is multi-threaded by default.
    #[tokio::test(flavor = "multi_thread")]
    async fn sqlite_record_issued_then_revoke_lifecycle() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::query(
            "CREATE TABLE issued_tokens (
                jti TEXT PRIMARY KEY,
                agent_did TEXT NOT NULL,
                expires_at TEXT NOT NULL,
                revoked INTEGER NOT NULL DEFAULT 0
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        let s = SqliteRevocationStore::new(pool);
        s.record_issued(rec("t1")).await.unwrap();
        assert!(!s
            .is_revoked("t1", crate::tombstone_cutoff(Utc::now(), 0))
            .unwrap());
        assert_eq!(
            s.owner_of("t1").await.unwrap().as_deref(),
            Some("did:web:agent.example")
        );
        s.revoke(rec("t1")).await.unwrap();
        assert!(s
            .is_revoked("t1", crate::tombstone_cutoff(Utc::now(), 0))
            .unwrap());
        // Tombstone-preservation: re-recording must not clear `revoked`.
        s.record_issued(rec("t1")).await.unwrap();
        assert!(s
            .is_revoked("t1", crate::tombstone_cutoff(Utc::now(), 0))
            .unwrap());
    }

    // ── E1: the tombstone must outlive the validator's acceptance window ──
    //
    // Enumerated per backend rather than totalled (Rule 55/64): memory, sqlite
    // and postgres each get their own guard, because "the fix works" is a claim
    // about three separate comparisons in three separate impls.

    /// A tombstone whose token has expired within the leeway is STILL live.
    /// Before the fix this returned false and a revoked token sailed through.
    #[tokio::test]
    async fn in_memory_tombstone_outlives_expiry_by_the_leeway() {
        let s = InMemoryRevocationStore::new();
        let expired_5s_ago = Utc::now() - chrono::Duration::seconds(5);
        s.revoke(RevocationRecord {
            jti: "inside-window".into(),
            agent_did: "did:web:agent.example".into(),
            expires_at: expired_5s_ago,
        })
        .await
        .unwrap();

        assert!(
            s.is_revoked("inside-window", crate::tombstone_cutoff(Utc::now(), 30))
                .unwrap(),
            "a token 5s past exp is still accepted under 30s leeway, so its tombstone must \
             still count as an active revocation"
        );
        // And the boundary still exists — this is not "tombstones live forever".
        assert!(
            !s.is_revoked("inside-window", crate::tombstone_cutoff(Utc::now(), 0))
                .unwrap(),
            "with no leeway the same tombstone is spent; the window is bounded by leeway, \
             not removed"
        );
    }

    /// THE PAIRED HALF. Widening the check alone is not a fix: eviction would
    /// delete the row at `expires_at` and the revoked token would be accepted
    /// again — through the other door, with the check-side test still green.
    #[tokio::test]
    async fn in_memory_eviction_spares_a_tombstone_inside_the_window() {
        let s = InMemoryRevocationStore::new();
        let expired_5s_ago = Utc::now() - chrono::Duration::seconds(5);
        s.revoke(RevocationRecord {
            jti: "spare-me".into(),
            agent_did: "did:web:agent.example".into(),
            expires_at: expired_5s_ago,
        })
        .await
        .unwrap();

        s.evict_expired(crate::tombstone_cutoff(Utc::now(), 30))
            .await
            .unwrap();
        assert!(
            s.is_revoked("spare-me", crate::tombstone_cutoff(Utc::now(), 30))
                .unwrap(),
            "eviction must not delete a tombstone the validator would still consult; \
             evicting at `expires_at` defeats revocation through the eviction door"
        );

        // Past the window it is still collected — the table stays bounded.
        s.evict_expired(crate::tombstone_cutoff(Utc::now(), 0))
            .await
            .unwrap();
        assert!(
            s.owner_of("spare-me").await.unwrap().is_none(),
            "once outside the acceptance window the row must still be reclaimed"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn sqlite_tombstone_outlives_expiry_and_survives_eviction() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::query(
            "CREATE TABLE issued_tokens (\
                jti TEXT PRIMARY KEY, \
                agent_did TEXT NOT NULL, \
                expires_at TEXT NOT NULL, \
                revoked INTEGER NOT NULL DEFAULT 0\
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        let s = SqliteRevocationStore::new(pool);
        s.revoke(RevocationRecord {
            jti: "sq-window".into(),
            agent_did: "did:web:agent.example".into(),
            expires_at: Utc::now() - chrono::Duration::seconds(5),
        })
        .await
        .unwrap();

        assert!(
            s.is_revoked("sq-window", crate::tombstone_cutoff(Utc::now(), 30))
                .unwrap(),
            "sqlite: tombstone must stay live for the leeway past exp"
        );
        s.evict_expired(crate::tombstone_cutoff(Utc::now(), 30))
            .await
            .unwrap();
        assert!(
            s.is_revoked("sq-window", crate::tombstone_cutoff(Utc::now(), 30))
                .unwrap(),
            "sqlite: eviction must spare a tombstone still inside the window"
        );
    }
}
