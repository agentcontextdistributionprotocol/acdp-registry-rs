-- RFC-ACDP-0021 key attestations: the derived index.
--
-- Each attestation is an ordinary context body under the registry DID
-- (§5.3). This table is a derived index of those bodies, written only by
-- `commit_publish` in the same transaction as the `contexts` row it names,
-- and used for listing (§5.4) and the publish-time checks (§6.3, §7). The
-- attestation's `content_hash`, status and metadata are read from the
-- joined `contexts` row, never copied here.
--
-- `token_hash` is UNIQUE: a token mints at most one attestation, ever
-- (§5.2 `token_replayed`). The hash is public in every attestation body, so
-- the constraint needs no expiry sweep.
--
-- No tenant column: attestations are registry-global and public.
--
-- Additive. SQLite migrations are not N-1 safe in this binary (no
-- `set_ignore_missing`): rolling back past this migration means restoring
-- the pre-upgrade backup or rolling forward.
CREATE TABLE IF NOT EXISTS key_attestations (
    ctx_id          TEXT    PRIMARY KEY REFERENCES contexts (ctx_id),
    controller      TEXT    NOT NULL,
    identity_type   TEXT    NOT NULL CHECK (identity_type IN ('oidc', 'spiffe')),
    issuer          TEXT,
    subject         TEXT    NOT NULL,
    org_wide        INTEGER NOT NULL CHECK (org_wide IN (0, 1)),
    key_id          TEXT    NOT NULL,
    algorithm       TEXT    NOT NULL,
    key_fingerprint TEXT    NOT NULL,
    not_before_ms   INTEGER NOT NULL,
    not_after_ms    INTEGER NOT NULL,
    token_hash      TEXT    NOT NULL UNIQUE,
    created_at_ms   INTEGER NOT NULL,
    CHECK ((identity_type = 'oidc') = (issuer IS NOT NULL)),
    CHECK (not_before_ms < not_after_ms)
);

CREATE INDEX IF NOT EXISTS idx_key_attestations_controller
    ON key_attestations (controller, created_at_ms DESC, ctx_id);
CREATE INDEX IF NOT EXISTS idx_key_attestations_identity
    ON key_attestations (controller, identity_type, issuer, subject, created_at_ms DESC, ctx_id);
