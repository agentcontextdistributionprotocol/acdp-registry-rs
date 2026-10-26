//! RFC-ACDP-0018 §8 change-hint subscriptions: the storage-side shapes every
//! backend shares.
//!
//! A subscription row carries its own delivery state (the hint worker's
//! cursor `last_hinted_seq`, the retry schedule and a lease), so there is no
//! per-change delivery queue: a hint is "pull now", and any number of
//! changes while a subscription is failing coalesce into one hint after it
//! recovers (§8.2).
//!
//! The `secret` is never stored in plaintext. Backends hold only
//! `secret_ct`, sealed by the registry's changes key with the
//! `subscription_id` as associated data, and the id of the key that sealed
//! it; sealing and opening happen above the store.

use acdp::error::AcdpError;
use acdp::types::change::{Subscription, SubscriptionStatus, WatchFilter};
use chrono::DateTime;

/// A subscription to insert.
#[derive(Debug, Clone, PartialEq)]
pub struct NewSubscription {
    /// Canonical lowercase v4 UUID.
    pub subscription_id: String,
    /// The requester DID that created it; every lookup is scoped by it.
    pub owner_did: String,
    /// The tenant it was created under; hints are evaluated as the owner
    /// in this tenant.
    pub tenant_id: Option<String>,
    pub filter: WatchFilter,
    pub callback_url: String,
    /// `nonce ‖ ciphertext` of the secret, never the secret itself.
    pub secret_ct: Vec<u8>,
    /// The changes key id that sealed `secret_ct`.
    pub secret_key_id: u8,
    pub created_at_ms: i64,
    /// The feed head at creation: a new subscription never hints for
    /// history.
    pub last_hinted_seq: u64,
}

/// One stored subscription, with its delivery state.
#[derive(Debug, Clone, PartialEq)]
pub struct SubscriptionRecord {
    pub subscription_id: String,
    pub owner_did: String,
    pub tenant_id: Option<String>,
    pub filter: WatchFilter,
    pub callback_url: String,
    pub secret_ct: Vec<u8>,
    pub secret_key_id: u8,
    pub status: SubscriptionStatus,
    pub created_at_ms: i64,
    /// Every feed entry at or below this seq has been hinted, or was not
    /// visible to the owner.
    pub last_hinted_seq: u64,
    /// When the worker may next try this subscription.
    pub next_attempt_at_ms: i64,
    /// When the current run of failed deliveries began; `None` while
    /// delivering.
    pub failing_since_ms: Option<i64>,
    /// Consecutive failed deliveries.
    pub attempts: u32,
    /// A worker's claim; never held across the HTTP call's commit.
    pub lease_until_ms: Option<i64>,
}

/// What `create_subscription` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateSubscriptionOutcome {
    Created,
    /// The owner already holds `max_per_owner` subscriptions; nothing was
    /// written.
    LimitReached,
}

impl SubscriptionRecord {
    /// The record as `GET /subscriptions/{id}` serves it: without `secret`.
    /// A record that no longer forms a valid subscription is a
    /// `RegistryInternal` error.
    pub fn to_subscription(&self) -> Result<Subscription, AcdpError> {
        let corrupt = |why: String| {
            AcdpError::RegistryInternal(format!(
                "stored subscription {} is malformed: {why}",
                self.subscription_id
            ))
        };
        let created_at = DateTime::from_timestamp_millis(self.created_at_ms).ok_or_else(|| {
            corrupt(format!(
                "created_at_ms {} is out of range",
                self.created_at_ms
            ))
        })?;
        let sub = Subscription {
            subscription_id: self.subscription_id.clone(),
            filter: self.filter.clone(),
            callback_url: self.callback_url.clone(),
            created_at,
            status: self.status,
            secret: None,
        };
        sub.validate().map_err(|e| corrupt(e.to_string()))?;
        Ok(sub)
    }
}

/// The stored wire form of a status.
pub fn status_str(status: SubscriptionStatus) -> &'static str {
    match status {
        SubscriptionStatus::Active => "active",
        SubscriptionStatus::Disabled => "disabled",
    }
}

/// A status from its stored wire form.
pub fn parse_status(s: &str) -> Result<SubscriptionStatus, AcdpError> {
    match s {
        "active" => Ok(SubscriptionStatus::Active),
        "disabled" => Ok(SubscriptionStatus::Disabled),
        other => Err(AcdpError::RegistryInternal(format!(
            "stored subscription status '{other}' is unknown"
        ))),
    }
}

/// The stored JSON form of a filter.
pub fn filter_json(filter: &WatchFilter) -> Result<String, AcdpError> {
    serde_json::to_string(filter)
        .map_err(|e| AcdpError::RegistryInternal(format!("subscription filter: {e}")))
}

/// A filter from its stored JSON form.
pub fn parse_filter(raw: &str) -> Result<WatchFilter, AcdpError> {
    serde_json::from_str(raw).map_err(|e| {
        AcdpError::RegistryInternal(format!("stored subscription filter does not parse: {e}"))
    })
}
