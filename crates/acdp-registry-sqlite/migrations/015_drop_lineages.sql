-- B8 Phase 5b: drop the dormant `lineages` table.
-- Mirror of the Postgres side (`015_drop_lineages.sql`); see that file for the
-- reasoning. On SQLite any applied migration already refuses an older binary
-- (no `set_ignore_missing`), so this adds no rollback constraint of its own.

DROP TABLE IF EXISTS lineages;
