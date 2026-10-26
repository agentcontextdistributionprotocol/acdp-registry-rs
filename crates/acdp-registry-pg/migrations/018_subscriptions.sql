-- RFC-ACDP-0018 §8 change-hint subscriptions.
--
-- Mirrors crates/acdp-registry-sqlite/migrations/018_subscriptions.sql.
--
-- One row per subscription, owned by the requester DID that created it
-- (every lookup is scoped by `owner_did`). `tenant_id` is the tenant the
-- owner created it under, so hints are evaluated with the same visibility
-- its `GET /changes` would get. The row carries its own delivery
-- state — the hint worker's feed position `last_hinted_seq`, the retry
-- schedule and a short lease — so there is no per-change delivery queue and
-- changes made while a subscription is failing coalesce into one hint
-- (§8.2).
--
-- `secret_ct` is the HMAC secret sealed with AES-256-GCM under a key derived
-- from the changes master key, with the subscription_id as associated data;
-- the plaintext secret is never stored. `secret_key_id` names the key that
-- sealed it.
--
-- Additive and N-1 safe: older binaries never touch this table and run
-- with `set_ignore_missing`.
CREATE TABLE IF NOT EXISTS subscriptions (
    subscription_id    TEXT    PRIMARY KEY,
    owner_did          TEXT    NOT NULL,
    tenant_id          TEXT,
    filter             JSONB   NOT NULL,
    callback_url       TEXT    NOT NULL,
    secret_ct          BYTEA   NOT NULL,
    secret_key_id      SMALLINT NOT NULL,
    status             TEXT    NOT NULL CHECK (status IN ('active', 'disabled')),
    created_at_ms      BIGINT  NOT NULL,
    last_hinted_seq    BIGINT  NOT NULL,
    next_attempt_at_ms BIGINT  NOT NULL,
    failing_since_ms   BIGINT,
    attempts           INTEGER NOT NULL DEFAULT 0,
    lease_until_ms     BIGINT
);

CREATE INDEX IF NOT EXISTS idx_subscriptions_owner ON subscriptions (owner_did);
CREATE INDEX IF NOT EXISTS idx_subscriptions_due
    ON subscriptions (status, next_attempt_at_ms);
