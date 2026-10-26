//! SQLite-backed `acdp-registry-store` implementation.
//!
//! Intended for local development, conformance tests, and the playground
//! profile. Production deployments use `acdp-registry-pg`.

mod changes;
pub mod store;
mod subscriptions;

pub use store::SqliteStore;
