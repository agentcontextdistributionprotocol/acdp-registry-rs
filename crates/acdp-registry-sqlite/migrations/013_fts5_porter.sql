-- B2: bring SQLite's full-text semantics to Postgres's.
--
-- `contexts_fts` was created with FTS5's default tokenizer (`unicode61`):
-- no stemmer, no stopwords. Postgres indexes with `to_tsvector('english',…)`
-- and queries with `plainto_tsquery('english',…)`, which applies snowball
-- stemming. Measured divergence before this migration:
--
--     q=running  against "run report"            sqlite 0 rows, pg 1 row
--     q=the      against "the quarterly figures" sqlite 1 row,  pg 0 rows
--
-- Adding the `porter` stemmer closes the first case. Measured after:
-- `MATCH '"running"'` matches both "run report" and "running reports", and
-- porter stems THROUGH FTS5's phrase quoting, so `fts5_escape` keeps quoting
-- every token and loses none of its operator-neutralizing property.
--
-- The second case is handled query-side in `fts5_escape` (porter does not
-- drop stopwords; measured: `MATCH '"the"'` still matches under porter), so
-- there is nothing to do about it here.
--
-- A tokenizer cannot be altered in place, so the virtual table is dropped and
-- rebuilt. This is DERIVED data — every column is a copy of `contexts` — so
-- the rebuild loses nothing and the migration is reversible by restoring the
-- previous tokenizer and rebuilding again. `contexts` itself is untouched.
--
-- The sync triggers live on `contexts`, not on `contexts_fts`, so they
-- survive the drop and keep working against the rebuilt table; their column
-- list is unchanged.

DROP TABLE IF EXISTS contexts_fts;

CREATE VIRTUAL TABLE contexts_fts USING fts5(
    ctx_id UNINDEXED,
    title,
    summary,
    description,
    domain,
    tags,
    agent_id,
    tokenize = 'porter unicode61'
);

-- Repopulate from the source of truth, matching the triggers' COALESCE shape
-- exactly so a rebuilt row is byte-identical to a trigger-written one.
INSERT INTO contexts_fts(ctx_id, title, summary, description, domain, tags, agent_id)
SELECT ctx_id,
       title,
       COALESCE(summary, ''),
       COALESCE(description, ''),
       COALESCE(domain, ''),
       tags,
       agent_id
FROM contexts;
