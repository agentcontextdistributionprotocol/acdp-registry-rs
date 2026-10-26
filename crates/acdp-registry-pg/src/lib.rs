//! Postgres-backed `acdp-registry-store` implementation.
//!
//! Logically identical to `acdp-registry-sqlite` but with native
//! `TIMESTAMPTZ`, `TEXT[]`, `JSONB`, and Postgres FTS.

mod changes;
pub mod rate_limit;
pub mod store;
mod subscriptions;

pub use changes::FEED_APPEND_LOCK_KEY;
pub use rate_limit::PgRateLimitBackend;
pub use store::PgStore;
