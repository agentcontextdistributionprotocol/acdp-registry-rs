//! Axum handlers for every ACDP HTTP endpoint.

mod auth;
mod changes;
mod context;
mod keyless;
mod log;
mod meta;
mod subscriptions;

pub use auth::{issue_challenge, issue_token, revoke_token};
pub use changes::{
    anchor_at, changes, evaluate_changes, head_cursor, parse_request, ChangesRequest, Since,
    MIN_SCAN_ROWS, REFILL_MAX_SCANS,
};
pub(crate) use context::caller_from_headers;
pub(crate) use context::version_at_least;
pub use context::{current, lineage, publish, republish, retract, retrieve, retrieve_body, search};
pub use keyless::{create_key_attestation, list_key_attestations};
pub use log::{log_checkpoint, log_entries, log_proof};
pub use meta::{capabilities, health, jwks, livez, registry_did_document};
pub use subscriptions::{create_subscription, delete_subscription, get_subscription};

mod admin;
#[cfg(feature = "playground")]
pub use admin::{admin_list, reload_pinned_keys};
pub use admin::{admin_republish, admin_retract, admin_status, lineage_audit};
