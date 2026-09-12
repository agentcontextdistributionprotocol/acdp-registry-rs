//! Postgres half of the E1 tombstone-window guard.
//!
//! # Why this is an integration test rather than a `#[cfg(test)]` module
//!
//! It reads `ACDP_REGISTRY_TEST_PG_URL` / `ACDP_REQUIRE_PG`, and
//! `conformance_gate.rs::every_directly_read_env_var_is_documented` scans `src/`
//! for direct `env::var` reads without distinguishing `#[cfg(test)]` code — so a
//! test-only variable inside `src/` is reported as undocumented operator
//! configuration. Every other pg-gated test in this workspace lives under
//! `tests/`, which the gate excludes. Moving it here satisfies the gate for the
//! right reason instead of documenting a test fixture as a config key.

use acdp_registry_auth::{tombstone_cutoff, PgRevocationStore, RevocationRecord, RevocationStore};
use chrono::Utc;

/// **E1, Postgres.** A tombstone must stay live for the validator's leeway past
/// the token's `exp`, and eviction must not delete it while it is still inside
/// that window. Both halves, because fixing only the check leaves eviction
/// defeating revocation through the other door.
///
/// Gated the way the rest of this workspace gates pg: an absent URL skips, but
/// `ACDP_REQUIRE_PG=1` turns a missing URL into a FAILURE rather than a silent
/// pass — otherwise "no postgres" and "postgres passed" are indistinguishable.
#[tokio::test(flavor = "multi_thread")]
async fn pg_tombstone_outlives_expiry_and_survives_eviction() {
    let url = match std::env::var("ACDP_REGISTRY_TEST_PG_URL") {
        Ok(u) => u,
        Err(_) => {
            assert!(
                std::env::var("ACDP_REQUIRE_PG").unwrap_or_default() != "1",
                "ACDP_REQUIRE_PG=1 but ACDP_REGISTRY_TEST_PG_URL is unset"
            );
            return;
        }
    };
    let pool = sqlx::PgPool::connect(&url).await.expect("pg connect");
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS issued_tokens (\
            jti TEXT PRIMARY KEY, \
            agent_did TEXT NOT NULL, \
            expires_at TIMESTAMPTZ NOT NULL, \
            revoked BOOLEAN NOT NULL DEFAULT false\
        )",
    )
    .execute(&pool)
    .await
    .expect("create table");

    // Unique per run: this database persists across runs and an exact-state
    // assertion against accumulated rows is how I shipped a flake in H-H.
    let jti = format!(
        "pg-window-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    );
    let s = PgRevocationStore::new(pool);
    s.revoke(RevocationRecord {
        jti: jti.clone(),
        agent_did: "did:web:agent.example".into(),
        expires_at: Utc::now() - chrono::Duration::seconds(5),
    })
    .await
    .unwrap();

    assert!(
        s.is_revoked(&jti, tombstone_cutoff(Utc::now(), 30))
            .unwrap(),
        "pg: a token 5s past exp is still accepted under 30s leeway, so its tombstone must \
         still count as an active revocation"
    );
    s.evict_expired(tombstone_cutoff(Utc::now(), 30))
        .await
        .unwrap();
    assert!(
        s.is_revoked(&jti, tombstone_cutoff(Utc::now(), 30))
            .unwrap(),
        "pg: eviction must not delete a tombstone the validator would still consult"
    );

    // Past the window it is still reclaimed — the table stays bounded.
    s.evict_expired(tombstone_cutoff(Utc::now(), 0))
        .await
        .unwrap();
    assert!(
        !s.is_revoked(&jti, tombstone_cutoff(Utc::now(), 30))
            .unwrap(),
        "pg: once outside the acceptance window the row must still be collected"
    );
}
