-- RFC-ACDP-0017 §10 event receipts.
--
-- The registry-signed `acdp-event-receipt/1` for each lifecycle event,
-- stored on the event's own row so the served `registry_state.event_receipts`
-- array is parallel to `lifecycle_events` by construction (one SELECT, one
-- snapshot). JSON text; NULL for events accepted before receipts were
-- minted. Written in the same transaction as the event, or once by
-- fill-on-append / the startup sweep (NULL slots only); never updated after.
--
-- Additive. Note SQLite migrations are not N-1 safe in this binary (no
-- `set_ignore_missing`): rolling back past this migration means restoring
-- the pre-upgrade backup or rolling forward.
ALTER TABLE lifecycle_events ADD COLUMN receipt TEXT;

-- The startup sweep's candidate scan (`DISTINCT ctx_id … WHERE receipt IS
-- NULL`) reads only un-receipted rows, so after the first sweep a restart
-- costs O(remaining NULLs), not O(all events).
CREATE INDEX IF NOT EXISTS idx_lifecycle_events_receipt_null
    ON lifecycle_events (ctx_id) WHERE receipt IS NULL;
