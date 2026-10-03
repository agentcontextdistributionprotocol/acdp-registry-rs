//! Postgres-backed `acdp-registry-store` implementation.
//!
//! Logically identical to `acdp-registry-sqlite` but with native
//! `TIMESTAMPTZ`, `TEXT[]`, `JSONB`, and Postgres FTS.

pub mod rate_limit;
pub mod store;

pub use rate_limit::PgRateLimitBackend;
pub use store::PgStore;
