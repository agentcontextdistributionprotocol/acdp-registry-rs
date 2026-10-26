-- RFC-ACDP-0018 change feed.
--
-- One row per change a recall consumer must hear about (§4): every
-- lifecycle event, every superseding publish (the row is for the OLDER
-- version) and every accepted key revocation. `seq` is the registry-wide,
-- strictly increasing feed position (§5.3); AUTOINCREMENT so a pruned or
-- rolled-back position is never reused. Rows are inserted only inside the
-- transaction that makes their change (rec-008) and are never updated.
--
-- Each row carries copies of its evidence (`event` for lifecycle rows, the
-- receipt minted in the same transaction) so a page is one range scan and
-- an entry's bytes cannot drift. `recorded_at` is not stored: it is derived
-- from the receipt when the entry is served.
--
-- The feed starts at this migration; there is no backfill of earlier
-- history. Additive. Note SQLite migrations are not N-1 safe in this binary
-- (no `set_ignore_missing`): rolling back past this migration means
-- restoring the pre-upgrade backup or rolling forward.
CREATE TABLE IF NOT EXISTS changes (
    seq            INTEGER PRIMARY KEY AUTOINCREMENT,
    kind           TEXT    NOT NULL
                   CHECK (kind IN ('lifecycle', 'superseded', 'key_revocation')),
    ctx_id         TEXT    NOT NULL,
    lineage_id     TEXT    NOT NULL,
    agent_id       TEXT    NOT NULL,
    superseded_by  TEXT,
    event          TEXT,
    receipt        TEXT    NOT NULL,
    appended_at_ms INTEGER NOT NULL,
    CHECK ((kind = 'lifecycle') = (event IS NOT NULL)),
    CHECK ((kind = 'superseded') = (superseded_by IS NOT NULL))
);

-- The §6 filter is an OR over these three columns; each arm is an indexed
-- range scan past the cursor.
CREATE INDEX IF NOT EXISTS idx_changes_ctx     ON changes (ctx_id, seq);
CREATE INDEX IF NOT EXISTS idx_changes_lineage ON changes (lineage_id, seq);
CREATE INDEX IF NOT EXISTS idx_changes_agent   ON changes (agent_id, seq);

-- `pruned_through`: every row with seq <= it has been deleted by retention
-- (§5.4). Moved in the same transaction as the delete, so a reader sees the
-- marker and the remaining rows from one snapshot. The head is
-- max(MAX(seq), pruned_through), which survives pruning the whole table.
CREATE TABLE IF NOT EXISTS changes_meta (
    id             INTEGER PRIMARY KEY CHECK (id = 1),
    pruned_through INTEGER NOT NULL
);
INSERT OR IGNORE INTO changes_meta (id, pruned_through) VALUES (1, 0);
