# Status and definition of done

**As of 2026-10-06, 0.4.1 is the stable line.** This repository is *done* while `main` is green and no issue is open.
That is the normal resting state, not a gap to fill.

## When to reopen it

Only for one of these:

1. An **external** bug report (someone who is not a maintainer or a project agent session).
2. A **RUSTSEC advisory** that affects a dependency we ship.
3. A **tagged** ACDP spec release whose conformance fixtures fail against this registry.
4. A sibling repo's **deployed** instance that needs a change here (a live control plane, console or playground, not a recipe or demo).

Anything else, including a good idea, a sibling repo's design decision, or a cleanup, waits for one of the four triggers.

## What does not create work

- **Follow-ups from a plan, `/reconcile` or a verifier pass.** They are recorded once in `ASSUMPTIONS.md` / `DECISIONS.md` and listed in
  [DEFERRED.md](DEFERRED.md). They do not become issues, plans or PRs on their own.
- **Log-only PRs.** Update the logs in the PR that makes the change, or not at all.
- **New guards that assert an exact count or line number** (mutant scope, survivor lines, `.rs:N` cites). Prefer a symbol- or
  behaviour-based check. A guard that must be re-pinned after every unrelated change costs more than the drift it catches.
- **Cross-repo asks without a deployed consumer.** A sibling repo asking for a feature nobody runs yet is recorded, not built.
- **Bot bumps.** Spec, `acdp-rs`, dependency and release-plz PRs are batched and merged on green; do not hand-supersede them.

## Deferred on purpose

| Item | Why deferred | Re-open when |
|---|---|---|
| Multi-audience `bearer_jwt` (#420) and the `GET /auth/revocations` feed (#421) | No control plane is deployed against this registry; it ships with `TRUSTED_ISSUERS` and `REVOCATION_FEEDS` empty. | A real control plane lists this registry as a trusted issuer (trigger 4). |

The analysis behind both is in the maintainer's local `plans/archive/multi-aud-and-revocation-feed.md` (plans are not tracked in git):
an allow-listed per-request `audience` bound into a v2 challenge signing input, a provenance-tracked revocation feed with its own
feed credential and rate limit.
