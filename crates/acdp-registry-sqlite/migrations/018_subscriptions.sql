-- RFC-ACDP-0018 §8 change-hint subscriptions.
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
-- Additive. SQLite migrations are not N-1 safe in this binary (no
-- `set_ignore_missing`): rolling back past this migration means restoring
-- the pre-upgrade backup or rolling forward.
CREATE TABLE IF NOT EXISTS subscriptions (
    subscription_id    TEXT    PRIMARY KEY,
    owner_did          TEXT    NOT NULL,
    tenant_id          TEXT,
    filter             TEXT    NOT NULL,
    callback_url       TEXT    NOT NULL,
    secret_ct          BLOB    NOT NULL,
    secret_key_id      INTEGER NOT NULL,
    status             TEXT    NOT NULL CHECK (status IN ('active', 'disabled')),
    created_at_ms      INTEGER NOT NULL,
    last_hinted_seq    INTEGER NOT NULL,
    next_attempt_at_ms INTEGER NOT NULL,
    failing_since_ms   INTEGER,
    attempts           INTEGER NOT NULL DEFAULT 0,
    lease_until_ms     INTEGER
);

CREATE INDEX IF NOT EXISTS idx_subscriptions_owner ON subscriptions (owner_did);
CREATE INDEX IF NOT EXISTS idx_subscriptions_due
    ON subscriptions (status, next_attempt_at_ms);
