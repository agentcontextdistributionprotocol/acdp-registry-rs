-- B8: index the two columns search filters use and nothing indexed.
-- Mirror of the Postgres side (`013_search_filter_indexes.sql`); see that file
-- for the reasoning, including why `data_period` is deliberately left a scan.

CREATE INDEX IF NOT EXISTS idx_ctx_domain ON contexts(domain);
CREATE INDEX IF NOT EXISTS idx_ctx_expires ON contexts(expires_at) WHERE expires_at IS NOT NULL;
