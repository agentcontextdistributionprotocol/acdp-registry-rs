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

When one of these is settled, edit its status in `ASSUMPTIONS.md` in the same PR as the change that settled it, and note the decision in
`DECISIONS.md`. Do not open a PR only to tidy a status line.
