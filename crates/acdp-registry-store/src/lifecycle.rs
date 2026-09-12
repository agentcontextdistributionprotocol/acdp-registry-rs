//! Keeping a served context's `status` consistent with the lifecycle events
//! served beside it.
//!
//! # The bug this exists for
//!
//! `get()` and `lineage()` load a context row and its lifecycle events as
//! **two separate pool queries with no shared snapshot**. A retraction commits
//! atomically between them, and the response then carries
//! `registry_state.status: "active"` *together with* a `retracted` event —
//! violating the RFC-ACDP-0013 §7.2 precedence (`retracted > superseded >
//! expired > active`) that both backends' `row_to_context` is documented to
//! guarantee. A consumer trusting `status` acts on retracted data.
//!
//! # Why reconcile rather than take a snapshot
//!
//! Wrapping both reads in one transaction would also fix it, and was the
//! first design. Reconciling is better here for a reason worth writing down:
//! it makes the served object **self-consistent by construction**, so a future
//! read path that forgets the transaction cannot reintroduce the bug. A
//! snapshot fixes this call site; this fixes the shape.
//!
//! Two facts were checked against the code before relying on them, because
//! the cheap fix is only safe if they hold:
//!
//! * **`lifecycle_events` is append-only.** No `DELETE` against it exists in
//!   either backend, so the event list can never be *missing* a retraction the
//!   denormalized column knows about. (An earlier draft of this plan rejected
//!   this approach on exactly that hypothetical, which turned out not to be
//!   real.)
//! * **The event and the flag are written in one transaction**
//!   (`commit_lifecycle_event` appends the event and applies its status effect
//!   together), so at any committed instant the two agree. Only the *reads*
//!   could disagree.

use acdp::types::lifecycle::{retraction_state, LifecycleEvent};
use acdp::types::primitives::Status;

/// Reconcile a row-derived `status` with the lifecycle events loaded alongside
/// it, so the pair can never contradict itself.
///
/// Retraction wins from **either** source. That is deliberate and it is not
/// symmetric:
///
/// * events say retracted, row says otherwise → `Retracted`. This is the live
///   bug: the events are the fresher read (they are queried second), so a
///   retraction that landed mid-read is honoured instead of being served as
///   `active`.
/// * row says retracted, events do not → `Retracted`. Cannot happen with a
///   consistent read, but a torn read in the other direction would otherwise
///   serve `active` for a retracted context, which is the dangerous direction.
///
/// The cost of failing closed like this is that a context retracted and then
/// republished could, under a torn read, be served as `retracted` slightly
/// after it became active again — stale, but never self-contradictory, and
/// stale-toward-retracted is the safe way to be wrong about whether data has
/// been withdrawn.
///
/// Expiry is projected separately and afterwards; this only settles the
/// retraction tier of the precedence.
pub fn reconcile_retraction(status: Status, events: &[LifecycleEvent]) -> Status {
    if status == Status::Retracted || retraction_state(events) {
        Status::Retracted
    } else {
        status
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acdp::types::lifecycle::LifecycleEventType;

    fn event(kind: LifecycleEventType) -> LifecycleEvent {
        // Built through the public shape rather than a fixture file so this
        // test has no I/O and runs in the unit pass.
        serde_json::from_value(serde_json::json!({
            "event_id": "01J0000000000000000000000A",
            "ctx_id": "ctx_test",
            "event_type": match kind {
                LifecycleEventType::Retracted => "retracted",
                LifecycleEventType::Republished => "republished",
                _ => unreachable!("only the two transitioning types are used here"),
            },
            "occurred_at": "2026-01-01T00:00:00.000Z",
            "actor": "did:web:agents.test:actor",
        }))
        .expect("valid lifecycle event")
    }

    #[test]
    fn events_saying_retracted_override_an_active_row() {
        // The live bug: row read before a retract, events read after it.
        let events = vec![event(LifecycleEventType::Retracted)];
        assert_eq!(
            reconcile_retraction(Status::Active, &events),
            Status::Retracted,
            "a retraction event must not be served next to status=active"
        );
    }

    #[test]
    fn a_retracted_row_survives_events_that_do_not_show_it() {
        // The opposite tear. Failing closed is the point.
        assert_eq!(
            reconcile_retraction(Status::Retracted, &[]),
            Status::Retracted
        );
    }

    #[test]
    fn republished_contexts_are_not_forced_retracted_on_a_consistent_read() {
        // retract -> republish, both events present, row already agrees.
        let events = vec![
            event(LifecycleEventType::Retracted),
            event(LifecycleEventType::Republished),
        ];
        assert_eq!(
            reconcile_retraction(Status::Active, &events),
            Status::Active
        );
        assert_eq!(
            reconcile_retraction(Status::Superseded, &events),
            Status::Superseded,
            "reconciliation must not disturb the supersession tier"
        );
    }

    #[test]
    fn no_events_leaves_the_row_status_alone() {
        assert_eq!(reconcile_retraction(Status::Active, &[]), Status::Active);
        assert_eq!(
            reconcile_retraction(Status::Superseded, &[]),
            Status::Superseded
        );
    }
}
