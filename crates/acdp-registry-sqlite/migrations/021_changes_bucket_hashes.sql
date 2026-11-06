-- RFC-ACDP-0018 §6.1 bucket filters: each feed row's bucket hashes.
--
-- Mirrors crates/acdp-registry-pg/migrations/021_changes_bucket_hashes.sql.
--
-- `lineage_hash` / `agent_hash` are lowercase hex(SHA-256(UTF-8)) of the
-- row's `lineage_id` / `agent_id`, all 64 digits, computed in Rust
-- (acdp_registry_store::change_bucket::bucket_hash). A `lineage_bucket` or
-- `agent_bucket` of any length is a prefix of them, served as an indexed
-- range scan: `hash >= prefix AND hash < prefix+1`. The default BINARY
-- collation compares bytewise, which is what the range needs.
--
-- SQLite cannot add a NOT NULL column without a default, and a '' default
-- would put every row in no bucket at all: a silent miss, which for a
-- bucket watcher can be a missed key_revocation. So the columns are
-- nullable and NOT NULL is enforced by the two triggers below instead:
-- an INSERT without both hashes, or an UPDATE that clears one, aborts.
--
-- SQLite has no SHA-256 function, so existing rows are backfilled in Rust
-- right after the migrations run (changes::backfill_hashes, called from
-- SqliteStore::migrate), which then refuses to start with any NULL left.
-- The backfill sets both columns in one UPDATE per row, so the UPDATE
-- trigger never sees half a backfill.
--
-- Additive. SQLite migrations are not N-1 safe in this binary (no
-- `set_ignore_missing`): rolling back past this migration means restoring
-- the pre-upgrade backup, which is a sequence reset (raise
-- `[changes] cursor_epoch`; see docs/OPERATIONS.md).
ALTER TABLE changes ADD COLUMN lineage_hash TEXT;
ALTER TABLE changes ADD COLUMN agent_hash TEXT;

CREATE TRIGGER IF NOT EXISTS changes_require_bucket_hashes
BEFORE INSERT ON changes
WHEN NEW.lineage_hash IS NULL OR NEW.agent_hash IS NULL
BEGIN
    SELECT RAISE(ABORT, 'changes row without bucket hashes (migration 021)');
END;

CREATE TRIGGER IF NOT EXISTS changes_keep_bucket_hashes
BEFORE UPDATE OF lineage_hash, agent_hash ON changes
WHEN NEW.lineage_hash IS NULL OR NEW.agent_hash IS NULL
BEGIN
    SELECT RAISE(ABORT, 'changes bucket hashes cannot be cleared (migration 021)');
END;

CREATE INDEX IF NOT EXISTS idx_changes_lineage_hash ON changes (lineage_hash, seq);
CREATE INDEX IF NOT EXISTS idx_changes_agent_hash   ON changes (agent_hash, seq);
