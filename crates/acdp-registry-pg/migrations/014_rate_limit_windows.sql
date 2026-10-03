-- Cluster-wide fixed-window counters for the `/auth/*` rate limiter
-- (FEAT-06 item 3). One row per live (scope, key), with `window_start` rolled
-- in place — the relational image of the in-memory `Bucket { window_start, count }`.
--
-- Named `rate_limit_windows`, not `auth_rate_limit_windows`: other scopes may
-- share the table later.
--
-- UNLOGGED: the contents are 60-second ephemeral counters. Skipping the WAL
-- removes the commit fsync from the row-lock hold on every `/auth/*` check and
-- keeps the limiter's write volume out of WAL, replication and backups. The
-- price is that the rows are truncated on crash recovery and absent after a
-- standby promotion (the table itself survives): one window's budget resets
-- once, which is no worse than the limiter's fail-open posture. Supported by
-- every Postgres target this repo documents.
--
-- Deliberately no index beyond the primary key. `window_start` changes on every
-- window roll, so indexing it would make each roll a non-HOT update plus an
-- index write; the pruner's DELETE is a short scan over a table bounded by the
-- number of *active* clients. fillfactor leaves the page space HOT updates need.
CREATE UNLOGGED TABLE IF NOT EXISTS rate_limit_windows (
    scope        TEXT    NOT NULL,   -- 'auth_per_ip' | 'auth_global'
    bucket_key   TEXT    NOT NULL,   -- resolved client IP; '' for the global scope
    window_start BIGINT  NOT NULL,   -- epoch seconds, floored to the window
    hits         INTEGER NOT NULL,
    PRIMARY KEY (scope, bucket_key)
) WITH (fillfactor = 70);
