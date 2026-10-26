-- RFC-ACDP-0018 change feed.
--
-- Mirrors crates/acdp-registry-sqlite/migrations/017_changes.sql; see it
-- for the row model. One row per change a recall consumer must hear about
-- (§4); `seq` is the registry-wide, strictly increasing feed position
-- (§5.3). Rows are inserted only inside the transaction that makes their
-- change (rec-008), as that transaction's last statement and under the
-- feed advisory lock, so seq order is commit order; never updated.
--
-- The feed starts at this migration; there is no backfill of earlier
-- history. Additive and N-1 safe: older binaries never touch these tables
-- and run with `set_ignore_missing`.
CREATE TABLE IF NOT EXISTS changes (
    seq            BIGINT  GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    kind           TEXT    NOT NULL
                   CHECK (kind IN ('lifecycle', 'superseded', 'key_revocation')),
    ctx_id         TEXT    NOT NULL,
    lineage_id     TEXT    NOT NULL,
    agent_id       TEXT    NOT NULL,
    superseded_by  TEXT,
    event          JSONB,
    receipt        JSONB   NOT NULL,
    appended_at_ms BIGINT  NOT NULL,
    CHECK ((kind = 'lifecycle') = (event IS NOT NULL)),
    CHECK ((kind = 'superseded') = (superseded_by IS NOT NULL))
);

CREATE INDEX IF NOT EXISTS idx_changes_ctx     ON changes (ctx_id, seq);
CREATE INDEX IF NOT EXISTS idx_changes_lineage ON changes (lineage_id, seq);
CREATE INDEX IF NOT EXISTS idx_changes_agent   ON changes (agent_id, seq);

-- Every row with seq <= pruned_through has been deleted by retention
-- (§5.4); moved in the same transaction as the delete.
CREATE TABLE IF NOT EXISTS changes_meta (
    id             INTEGER PRIMARY KEY CHECK (id = 1),
    pruned_through BIGINT  NOT NULL
);
INSERT INTO changes_meta (id, pruned_through) VALUES (1, 0) ON CONFLICT (id) DO NOTHING;
