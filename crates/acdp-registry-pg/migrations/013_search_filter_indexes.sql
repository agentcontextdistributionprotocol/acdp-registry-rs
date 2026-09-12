-- B8: index the two columns search filters use and nothing indexed.
--
-- `GET /contexts/search` filters on `domain` and on `expires_at`, and neither
-- had an index in either backend, so both filters were full scans. Added to
-- both backends in the same change so the two stay comparable.
--
-- `expires_at` is nullable and the filter always pairs with `IS NOT NULL`, so
-- the index is partial — smaller, and it matches the predicate the query
-- actually writes.
--
-- NOT addressed here, deliberately: the `data_period` filters remain scans.
-- They read out of `body_json` and are compared through `unixepoch(...)` /
-- `::timestamptz`, so indexing them needs an expression or generated column —
-- a schema decision with no measured volume behind it yet, priced in
-- ASSUMPTIONS.md rather than guessed at.

CREATE INDEX IF NOT EXISTS idx_ctx_domain ON contexts(domain);
CREATE INDEX IF NOT EXISTS idx_ctx_expires ON contexts(expires_at) WHERE expires_at IS NOT NULL;
