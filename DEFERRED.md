# Deferred items

Open assumptions and escalations recorded in [ASSUMPTIONS.md](ASSUMPTIONS.md). **None has an owner, an SLA or an issue.** Each is
revisited only when it blocks a trigger in [STATUS.md](STATUS.md). Nothing here is a to-do list.

The authoritative, current list is the census tool's output, not this file:

```sh
python3 docs/assumptions-status-census.py --open
```

After the 2026-10-18 reconcile it reports 0 open status declarations. What stays deferred is listed here, one line each, with
the trigger that reopens it:

- H-A/P1: add a 408 `x-request-id` assertion when `TimeoutLayer`'s duration becomes configurable or the layer moves; the 413 test
  currently guards the same boundary.
- #415: a registry-side TLS check for bearers, only if RFC-ACDP-0008 adds a MUST aimed at registries or a forwarded-proto convention.
- W7(f): `#[non_exhaustive]` on `AuthConfig`, at the next deliberate breaking config release.
- Federation proxy: a foreign context failing hash, signature or key checks answers 400; revisit if RFC-ACDP-0006/0007 assigns proxies
  a code for upstream-integrity failures (0.15.1 adoption, (b)).
- RFC-ACDP-0004 §5.1 (pinned 093279a, line 165) says `registry_state.registry_receipt` on lineage members, but the receipt is
  top-level on the envelope everywhere else (acdp-rs, the evidence schema). We serve it top-level; revisit if the spec erratum lands
  the other way.
- Webhook payloads (`context.retracted` / `context.republished`) do not carry the event receipt; revisit if RFC-ACDP-0009 adds it.
- The lifecycle webhook also fires on a byte-identical replay of an event, because `into_context()` drops the outcome kind
  (`crates/acdp-registry-core/src/handlers/context.rs:1629`). This predates RFC-0017. Revisit when a subscriber reports duplicates,
  or when the SDK exposes the replay outcome on the lifecycle path.
- Event receipts are signed for `did:web:<current registry.authority>` without comparing it to the context's `origin_registry`;
  add a skip-and-count guard if `registry.authority` ever becomes changeable for a populated store.
- `[changes]` key rotation keeps no previous-keys list, so rotating expires every cursor (consumers resync).
  Add one before `acdp-registry-events` has paying subscribers (RFC-ACDP-0018 plan F7).
- `GET /changes` has no per-requester rate limit of its own (plan A22); each request's work is bounded by the scan budget.

When one of these is settled, edit its status in `ASSUMPTIONS.md` in the same PR as the change that settled it, and note the decision in
`DECISIONS.md`. Do not open a PR only to tidy a status line.
- RFC-ACDP-0018: a retired-key ring for `[changes]` (like `receipt.retired_keys`, no schema change) — do before the first prod `[events] enabled = true` or the first planned `[changes]` rotation with live subscriptions.
- RFC-ACDP-0018: the conformance_gate scan for the feed bypass writers (`put`, `mark_superseded`) covers only the server and core crates.
- RFC-ACDP-0021: lifecycle events signed by a keyless (attested) key are refused (R13); accept them when RFC-ACDP-0009 §2.14 lands lifecycle-event attestations.
- RFC-ACDP-0021: the Fulcio certificate binding (RFC-ACDP-0009 §2.13) is not implemented; only the registry-minted key attestation is accepted.
- RFC-ACDP-0021: no webhook or change-feed hint fires when a key attestation is minted or superseded; add one if a consumer needs push notice of new keyless keys.
