-- RFC-ACDP-0018 §6.1 bucket filters: each feed row's bucket hashes.
--
-- Mirrors crates/acdp-registry-sqlite/migrations/021_changes_bucket_hashes.sql;
-- see it for the column model. `lineage_hash` / `agent_hash` are lowercase
-- hex(SHA-256(UTF-8)) of `lineage_id` / `agent_id`, all 64 digits. The
-- binary binds them (acdp_registry_store::change_bucket::bucket_hash); a
-- bucket of any length is served as the index range scan
-- `hash >= prefix AND hash < prefix+1`.
--
-- Runs in sqlx's per-migration transaction. The ALTER takes ACCESS
-- EXCLUSIVE on `changes` and holds it through the backfill, SET NOT NULL,
-- the CHECKs and both index builds, until commit: `/changes` reads, the
-- hint worker, prune, lifecycle events and superseding or revocation
-- publishes all wait (plain first publishes do not touch `changes`).
-- Size the hold before deploying with `SELECT count(*) FROM changes;`
-- (UPGRADING.md, docs/OPERATIONS.md).
--
-- lock_timeout bounds the wait to ACQUIRE that lock. Without it the ALTER
-- queues behind any open snapshot read of `changes`, and every later reader
-- and writer queues behind the ALTER. With it the migration fails fast
-- (SQLSTATE 55P03), `migrate()` returns Err, this replica fails startup and
-- the old replicas keep serving; retry in a quieter moment. SET LOCAL is
-- scoped to this transaction, so it never reaches the pool.
SET LOCAL lock_timeout = '5s';

-- COLLATE "C": bytewise, cheaper comparisons. Over [0-9a-f] any collation
-- orders the same, so this is for cost, not correctness.
ALTER TABLE changes ADD COLUMN IF NOT EXISTS lineage_hash TEXT COLLATE "C";
ALTER TABLE changes ADD COLUMN IF NOT EXISTS agent_hash   TEXT COLLATE "C";

-- The N-1 shim: an older binary inserts without these columns during a
-- rolling deploy or a rollback, and still gets the right hashes, so NOT
-- NULL below is safe. Fills only a NULL, never overriding a value the
-- binary bound; a parity test pins it to the Rust derivation.
CREATE OR REPLACE FUNCTION changes_fill_bucket_hashes() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.lineage_hash IS NULL THEN
        NEW.lineage_hash := encode(sha256(convert_to(NEW.lineage_id, 'UTF8')), 'hex');
    END IF;
    IF NEW.agent_hash IS NULL THEN
        NEW.agent_hash := encode(sha256(convert_to(NEW.agent_id, 'UTF8')), 'hex');
    END IF;
    RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS changes_fill_bucket_hashes ON changes;
CREATE TRIGGER changes_fill_bucket_hashes
    BEFORE INSERT ON changes
    FOR EACH ROW EXECUTE FUNCTION changes_fill_bucket_hashes();

-- Backfill: the feed is bounded by retention, so this is one statement in
-- this transaction rather than a background job that would serve wrong
-- bucket answers until it finished.
UPDATE changes SET
    lineage_hash = COALESCE(lineage_hash,
                            encode(sha256(convert_to(lineage_id, 'UTF8')), 'hex')),
    agent_hash   = COALESCE(agent_hash,
                            encode(sha256(convert_to(agent_id, 'UTF8')), 'hex'))
WHERE lineage_hash IS NULL OR agent_hash IS NULL;

ALTER TABLE changes ALTER COLUMN lineage_hash SET NOT NULL;
ALTER TABLE changes ALTER COLUMN agent_hash   SET NOT NULL;

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_constraint
                   WHERE conrelid = 'changes'::regclass
                     AND conname = 'changes_lineage_hash_hex') THEN
        ALTER TABLE changes ADD CONSTRAINT changes_lineage_hash_hex
            CHECK (lineage_hash ~ '^[0-9a-f]{64}$');
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_constraint
                   WHERE conrelid = 'changes'::regclass
                     AND conname = 'changes_agent_hash_hex') THEN
        ALTER TABLE changes ADD CONSTRAINT changes_agent_hash_hex
            CHECK (agent_hash ~ '^[0-9a-f]{64}$');
    END IF;
END
$$;

CREATE INDEX IF NOT EXISTS idx_changes_lineage_hash ON changes (lineage_hash, seq);
CREATE INDEX IF NOT EXISTS idx_changes_agent_hash   ON changes (agent_hash, seq);
