-- B8 Phase 5b: drop the dormant `lineages` table.
--
-- Nothing has read `lineages` since it was created, and nothing has written it
-- since 0.3.0 (Phase 5a, #359): every lineage read is served from `contexts`,
-- grouped by `lineage_id`. This is the second half of the two-release
-- retirement recorded in DECISIONS.md ("B8 Phase 5a", then the 2026-10-18
-- reconcile that confirmed Q2).
--
-- Rollback: the Postgres migrator tolerates an unknown applied version
-- (`set_ignore_missing`), so a 0.3.0-or-later binary runs against this schema
-- unchanged. A 0.2.x binary does not: its publish still upserts `lineages` and
-- fails. docs/UPGRADING.md repeats this.

DROP TABLE IF EXISTS lineages;
