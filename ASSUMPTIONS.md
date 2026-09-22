# Assumptions log — `reg1-reg7-conformance-deny`

Decisions made against `plans/reg1-reg7-conformance-deny.md`'s Open Questions during
`/drive`. All 8 entries below were reconciled on 2026-08-29 — see `DECISIONS.md` for the
full recommendation + human decision on each. This file is kept as the original record;
`DECISIONS.md` is the durable, current source of truth.

## RECONCILED (2026-08-29) — see DECISIONS.md for full detail

1. **Inline `actions/checkout@v4` vs `checkout-spec@v1`.** CONFIRMED as-is — the `v1`
   tag doesn't even contain the shared action yet. Follow-up: file an `acdp-ci` issue.
   *Superseded 2026-09-06 (`#155`): `v1` now contains the action and this repo adopted
   `checkout-spec`; the inline form is gone. The original entry stands as written — its
   premise changed, it was not wrong. See `DECISIONS.md` § "1. `checkout-spec@v1` vs
   inline checkout".*
   *Measured vs inferred, kept honest deliberately: what is **measured** is that `v1`
   today dereferences to commit `0159101…` and that `git ls-tree -r v1` lists
   `actions/checkout-spec/action.yml`. That the tag was **re-tagged** (moved) is an
   **inference** — consistent with the evidence, but tag movement leaves no readable
   history, so it cannot be confirmed after the fact from the repo alone. Nothing in
   `#155` depends on it: the action is pinned at the immutable commit SHA, not at `v1`.*

2. **`bump-spec.yml` out of scope.** Changed to NEEDS-FOLLOWUP — add it as a near-term
   follow-up (inert until dispatched), paired with a cross-repo spec-matrix item.

3. **`can`/`lin` not excused.** CONFIRMED — policy correct and self-enforcing. `can`'s
   possible cheap-closure path (direct content-hash test) noted for the stateful-replay
   follow-up, not separately scheduled.

8. **Plan-text "yields exactly four" overclaim.** CONFIRMED, no edit needed — the plan
   already self-corrects before the overclaimed line in reading order.

4. **REG-1 acceptance criterion "as applicable" reading.** Narrowed on reconcile: `lc`/
   `fed`/`caps` are legitimately not-applicable; `vis`/`idem` are core-required and the
   gap should be scheduled, not left indefinite. Shipped code stands; NEEDS-FOLLOWUP for
   a stateful-replay phase.
   **Correction (2026-09-01):** the "`lc`/`fed`/`caps` legitimately not-applicable"
   framing was wrong — verified against
   `crates/acdp-registry-server/tests/conformance.rs`'s shipped `EXCUSED`/`DEFERRED`
   lists (~6585-6735): only `fed` is `EXCUSED`; `caps` and `lc` are `DEFERRED` (open,
   #115), not not-applicable. The
   `vis`/`idem` stateful-replay follow-up did ship (REG-10 Phases 5-11) and both are now
   `COVERED`. Full detail in `DECISIONS.md`'s 2026-09-01 correction on entry #4.

5. **`h2` advisory fix bundled into REG-7's PR.** CONFIRMED — no bundling policy
   violated, fix was a prerequisite for REG-7's own acceptance criterion, already merged.

6. **Stale `deny.toml` entries left untouched.** CONFIRMED, deferred to REG-9.

7. **`storage-memory` uncovered by CI.** CONFIRMED as flagged, but elevated from a
   passive note to NEEDS-FOLLOWUP — file a trackable backlog item.

---

**Outstanding follow-ups from this reconcile pass** (see `DECISIONS.md`'s Summary for
full detail — none block anything already shipped):
1. File an `acdp-ci` issue re: `v1` tag + DELIVERY-STANDARD.md staleness.
2. Add `bump-spec.yml` to this repo.
3. File a cross-repo item for the spec repo's dispatch matrix + DELIVERY-STANDARD status.
4. Schedule a "stateful replay" REG-item for `vis`/`idem` coverage.
5. File a backlog item for `storage-memory` CI coverage.

---

## `reg2-reg5-reg6-reg8-reg9-wave4` — logged during `/drive` 2026-08-29

Plan: `plans/reg2-reg5-reg6-reg8-reg9-wave4.md`. The plan's own Open Questions section
(lines 1252-1300) already proposed a clearly-best, cheap-to-reverse default for each of
its five open questions; per `/implement`'s stop-condition tiers none is a fork with no
defensible default, so the pipeline proceeded on each proposed default rather than
pausing. Logged here for `/reconcile`.

**RECONCILED (2026-08-29) — all 5 confirmed as recommended, see `DECISIONS.md` for full
detail.** OQ2 additionally received a dedicated Fable one-way-door pass during
`/implement` itself. OQ3's assumed premise (a spec-side `rev-001` documentation gap)
turned out false on direct verification — the fixture is correctly covered via
`conditional_fixtures`, so no issue was filed. Five low-priority, non-blocking follow-ups
logged in `DECISIONS.md`'s Summary; none scheduled.

### OQ1 — accept the wit-002/wit-004 vacuous-pass substitution
- **Plan:** plans/reg2-reg5-reg6-reg8-reg9-wave4.md
- **Assumed:** REG-2's literal acceptance text ("wit-002 and wit-004 pass in this repo's
  harness") is already true today, vacuously, via the non-HTTP skip path — not via any
  behavioral coverage.
- **Chose:** refuse the vacuous reading. Phase 4 executes wit-004's real cryptographic
  vector against the registry's own cosignature+quorum verification path; Phase 5
  strengthens the registry's existing (but non-discriminating) fork-refusal unit tests to
  pin wit-002's forged root and assert failure *reasons*, not just the error variant.
- **Alternatives:** take the vacuous pass, bump the pin, and write one sentence
  documenting that the fixtures skip as non-HTTP. Cheaper, but would let a wire-level
  "conformance" claim stand on a skip line — inconsistent with this repo's established
  posture (REG-1's own coverage ratchet exists to prevent exactly this).
- **Blast radius if wrong:** low — Phases 4-5 are additive test coverage plus two small
  wire-mapping arms (Phase 2/3, separately assumption-logged below); nothing they add is
  load-bearing for anything else in the plan. Reverting means deleting two test functions
  and two match arms.
- **Status:** CONFIRMED (2026-08-29)

### OQ2 — advertise `acdp_version: "0.4.0"` when aggregating witness cosignatures
- **Plan:** plans/reg2-reg5-reg6-reg8-reg9-wave4.md
- **Assumed:** a registry with `[[witnesses]]` configured (and therefore aggregating
  RFC-ACDP-0015 §6.1 `witness_signatures`) should stop under-claiming `acdp_version:
  "0.3.0"` in its served capabilities document.
- **Chose:** add a `0.4.0` rung to `build_capabilities`'s version ladder, gated on
  `!cfg.witnesses.is_empty()`, ordered before the existing `0.3.0` rung. This is the
  wave's only wire-contract change, so Phase 3 was routed to a fresh **Fable** verifier
  rather than the default Opus, per `/implement`'s one-way-door stop-condition rule for
  public API contracts — see Phase 3's `PROGRESS.md` entry for Fable's verdict.
- **Alternatives:** (a) leave it at 0.3.0 — rejected, this is the actual drift (serving a
  0.4.0 wire member under a 0.3.0 banner); (b) a new opt-in config flag — rejected, the
  spec is explicit there is no new capability flag for this, and a flag would let the
  advertisement drift from actual behavior; (c) gate on `cfg.log.enabled` instead of
  `!cfg.witnesses.is_empty()` — rejected, over-claims for any transparency-log registry
  that aggregates nothing.
- **Blast radius if wrong:** low-medium — config-derived, one `if` branch, no persisted
  state, reverts with a single-commit revert. But it is a public, wire-visible
  advertisement read by consumers (and downstream family members touching witness
  surfaces this wave and next — UI-2, CP-2), so it's the one item in this wave worth a
  deliberate second look rather than a rubber stamp.
- **Status:** CONFIRMED (2026-08-29)

### OQ3 — file a spec-repo issue for the `rev-001` profiles.md/profiles.json divergence
- **Plan:** plans/reg2-reg5-reg6-reg8-reg9-wave4.md
- **Assumed:** at spec pin `31cf874`, `registries/profiles.md`'s `acdp-registry-core` row
  lists `rev-001` among its conformance fixtures, but `registries/profiles.json`'s
  `acdp-registry-core.required_fixtures` (72 entries) does not contain it — a documented
  spec-side inconsistency, not a bug in this repo (the coverage ratchet reads the JSON,
  so nothing here breaks).
- **Chose:** file an issue in the spec repo describing the divergence — issue-filing is
  unrestricted cross-repo per `/plan`'s Cross-repo work section (never a write to the spec
  repo itself). Not yet filed as of this log entry.
- **Alternatives:** ignore it (it's not blocking); silently work around it in this repo's
  own harness (would hide a spec authoring bug rather than surface it upstream).
- **Blast radius if wrong:** near zero — worst case is a spurious issue that the spec
  maintainer closes as expected behavior.
- **Status:** CONFIRMED (2026-08-29) — premise was wrong (rev-001 IS covered, via conditional_fixtures); no issue filed.

### OQ4 — REG-8's reach: also SHA-pin `peter-evans/repository-dispatch`
- **Plan:** plans/reg2-reg5-reg6-reg8-reg9-wave4.md
- **Assumed:** the wave named only `docker.yml` and `release-plz.yml` for REG-8, but
  `notify-website.yml` also carries a credential-adjacent third-party action
  (`peter-evans/repository-dispatch@v4`, consuming a bot token minted by
  `actions/create-github-app-token@v2`) that `acdp-rs` already SHA-pins.
- **Chose:** pin `peter-evans/repository-dispatch` too, in the same PR (Phase 9), at the
  same SHA `acdp-rs` uses (`28959ce8df70de7be546dd1250a005dd32156697` — exact parity
  verified). Left `actions/create-github-app-token@v2` on its major tag (first-party
  tier, matching the sibling) and left every `acdp-ci/.github/workflows/*@v1`
  reusable-workflow ref untouched (pinning those would break family propagation).
- **Alternatives:** pin only what the wave named literally (`docker.yml`,
  `release-plz.yml`) and leave `notify-website.yml` for a future pass — rejected as
  needlessly narrow given the one-line cost and the direct sibling precedent.
- **Blast radius if wrong:** near zero — one more immutable SHA pin, reverts with the
  same single-commit revert as any other Phase 9 pin.
- **Status:** CONFIRMED (2026-08-29)

### OQ5 — PR count: keep PR C (axum-server 0.8) and PR D (axum 0.8) as two separate PRs
- **Plan:** plans/reg2-reg5-reg6-reg8-reg9-wave4.md
- **Assumed:** the plan's default is 5 PRs, with the axum-server advisory fix (Phase 7)
  isolated from the full axum/tower/tower-http migration (Phase 8) so a router regression
  in the latter cannot block the security fix in the former.
- **Chose:** kept 5 PRs as planned rather than collapsing C+D into one PR with two
  commits (the plan's offered alternative for "too many PRs for a solo maintainer").
- **Alternatives:** collapse C+D — the plan states this is a defensible alternative that
  preserves the revert boundary at the commit level while halving review overhead; not
  taken because the plan's own stated default already has the stronger reasoning (a
  reviewer can merge/revert C independently of whether D is ready) and nothing in this
  run's context indicated the maintainer finds 5 PRs burdensome.
- **Blast radius if wrong:** trivial — purely a review-ergonomics preference, no code
  difference either way; reversing the decision later just means opening D against C's
  merged `main` state instead of C's branch, or squashing two already-merged PRs'
  history, neither of which is costly.
- **Status:** CONFIRMED (2026-08-29)

---

## `reg3-anchors` Phase 4 — make `acdp_version: "0.5.0"` reachable — logged during
`/implement` 2026-08-29

Plan: `plans/reg3-anchors.md`, Phase 4 (`"Make acdp_version: 0.5.0 reachable in the
capability ladder"`) — the plan's single flagged **one-way-door** item, routed to a
dedicated Fable verification pass per `/implement`'s stop-condition rule for
public-API-contract changes, mirroring how the prior wave routed OQ2 (the witness
0.4.0 rung).

- **Plan:** plans/reg3-anchors.md
- **Assumed:** without this phase, Phase 3's RFC-ACDP-0016 §10/§14 version gate is dead
  on arrival in production — the pre-existing ladder in `build_capabilities`
  (`crates/acdp-registry-server/src/main.rs`) topped out at `"0.4.0"`, so no
  configuration of the shipped binary could ever advertise `>= 0.5.0`, and every
  anchored publish would be rejected forever.
- **Chose:** option (a), implemented as the `max()`-over-per-feature-version-claims
  refactor the plan explicitly prefers over a literal unconditional `"0.5.0".into()`.
  `build_capabilities`'s four-rung ordered if/else ladder is replaced by
  `ladder_claims`/`ladder_rung_claim`/`acdp_version_claim`: each pre-existing rung keeps
  its own independent predicate and claim (witnesses configured → `"0.4.0"`;
  lifecycle/log/head-receipts configured → `"0.3.0"`; a configured receipt key alone →
  `"0.2.0"`; base floor → `"0.1.0"`), and a fifth, **unconditional** claim of `"0.5.0"`
  is added for `anchors` support (RFC-ACDP-0016 §10: "no new profile ... anchors is a
  body field, not a registry surface" — the accept/reject/store/serve handling runs on
  every publish regardless of config, so there is no admin-config gate to check and
  therefore no "claimed but unexercised" state to overclaim). Because the anchors claim
  is both unconditional and the largest value among all claims, it wins `max()` for
  every configuration: every reachable deployment of the shipped binary now advertises
  `acdp_version >= "0.5.0"`, including a completely bare one. This executes OQ2's own
  recorded follow-up (`DECISIONS.md`, 2026-08-29 entry for
  `plans/reg2-reg5-reg6-reg8-reg9-wave4.md`'s OQ2 — *"if a 5th acdp_version rung is
  ever added, consider replacing the ordered if/else ladder with an order-independent
  max() over per-feature version claims"*) rather than superseding OQ2's decision:
  OQ2's conditional 0.4.0-ahead-of-0.3.0 ordering is unchanged, just re-expressed as one
  candidate claim among several, still independently falsifiable (verified directly:
  `capabilities_acdp_version_ladder`'s four original assertions now target
  `ladder_rung_claim`, the pre-anchors max, so they stay green even though
  `build_capabilities` itself now always returns `"0.5.0"`; a fifth assertion proves
  the anchors claim is reachable through the full path; deleting the anchors claim from
  `acdp_version_claim`'s `max()` set was confirmed, by temporarily editing the code and
  re-running the suite, to turn only that fifth assertion red while the other four stay
  green).
- **Alternatives:**
  - (b) An explicit `[registry]` config opt-in (default `false`) that lifts the
    ceiling to 0.5.0. Preserves the pre-Phase-4 ladder's per-deployment signaling value
    and operator control over a publicly-observable wire claim, at the cost of one new
    config field, one `validate_config` line, and a "why does this knob exist" question
    in review. The plan calls this "the strongest alternative" — stronger than its own
    first draft credited — but it was not chosen because it is not what the plan's
    approach section ultimately prefers, and because RFC-ACDP-0016 §10 gives no
    principled admin-facing axis to gate the knob on (anchors handling is unconditional
    code; a config flag would just be ceremony wrapping a value that's true either way).
  - (c) Leave the ladder alone. Fully spec-conformant (§10 requires rejection below
    0.5.0, not that anyone advertise 0.5.0) and zero-risk, but anchors then never work
    on any real deployment, making Phases 2-3 and 5-7 of this plan inert in production.
    Rejected, but named so the cost of doing nothing is explicit.
- **Blast radius if wrong:** cheap to reverse in code — the whole change is one
  config-derived expression with no persisted state; deleting `ANCHORS_VERSION_CLAIM`'s
  use in `acdp_version_claim` is a one-commit revert back to the pre-Phase-4 ladder
  shape. It is **not** cheap to reverse in the world: `acdp_version` is a public,
  wire-visible advertisement that consumers read and change behavior on, and every
  reachable deployment's advertised version jumps to `"0.5.0"` the moment this ships —
  an advertised version that goes up and then back down is a worse signal to consumers
  than one that never moved. This is the concrete reason the phase is flagged
  one-way-door and routed to Fable rather than the default Opus verification pass.
- **Status:** CONFIRMED (2026-08-29) — see `DECISIONS.md` for the full Fable
  recommendation and human decision.

---

## `reg10-conformance-and-ci-hygiene` — logged during `/implement` 2026-08-31

### Pin durability over upstream's default ergonomics (Phase 1)
- **Plan:** plans/reg10-conformance-and-ci-hygiene.md
- **Assumed:** the human's decision on the `dtolnay/rust-toolchain` orphaned pin — prefer a
  SHA reachable from the default branch plus an explicit input, over a convenient-but-
  unreachable ref-selector SHA — is a *principle* that generalizes, not a one-off ruling on
  that single action.
- **Chose:** applied the same resolution to `taiki-e/install-action` without stopping to ask
  again. Pinned `1ed6d7be…  # v2.87.2` (`compare main...` → `identical` when measured for
  this entry; re-measured 2026-09-01 it reads `ahead 0, behind 14` — same conclusion, a true
  ancestor of `main`, which has simply advanced since) and passed
  `tool: cargo-llvm-cov` explicitly, replacing `ea647c55… # cargo-llvm-cov` (`ahead 1,
  behind 0` — not in `main`'s history; upstream's README calls hash-pinning tool tags
  "strongly discouraged" for exactly this reason).
- **Alternatives:** (a) stop and ask a second time — rejected, it is the identical tradeoff
  in the same phase, and re-asking spends the human's attention on a settled question;
  (b) keep the tool-tag pin and accept ~daily orphaning — rejected outright, it reproduces
  the defect this phase exists to remove; (c) revert install-action to `@cargo-llvm-cov`
  unpinned — rejected, abandons the phase's goal for one action.
- **Blast radius if wrong:** near zero. One workflow line plus a `with:` block; revert is a
  one-line change. If the explicit `tool:` were wrong the coverage job fails loudly at
  `cargo llvm-cov`, in CI, before merge.
- **Status:** CONFIRMED (2026-09-01) — see `DECISIONS.md`. The generalization itself was also ruled on; the standing bar is recorded there.

### Amended acceptance criterion 4 (Phase 1)
- **Plan:** plans/reg10-conformance-and-ci-hygiene.md
- **Assumed:** AC4 as written ("the coverage job's install-action pin still defaults
  `tool: cargo-llvm-cov`") encoded a *means*, not the *end*. Its intent is that the coverage
  job installs cargo-llvm-cov.
- **Chose:** amended AC4 to "the coverage job installs `cargo-llvm-cov`, via an explicitly
  passed `tool:` input on a `main`-reachable pin." The original letter is unsatisfiable on a
  durable pin, since the `default:` exists only in the generated tool-tag commit.
- **Alternatives:** hold AC4 literally and keep the orphan-prone pin — rejected; that would
  let a criterion written before the facts were known dictate a worse outcome.
- **Blast radius if wrong:** none beyond the item above; this is bookkeeping on the same
  change.
- **Status:** CONFIRMED (2026-09-01) — see `DECISIONS.md`.

### Memory `test` leg ships without an anti-vacuity guard (Phase 2)
- **Plan:** plans/reg10-conformance-and-ci-hygiene.md
- **Assumed:** the `cargo test (memory)` leg's value is that it links and runs the binary's
  harness under the memory cfg; the load-bearing half of #109's fix is the `clippy (memory)`
  leg's `--all-targets` compile, which cannot go vacuous.
- **Chose:** shipped the leg with no assertion on its own test count. Today it runs 40 tests
  (37 unit + 2 from `tests/anchors_uri_never_dereferenced.rs` + 1 from
  `tests/conformance_gate.rs`). `conformance.rs`, `http_integration.rs` and
  `metrics_integration.rs` each run 0, all being `#![cfg(feature = "storage-sqlite")]`;
  `pg_integration.rs` also runs 0, but because it is `#![cfg(feature = "storage-pg")]`
  (`tests/pg_integration.rs:20`). If someone later adds a `storage-sqlite` cfg gate to
  `anchors_uri_never_dereferenced.rs`, the leg silently drops to 38 with no signal
  (`conformance_gate.rs` is ungated and survives).
- **Alternatives:** (a) reuse `tests/conformance_gate.rs` by setting
  `ACDP_REQUIRE_CONFORMANCE` on the new memory step — rejected because it does not work:
  that guard asserts `cfg!(feature = "storage-sqlite")` is *on*
  (`tests/conformance_gate.rs:15`), so pointing it at the memory leg would make the leg
  fail, not guard it. A correct guard needs a new always-compiled test file asserting its
  own presence — a source change outside this phase's scope, which is CI plumbing only;
  (b) assert a hardcoded test count — rejected, it turns every legitimate new test into a
  CI failure.
- **Blast radius if wrong:** low and slow. The compile/lint coverage survives regardless; only
  the run-the-harness half could erode, and only via a future edit that adds a sqlite cfg gate
  to a currently-ungated test file.
- **Status:** CONFIRMED (2026-09-01) — see `DECISIONS.md`. The count guard stays unbuilt; a separate, larger gap (the leg exercises `MemoryStore` essentially zero times) was filed as its own issue (#137) rather than folded in here.

### `acdp-deps-bot` holds `workflows: write` in this repo (Phase 3)
- **Plan:** plans/reg10-conformance-and-ci-hygiene.md
- **Question:** `bump-spec.yml` delegates to `bump-spec-ref.yml@v1`, whose token-mint step
  requests `permission-workflows: write` (`bump-spec-ref.yml:59`) — required because the spec
  pin lives in `.github/workflows/ci.yml`, and GitHub blocks App pushes touching anything
  under `.github/workflows/` without that scope. Does the `acdp-deps-bot` installation
  actually grant it here?
- **Answer: yes — verified directly, not assumed.**
  `GET /orgs/agentcontextdistributionprotocol/installations` returns installation
  `145550409` (`app_slug: acdp-deps-bot`) with `repository_selection: "all"` and
  `permissions.workflows: "write"`. That is the org-wide install described at
  `acdp-ci/DELIVERY-STANDARD.md:250-273`, read from the API rather than taken from the doc.
  Confirmed empirically as well: the same `bump-spec-ref.yml@v1` already runs green in
  `acdp-rs` and has opened `app/acdp-deps-bot`-authored PRs whose only changed file is
  `.github/workflows/ci.yml` — #185, #182, #168, #167 on `deps/spec-*` branches. #168 is
  `deps/spec-417211f6a13a`, i.e. this workflow produced the adoption PR for the very spec
  pin this repo currently carries.
- **Correction:** this entry was first logged as UNCONFIRMED, on the reasoning that checking
  the installation needed admin credentials this session lacks. That was wrong — the
  installations endpoint answers it with ordinary `gh` auth. The Phase 3 verifier caught it;
  the claim above is the re-checked result.
- **Residual risk:** only that the org-wide grant is later narrowed. If it were, the run's
  first step ("Mint GitHub App token") would 422 and fail before `actions/checkout`, the
  rewrite, or any `git push` — nothing reaches `main` and no PR opens. The fix would be an
  org App-settings grant, not a code change here.
- **Status:** CONFIRMED (2026-08-31)
- **The REASON went stale (2026-09-14, U-536, PR #304); the answer did not.** The question above
  turns on the pin living in `.github/workflows/ci.yml`, which is why App pushes needed
  `workflows: write` at all. The pin now lives in `.spec-pin` at the repository root, so a spec
  bump PR touches a plain data file and no longer needs that scope for this repo's pin. Nothing
  breaks and nothing needs changing: `bump-spec-ref.yml`'s token-mint step still REQUESTS
  `permission-workflows: write` (it is shared with repos whose pin is still in a workflow), and
  the `acdp-deps-bot` installation still grants it — verified unchanged. Recorded because the
  entry reads as a live dependency on a file layout that no longer holds, and the next person
  to narrow that scope would look here to decide whether it is safe. It now is, for this repo.

### `conformance (spec fixtures)` should join the required branch-protection contexts (Phase 11)
- **Plan:** plans/reg10-conformance-and-ci-hygiene.md
- **Assumed:** the plan's Phase 11 acceptance criteria require recording an explicit
  decision on whether `conformance (spec fixtures)` joins `rustfmt`/`clippy`/`tests` as a
  required status-check context, with the plan itself recommending "yes," and explicitly
  forbidding the executor from changing branch protection directly (repo-admin action,
  out of this diff's scope).
- **Verified, not assumed:** read this repo's actual branch protection via
  `gh api repos/agentcontextdistributionprotocol/acdp-registry-rs/branches/main/protection`
  — `required_status_checks.contexts` is exactly `["rustfmt", "clippy", "tests"]`.
  `conformance (spec fixtures)` is confirmed NOT currently required, read-only, no change
  made.
- **Decision recorded (adopting the plan's recommendation):** `conformance (spec
  fixtures)` SHOULD be added to the required contexts. Reasoning: Phase 11's coverage
  ratchet now has two halves — a spec-independent half (the new
  `known_families_partition_into_covered_excused_or_deferred` set-equality test and
  `covered_direct_families_have_present_test_functions`'s source-presence scan, both in
  the required `tests` job) and a spec-dependent half (the `Replayed`-mechanism assertion
  inside `replays_spec_fixtures_when_present`, which needs `ACDP_SPEC_DIR` and lives only
  in the advisory `conformance` job). Leaving `conformance` advisory means a regression
  that silently drops a `COVERED`-`Replayed` family's exchanges (e.g. `pub`/`ret`/`vis`)
  while `COVERED` itself and any `Direct` tests stay textually intact would pass every
  required check and merge clean — exactly the failure mode this whole phase exists to
  close, just moved one layer down.
- **Not executed:** changing `required_status_checks.contexts` is a repo-admin branch
  protection change, explicitly out of scope for this diff per the phase brief. Recorded
  here as the decision + reasoning; the actual settings change is a follow-up for a human
  with admin access on this repository.
- **Blast radius if wrong:** low. If a human disagrees and leaves `conformance` advisory,
  nothing in this diff breaks — the ratchet still gains real teeth in the required `tests`
  job via the two unconditional tests; only the `Replayed`-mechanism half stays
  advisory-only, the same gap that exists today for the whole ratchet.
- **Status:** CONFIRMED (2026-09-01) — actioned by a repo admin and recorded in the "Executed" bullet below; independently re-verified 2026-09-13 (U-507).
- **Executed (2026-09-01):** a repo admin actioned the recorded recommendation.
  Re-verified via the same read-only call,
  `gh api repos/agentcontextdistributionprotocol/acdp-registry-rs/branches/main/protection`
  — `required_status_checks.contexts` is now exactly `["rustfmt", "clippy", "tests",
  "conformance (spec fixtures)"]`. All other protection settings were left unchanged:
  `strict: true`, `enforce_admins: false`, `allow_force_pushes: false`,
  `allow_deletions: false`, `required_linear_history: false`.
- **Status (updated 2026-09-01):** CONFIRMED — the branch-protection change recommended
  above has been made, flipping the original UNCONFIRMED status above.

### First-party reusable workflows are trusted by mutable major tag (Phase 3)
- **Plan:** plans/reg10-conformance-and-ci-hygiene.md
- **Logged retroactively 2026-09-01.** This assumption was made during Phase 3 but never
  written down at the time — it surfaced during `/reconcile`'s review of the phase, not
  from the log. Recording it here so the decision is durable rather than transcript-only.
- **Assumed:** Phase 1's SHA-pinning mandate (#111) scopes to *third-party* actions, and
  first-party `agentcontextdistributionprotocol/*` reusable workflows are legitimately
  trusted by major tag.
- **Chose:** `bump-spec.yml:18` references
  `agentcontextdistributionprotocol/acdp-ci/.github/workflows/bump-spec-ref.yml@v1` with
  `secrets: inherit`, matching the two first-party refs already in this repo
  (`auto-merge.yml:8`, `bump-acdp.yml:13`). A related convention is stated upstream in
  `acdp-ci/.github/workflows/auto-merge.yml:10-11` — "third-party actions are SHA-pinned
  (matching acdp-rs); first-party actions/* are trusted by major tag" — but read it
  narrowly: the literal token is `actions/*`, GitHub's own namespace, so the sentence does
  *not* itself sanction trusting `agentcontextdistributionprotocol/*` reusable workflows by
  major tag. The support for those is the three `@v1` refs already in this repo, which is
  precedent, not justification. Do not cite this line as though it settled the question.
- **Alternatives:** SHA-pin the reusable-workflow ref — rejected as only partly effective:
  the callee itself consumes `actions/checkout@v7` and `actions/create-github-app-token@v3`
  (those two and no others), so pinning the outer hop moves the mutable edge inward
  rather than removing it, while costing a commit here per upstream fix across ~10 sibling
  repos with no bump automation for it.
- **Blast radius if wrong:** high in principle. `secrets: inherit` exposes the org
  `ACDP_BOT_APP_ID`/`ACDP_BOT_PRIVATE_KEY` and this repo's `CARGO_REGISTRY_TOKEN` to
  whatever `v1` resolves to at run time. Mitigating: `bump-spec.yml` triggers only on
  `repository_dispatch`/`workflow_dispatch`, so it is not fork-reachable; and since
  2026-08-30 an active `protect-v-tags` ruleset on `acdp-ci` (id `21899019`) blocks
  `creation`/`update`/`deletion`/`non_fast_forward` on `refs/tags/v*`, admin-bypass only.
  The residual actor who can move `v1` is the same sole org admin who can already push
  directly to this repo.
- **Observed, not actioned:** `auto-merge.yml` carries the same `@v1` on an `on:
  pull_request` trigger — far more reachable — but has no `secrets: inherit`, so it
  receives only the job's `contents: write`/`pull-requests: write` token, which GitHub
  further downgrades to read-only for fork PRs. Different risk shape, not strictly worse;
  reviewed and deliberately left as-is.
- **Status:** CONFIRMED (2026-09-01) — see `DECISIONS.md`.

## REG-11 Phase 1 — extending #136's fix-forward past the planned two-file scope
- **Plan:** `plans/backlog-reg11.md` (Phase 1)
- **Assumed:** the plan's guard — *"If the two fixes surface a third breakage, split #136
  rather than growing this phase"* — was aimed at an **unrelated** crate breaking, not at
  the same two API migrations appearing at more call sites.
- **Chose:** extend the fix-forward to the three remaining sites rather than splitting the
  PR. The CI log this plan was written from under-reported the blast radius: it showed
  `rand` 0.8→0.10 and `hmac` 0.12→0.13 breaking two files, but the same two migrations also
  hit `crates/acdp-registry-server/src/main.rs:686,688` (identical `RngCore`/`thread_rng`
  pattern), `crates/acdp-registry-auth/src/jwt.rs:347` and
  `crates/acdp-registry-server/tests/http_integration.rs:362` (both `rand::rngs::OsRng`,
  renamed to `SysRng` in rand 0.10).
  Applied the three-part bar for extending a prior ruling:
  1. **Reason applies unchanged?** Partly. The stated reason was "the breakage is narrow and
     the compiler names the exact fix" — still true (same two migrations, mechanical, each
     confirmed against vendored upstream sources). The literal "2 files" premise is falsified.
  2. **Same unit of work?** Yes — one dependabot PR, one dependency bump.
  3. **Bounded and fails loudly?** Yes — a wrong edit fails the build in CI, visibly.
- **Alternatives:** split `rand` out of the grouped PR. Rejected because the `rand` bump
  cannot land at all until every `rand` call site is migrated, so splitting does not reduce
  the work — it only fights `.github/dependabot.yml`'s deliberate `major-updates` grouping
  and would require hand-editing `Cargo.toml`/`Cargo.lock` to exclude one member of a group.
- **Blast radius if wrong:** bounded and cheap. Worst case CI stays red on a dependabot
  branch that was already red; nothing reaches `main`. Reversal is `git revert` on an
  unmerged branch.
- **Status:** CONFIRMED

## REG-11 Phase 1 — #136's two non-mechanical bumps (serial_test MSRV, jsonwebtoken crypto provider)
- **Plan:** `plans/backlog-reg11.md` (Phase 1)
- **Assumed:** the plan's premise that only 2 of #136's 12 bumps were implicated. **False.**
  Four are: `rand` and `hmac` (mechanical, fixed in `5d0eda1`/`2d263ab`), plus two that
  needed decisions rather than fixes. The plan recorded the "other ten compile clean" claim
  as *inferred from CI log absence, not measured* — that caveat was correct and this is
  exactly what it was hedging against. The first CI run only surfaced the errors that
  aborted the build earliest.
- **Chose:** both decided by the user on 2026-09-05, not assumed:
  1. **`serial_test`: hold at 3.x.** It bumps to 4.0.1 which requires rustc 1.93.1, against a
     declared `rust-version = "1.88"` (`Cargo.toml:17`) with a dedicated `msrv (1.88)` CI job.
     It is a **dev-dependency only** (`crates/acdp-registry-server/Cargo.toml:65`) and never
     ships in the binary, so letting it raise the MSRV floor for every downstream consumer of
     these 8 published crates would be backwards.
  2. **`jsonwebtoken` 11: enable the `rust_crypto` feature.** v11 compiles but panics at
     runtime ("Could not automatically determine the process-level CryptoProvider"),
     requiring exactly one of `rust_crypto` / `aws_lc_rs`. Chose `rust_crypto` for
     consistency with the existing pure-Rust stack (`ed25519-dalek`, `sha2`, `hmac` are all
     RustCrypto) and to avoid adding a C/assembly build dependency that would complicate the
     multi-stage Docker build and cross-compilation.
- **Alternatives:** raise MSRV to 1.93.1 (rejected — a test-only crate should not dictate the
  consumer compatibility contract); `aws_lc_rs` (rejected — faster and FIPS-adjacent, but
  diverges from the pure-Rust stack and adds a toolchain dependency); close #136 and split
  per-crate (rejected — discards the completed rand/hmac migration and fights the deliberate
  `major-updates` grouping in `.github/dependabot.yml`).
- **Blast radius if wrong:** `serial_test` — none at runtime; worst case a future test-only
  API is unavailable until MSRV rises for an independent reason. `jsonwebtoken` — this is the
  JWT signing/verification backend for the auth path, so a wrong provider choice is a
  correctness-and-performance issue, though not a disclosure one; reversible by flipping one
  Cargo feature.
- **Status:** CONFIRMED (2026-09-05) — both options presented to the user with tradeoffs; the
  recommended option was chosen in each case.

## REG-11 Phase 1 — RUSTSEC-2023-0071 (`rsa` Marvin attack) suppressed in `deny.toml`
- **Plan:** `plans/backlog-reg11.md` (Phase 1)
- **Assumed:** nothing — this was escalated to a Fable subagent at the user's explicit
  request and decided on verified evidence.
- **Chose:** keep `jsonwebtoken`'s `rust_crypto` feature and add an advisory ignore.
  **CORRECTION (2026-09-06):** this was first written, and committed in `fde8d94`'s message,
  as "the repository's first advisories.ignore entry". **That is false.** `RUSTSEC-2025-0134`
  (rustls-pemfile) was an `ignore` entry until PR #97 (`1cc4f27`) deleted it when
  `axum-server` 0.8 dropped the dependency — see `git show 1cc4f27^:deny.toml` and the
  repo's own narration at `CHANGELOG.md:1148-1152`. This is the **second** such entry, and
  the precedent it sets is a good one: the prior entry was *removed when it became
  unnecessary* rather than left to rot. The claim in `fde8d94`'s commit body cannot be
  edited (it is squash-merged history); this entry is the correction of record. Justified on two independent grounds, each sufficient alone:
  1. **Unreachable, enforced by library dispatch order.** Every verification pins exactly
     one algorithm — `Validation::new(self.material.algorithm())`
     (`crates/acdp-registry-auth/src/jwt.rs:224`), which is only `HS256` or `EdDSA`
     (`:88-93`). jsonwebtoken 11 rejects a header `alg` outside `validation.algorithms`
     with `InvalidAlgorithm` at `decoding.rs:278-280` — **before** constructing any crypto
     verifier at `:282`. An attacker sending `alg: RS256` executes zero `rsa` instructions.
     Independently re-verified: no permissive algorithms list exists anywhere in the
     workspace, `jsonwebtoken` is imported in exactly one file, and there is no
     `jsonwebtoken::jwk` / RSA-key-import path. The DID challenge path whitelists
     `ed25519`/`ecdsa-p256` by name before key resolution (`service.rs:150`).
  2. **Inapplicable even if reached.** Marvin recovers an RSA **private** key via timing of
     private-key operations. This registry holds no RSA private key material at all.
     There is nothing to recover.
  Also established: the registry only ever verifies JWTs it minted itself (`iss` pinned to
  `self.issuer`, verified with its own key), and `jsonwebtoken` **9** uses `ring`, not the
  `rsa` crate — so holding at 9 would not have been "equally exposed"; the bump does add the
  edge, but the edge is inert.
- **Alternatives:** `aws_lc_rs` (rejected — legitimate, and adds no new toolchain since
  `aws-lc-sys` is already present via rustls, but it would put a second Ed25519
  **implementation** on the trust boundary alongside `ed25519-dalek`.
  **Qualification (2026-09-06):** `rust_crypto` does not avoid a dual-*version* graph —
  `cargo tree` on `fde8d94` shows `ed25519-dalek` **2.2.0** (via `jsonwebtoken 11`, the
  EdDSA JWT verify path) coexisting with **3.0.0** (via `acdp-crypto 0.8.5` and this repo's
  `decode_ed25519_pem_to_public`). The argument holds for distinct *implementations* — same
  crate lineage rather than two unrelated codebases — but it is weaker than first stated
  and should not be read as "one Ed25519 everywhere"); hold `jsonwebtoken` at 9 (rejected —
  merely defers a bump on a moving crate line).
- **Blast radius if wrong:** the suppression hides a real, unpatched timing side channel in
  a crate that ships in the binary. It is safe **only while** the reachability argument
  holds, so the `deny.toml` entry carries an explicit re-review trigger list. The most
  fragile item: the argument depends on jsonwebtoken's internal dispatch order, so any
  future major bump of that crate must re-read `decode()`.
- **Status:** CONFIRMED (2026-09-06) — user asked for Fable to take the call and stated a
  preference for `rust_crypto`; Fable tested that preference rather than deferring to it and
  independently reached the same answer. Crux claims re-verified against source.

## `secure_compare::ct_eq` visibility: `pub` rather than `pub(crate)`

- **Plan:** `plans/u003-metrics-ct-eq.md` (U-003 — landing PR #171, #168)
- **Assumed:** exporting `ct_eq` as `pub` from `acdp-registry-core` — rather than
  `pub(crate)` — is acceptable, even though both of today's callers
  (`handlers::admin::require_admin_bearer` and `metrics::metrics_endpoint`) are inside the
  crate and `pub(crate)` would therefore compile.
- **Chose:** kept `pub`, exactly as PR #171 wrote it. This unit is a **landing job** for a
  branch that was already written and reviewed; narrowing the visibility would be a
  gratuitous divergence from the reviewed diff, and would have broken the property that
  makes this landing verifiable — that the merge commit's tree hash
  (`cde552125d3c3e3be2b05c1c31b82afbce8226f4`) is bit-identical to the pre-merge trial, so
  nothing was smuggled in under cover of a "small tidy-up".
- **Alternatives:** `pub(crate)` (rejected for this unit — correct-looking, but it is a
  rewrite of reviewed code for no behavioural gain, and it can be done at any time);
  re-exporting at the crate root as `acdp_registry_core::ct_eq` (rejected — widens the
  surface further, and the module path already reads well at both call sites).
- **Blast radius if wrong:** minimal and cheaply reversible. `acdp-registry-core` is an
  internal workspace crate with no external consumers, so this is not a published API
  commitment; narrowing to `pub(crate)` later is a one-line mechanical change that the
  compiler fully verifies. Nothing is foreclosed.
- **Status:** CHANGED -> CONFIRMED (2026-09-10). Reconciled to `pub(crate)`; see `DECISIONS.md` entry 5. The kept-`pub` rationale did not survive analysis.

## `predecessor_admission` enforcement: store-level coverage, not end-to-end wiring

- **Plan:** `plans/u-001-acdp-0.10.0-predecessor-admission.md` (U-001)
- **Assumed:** proving that each store *invokes* the admission closure and propagates its
  `Err` is the right scope for this unit, and that no test here needs to prove the closure is
  actually threaded in on a real HTTP publish.
- **Chose:** store-level tests only. Both regression suites call `commit_publish` directly
  with a hand-built closure. The `Some(..)` construction lives entirely in upstream
  `RegistryServer::commit_via_store` (`acdp-server-0.10.0/src/registry/server.rs:716-727`),
  which this repo does not own and cannot meaningfully re-test; and the conformance fixtures
  do not cover the RFC-ACDP-0014 §4 reject path at all (spec issue #57, upstream).
- **Alternatives:** an end-to-end HTTP test superseding a key-revocation context. Rejected
  for this unit: it would live in `crates/acdp-registry-server/tests/conformance.rs`, which
  is held by U-004, and it would be testing upstream's wiring rather than ours.
- **Blast radius if wrong:** if upstream ever stopped passing `Some(..)`, our stores would
  silently never be asked to enforce, and nothing in this repo would notice. Low likelihood
  (upstream has its own tests), but the residual is real and is recorded here rather than
  left implicit.
- **Status:** UNCONFIRMED

## Corrupt predecessor `body_json` is reported as `RegistryInternal`, not `SchemaViolation`

- **Plan:** `plans/u-001-acdp-0.10.0-predecessor-admission.md` (U-001)
- **Assumed:** a predecessor row whose `body_json` will not deserialize is a registry-side
  integrity fault, not a malformed client request.
- **Chose:** `AcdpError::RegistryInternal(format!("decode body: {e}"))`, exact parity with
  how `row_to_context` already reports the same failure in both stores (pg `:1469-1470`,
  sqlite `:1545-1546`).
- **Alternatives:** `SchemaViolation`, which is non-transient and would avoid the wart below.
  Rejected: it blames the producer for the registry's own bad data and diverges from every
  other decode failure in these stores.
- **Blast radius if wrong:** `RegistryInternal` reports `is_transient() == true`
  (`acdp-primitives-0.10.0/src/error.rs:297-306`), so a *permanently* corrupt predecessor row
  is advertised to producers as retryable and invites a retry loop. The wart is pre-existing
  and repo-wide (every `row_to_context` decode has it); fixing it is its own unit, not
  something to smuggle into a dependency bump. Reversible in one line.

## `WEBHOOK_SCHEMA_VERSION` stays `"1.0"` across the `event_id` wire rename

- **Plan:** `plans/u-002-webhook-duplicate-event-id.md`
- **Assumed:** renaming the `context.retracted` / `context.republished` lifecycle id from
  `event_id` to `lifecycle_event_id` does not warrant bumping the envelope's
  `WEBHOOK_SCHEMA_VERSION`.
- **Chose:** keep `"1.0"` and instead narrow the constant's doc comment
  (`crates/acdp-registry-webhook/src/lib.rs:20-46`), which previously promised a bump on
  "any backwards-incompatible change to the serialized event shape".
- **Rationale CORRECTED at reconcile (2026-09-10).** The original argument — "only two of
  five variants changed, so bumping misreports the other three" — **does not hold**, and is
  recorded here so no one cites it again. Both downstream consumers parse all five event
  types through **one open union** (`acdp-control-plane` `AcdpWebhookEvent`
  `src/contracts/acdp.ts:29-102`; `acdp-playground` `WebhookType_Open`
  `acdp_client/models.py:255-288`). "Only some variants changed" is a distinction that exists
  in this repo's Rust enum and in no receiver's model. The decision survives on a different
  and sounder argument: `schema_version` is stamped **per delivery**, so it describes that
  delivery's envelope, not the stream — a `search_executed` body carrying `"1.1"` would
  assert that something about *that delivery* changed when nothing did. Bumping is not a
  blast-radius trade-off, it is emitting a falsehood on every delivery — the envelope, which
  is what the value describes, changed for none of them. Independently
  corroborated by **RFC-ACDP-0009 §2.10**, which reserves this profile's version field as the
  schema version of the event *envelope* — the narrowed scope matches what the spec already
  reserved, rather than being a carve-out invented to fit this case.
- **Also corrected:** the plan said "the only known consumer". There are **two** —
  `acdp-playground/acdp_client/models.py:282` also declares `schema_version`. Neither reads
  it (verified by workspace-wide grep across all sibling repos), so the conclusion is
  unchanged, but the claim as written was wrong.
- **Alternatives:** bump to `"2.0"` (rejected — punishes the unchanged majority); bump to
  `"1.1"` (rejected — identical blast radius for no additional signal); leave the doc comment
  as-is and not bump (rejected — ships code contradicting its own stated contract).
- **Blast radius if wrong:** a receiver that wanted to branch on the version to detect this
  rename cannot. Recovery is a one-line bump in a later release; nothing persists or migrates.
  The constant is read at exactly one site (`lib.rs`, envelope construction) and pinned by one
  test assertion.
- **Status:** CONFIRMED (2026-09-10) — decided by Opus at `/reconcile`, decision unchanged,
  rationale corrected as above. See `DECISIONS.md`.

## `#[serde(rename)]` is symmetric, and that is deliberate

- **Plan:** `plans/u-002-webhook-duplicate-event-id.md`
- **Assumed:** changing the field's name for `Deserialize` as well as `Serialize` is safe.
- **Chose:** the symmetric `#[serde(rename = "lifecycle_event_id")]` rather than the
  asymmetric `#[serde(rename(serialize = ...))]`. `WebhookEvent` does derive `Deserialize`
  (`crates/acdp-registry-types/src/event.rs:8`), but nothing in this workspace deserialises it
  — verified by grep across all crates. Asymmetry would leave the type able to *read* a key it
  will never *write*, which is exactly the read/write skew that rots a wire format.
- **Alternatives:** asymmetric rename (rejected, above); adding `#[serde(alias = "event_id")]`
  for read compat (rejected — buys nothing in-repo, and would reintroduce the very ambiguity
  being removed if the envelope ever gains a `Deserialize`).
- **Blast radius if wrong:** an out-of-tree deserialiser of this type reading the old key
  breaks. None is known; the one known consumer parses structurally and tolerates unknown keys
  via `[k: string]: unknown` (`acdp-control-plane/src/contracts/acdp.ts:102`). The crate is
  **not published** (`release-plz.toml:5` `publish = false`; absent from crates.io) and no
  Rust consumer exists in any sibling repo, so reversal is a two-word edit.
- **Reconcile strengthened the reasoning (2026-09-10).** Symmetric is not merely safe, it is
  the **only correct form of the three**, and the other two fail *quietly*:
  `rename(serialize = ...)` would read `event_id` — a key still on the wire, holding the
  *envelope delivery id* — into the lifecycle field: no error, wrong value, which is exactly
  the confusion #179 removes, resurrected on the read side. `alias = "event_id"` maps both
  names to one field slot, so serde rejects **every current body** with a duplicate-field
  error, and rejects old duplicate-key payloads too. On an old payload the shipped form fails
  loudly with `missing field lifecycle_event_id`, which is the right outcome — and, with
  `schema_version` deliberately frozen, is the only automatic detector an old payload has.
- **Gap closed:** nothing asserted the read side, so this entry rested on untested behaviour.
  `the_emitted_body_round_trips_into_the_lifecycle_field` now deserialises a real emitted body
  and asserts the lifecycle field gets the actor-minted id and **not** the envelope id.
- **Status:** CONFIRMED (2026-09-10) — decided by Opus at `/reconcile`, with a test added so
  the read path is a guarantee rather than an argument. See `DECISIONS.md`.

## `acdp-control-plane` needs telling; this lane must not edit it

- **Plan:** `plans/u-002-webhook-duplicate-event-id.md`
- **Assumed:** the downstream control plane should learn that `lifecycle_event_id` now exists
  and that its own note at `acdp-control-plane/src/contracts/acdp.ts:81-85` — which documents
  the old last-wins behaviour as a deliberate dedup fallback — goes stale on merge.
- **Chose:** file a **GitHub issue** in `agentcontextdistributionprotocol/acdp-control-plane`
  from `/ship` after merge, so it can cite the merged PR. **No edit to that repo's files.**
  Authorised by the user's standing instruction for this workspace ("only this repo changes;
  for cross-repo changes file GitHub issues and ping that repo's Claude session"), not by this
  plan — CHARTER rule 6 forbids cross-repo *edits* outright and a plan may not self-exempt.
- **Alternatives:** edit `acdp.ts` directly (rejected — forbidden); say nothing (rejected —
  leaves a first-party repo carrying a comment that is now false).
- **Blast radius CORRECTED at reconcile (2026-09-10).** "The fallback keeps working either
  way" was **too strong**. It holds for delivery retries — its documented purpose. It stops
  collapsing *distinct deliveries of the same logical lifecycle event*: the SDK's lifecycle
  commit has an idempotent-replay outcome, and this registry emits the webhook
  unconditionally, so a producer resubmitting a byte-identical signed event yields a second
  delivery with a new envelope id but the same actor-minted id. **With the header present
  (the normal path) nothing changes.** With the header stripped, the old fallback keyed on the
  actor-minted id and collapsed the pair; the new one keys on the delivery id and does not —
  costing a duplicate ingest row, a double SSE emit and a double outbound webhook. The
  lifecycle projection stays correct (its upsert guard makes re-applying a transition a no-op).
  This is the fallback becoming *consistent with* the primary path rather than accidentally
  stronger than it — defensible, but it must be stated in the issue rather than found by
  their on-call.
- **Authorization CONFIRMED, and the earlier challenge was half right.** Filing an issue *is*
  a cross-repo write. But CHARTER rule 6 is a **routing** rule — it says such writes go to the
  human and that *the leader cannot authorize them*; it does not say the permission cannot
  exist. The human already granted exactly this mechanism in advance ("for cross-repo changes
  file GitHub issues and ping that repo's Claude session"), so rule 6 is **satisfied, not
  bypassed**. The reviewer was right that the earlier prose asserted the exemption without
  naming its source; that wording has been corrected to cite the source.
- **Two corrections to the plan:** (a) the standing instruction has two halves and the second
  was dropped — **ping that repo's Claude session**, not just file the issue; (b) the issue
  must **not** say the control plane "can key on `lifecycle_event_id`". Read as dedup advice
  that would put their body fallback back into disagreement with `X-ACDP-Event-Id`, re-creating
  the divergence #179 removes. `lifecycle_event_id` is actor provenance, **not** a dedup key.
- **Timing:** after merge and **only** after merge. A pre-merge issue would tell another team a
  wire format changed when it has not. If the PR never merges, nothing is owed and nothing is
  filed.
- **Status:** CONFIRMED (2026-09-10) — decided by Opus at `/reconcile`; disposition and
  authorization stand, content and blast radius corrected. Issue owed **on merge**. See
  `DECISIONS.md`.

## U-005 — operator-facing docs sweep (`#180`, lane-1, 2026-09-10)

- **Assumed then verified:** that `crates/acdp-registry-server/src/main.rs` is the sole
  enforcement point for both documented claims. Confirmed — exactly one `changeme` check
  exists tree-wide, and the playground/receipt interaction is two adjacent bails in one
  block. **Status: CONFIRMED.**
- **Corrected mid-unit, twice, both caught before shipping:**
  (a) the plan asserted a *single* playground guard; there are **two** (`main.rs:259`,
  `:267`), and the second — `pinned_only = true` with an empty `pinned_keys` — would have
  made the corrected documentation strand an operator at startup. Issue `#180` had stated
  this correctly and the plan dropped it.
  (b) the plan asserted "Railway deployments do enable auth" as the basis for ranking
  `docker/RAILWAY.md:45` the *least* severe site. **False** — no `AUTH__ENABLED` exists
  anywhere in `docker/` or `.github/`, `AuthConfig::default()` is `enabled: false`, and no
  config file is mounted on Railway. It is the **most** severe site. **Status: CONFIRMED
  (corrected).**
- **Deliberately bounded, not assumed away:** with auth disabled every caller is anonymous
  (`handlers/context.rs:1354-1358`) and `/auth/*` is not mounted (`core/src/lib.rs:42`), but
  publishes remain bound to DID-signature verification. This unit therefore does **not**
  claim unauthenticated publish is possible. Overstating it would have made the finding
  easier to dismiss. **Status: CONFIRMED.**
- **UNCONFIRMED — awaiting human ruling; re-checked 2026-09-13 (U-507) and still open. **Settled by:** the R3 ruling on whether the Railway recipe enables auth. **Owner:** the human — not the leader, which holds no authority over a product recipe decision.** The question is whether `docker/RAILWAY.md` should require
  `ACDP_REGISTRY_AUTH__ENABLED = true`. Raised as `blocked`, forwarded by the leader, not
  acted on. The documentation of the gap ships regardless; only the recipe change waits.
- **Not re-litigated:** `auth.enabled = false` in the compose stack stays (leader-confirmed;
  the demo must boot). Renaming the `changeme` placeholder was rejected — the guard already
  matches case-insensitively after trimming, so the literal was never the fragile part.

## `cur-002`'s message-leak rationale is documented, not satisfied

- **Plan:** `plans/u004-conformance-cur-rcpt-lhr-log.md` (U-004 — conformance #130)
- **Assumed:** satisfying `cur-002`'s machine-checkable `expected` block (`error_code`,
  `http_status`, `content_type`, `outcome`) is sufficient coverage for that vector, even
  though its prose `rationale` additionally says a registry "MUST NOT leak why a cursor
  failed to parse beyond the registered code".
- **Chose:** assert the `expected` block only, and state the gap explicitly in the test's doc
  comment. This registry answers `{"error":{"code":"invalid_cursor","message":"invalid
  cursor: cursor is not valid base64"}}` — the message names the parse reason. It echoes no
  caller input and exposes no registry state, so it is not an information-disclosure defect in
  any practical sense, but it is not literally what the rationale asks for either.
- **Alternatives:** change the message to a bare `"invalid cursor"` (rejected — that is a
  `src/` edit, and `crates/acdp-registry-types/src/error.rs` is outside this unit's granted
  paths; widening scope to satisfy prose would be exactly the kind of quiet scope creep the
  lane claims exist to prevent); assert the message text and fail (rejected — that would make
  the ratchet red for a defect this unit is not authorised to fix, blocking a legitimate
  coverage gain); say nothing (rejected — an undocumented known gap is how a false coverage
  claim survives).
- **Blast radius if wrong:** low and bounded. If the leak matters, the fix is a one-line
  message change in `error.rs` plus tightening this test's assertion; nothing built on top of
  the current behaviour would need to change. The risk of the current choice is only that the
  gap is forgotten — which this entry exists to prevent.
- **Status:** CONFIRMED (2026-09-10) — and on stronger grounds than this entry claims: the leak clause has no RFC backing and `rationale` is corpus-wide never-asserted, so it is not a tolerated gap. This entry's body is WRONG about where the message literals live (they are in the two store crates, not `error.rs`) and examined only one of seven arms. Corrections and the applied tripwire: see `DECISIONS.md` entry 10.
- **Status:** RETIRED — OBSOLETE (2026-09-10, W2-U2 / #187). The assumption no longer
  describes the code. `#187` lifted the codec into `acdp-registry-store::cursor` and
  collapsed every parse-failure arm to one payload, so the wire message is now exactly
  `{"error":{"code":"invalid_cursor","message":"invalid cursor: malformed"}}` — it names no
  parse step, and `cur-002`'s rationale is satisfied rather than documented-as-unsatisfied.
  The tripwire this entry was paired with was observed going red on that exact body and has
  been rewritten into a positive assertion of the absence; the two doc paragraphs that
  described the gap are rewritten in the same commit. Retired here rather than struck: the
  entry is the record of why the gap was tolerated for one unit, and deleting it would erase
  the reasoning that `#187` acted on. Its two factual errors stand corrected above and in
  `DECISIONS.md` entry 10 — note the arm count in that correction is itself superseded: the
  tree had EIGHT arms per store, one of which (`cursor missing mint`) was unreachable and is
  deleted, not collapsed. See `DECISIONS.md` entry 11.

## W2-U1 — #185 pinned-keys guard hoist (lane-1, 2026-09-10)

- **Verified, not assumed:** that hoisting the guard has zero collateral. Two independent
  grounds, both checked: `acdp-registry-server` is bin-only (`[[bin]]`, no `[lib]`, no
  `build.rs` in the repo, no `CARGO_BIN_EXE_`/`#[path]`/`include!` in its tests), so
  `validate_config` is unreachable from `tests/**`; and every `pinned_only = true` site in the
  repo already supplies at least one pinned key. **Status: CONFIRMED.**
- **Corrected during plan review, both before any code was written:**
  (a) the draft bail message said "accepts any signature from **any agent**" — false, `did:key`
  routes to a verified path before the playground gate. Same nuance a review caught in U-005.
  (b) the draft justified leaving runtime semantics alone on the state being "unreachable
  through the real binary" — false, the admin reload endpoint validates nothing.
  **Status: CONFIRMED (corrected).**
- **RESOLVED (U-507, 2026-09-13) — the placement call was made, in `acdp-registry-core`:** whether the shared playground validation
  that #192 and #193 both need should live in `acdp-registry-types` or `acdp-registry-core`.
  Both issues suggest a shared validator; the placement call belongs to whoever takes them,
  with the whole surface in view. Not blocking this unit.
- **Known residual gaps, filed rather than fixed** (both need `acdp-registry-core`, outside
  this unit's grant): **#192** — `POST /admin/pinned-keys/reload` applies config with no
  validation, so this guard and every other config guard is bypassable at runtime. **#193** —
  pinned-key entries are never validated at startup, so a typo'd `algorithm` or an all-expired
  list boots clean and then fails every publish. #193 is notable: an all-expired list is
  *literally* the "reject every publish outright" state #185's old message misnamed, so the
  message described a real failure mode attached to the wrong config.
- **Not re-litigated:** the behaviour change itself (leader-ruled lane-decidable — rejects a
  self-contradictory config, no shipped default affected).

## W2-U3 — release-plz un-stall, docker tag pipeline, Railway auth posture

- **Plan:** `plans/w2-u3-release-ci-plumbing.md`

### `git_only = true` is the right fix, and `git_tag_name` is the right escape from the dead baseline
- **Assumed:** release-plz resolves the previous release from the cargo registry even under
  `publish = false`, so with nothing on crates.io every crate reads as never-released and the
  existing tag then blocks the proposed version.
- **Chose:** `git_only = true` plus `git_tag_name = "{{ package }}/v{{ version }}"`, retiring the
  eight dead `*-v0.1.0` tags by making them stop matching rather than deleting them.
- **Alternatives:** `publish = true` (rejected outright — a one-way door that would publish eight
  crates to crates.io); deleting the old tags (rejected — orphans eight published GitHub
  Releases); bumping the workspace version by hand (out of scope, and does not work: `git_only`
  still resolves the June tag and still dies in `cargo package`).
- **Evidence:** reproduced red/green/forward locally on release-plz 0.3.160 in throwaway clones.
  DEBUG confirms `Processing 8 packages in git_only mode` and `0 tags matched pattern` with zero
  crates.io fetches.
- **Blast radius if wrong:** releases stay stalled; no data or wire effect. Reversible by
  reverting one config file.
- **Status:** CONFIRMED

### The bootstrap run MINTS the new-shape tags — the one link with no local evidence
- **Assumed:** `release-plz release` will create `acdp-registry-<crate>/v0.1.0` tags on the first
  post-merge run.
- **Chose:** to proceed on it, because the alternative (forcing a tag by hand) is a remote write
  no lane may make.
- **Why it is NOT proven:** every local proof ran `release-plz update`. The `release` path was
  never exercised — it needs a token and a real remote. The forward-proof PLANTS by hand exactly
  the tag shape it assumes the bootstrap will create, so it demonstrates what happens AFTER tags
  exist, not that they come to exist.
- **Consequence for acceptance:** criterion 2a has two halves and only one is demonstrated. A
  first run that is green and PR-less but mints NO tags is a FAILURE, not a pass.
- **Blast radius if wrong:** the stall persists in a new form; docker's tag trigger stays dead.
  No irreversible effect.
- **Status:** CONFIRMED
- **Update, 2026-09-11 — PARTIALLY narrowed, still UNCONFIRMED.** The `release` path has now been
  exercised locally after all, which the paragraph above says was never done; that sentence was
  true when written and is now superseded rather than wrong. `release-plz release --dry-run`, run
  against a throwaway clone of this branch with a live token, reaches
  `release_package_if_needed`, shows `tag_exists` does **not** short-circuit on the new tag shape,
  lands in the tag-creation branch, and names all eight expected tags. What it still does not do
  is cross the dry-run boundary and actually push a tag. So the half that remains unproven is
  narrower than before — the code path is demonstrated, the remote write is not — but criterion
  2a is still only dischargeable by observing the real post-merge run.

### Disabling GitHub Releases for the bootstrap merge does not disable git tags
- **Assumed:** `git_release_enable = false` suppresses only GitHub Release objects, leaving tag
  creation untouched — so the bootstrap merge still does the one thing it exists to do.
- **Chose:** to ship the flag off for merge A, per the human's two-merge ruling, and restore it in
  merge B.
- **Why this entry exists at all:** if the two were coupled, merge A would accomplish nothing and
  merge B would re-enter the original deadlock. Neither the leader nor I was willing to assert it
  from the docs.
- **Status:** **CONFIRMED (2026-09-11)** — and this one is genuinely closed, not deferred.
  Source, at the exact version CI pins (0.3.160): `is_git_release_enabled` reads
  `config.git_release.enabled` and `is_git_tag_enabled` reads `config.git_tag.enabled`
  (`release_plz_core/src/command/release.rs:156-164`), held as distinct struct fields (`:284-285`);
  `create_git_tag_and_release` guards them in two sequential, independent `if` blocks (`:995`,
  `:1016`) with the tag block first. Repo-wide, `config.git_release.enabled` has exactly one read
  site. Behaviour: `release --dry-run` lists the Release item with the flag on and drops it with
  the flag off, listing all eight tag creations either way.
- **Version caveat, and why it does not bite:** the binary used was 0.3.162, not the pinned
  0.3.160. An independent verifier diffed the two underlying `release_plz_core` versions (0.37.2
  vs 0.37.0) and found `src/command/release.rs`, `src/project.rs`, `src/git/forge.rs` and
  `release_plz/src/config.rs` **byte-identical**, so the evidence transfers on this path. Note the
  flip side, which is a live trap for anyone repeating this: six files under
  `release_plz_core/src/` **do** differ across those versions — `next_ver.rs`,
  `command/update/updater.rs`, `command/release_pr/mod.rs`, `command/release_pr/git.rs`,
  `clone/mod.rs` and `repo_url.rs` — and `next_ver.rs` differs precisely in the `git_only`
  worktree-reconstruction logic. So local evidence from `release-plz update` at 0.3.162 would
  **not** transfer to 0.3.160.

  (Corrected 2026-09-11: an earlier draft of this entry named `src/update_request.rs` as the
  second differing file. That was wrong twice over — the path is
  `src/command/update/update_request.rs`, and it is byte-identical across the two versions. The
  real second update-path difference is `updater.rs`. The error was conservative, over-warning
  rather than under-warning, but it was still an unchecked claim in an entry whose whole purpose
  is to distinguish checked from unchecked.)

### GitHub's ref matcher accepts `acdp-registry-server/v*` on a real push event
- **Assumed:** the glob matches the ref NAME, and a literal `/` followed by `*` behaves as
  documented.
- **Evidence:** GitHub's filter-pattern documentation, confirmed by an independent verifier —
  patterns are evaluated against the ref name, `*` does not cross `/`, and `feature/*` is the
  documented working form.
- **Why it is NOT proven:** a tag event cannot be staged before merge. The metadata harness
  validates the version COMPUTATION at the pinned action SHA; it says nothing about whether
  GitHub dispatches the workflow.
- **Blast radius if wrong:** the docker trigger stays dead exactly as it is today — no
  regression, just no fix. The guard step cannot catch it, because the guard only runs once the
  workflow has already triggered.
- **Status:** CONFIRMED

### The `ACDP_BOT` App has `contents: write` + `pull-requests: write` on THIS repo
- **Assumed:** yes, from `repository_selection: all` on the org App and its use in three existing
  workflows here.
- **Why it is NOT proven:** verifying needs a JWT; org secret listing needs admin.
- **Mitigation that made it safe to proceed:** failure is loud and immediate — token minting
  fails before any side effect — and the token is now explicitly narrowed with
  `permission-contents` / `permission-pull-requests` rather than inheriting every installation
  permission (which would have included `workflows: write`).
- **Blast radius if wrong:** the release-plz job fails at the mint step. Reversible in two lines.
- **Status:** CONFIRMED

### Rule-10 / rule-15 sweep: a FOREIGN pin went stale because of this branch, and I cannot fix it
- **Observed:** `ASSUMPTIONS.md` (U-005's entry) cites `docker/RAILWAY.md:45`. That was correct at
  this branch's merge-base. Phase 5 added twelve lines above it, so `:45` now lands on the
  image-tag pin line; the `ACDP_REGISTRY_AUTH__JWT_SECRET` row it meant to cite has moved. If
  Phase 6 lands it moves again.
- **Not repointed, deliberately:** that entry belongs to another unit and `ASSUMPTIONS.md` is
  APPEND-ONLY for this lane. Editing a foreign entry is not mine to do even to correct it.
  Recording it here instead, and flagged to the leader as a claim-request candidate.
- **The durable fix** is the one CHARTER rule 15 already prescribes: cite by quoted content, not
  by line. This is the second time in two units that a docs-only edit invalidated a pin in a
  file the editing lane was not allowed to touch.
- **Status:** CONFIRMED

## W3-U1 — #192/#193: validating playground config at both doors (2026-09-10, lane-1)

- **CONFIRMED (U-507, 2026-09-13) — a deliberate departure from a written acceptance criterion, now pinned by a test that names this very decision.** The unit
  assignment's AC3 says an unusable pinned-key list "MUST" be refused at startup and names
  "all entries expired" as qualifying. This unit **refuses the five structural defects**
  (unknown `algorithm`, non-base64 key material, wrong `ed25519`/`ecdsa-p256` byte length or
  missing `0x04` tag, and `valid_from >= valid_until`) but **warns, loudly and branched, on
  an all-expired list rather than refusing it.** Reasoning in full in `DECISIONS.md`
  (`W3-U1-b`); in short: expiry is time-dependent, so refusing makes bootability a function
  of the wall clock and turns a rotation lapse into an outage during the next unrelated
  restart — and under `pinned_only = false` the state is behaviourally identical to having
  no pins at all, which is a supported configuration. Flagged to the leader in the done
  report rather than taken silently. **If the leader wants AC3's literal reading, the change
  is small and localized** — the warning branch becomes an `Err` — but it should arrive as a
  config key (`playground.refuse_on_no_live_pin`) rather than a default, because refusing is
  plainly wrong for the lax case. **Status: UNCONFIRMED (re-checked U-507 2026-09-13 — and deliberately NOT closed by silence). The flag was raised in the done report as the entry says, and no `playground.refuse_on_no_live_pin` key exists anywhere in the tree, so the shipped behaviour is unchanged. Settled by: the leader either requesting AC3's literal reading or declining it. Owner: the leader.**
- **Resolved from this file's own W2-U1 entry above:** that entry left placement of the
  shared validator (`acdp-registry-types` vs `acdp-registry-core`) deliberately undecided for
  whoever took #192/#193. Taken here: **`acdp-registry-core`**, because the rules being
  enforced are properties of what the runtime accepts and their authority
  (`PinnedAlgorithm::parse` and the two key decoders) is private to
  `crates/acdp-registry-core/src/playground.rs`. `acdp-registry-types` ends with zero diff.
  **Status: CONFIRMED** (see `DECISIONS.md` `W3-U1-a`, which also reconciles this against the
  `ct_eq` precedent in entry 5).
- **CONFIRMED by mutation, not by assertion:** that the `#192` tests prove the *live cell* is
  untouched on rejection and not merely the status code. Reordering the handler to swap first
  and validate after leaves the status assertion **green** and fails only at the cell
  assertion. A status-only test would have passed against a handler that corrupts running
  config on every rejection. **Status: CONFIRMED.**
- **CONFIRMED by mutation:** that the validator cannot silently go stale when
  `PinnedAgentKey` gains a field. Exhaustive destructuring without `..` makes that a compile
  error (`error[E0027]`), observed by actually adding a field. **Status: CONFIRMED.**
- **Not re-litigated, and explicitly out of scope:** runtime pin-evaluation semantics.
  `PinOutcome::Skipped` still means "no policy active". This unit refuses bad config at the
  two doors; it does not change what a good config means.

## Run-close sweep — #190/#191 docs truth pass and the full local matrix (2026-09-11, leader)

- **CONFIRMED by execution, not by reading CI: every feature configuration builds, and the
  three CI has never built are clean.** Ran the full CI-equivalent matrix locally —
  `fmt`, clippy ×4, `rustdoc`, tests on sqlite/playground/pg/memory, conformance in
  require-mode against the pinned spec checkout, `cargo-deny`, MSRV 1.88 — plus #200's three
  unbuilt configurations (`storage-pg,playground`, `storage-memory,playground`, and no
  backend at all). All green. **Status: CONFIRMED.**
- **CONFIRMED, and it contradicts #200's suggested fix:** the no-backend build emits **7
  dead-code warnings**, so adding it to the clippy job (which runs `-D warnings`) turns CI
  red. The issue says "add the three configurations to the existing build matrix"; applied
  literally, that breaks the build it was filed to protect. The configuration itself is
  deliberate — the `compile_error!` at `main.rs:8-17` rejects only *pairs*, and there is an
  explicit `#[cfg(not(any(...)))] fn run()` that bails with a useful message — so the fix is
  a scoped `allow(dead_code)`, not a guard. **Status: CONFIRMED** (recorded on #200; the
  `main.rs` half was still under lane-1's claim when this was written).
- **CONFIRMED by the gate firing on me.** The first matrix run failed `test-memory`, and the
  fault was mine: I exported `ACDP_REQUIRE_CONFORMANCE=1` process-wide, where CI scopes it to
  the conformance job. `conformance_gate.rs` refused a require-mode run with `storage-sqlite`
  off — precisely the "compiled to nothing, proves nothing" case it exists to catch. Worth
  recording as a *success*: this file's recurring finding all run has been verification that
  looks sound and cannot fire, and this is the counter-example — a guard that fired against
  its own author. **Status: CONFIRMED.**
- **CONFIRMED against the artefact — `lifecycle.enabled = false` does not stop emission.**
  There is no `lifecycle.enabled` check anywhere in either store. `lifecycle_events` is
  attached and the `retracted` status derived from stored columns unconditionally
  (`crates/acdp-registry-sqlite/src/store.rs:489`, `:515`). Disabling the profile makes the
  endpoints answer `501` and changes nothing about what reads emit. **Status: CONFIRMED.**
- **CONFIRMED — `invalid_log_proof` is reachable from this registry's own handler**, which
  three separate sites denied. `/log/proof` echoes the leaf via `record.leaf()`
  (`handlers/log.rs:359`) and a stored leaf that no longer parses under the closed schema
  raises it locally; `store/src/log.rs`'s own tests pin the reject cases. **Status:
  CONFIRMED.**
- **CONFIRMED as a deliberate, documented non-decision (U-507, 2026-09-13) — still recorded, still not decided, and the reasoning is now in the code beside the mapping:** that path answers `502`, which blames an
  upstream for a local data fault. It is defensible (the wire code is registered to
  RFC-ACDP-0012 §11's federation meaning) and changing it is a wire change. Noted next to the
  mapping in `error.rs` rather than fixed inside a docs pass. **Status: CONFIRMED as a deliberate non-decision (U-507) — **Settled by:** a wire-contract change, **Owner:** the spec holder —
  recorded, not decided.**
- **Assumption made explicit, because re-pointing would have hidden it:** the ten stale
  `417211f` citations in `conformance.rs` asserted counts that are **still true** at the CI
  pin `d1f06d0` — verified by running the suite against that checkout (67 pass, require-mode
  on). Only the coordinate was wrong, which is why nothing could turn red. The literal is
  gone rather than corrected: a symbolic "at the CI-pinned spec" cannot drift, and if a count
  ever stops holding after a bump, the test fails, which is the signal the counts exist to
  provide. **Status: CONFIRMED.**
- **Confirmed by sweeping rather than by report, and it validates lane-3's warning:**
  `docs/OPERATIONS.md` pinned `store/src/lib.rs:74` for `anonymous_public_reads`; `:74` is
  `tenant`. That pin was in no issue and no sweep had touched it. lane-3 predicted this at
  stand-down — two of the six pins it re-derived in #203 were wrong *before* any drift, so
  the untouched pins carry an independent error rate and a diff-driven sweep preserves every
  one of them. This pass re-derived all twenty pins in `docs/`, not only the moved ones.
  **Status: CONFIRMED.**
- **Scope boundary, stated rather than assumed:** whether the registry should emit
  `Cache-Control: private` or `no-store` on requester-relative responses is **not** decided
  here. `#190` was a false claim and is fixed by making the prose true; the wire question is
  `#205`. Shipping a header change inside a docs correction would be the same defect as the
  original claim, pointing the other way. **Status: RESOLVED (U-507, 2026-09-13) — split out by design, and #205 then decided it: `private`, not `no-store` (`acdp-registry-core/src/lib.rs:107`).**

## W3-U5 — the quickstart did not boot (lane-1, 2026-09-11)

- **CORRECTION to `U-005`'s first bullet above, which is left standing per `D-005`.** That
  bullet reads: *"Assumed then verified: that `crates/acdp-registry-server/src/main.rs` is the
  sole enforcement point for both documented claims. Confirmed — exactly one `changeme` check
  exists tree-wide."* Every clause of that is **still true**, and it is still the wrong
  conclusion. There is exactly one `changeme` check, it is in `main.rs`, and it was correctly
  gated on `auth.enabled`. What the bullet missed is that the literal check was never the only
  thing between a placeholder and a boot: `serve_with_store` passes any non-empty `jwt_secret`
  to `JwtSecret::from_base64` (`crates/acdp-registry-auth/src/jwt.rs`), which imposes a 32-byte
  floor with **no** `auth.enabled` gate. `changeme` is valid base64 of six bytes. The stack
  died on the length floor, not on the `changeme` guard, and `U-005` verified the wrong door.
  **Status of the original assumption: CONFIRMED but NON-LOAD-BEARING — true, and it did not
  support what was built on it.**
- **Also corrected: `U-005`'s closing bullet**, *"Not re-litigated: `auth.enabled = false` in
  the compose stack stays (leader-confirmed; the demo must boot)."* The premise held — auth
  stays off — but "the demo must boot" was recorded as a settled constraint without anyone
  checking that the demo **did** boot. It did not. A constraint asserted and never measured is
  indistinguishable from a constraint satisfied, until someone runs it. **Status: CONFIRMED
  (the constraint), VIOLATED (the tree at the time).**
- **CONFIRMED by observation, not by reading:** the hoist changes no boot outcome. All seven
  rows of the matrix reproduce with identical `BOOTED`/`EXITED` and identical `rc`; verified
  again at the compose level against a real Postgres. One scoped exception, found by the gate
  and not by me: for a config with **two** fatal faults the first error reported can differ,
  since validation now precedes the backend checks and `PgStore::connect`. **Status:
  CONFIRMED (scoped).**
- **CONFIRMED by mutation:** that `every_changeme_casing_is_rejected_by_the_serve_path_decoder`
  derives the 32-byte floor from `acdp-registry-auth` rather than restating it — lowering that
  floor to 6 fails it. Its first draft hardcoded `< 32`, never entered production code, and
  would have stayed green with the guard deleted outright. **Status: CONFIRMED.**
- **CONFIRMED by mutation:** that `/auth/*` is genuinely unmounted with auth disabled, which is
  what the auth-off `info!` asserts to operators. Mounting the subrouter unconditionally fails
  `auth_disabled_does_not_mount_the_auth_routes`. **Status: CONFIRMED.**
- **ASSERTED, THEN REFUTED by the phase-1 gate:** that no test pinning that invariant was
  writable inside this unit's granted paths. It was writable throughout — `main.rs` already
  imports `build_router`/`AppStateInner`, `tower` is a regular dependency, and
  `SqliteStore::connect_in_memory()` ships under the default feature. The claim was made
  without checking and pointed in the direction that avoided work. **Status: REFUTED.**
- **ASSERTED, THEN REFUTED by the phase-2 gate:** that the EdDSA `jwt_private_key_pem` check
  being gated on `auth.enabled` means it is not enforced with auth off. It is enforced — from
  the serve path, after `store.migrate()`. Observed: auth off + `EdDSA` + empty PEM exits
  `rc=1` with `auth.jwt_signing_alg=EdDSA but auth.jwt_private_key_pem is empty`, printed
  *after* the `starting acdp-registry` line. My evidence for the original claim was four
  `validate_config` line cites; the serve path was never consulted. **This is the unit's own
  root-cause error recurring inside the fix for it.** **Status: REFUTED, doc corrected.**
- **ASSERTED, THEN REFUTED by the phase-2 gate:** that "a non-empty secret is checked
  regardless of `auth.enabled`" could be written without an algorithm qualifier. Under EdDSA
  `jwt_secret` is never examined, so the unqualified form was false for precisely the
  algorithm `SECURITY.md` recommends one bullet later. Observed: `EdDSA` + a valid PEM +
  `jwt_secret = "changeme"` boots normally with auth off **and** with auth on. **Status:
  REFUTED, HS256 scoping restored and the EdDSA carve-out given its own bullet.**
- **Deliberately bounded, not assumed away:** this unit narrows `validate_config`'s
  validate-before-migrate contract to `jwt_secret` and does **not** restore it. The EdDSA PEM
  case still fails late. Every document it touches is scoped to say so. **Status: OPEN (re-verified U-507 2026-09-13 — `validate_config` catches only an EMPTY EdDSA PEM; a MALFORMED one still fails late). **Settled by:** parsing the PEM there. Still owned
  by nobody, reported in this lane's `done`.**
- **SUPERSEDED by #270 (U-507, 2026-09-13) — no longer true; CI now boots the shipped stack, with and without auth:** that `.github/workflows/docker.yml` sets no
  `jwt_secret`, so CI never exercised the stack the repo ships and green CI was never evidence
  about the compose file. `.github/**` is not this lane's to change; the leader ruled it a
  separate unit.
- **Reported, not fixed:** compose renders `ACDP_REGISTRY_AUTH__JWT_SECRET` as *set-to-empty*
  rather than absent, and an env var outranks the TOML file, so a `jwt_secret` uncommented in
  `docker/config.docker.toml` is silently discarded unless the operator also sets the shell
  variable. Harmless with auth off; loud with auth on; with auth on plus
  `allow_ephemeral_secret = true`, a downgrade to a process-lifetime key carrying **one
  startup `warn!`** and nothing further. The first draft of this bullet called that downgrade
  **silent**, which the ship gate refuted against `main.rs:838-844`: the `warn!` names the
  hazard verbatim. That is this unit's own error class — overstating an exposure — recurring a
  fourth time, in the ledger written to record it, and it survived because it was the one claim
  in the diff with no captured transcript behind it. `config.docker.toml`
  is not in this unit's grant, so the caveat went into the compose header instead.
- **Premise correction, derived from the artefact:** the dispatch described the false changelog
  sentence as **released** and pinned it at `CHANGELOG.md:2637`. Neither holds. `CHANGELOG.md`
  carries exactly one `##` header (`## [Unreleased]`), so nothing in it sits in a released
  version section, and the sentence was at `:2686` when I found it and `:2727` after a rebase —
  which is itself the argument for content anchors over line pins. **Status: CONFIRMED
  (corrected).**

## W3-U7 + W3-U8 — the two false `auth.enabled` doc claims, and #209 (leader lane)

- **CONFIRMED, read from `main.rs` at `abfebf7`, not from the issue text.** The `changeme`
  literal check and the base64/≥32-byte length check are gated on
  `jwt_signing_alg != "EdDSA" && !jwt_secret.is_empty()` — **not** on `auth.enabled`. The
  *empty*-secret check **is** gated on `auth.enabled` (plus `!allow_ephemeral_secret`), and
  the source comment states that asymmetry is deliberate: an auth-off registry with no secret
  is supported. `docs/OPERATIONS.md` and `docs/AUTHENTICATION.md` both asserted the ungated
  checks were gated; both now describe the split as a two-row table rather than a sentence,
  because the previous wording failed by binding two differently-gated facts with "both".
- **CONFIRMED: the guard is non-`EdDSA`, not HS256-only.** The docs said "on HS256", which
  **under**-claims enforcement — `RS256` + `changeme` is also refused. Corrected to name the
  actual condition. Under `EdDSA` the secret is never examined; that carve-out is stated
  explicitly in both files so a future edit cannot flatten it into "always checked".
- **CONFIRMED: the compose file ships no JWT default.** `docker-compose.yml:69` is
  `${ACDP_REGISTRY_JWT_SECRET:-}`. `OPERATIONS.md` still described the removed `changeme`
  default and called the resulting stack startable; it was not startable, which is what W3-U5
  fixed.

### The foreign pin lane-2 flagged and correctly refused to touch — now discharged
- **Observed:** `ASSUMPTIONS.md:713` (U-005's entry) cites `docker/RAILWAY.md:45` meaning the
  `ACDP_REGISTRY_AUTH__JWT_SECRET` row. lane-2's W2-U3 entry above records that the pin went
  stale when Phase 5 added twelve lines, and that fixing a foreign append-only entry was not
  the lane's to do. It was right on both counts.
- **Re-pointed by content, per CHARTER rule 15, without editing the foreign entry.** The row
  U-005 meant is the table row whose first cell is `` `ACDP_REGISTRY_AUTH__JWT_SECRET` `` in
  `docker/RAILWAY.md`'s "Set the env vars" table. **Cite it that way, not by line.** This
  commit did not move it: `RAILWAY.md` is 91 lines before and after.
- **Status: CONFIRMED (discharged).** Third instance in three units of a docs-only edit
  invalidating a pin in a file the editing lane could not touch. The durable fix remains
  rule 15.

### Left standing deliberately — `docker/RAILWAY.md:57` and `:68` are FALSE on `main` today
- **Observed:** both still say the `changeme` check is gated on auth being enabled and that
  the Railway `JWT_SECRET` "is never validated". Both are false — and `:68` was false *before*
  W3-U5 too, because the ≥32-byte floor already ran ungated from the serve path, so a Railway
  deploy carrying `changeme` was already dying at boot, just later and with a worse message.
- **Not repaired here, and this is a judgement call worth recording.** Both lines sit inside
  the region rewritten wholesale by the **held R3 patch**
  (`archive/lane-2/20260911T030908Z-w2-u3-phase6-held.patch`, still uncommitted, awaiting a
  human ruling on whether the Railway recipe should enable auth). Editing them now would
  conflict with that patch and pre-empt the ruling. Verified this commit leaves it applying
  cleanly.
- **Status: SUPERSEDED — the standalone factual fix landed, independently of R3 (U-507, 2026-09-13).** The evidence is
  settled; only the remedy is open. **If R3 is declined, these two lines still need a
  standalone factual fix** — they do not become true by the recipe staying auth-off.

## #205 — Cache-Control posture on requester-relative responses (leader lane)

- **CONFIRMED: there was no live cache-poisoning bug.** All three `Cache-Control` emissions on
  `main` before this change (`handlers/meta.rs:74/:95/:126`) are on requester-invariant
  documents. This closed a hardening gap; the CHANGELOG and PR say so explicitly rather than
  claiming a fix for a live defect.
- **CONFIRMED by falsification, not by reading:** removing the data-plane layer reddens
  `cache_posture_covers_every_data_plane_route`; restoring it greens. Removing/moving the auth
  layer inside the limiter reddens `credential_endpoints_are_never_stored`.
- **UNCONFIRMED and unfalsifiable from this repo (re-checked U-507 2026-09-13). **Settled by:** an operator observing a real CDN. **Owner:** operators. A CDN in "cache everything / ignore origin headers" mode defeats both
  `private` and `Vary`.** No origin header can fix this. It stays an operator note in
  `RECEIPTS.md`, downgraded to defense-in-depth rather than deleted. Not testable from here.
- **UNCONFIRMED — `private` carries no validators. Re-checked U-507 2026-09-13: the fix has NOT landed — there is no `ETag` anywhere in `acdp-registry-core`. **Settled by:** ETags plus explicit freshness. **Owner:** unassigned.** No `ETag`, no `Last-Modified`, no
  `max-age`, so a requester's *own* cache may briefly reuse a context retracted since. Accepted
  deliberately: they already held those bytes. **The future fix is ETags plus explicit
  freshness, NOT `no-store`** — reaching for `no-store` would trade a real client-caching
  capability for protection against caches that ignore directives anyway.
- **UNCONFIRMED — `/log/checkpoint` inherits `private` it does not need; still true (`if_not_present` at `lib.rs:121`; `handlers/log.rs` sets no cache header of its own). **Settled by:** an explicit short public TTL on that route. **Owner:** unassigned.** It is hash-only and
  requester-invariant (`handlers/log.rs`, `State` only). It gives up shared cacheability it has
  never used. `if_not_present` was chosen precisely so an explicit short public TTL can be added
  later without touching the layer.
- **RESOLVED (U-507, 2026-09-13) — #218 is CLOSED/COMPLETED, and this entry's own announcement mechanism fired: the `EXEMPT` row is gone, replaced by `no-store`, overriding, on both the 200 and 401 arms.** It
  is a direct Prometheus scrape target that sets no cache header, but it is also bearer-gated
  (`metrics.rs`), so its 200-vs-401 outcome is authorization-relative — the same gap #205 closed
  elsewhere, under the same CDN threat model. Tracked as **#218**, filed with the proposed
  posture (`no-store`, same category as `/admin/*`) and acceptance criteria — not left as a
  one-line dismissal. The exemption is also load-bearing in the test suite: `/metrics` is listed
  in `NON_DATA_ROUTES` marked `EXEMPT`, so deleting that line is how the fix announces itself.

### Correction recorded rather than quietly fixed (second one)
The bullet above previously read "**Follow-up issue filed**" *before any issue existed*. The
final verification gate checked `gh issue list` and found nothing. A claim of completed work,
asserted rather than verified — the identical defect class to the 429 correction below, in the
very file that records it. #218 now exists; the claim is true as written.

Two more claims in this run's first draft failed the same way and were repaired in the same
pass:
- `lib.rs` and the test-file header both said `cache_posture_covers_every_data_plane_route`
  "is what notices" a route added outside the `data` group. It could not: the table is static,
  so it catches a route *removed from* `data` and is structurally blind to one *added outside*
  it — the direction that is the actual #190 defect. Falsified by mounting a real
  requester-relative route on the `acdp` builder: all tests stayed green. Replaced with
  `every_route_in_the_core_router_is_classified`, which scans the router's own source and fails
  on any path it cannot place in a posture group. Re-falsified: the same mutation now reddens.
- `CHANGELOG.md` and `docs/RECEIPTS.md` said "a test pins that" of the three `/.well-known/*`
  documents. Only `/.well-known/acdp.json` was asserted anywhere in the repo. Now all three are
  (`every_well_known_document_keeps_public_caching`), including `did.json`'s 404 arm.

### Correction recorded rather than quietly fixed
The first version of the 429 assertion **claimed to pin layer order and did not.** It drove
`limits.challenge_rate_per_minute`, enforced inside the *handler* (`state.rs`) and therefore
within both layers, so the 429 carried the header under either ordering. The falsification run
left it GREEN, which is how it was caught — review had not. Retargeted at
`rate_limit.per_ip_per_minute` so the 429 comes from the `auth_rate_limit` **middleware**, the
only path that can bypass an inner header layer. This is the run's recurring defect class — a
claim asserted from reasoning rather than from a probe — reproduced inside the very unit whose
tests exist to prevent it.

## W3-U10 — CI builds every valid feature configuration (#200)

### SUPERSEDED (U-510 / #265, 2026-09-13): the four new steps ran `clippy`, not `cargo build` — build steps now run beside them
Issue #200 and the unit assignment both say "`--all-targets` builds; no test run needed."
The four steps added to the `clippy` job run `cargo clippy … -- -D warnings` instead.
Reasoning: all four *existing* feature steps are clippy, the job is measured at 24–43s so
the difference is seconds, and building-only would create two classes of configuration —
one lint-checked, one not — which is a smaller instance of the coverage gap being closed.
Reverses in one line per step if the leader disagrees.

### CONFIRMED by measurement, and it reversed two design decisions
Both of this unit's first-draft designs were wrong, and both were caught by a review round
rather than by the author:
- `#[cfg]`-gating the seven dead items **does not compile.** Those items are what keep ten
  `use` lines alive; gating them turns 7 dead-code warnings into 10 unused-import errors.
  The `cfg_attr`-scoped allow is the correct answer, and the assignment's own escape hatch
  ("if a blanket allow is genuinely the right answer for some item, say why in the code")
  is what it is invoked under.
- Putting the new steps in a **separate `features` job would not have closed #200.**
  Branch protection requires exactly `rustfmt`, `clippy`, `tests`,
  `conformance (spec fixtures)`. A new job is not a required check, so a break in one of
  the four new configurations could still have landed green — the precise harm the issue
  was filed about. The steps go inside `clippy`.

### Correction recorded rather than quietly fixed
The first draft justified that separate job partly on "the clippy job already runs four
full compiles against a 30-minute timeout." **That was asserted, never measured, and is
false**: across the 12 most recent runs the job takes 24–43 seconds, and its four feature
steps cost 9s + 3s + 3s + 0s. An unmeasured cost stated as a reason — written roughly an
hour after this same lane flagged "an untranscribed claim is an unverified claim" as W3-U5's
headline finding. The rewrite's replacement cost argument is measured where it claims to be,
and explicitly marks the parts that are not ("the added seconds are not measured and are not
claimed as such").

### Correction recorded rather than quietly fixed (second one)
The rewrite then asserted that "every integration test is backend-gated at the file level,"
so the backend-less rows compile the test suite to nothing. **False.**
`tests/anchors_uri_never_dereferenced.rs` and `tests/conformance_gate.rs` carry no gate —
the `#![cfg(feature = "storage-sqlite")]` on `conformance_gate.rs:2` is prose *about another
file* and reads as an attribute. Measured under `--no-default-features`: 7 test executables
build and 71 tests run. The tidier false version came within one acceptance criterion of
being written into `ci.yml` as permanent repo documentation.

### The feature space is eight, not the seven #200 describes
`Cargo.toml` declares four features; with the pair guard the space is 4 backend states ×
playground on/off = 8. #200 lists three missing configurations and this plan's first draft
inherited that count, omitting `--no-default-features --features playground` — which is
buildable and **fails today** with the same 7-bin/2-test dead-code errors. A comment calling
itself "the single index of the whole feature space" would have shipped with a hole in it,
reproducing the exact enumeration rot the unit exists to close. Four steps, not three.

### The two `<backend>,playground` steps are coverage, not guards — stated, not dressed up
Both are green before and after this change. `feature = "playground"` does not appear in
`crates/acdp-registry-server/src/` at all; it gates only `acdp-registry-core`
(`src/lib.rs:369,393`, `handlers/admin.rs:26,28`), which is generic over
`S: ExtendedRegistryStore`, so there is no backend×playground-specific site for them to catch
anything at today. A planned falsification for them was dropped once it became clear it would
have had to *create* its own target to have something to break. By CHARTER Rule 41's own
standard they are not yet guards; they are future-proofing, and the PR body says so.

### Correction recorded rather than quietly fixed (third one)
The shipped attribute allowed `dead_code, unused_imports`. **`unused_imports` was never
needed.** Measured across all six backend-less target/feature combinations: `allow(dead_code)`
alone is rc=0 everywhere, and the regression itself produces seven `is never used` items in
the bin and two in the test target with **zero** unused-import diagnostics. The justification
written into the plan — "F1 proves it is genuinely needed" — was a conclusion carried over
from the `#[cfg]`-gate design that F1 *rejected*: under that design the items vanish and
orphan their imports; under the shipped one they stay, so the imports stay used. The allow was
one lint wider than the evidence, enlarging the hidden class for free. Narrowed, and the
comment in `main.rs` now says why `dead_code` alone is the right width.

### Correction recorded rather than quietly fixed (fourth one)
The `ci.yml` comment and the CHANGELOG both pointed at the pair guard as `main.rs:8-17`.
**This diff moved it to `main.rs:37-46`** — the new explanatory comment is 29 lines, so the
pointer landed in the middle of prose about itself. A citation invalidated by the very change
that ships it.

### The flag-mutation hazard needed two baselines, not one
The first version of the ci.yml hazard block said "ANY of the four steps minus `--all-targets`
passes silently", presented as measured against the regression. Against the regression that is
**false** — steps 7 and 8 fail rc=101, because the bin target is itself dirty. The
`--all-targets` hazard is a property of the tree **as shipped**; the `--no-default-features`
hazard can only be demonstrated **against the regression**. Two questions, two baselines, and
collapsing them inverted one answer. Both are now stated with their baseline named. Measured:
7/8 minus `--no-default-features` -> rc=0 silent; 5/6 minus it -> rc=101 loud; all four minus
`--all-targets` (shipped tree) -> rc=0 silent.

### Environment trap that produced two false readings
The Bash tool's shell is **zsh**, which does not word-split unquoted parameter expansions. A
loop passing flags via `$f` sent `--features playground` as a *single argument*, so cargo
exited **rc=1** with `unexpected argument` — which reads as a failing configuration if only
the exit code is checked. A real compile failure here is **rc=101**. Related: never put `$?`
and a `$(command substitution)` in the same `echo`; the substitution runs during word
expansion and clobbers the status. Capture `rc=$?` on its own line.

### Filed, not fixed: a ninth configuration (#221)
`acdp-registry-types` declares `default = ["axum"]` with a manifest comment calling the
non-axum build a supported consumer scenario, and no CI job builds it (rc=0 today, so valid
and unexercised). #200's body asserts it is "covered incidentally by existing jobs" — false.
Not folded in: it is a different crate's feature space, and this unit's ci.yml comment and
CHANGELOG were framed around the server binary through two review rounds. Widening a reviewed
frame at ship time is how reviewed material becomes unreviewed. Named in the ci.yml comment as
known-and-unbuilt; filed as #221.

### Correction recorded rather than quietly fixed (fifth one) — the same defect, one layer down
The fix for the stale `main.rs:8-17` citation replaced it with `main.rs:37-46`. **Also wrong.**
The guard is at `main.rs:45-54`. The error was arithmetic standing in for reading: the entry
above said "the new explanatory comment is 29 lines" and added 29 to the old numbers. The
inserted block is **37 lines** — a 28-line comment, the 8-line attribute, and a blank.

The sharper point is not the arithmetic. The line number *was* read correctly from the file
earlier in this unit, and then a later edit to the same file — widening the comment while
closing a different gap — invalidated that reading, which was never re-taken. So:
**a verified line number is valid only until the next edit to that file.** Verification has a
shelf life, and editing the file is what expires it. Now read, not computed:
`#[cfg(any(` at :45, `compile_error!(` at :50, `);` at :54.

### Correction recorded rather than quietly fixed (sixth one) — inside the fifth's own fix
The rewritten EDITING HAZARD block justified the `--all-targets` hazard with "the backend-less
builds carry 2 dead-code items in the test target **that the bin target does not**". False: the
test target's 2 (`serve_with_store`, `spawn_shutdown_watcher`) are a strict **subset** of the
bin target's 7. And the consequence runs backwards — because both are also bin-dead, a
`--bins`-only mutation would still redden. The outcome claim (all four steps minus
`--all-targets` pass) is true and measured; only the reason given for it was invented. That is
the identical defect this block was rewritten to fix, recurring inside the rewrite.

## `ACDP_REQUIRE_PG` inherits `is_ok()` semantics, so `ACDP_REQUIRE_PG=0` enables require-mode

- **Plan:** `plans/h-b-storage-parity.md` (H-B Phase 1, B4)
- **Assumed:** matching the established `ACDP_REQUIRE_CONFORMANCE` contract matters more
  than the surface surprise of `=0` meaning "on".
- **Chose:** `std::env::var("ACDP_REQUIRE_PG").is_ok()` — any value, including the empty
  string, enables require-mode. Identical to `conformance.rs:2704-2708`, which carries an
  explicit "Do not 'improve' this to a truthiness check" comment. I carried that comment
  across and named the reason: two require-flags in one repo disagreeing about what `=0`
  means is a worse trap than either one being individually surprising, because the person
  who hits it will have read the other one first.
- **Alternatives:** a truthiness check (`== "1" || == "true"`). Rejected: it diverges from
  the sibling for no gain, and CI sets `"1"` either way so the distinction is invisible in
  the only place it currently runs.
- **Blast radius if wrong:** someone sets `ACDP_REQUIRE_PG=0` expecting to disable the gate
  and gets a red run. Cost to reverse: one line. Visible immediately, not silently.
- **Status:** CONFIRMED (2026-09-12) — matches the sibling gate byte-for-byte; see DECISIONS.md H-B #1.

## Phase 1 gates 23 of 34 pg tests; the other 11 belong to lane-3

- **Plan:** `plans/h-b-storage-parity.md` (H-B Phase 1, B4)
- **Assumed:** landing the gate for the 23 tests I own is better than waiting for lane-3 to
  apply the same helper to `crates/acdp-registry-server/tests/pg_integration.rs:51-54`.
- **Chose:** ship my half now; send lane-3 the exact helper shape through the leader
  (`outbox/lane-2/20260912T011543Z-fyi.md`) rather than editing their file. My
  `ci.yml:221` line sets the env var for the whole step, which covers *both* suites, so
  lane-3's 11 tests become gated the moment they apply the helper — no second CI change.
- **Alternatives:** (a) edit their file — forbidden by the lane claim, and it is exactly the
  cross-lane write the claim exists to prevent; (b) block Phase 1 on lane-3 — serializes
  two independent lanes for a one-line change.
- **Blast radius if wrong:** until lane-3 lands their half, an absent Postgres reddens on 23
  tests instead of 34. The gate is strictly better than the status quo either way; the risk
  is only that someone reads "pg is gated" as covering all 34. Mitigated by saying 23-of-34
  explicitly in the PR body rather than implying completeness.
- **Status:** RESOLVED (2026-09-12) — lane-3's #227 landed the same helper; gating is now **34 of 34**, verified in the tree. See DECISIONS.md H-B #2.

## `unixepoch(…, 'subsec')` over a canonical stored timestamp column for `data_period`

- **Plan:** `plans/h-b-storage-parity.md` (H-B Phase 2a, B1)
- **Assumed:** a query-side numeric conversion is the right fix, rather than normalizing the
  stored representation or adding a generated canonical column.
- **Chose:** `unixepoch(json_extract(body_json, '$.data_period.start'), 'subsec')` compared
  against `unixepoch(?, 'subsec')`. Both sides go through one parser, so every combination of
  `Z` vs `+00:00` and 0/3/6/9 fractional digits normalizes at once. Verified the bundled
  SQLite is **3.46.0** (`libsqlite3-sys-0.30.1/sqlite3/sqlite3.h:149`); `'subsec'` needs ≥3.42.
- **Alternatives:** (a) bind a `Z`-normalized string — **measured insufficient**, `'…00Z'`
  still sorts after `'…00.500Z'`; (b) migrate `body_json` to a canonical timestamp form —
  rejected outright, those bytes are the `content_hash` preimage, so rewriting them breaks
  signature verification; (c) a generated canonical column plus an index — defensible and
  index-friendly, but a schema change for a filter that has no index today and no measured
  volume.
- **Blast radius if wrong:** the filter does a full scan and converts per row, so a registry
  with very many contexts pays for it in search latency. Nothing is stored differently, so
  reversal is a one-line revert with no data migration. If latency is ever observed, (c) is
  the upgrade path and this entry is the record of why it was deferred.
- **Status:** CONFIRMED (2026-09-12) — query-side only; the canonical-column upgrade stays deferred until latency is measured. See DECISIONS.md H-B #3.

## B2 split out of Phase 2 rather than shipped alongside B1

- **Plan:** `plans/h-b-storage-parity.md` (H-B, B2)
- **Assumed:** the plan's "make SQLite adopt Postgres's `q=` semantics" is still the right
  call, but it cannot be delivered to the same standard as B1 in the same phase.
- **Chose:** ship B1 and the harness now; move B2 to its own phase. Reason found while
  implementing: matching pg means FTS5's `porter` tokenizer, which is **not** snowball
  `english`, plus a stopword list this repo would have to hand-maintain. The resulting
  parity would be pinned per-word rather than structural — and a hand-maintained table
  cannot catch the words nobody thought to add. Letting that ride along with B1's
  exactly-correct fix would have put a weak parity claim behind a strong one.
- **Alternatives:** (a) make pg adopt SQLite's semantics (`simple` instead of `english`) —
  gives *exact* structural parity and needs no word list, but removes stemming from the
  production backend, a real search-quality regression; (b) ship approximate parity now and
  call it done — rejected, it is the same species of overclaim as the comment this phase
  deleted.
- **Blast radius if wrong:** `q=` keeps diverging between backends until B2 lands. Mitigated
  by the corrected comment, which now states the divergence with measured numbers instead of
  denying it, so nobody builds on a false guarantee in the meantime.
- **Status:** RESOLVED (2026-09-12) — the split is complete; B2 shipped in #231. See DECISIONS.md H-B #4.

## `q=` semantics: Postgres wins, SQLite raised to it via porter + a verified stopword list

- **Plan:** `plans/h-b-storage-parity.md` (H-B Phase 2b, B2)
- **Assumed:** of the two ways to make `q=` agree, keeping Postgres's behaviour and changing
  SQLite is the right trade.
- **Chose:** SQLite adopts Postgres. `tokenize = 'porter unicode61'` (migration 013) for
  stemming, plus query-side stopword removal in `fts5_escape` using PostgreSQL 16's own
  `english.stop`. Postgres is the production backend, stemming is better search behaviour,
  and the cost falls on SQLite's FTS index — derived data, rebuilt from `contexts`, so the
  change is reversible and no context data is rewritten. Measured before committing to it:
  porter stems *through* FTS5 phrase quoting, so `fts5_escape` keeps quoting every token and
  loses none of its operator-neutralizing property.
- **Alternatives:** (a) **pg adopts SQLite** (`simple` instead of `english`) — would give
  *exact structural* parity with no word list and no stemmer mismatch possible, and was
  genuinely tempting for that reason; rejected because it removes stemming from the
  production backend, a real search-quality regression, to buy a testing property. (b) Ship
  approximate parity without saying so — rejected; that is the same overclaim as the comment
  this phase deleted.
- **Blast radius if wrong:** SQLite search results change — inflected queries start matching,
  stopword-only queries stop matching. Reversible by restoring the previous tokenizer and
  rebuilding the derived index; no data migration either way. The residual correctness gap is
  that porter and snowball disagree on some words, so parity is pinned per-mechanism rather
  than proven across the language — stated on `fulltext::PG_ENGLISH_STOPWORDS` and in the
  parity suite's docs rather than left implicit.
- **Status:** UNCONFIRMED — **escalated to the human**, re-checked 2026-09-13 (U-507) and still open. Reversible, but a product judgement on a public API taken against a defensible alternative. Recommendation unchanged: confirm as taken. **Settled by:** the owner accepting or rejecting the taken behaviour. **Owner:** the human. See DECISIONS.md H-B #10.

## The stopword table is verified against Postgres rather than trusted

- **Plan:** `plans/h-b-storage-parity.md` (H-B Phase 2b, B2)
- **Assumed:** a hand-copied 127-entry table will go stale and nobody will notice, which is
  the characteristic failure of hand-maintained tables.
- **Chose:** keep the table (it must be available to SQLite at runtime, with no database in
  reach) but *check* it: the pg parity suite asserts, for every entry, that
  `to_tsvector('english', w)` is empty according to the live server, plus a negative control
  so the check cannot pass vacuously. Drift reddens a test instead of quietly skewing search.
- **Alternatives:** query Postgres at runtime (impossible for the SQLite backend, which may
  run with no Postgres anywhere); trust the copy (the failure mode above); derive it from a
  crate (adds a dependency for 127 strings).
- **Blast radius if wrong:** the one direction the check cannot cover is Postgres *gaining* a
  stopword this list lacks — their list is a file not enumerable from SQL. Then SQLite would
  keep a term Postgres drops, and that specific divergence would go unnoticed. Documented on
  the constant; it is the reason the suite pins mechanisms rather than claiming exhaustive
  agreement.
- **Status:** CONFIRMED (2026-09-12) — the pg suite checks all 127 entries against the live server. See DECISIONS.md H-B #5.

## B3 fixed by reconciling status against events, not by taking a read snapshot

- **Plan:** `plans/h-b-storage-parity.md` (H-B Phase 3, B3)
- **Assumed:** making the served object self-consistent by construction is worth more than
  making one call site take a consistent snapshot.
- **Chose:** a shared `reconcile_retraction(status, events)` in `acdp-registry-store`, applied
  in `get()` and `lineage()` on both backends. Retraction wins from either source. The
  property this buys over a transaction is that a *future* read path which forgets the
  snapshot cannot reintroduce the bug — the fix is in the shape of the data, not in the
  discipline of the caller. It is also one shared helper rather than two hand-mirrored
  per-backend transactions, so the backends cannot drift on what "consistent" means.
- **Two preconditions verified against the code first**, because the cheap fix is only sound
  if they hold, and **my own plan had rejected this approach on the second one**: (a)
  `lifecycle_events` is append-only — no `DELETE` exists in either backend, so the event log
  can never be missing a retraction the denormalized flag knows about; (b) the event and the
  flag are written in one transaction, so only the reads could ever disagree. The plan's
  objection — "deriving from events discards the column and breaks if events are pruned" —
  described a hazard this codebase does not have.
- **Alternatives:** (a) a read transaction per call — sqlite WAL gives a consistent snapshot
  on `BEGIN DEFERRED`, but Postgres `READ COMMITTED` does *not* (each statement re-snapshots),
  so it would have needed `REPEATABLE READ` set per backend: more moving parts, and it fixes
  only the call sites that remember to do it. (b) One statement aggregating events as JSON —
  atomic by construction and one round trip, but it encodes the event wire shape in SQL, so
  the mapping would have to stay in sync with the Rust struct by hand.
- **Blast radius if wrong:** a context retracted and then republished could, under a torn
  read, be served as `retracted` slightly after becoming active again. Stale, never
  self-contradictory, and stale-toward-retracted is the safe direction to be wrong about
  whether data has been withdrawn. Reversal is deleting four call-site lines.
- **Status:** CONFIRMED (2026-09-12) — both preconditions verified in code; reconciliation beats a snapshot on shape. See DECISIONS.md H-B #6.

## B5's busy timeout is an in-crate constant, not a config field — and has no behavioural test

- **Plan:** `plans/h-b-storage-parity.md` (H-B Phase 4, B5)
- **Assumed:** an explicit value that is written down beats an implicit one inherited from a
  dependency, even if it is not yet tunable.
- **Chose:** a named `SQLITE_BUSY_TIMEOUT` constant (30s) in the sqlite crate. The finding
  asked for it to come "from config", but the storage config lives in `acdp-registry-types`,
  outside this change's path scope — adding a field there is a separate change, flagged rather
  than smuggled in.
- **Stated limit — this one has NO falsifying test, unlike every other guard in this unit.**
  Reddening it deterministically means holding `BEGIN IMMEDIATE` across a slow callback and
  racing a second writer against a wall clock; such a test is timing-dependent, and a flaky
  guard gets deleted, which is worse than an honest gap. The change is a one-line
  configuration of an existing mechanism, and it is recorded here as untested rather than
  described as guarded.
- **Blast radius if wrong:** 30s is too long for a caller that would rather fail fast, or too
  short for a pathological disk. Either way it is one constant, and the old behaviour was an
  undocumented 5s from sqlx.
- **Status:** CONFIRMED (2026-09-12) as an accepted, documented gap — recommendation is NOT to add a flaky timing test. See DECISIONS.md H-B #7.

## B7 is a parity fix, not a live exploit closed — and both the finding and my own read were wrong

- **Plan:** `plans/h-b-storage-parity.md` (H-B Phase 4, B7)
- **Assumed, then measured:** the finding said the `as i32` narrowing was unreachable "because
  `put()` has no production callers". That reason is **false** — the casts were in
  `commit_publish` and in the row INSERT, both on the live publish path. I then concluded it
  was therefore reachable, and **that was also false**: measured on both backends, a publish
  carrying `version = 3_000_000_000` is refused before the store sees it, because the request
  builder requires `version == 1` for a first publish and `prev + 1` for a supersession.
  Reaching 2^31 needs ~2 billion sequential supersessions.
- **Chose:** widen `contexts.version` to `BIGINT` and use `i64::from` anyway. It removes a real
  divergence (SQLite was already lossless), it is a widening so nothing can fail to fit, and
  the cost is one table rewrite. Keeping a narrowing cast on the publish path because today's
  validation happens to prevent it would be relying on a constraint enforced in a different
  crate.
- **Alternatives:** leave it and document — rejected, the divergence is exactly what this unit
  exists to remove; assert on `version` at the store boundary instead — duplicates validation
  the SDK already does, in the wrong layer.
- **Blast radius if wrong:** `INTEGER` → `BIGINT` rewrites the table under an ACCESS EXCLUSIVE
  lock. Acceptable at this scale, worth scheduling on a very large `contexts`.
- **Status:** CONFIRMED (2026-09-12) — measured reversible (MAX(version)=2, zero rows over i32::MAX), so it drops out of the critical tier. See DECISIONS.md H-B #8.

## `lineages` is write-only and was deliberately NOT dropped

- **Plan:** `plans/h-b-storage-parity.md` (H-B Phase 4, B8)
- **Assumed:** a table written on every insert and read by nothing is dead write amplification
  worth removing.
- **Verified exhaustively before deciding:** `lineages` has `INSERT` only
  (`sqlite/src/store.rs`, `pg/src/store.rs`) and **zero** `SELECT`/`JOIN`/`UPDATE` anywhere in
  the repository — src, tests, and migrations all swept, not just the two store files.
- **Chose: do not drop it.** Two reasons, either sufficient. (a) Dropping a table is a one-way
  door and this unit has no mandate for one. (b) `crates/acdp-registry-server/tests/pg_integration.rs`
  TRUNCATEs it in test setup, so removing the table means editing a file outside this change's
  path scope — the finding cannot be actioned without a cross-boundary edit even if it were
  desirable.
- **Blast radius if wrong:** every insert keeps paying for one extra row write. Measured cost:
  one INSERT per publish, inside a transaction that already writes several rows.
- **Status:** CLOSED (2026-09-12) — finding handed to the coordinator with its evidence; no action taken here. See DECISIONS.md H-B #9.

## H-A / P2 — 408 is not given an RFC-ACDP-0007 §5 envelope

- **Plan:** plans/h-a-wire-surface-observability.md (phase P2)
- **Assumed:** that a middleware-generated 408 cannot be given an honest §5 error envelope
  from inside this lane's claim.
- **Chose:** scope P2 to 413 only. `acdp_wire_code`
  (`crates/acdp-registry-types/src/error.rs:138-201`) has no timeout arm and falls through to
  `internal_error`, and `AcdpError` has no timeout variant. Emitting `internal_error` for a
  client-side timeout attributes a client condition to a server fault — worse than the current
  silence, because it would be actively misleading in exactly the logs an operator reaches for.
  Minting `request_timeout` changes the shared §5 wire-code registry, which lives in
  `acdp-registry-types` and is outside this lane's granted paths.
- **Alternatives:** (a) emit `internal_error` for 408 — rejected as actively wrong; (b) invent a
  `request_timeout` string locally in the rewriter without registering it in §5 — rejected as a
  silent, unilateral extension of a shared wire contract; (c) edit `acdp-registry-types`
  anyway — rejected, outside the claim, and raised as a claim-request instead.
- **Blast radius if wrong:** none to correctness. A 408 continues to behave exactly as it does
  on `main` today (media type stamped by the outermost `if_not_present` layer, no envelope).
  The cost is that a client parsing error envelopes uniformly still gets no `error.code` on a
  timeout. Reversible in one commit once a §5 code exists.
- **Status:** CONFIRMED (U-507, 2026-09-13) — the constraint is unchanged: `AcdpError` still has no timeout variant and `acdp_wire_code` no timeout arm, so 413-only remains correct.

## H-A / P1 — the 408 `x-request-id` is correct by construction but not pinned by a test

- **Plan:** plans/h-a-wire-surface-observability.md (phase P1, acceptance criterion 2)
- **Assumed:** that the 408 arm gets `x-request-id` for the same reason the 413 arm does — both
  are generated by layers now sitting inside the relocated request-id pair.
- **Chose:** state this as an honest coverage limit rather than claim it as tested. `TimeoutLayer`
  is constructed with a hard-coded `Duration::from_secs(30)` in `build_router` and nothing in the
  config plumbs it, so pinning the 408 would mean either a 30-second test or making the timeout
  configurable purely to test it. The falsification that moves the request-id pair back inside
  the body limit reddens the whole middleware-generated class, of which the 408 is a member — so
  the mechanism is evidenced, the specific arm is not.
- **Alternatives:** (a) claim AC2 covers 408 because the mechanism is shared — rejected; that is
  precisely the "asserted from reasoning rather than a probe" pattern this unit exists to fix;
  (b) make the timeout configurable — rejected as scope creep into a phase that is about layer
  order, though it is the right follow-up if a 408 test is ever wanted.
- **Blast radius if wrong:** a future refactor could move the timeout layer outside the
  request-id pair and lose the id on 408s with no test failing. Bounded: the 413 guard covers the
  same layer boundary, so the regression would have to be specific to the timeout layer alone.
- **Status:** UNCONFIRMED — handed to the coordinator as a standalone decision with this evidence
  rather than actioned here. Re-checked 2026-09-13 (U-507): still undecided. **Settled by:** the coordinator ruling on the taxonomy. **Owner:** the leader — this one is genuinely the leader's, not the human's.


## H-A / P4 — A9: rate-limit scope taxonomy generated from one list

- **Plan:** plans/h-a-wire-surface-observability.md (phase P4)
- **Assumed:** that a `/auth/challenge` rejection from the process-global ceiling and one from
  the per-agent budget are operationally different events that an operator needs to tell apart,
  and that collapsing them into `challenge_per_agent` hid the `agent_id`-rotating flood the
  global ceiling exists to catch (#24) inside the ordinary noisy-agent case.
- **Chose:** split the collapsed `check_global().and_then(|()| check(agent_id))` into two
  sequential early-returns, each recording its own scope. Semantics are unchanged — `and_then`
  already skipped `check` on a global `Err`, and each branch still surfaces its own
  `Retry-After`.
- **DIVERGENCE FROM THE PLAN, stated deliberately.** The plan specified a hand-written enum with
  an exhaustive no-wildcard `match` for `label()`, plus a hand-written `ALL`, and explicitly left
  the `ALL`-omission gap **open**: "a variant missing *from* `ALL` is invisible to
  `every_rate_limit_scope_is_documented`, which iterates `ALL` — the same omission class that
  lost `lifecycle_per_agent`. Closing it needs enum, `ALL` and `label()` generated from one
  `macro_rules!`. That is worth doing but is not this phase."

  I closed it in this phase instead. The macro is ~15 lines, it is the mechanism the plan itself
  named as correct, and the gap it leaves open is not hypothetical — this taxonomy has already
  drifted in **both** directions (`lifecycle_per_agent` emitted but undocumented; a
  `challenge_global` documented in the `metrics.rs` docstring that nothing emitted). Shipping a
  phase whose stated purpose is omission-proofing while leaving the dominant omission path open,
  when the fix is fifteen lines and falsifiable, was the wrong trade. The plan's reason for
  deferring was scope, not a technical objection.
- **Also changed:** `record_rate_limit_rejection` now takes `RateLimitScope` rather than
  `&'static str`, so an undocumented or mistyped label cannot be constructed. This is a `pub`
  signature change in `acdp-registry-core`, which has exactly one consumer
  (`acdp-registry-server`, in-workspace, by path). Not a published-API break.
- **Alternatives:** (a) hand-written enum + hand-written `ALL` — rejected, leaves the omission
  path the phase exists to close; (b) keep `&'static str` and rely on the doc test — rejected,
  a typo produces a new series rather than a failure, and the doc test only iterates known
  scopes so it cannot see an unknown one.
- **Blast radius if wrong:** an operator's `challenge_per_agent` alert loses volume to
  `challenge_global`. Disclosed in the CHANGELOG under `### Changed` and in an operator note in
  `docs/HTTP-API.md` next to the metric table, both stating the direction of the shift.
- **Status:** UNCONFIRMED (re-checked U-507 2026-09-13; unfalsifiable from this repo). **Settled by:** an operator confirming nothing alerts on the old series. **Owner:** operators. The label rename is a deliberate, documented break of an existing
  series; whether any deployment actually alerts on `challenge_per_agent` is not knowable here.


## H-A / P5 — A1a: the publish bucket is charged on success, not on attempt

- **Plan:** plans/h-a-wire-surface-observability.md (phase P5)
- **Assumed:** that `check`'s combined test-and-charge, run on an unverified `req.agent_id`, is a
  security defect in the WRITE half only, and that the pre-pipeline rejection the comment at
  `context.rs` defends needs only a READ.
- **Chose:** `peek` (read-only, never inserts, never rolls the window over) at the original call
  site; `record` (infallible charge, reproduces the rollover) adjacent to the canonical
  `record_publish("inserted")` success marker, plus a second `record` on the playground replay
  early-return so all four branches charge replays identically.
- **Decision — document the concurrency bound, do NOT reserve.** The enforced bound is
  `limit + concurrent in-flight publishes for that agent`. A reservation would need somewhere to
  live, reintroducing exactly the attacker-keyed map growth that `peek`-not-inserting removes,
  and would contradict the `peek_does_not_create_a_bucket` guard. In-flight publishes are
  additionally bounded by server connection concurrency, not by the attacker alone.
- **Decision — disclose the late-failure throttling gap, do NOT commit to a fix here.** Filed as
  **#242**. The two available shapes are per-branch charge sites (clean at only one of four
  branches, because verification happens inside the SDK call on the blocking pool) and post-hoc
  error classification (a denylist over a `#[non_exhaustive]` enum that fails OPEN as variants
  are added).
- **CORRECTION TO THE PLAN'S OWN PREDICTION, found by falsification.** The plan stated that an
  "increment only" `record` would accumulate across windows and "permanently trip a bucket" —
  i.e. fail closed. Against this `peek` it does the OPPOSITE and fails **open**: `peek` reads an
  expired window as count 0 independently, so a `record` that never advances `window_start`
  leaves every later `peek` seeing a long-expired window and returning `Ok` forever. The limiter
  silently stops limiting after the first window. My first guard asserted only that budget was
  available again after the window turned, which is true under BOTH implementations, so the
  mutation ran green; the guard was rewritten to spend the full budget inside the new window and
  to assert `window_start` and `count` directly.
- **Blast radius if wrong:** a publish limiter that under- or over-charges. Bounded by the guards
  above, each falsified individually.
- **Status:** CONFIRMED as deliberate trades (U-507, 2026-09-13) — the entry classifies them itself and both still hold: the concurrency bound and the #242 gap are deliberate, documented
  trades, not settled questions.

## `search_in_tenant`'s default treats the backend as untenanted rather than refusing

- **Plan:** `plans/h-h-tenant-aware-search.md` (H-H Phase 1)
- **Assumed:** the new trait method must be *defaulted*, not required — and what the default
  does is a security decision rather than a compatibility formality.
- **Measured first:** `ExtendedRegistryStore` has exactly three implementors, and one of them
  is `MemoryStore` at `crates/acdp-registry-server/src/memory_ext.rs:99`, in a crate outside
  this lane's claim. A required method would not compile and could not be fixed from here.
  `MemoryStore` overrides **no** tenant method, so it inherits `tenant_of_ctx`'s default of
  `Some("default")`.
- **Chose:** follow the precedent `tenant_of_ctx` set in its own doc — satisfy the trait
  "without claiming a wrong answer". `None` and `Some(RESERVED_TENANT)` delegate to
  `RegistryStore::search`; any other tenant returns the empty page with `total_estimate: 0`
  and no cursor. On a backend where every row is `default`, both answers are *correct* under
  the one contract the method states: `Some(t)` means exactly the rows whose tenant is `t`.
- **Alternatives:** (a) delegate unconditionally — rejected outright; it hands tenant A's rows
  to a caller asking for tenant B, which is a silent cross-tenant disclosure in the default
  path, the worst possible place for one. (b) `Err(NotImplemented)`, which has real precedent
  in `MemoryStore::list_contexts` — rejected because a correct answer genuinely exists here,
  so refusing would break the memory backend the moment H-H-w wires the handler.
- **Blast radius if wrong:** a future backend that records tenants but forgets to override
  would serve the default's answer. Bounded by the doc comment stating the override
  obligation, and by both SQL backends overriding it in phases 2–3. Reversible in one commit.
- **Status:** CONFIRMED (2026-09-12) — reconcile reopened a 4th option (fail closed for every `Some`) and rejected it: it would make the memory backend diverge from both SQL backends on `Some(RESERVED_TENANT)`. Residual risk recorded. See DECISIONS.md H-H #1.

## The store does NOT re-enforce the reserved-tenant rejection

- **Plan:** `plans/h-h-tenant-aware-search.md` (H-H Phase 1)
- **Assumed:** `RESERVED_TENANT`'s doc says `"default"` MUST NOT be assertable as a real
  tenant, so `search_in_tenant` might owe a second check.
- **Verified, not assumed:** it already has exactly one enforcement point —
  `reject_reserved_tenant` at `crates/acdp-registry-core/src/handlers/context.rs:189`, which
  refuses it from header or token, so untenanted rows stay reachable only through the
  *absence* of an assertion. So `Some(RESERVED_TENANT)` cannot arrive from the HTTP path.
- **Chose:** do not duplicate the check in the storage layer. It is an authorization
  judgement, and `list_contexts` applies its predicate for any `Some` — adding a rejection to
  `search_in_tenant` alone would manufacture exactly the kind of path divergence unit H-B
  existed to remove.
- **Alternatives:** reject it in the store as defence in depth — rejected as an auth decision
  in the wrong layer, and inconsistent with the sibling method.
- **Blast radius if wrong:** a non-HTTP caller (a background job, a future transport) passing
  `Some("default")` would receive the untenanted bucket. On the SQL backends that is
  `WHERE tenant_id = 'default'` — the untenanted bucket precisely, not everything — so the
  exposure is the aliasing `RESERVED_TENANT` warns about, reachable only by bypassing the
  handler. Cheap to add later if a second caller ever appears.
- **Status:** CONFIRMED (2026-09-12) — the premise is now VERIFIED, not assumed: search resolves tenancy via `tenant_for_request` (`context.rs:939`), which calls `reject_reserved_tenant`. See DECISIONS.md H-H #2.

## `tokio` added as an unconditional dev-dependency of `acdp-registry-store`

- **Plan:** `plans/h-h-tenant-aware-search.md` (H-H Phase 1)
- **Assumed:** testing an `async` default impl needs a runtime, and the existing `tokio` dep
  is optional behind `test-support` so it is not available to a plain `cargo test`.
- **Chose:** add `[dev-dependencies] tokio = { workspace = true }`. Dev-dependencies never
  reach a downstream build, and the workspace already pins `features = ["full"]`, so this adds
  no new feature surface and nothing to the shipped artifact.
- **Alternatives:** (a) hand-poll the future with a no-op waker to avoid the dep — rejected as
  obscure for no gain; (b) put the tests behind `test-support` — rejected, it would mean the
  default impl's guard does not run in a normal `cargo test`, which is where it matters most.
- **Blast radius if wrong:** none to consumers; a dev-only dependency on a crate already in
  the tree.
- **Status:** CONFIRMED (2026-09-12) — dev-only, nothing reaches the shipped artifact. See DECISIONS.md H-H #3.

## No new index for the tenant-scoped search path

- **Plan:** `plans/h-h-tenant-aware-search.md` (H-H Phase 2, assign item 5)
- **Assumed (by the assign):** the search path needs a composite index the way
  `list_contexts` did.
- **Measured instead:** `EXPLAIN QUERY PLAN` over 2000 rows across 20 tenants, after
  `ANALYZE`. Tenant-scoped: `SEARCH contexts USING INDEX idx_ctx_tenant (tenant_id=?)`.
  Tenant-spanning: `SCAN contexts`. Both then `USE TEMP B-TREE FOR ORDER BY`.
- **Chose:** add no migration. The predicate is already index-assisted, and the `ORDER BY`
  cannot be index-satisfied on this query in either case because `COUNT(*) OVER ()` must
  materialize the full matching set first — so the temp B-tree is pre-existing rather than
  introduced here, and a new index would not remove it.
- **Alternatives:** add a composite `(tenant_id, created_at DESC)` index — rejected: one
  already exists (`idx_ctx_tenant_created`, migration 006/007) and the planner does not
  choose it, so a *third* index would be redundant storage and write cost for no measured
  gain. Notably the plan predicted that index would be the one used; it is not.
- **Blast radius if wrong:** a busy mixed-tenant registry could see slower tenant-scoped
  searches than necessary. Bounded: the alternative is strictly additive later, and the
  measurement above is the baseline to re-run against. Reversible.
- **Status:** CONFIRMED (2026-09-12) — on the measurements, both backends and both selectivities. See DECISIONS.md H-H #4.

## The tenant parity fixture isolates by unique tenant name, not by cleanup

- **Plan:** `plans/h-h-tenant-aware-search.md` (H-H Phase 3)
- **Assumed initially (WRONG):** that a fixed pair of tenant names was fine, because every
  other assertion in `parity.rs` uses fixed seeds.
- **What actually happened:** the pg suite went red on the second and third runs —
  `total_estimate` `Some(9)` where `Some(3)` was expected. Postgres is a **persistent**
  fixture, so rows accumulate; SQLite hid it behind a fresh tempfile per run. The sibling
  assertions survive this only because they test *membership* (`search_contains`) rather than
  an exact count, which this one cannot do — the tenant-scoped count IS the property under
  test.
- **Chose:** derive both tenant names from a nanosecond timestamp so each run occupies its own
  namespace. Verified by three consecutive pg runs, then re-falsified to confirm the isolation
  did not weaken the guard.
- **Alternatives:** (a) delete the fixture's rows afterwards — rejected: a failing assertion
  would skip the cleanup and poison the next run, which is how a flake becomes permanent;
  (b) assert `>=` instead of `==` — rejected, it would no longer detect a cross-tenant count
  oracle, which is the A2 finding this exists to pin.
- **Blast radius if wrong:** test-only. A clock moving backwards between runs could collide,
  which needs a same-nanosecond collision to matter.
- **Status:** CONFIRMED (2026-09-12) — 3 consecutive green pg runs plus a re-falsification proving the fix did not neuter the guard. See DECISIONS.md H-H #5.

## `cursor.rs`'s disclosure claim is made per-dimension rather than restored

- **Plan:** `plans/h-h-tenant-aware-search.md` (H-H Phase 4)
- **Assumed:** that once the predicate moved into SQL, the original claim — a cursor holds
  only "an identifier the requester was already shown" — could simply be restored.
- **Chose:** not to restore it. It is true for §4.5 visibility (in SQL on both backends, so
  the scan never touches a row the requester may not see) and true for tenancy **only for
  callers of `search_in_tenant`**. The HTTP handler still calls the protocol-level
  `RegistryStore::search` and filters afterwards, so on the live path the claim remains false.
  The docs now state the guarantee per dimension and name the mechanism: a cursor discloses
  nothing beyond what the *scan that produced it* was allowed to see.
- **Alternatives:** (a) restore the original sentence — rejected: it would be false for the
  deployed path, and a subtly-false comment is worse than a known-false one because it reads
  as verified; (b) delete the paragraph — rejected: the anchor-vs-served distinction is
  exactly what a future reader needs in order not to reintroduce this.
- **Blast radius if wrong:** documentation only, but it is the doc a future filter author will
  read when deciding whether their filter can run post-query. Getting it wrong reintroduces
  the leak.
- **Status:** CONFIRMED (2026-09-12) — it states the general rule and the actionable consequence, not just an enumeration. See DECISIONS.md H-H #6.

## H-A / P3 — #218 resolved: `/metrics` answers `no-store` on every arm (200, 401, 405)

- **Plan:** plans/h-a-wire-surface-observability.md (phase P3)
- **Assumed:** that `/metrics` being authorization-relative (200-vs-401 gates on
  `metrics.bearer_token`) makes it unsafe for a shared cache, and that the 401 arm matters more
  than the 200 arm.
- **Chose:** attach `no-store` with a `route_layer` on a one-route sub-router merged into `aux`,
  using `overriding`. `route_layer` covers the 401 and 405 arms, which a handler-side header
  would not. Scoped to the route rather than the group because `aux` also carries
  `/.well-known/jwks.json` and `/.well-known/did.json`, which keep `public, max-age=300`.
- **Alternatives:** (a) `.layer()` on `aux` wholesale — rejected, and *demonstrated* to clobber
  jwks.json's `public, max-age=300`; (b) setting the header in the handler — rejected, misses
  the 405 arm; (c) `if_not_present` — rejected, the documented guarantee is unconditional so the
  layer must be too, matching `/admin/*`.
- **Blast radius if wrong:** a scraper that relied on caching `/metrics` sees more origin hits.
  Prometheus does not cache. Reversible in one commit.
- **Status:** CONFIRMED — closes #218. The classification row was **replaced**, not deleted: the
  prior guidance in `http_integration.rs` and in this file said deleting it "is how the fix
  announces itself", which is wrong and was demonstrated to fail the build with
  `["/metrics"] -- carry no declared cache posture`. Both places corrected.

## H-A / P3 — the prescribed falsification for A7's route-scoping could not fire

- **Plan:** plans/h-a-wire-surface-observability.md (phase P3, Tests + falsification)
- **Assumed:** that `every_well_known_document_keeps_public_caching` in `http_integration.rs`
  could serve as the guard proving the `/metrics` `no-store` is route-scoped rather than
  group-scoped, by reddening when the layer is applied to `aux` wholesale.
- **Chose:** it cannot, and this was verified rather than reasoned. That harness runs with
  `metrics.enabled = false`, so the entire `if metrics_enabled { .. }` block — the code the
  mutation edits — never executes there. I applied the mutation with an assert that it landed,
  confirmed by reading the changed lines back, and the test still passed. A probe aimed at code
  its harness never runs. The real guard,
  `metrics_no_store_is_scoped_to_the_route_not_the_aux_group`, lives in
  `metrics_integration.rs` where metrics are on, and reddens with
  `left: Some("no-store") / right: Some("public, max-age=300")`.
- **Alternatives:** build a metrics-enabled config inside `http_integration.rs` — possible but
  wrong home; that file's harness deliberately has metrics off and `metrics_integration.rs`
  already owns this surface.
- **Blast radius if wrong:** none — the working guard exists and is falsified. Recorded because
  the *plan* asserted a falsification that could not fire, which is the same defect class this
  unit exists to remove, one level up: an unfireable probe presented as evidence.
- **Status:** CONFIRMED

## Guarantees are falsified per assertion, via accumulation rather than separate tests

- **Plan:** `plans/h-h-tenant-aware-search.md` (H-H Phase 5, CHARTER rules 51–52)
- **Assumed initially (WRONG):** that one test asserting four related properties was adequate
  coverage of those four properties.
- **What the audit found:** `assert!` aborts at the first failure, so only 2 of phase 1's 4
  assertions had ever been shown to fail, and `matches.is_empty()` was **structurally
  unfalsifiable** — the sentinel returned no rows, so no mutation could make it fail.
- **Chose:** two different remedies for two different shapes. For the unit tests, **one test per
  guarantee** — cheap, and a single mutation then produces four independent verdicts. For the
  shared cross-backend assertion, **accumulate violations and report them all at once**, because
  splitting it would have meant 4 public functions × 2 backends and a thin-caller file that is
  supposed to stay thin.
- **Alternatives:** split the parity assertion into one function per guarantee — rejected: it
  multiplies the per-backend caller boilerplate the module's own docs warn against, and
  accumulation achieves the same property (every guarantee evaluated every run) with a strictly
  better failure message.
- **Blast radius if wrong:** a guarantee could regress while the suite stays green. Bounded by
  the recorded mutation matrix, which shows all six guarantees firing on both backends.
- **Status:** CONFIRMED (2026-09-12) — 6/6 guarantees fire on both backends; matrix in the plan and PROGRESS.md. See DECISIONS.md H-H #7.

## `visible_ctx_ids` returns a set rather than a mask aligned to the input

- **Plan:** `plans/h-i-s-batched-visibility.md` (H-I-s Phase 1)
- **Assumed:** that callers want "which of these may I disclose?" rather than "answer per input
  position".
- **Chose:** `HashSet<String>`. Order-free, duplicate-safe, and it reads correctly at the call
  site (`if visible.contains(id)`).
- **Alternatives:** a `Vec<bool>` parallel to the input — rejected because it silently
  mis-associates if any caller ever reorders, filters or de-duplicates its input between building
  the slice and reading the mask, and nothing in the type system would catch that. A
  `HashMap<String, bool>` — same information as the set, with absent-vs-false as a second way to
  say no.
- **Blast radius if wrong:** low; the method is crate-local, unreleased, and has one prospective
  caller (H-I-w). Changing the return type is a compile error, not a silent behaviour change.
- **Status:** CONFIRMED (2026-09-12) — see DECISIONS.md H-I-s #5.

## The default impl is behaviour-preserving (N calls), not fail-closed

- **Plan:** `plans/h-i-s-batched-visibility.md` (H-I-s Phase 1)
- **Assumed:** that a defaulted method on this trait must be *correct* for an untenanted backend,
  not merely compilable.
- **Chose:** the default does exactly what the caller did before — one `RegistryStore::get` per
  id, then the §4.5 rule, then the tenant gate. Same answers, N round-trips.
- **Why defaulted at all (a boundary condition, not a preference):** `ExtendedRegistryStore` has
  three implementors and one is `MemoryStore` in `crates/acdp-registry-server/`, outside this
  lane's claim. A required method would be a compile break in a file this unit may not edit.
- **Alternatives:** fail closed (return an empty set) for every call — rejected, and this was
  settled in H-H rather than re-derived: it makes the memory backend disagree with both SQL
  backends about rows it can see perfectly well, which is the H-B divergence defect reintroduced
  in the name of safety. Return `Err(NotImplemented)` — rejected: turns a working backend into a
  500 on a read path.
- **Blast radius if wrong:** a backend inheriting the default is slow, not wrong. The failure mode
  chosen is *cost*, which is observable, over *silence*, which is not.
- **Status:** CONFIRMED (2026-09-12) — see DECISIONS.md H-I-s #2.

## The §4.5 rule is expressed a third time, in Rust, and contained rather than eliminated

- **Plan:** `plans/h-i-s-batched-visibility.md` (H-I-s Phases 1 and 4)
- **Assumed initially (WRONG):** that the authoritative rule could be called rather than restated.
- **What checking found:** `RegistryServer::retrieve` is `store.get()` + `can_retrieve(...)`, and
  `can_retrieve` is **`pub(crate)`** in `acdp-server-0.13.1` (`src/registry/server.rs:1257`).
  `RegistryStore` — the trait this crate can reach — offers only a raw `get`. So the duplication
  is **forced**, not chosen.
- **Chose:** name it (`retrieve_visible`), document the duplication where it lives, and contain it
  with a three-way differential test in which the Rust default and both SQL overrides answer for
  the same rows.
- **Alternatives:** have the SQL backends call `retrieve_visible` per row — rejected: that is the
  N+1 this unit exists to remove. Vendor or fork upstream to widen `can_retrieve`'s visibility —
  rejected as disproportionate, and it is an upstream change, so it becomes an issue rather than a
  local edit. Ask upstream to make it `pub` — worth doing, and the better long-term fix; noted for
  a follow-up rather than blocking here.
- **Blast radius if wrong:** the rule drifts in one of three places and a disclosure boundary moves
  silently. That is the highest-consequence assumption in the unit, which is why the containment
  is a test rather than a comment: mutating the default alone reddens both backends' suites
  (verified, mutations E1/E2).
- **Status:** CONFIRMED (2026-09-12) — see DECISIONS.md H-I-s #1.

## SQLite batches 900 ids per query; Postgres does not chunk at all

- **Plan:** `plans/h-i-s-batched-visibility.md` (H-I-s Phases 2 and 3)
- **Assumed:** that SQLite's default host-parameter ceiling (999) is the binding constraint, and
  that the five disclosure binds plus the tenant bind need headroom beside the ids.
- **Chose:** 900 for SQLite. Postgres binds the whole list as one array (`= ANY($3)`) so it has no
  ceiling to respect and does not chunk.
- **Why chunk at all when the caller's page cap is 256:** the method is public and takes an
  arbitrary slice. `LOG_ENTRIES_PAGE_CAP` bounds the *handler*, not this method, so relying on it
  would be a correctness bug waiting for a second caller.
- **Alternatives:** a temp table or JSON-array parameter on SQLite — more machinery than a
  bounded `IN` list needs at this size. Refuse an oversized slice — pushes a backend detail onto
  every caller.
- **Blast radius if wrong:** a too-large chunk fails loudly as a SQL error on an input no current
  caller produces. Not silent.
- **Status:** CONFIRMED (2026-09-12) — see DECISIONS.md H-I-s #6.

## AC1 is satisfied transitively, not by a direct two-backend comparison

- **Plan:** `plans/h-i-s-batched-visibility.md` (H-I-s Phase 3)
- **Assign asked for:** a parity test proving SQLite and Postgres return **identical** visibility
  sets for the same fixture.
- **What is delivered, precisely:** not a direct comparison, and it cannot be — `parity.rs`
  deliberately does not depend on either backend crate (they depend on it), because importing both
  to build a "both backends" test would invert the dependency graph; its module docs say so. Both
  backends are instead compared against the **same third implementation**
  (`visible_by_n_calls`, one function shared by both suites), so `sqlite == reference` and
  `pg == reference` yields `sqlite == pg`.
- **Alternatives:** a new test crate depending on both backends — possible, and the only way to
  get a literal side-by-side comparison; rejected as disproportionate when transitivity gives the
  same guarantee. Duplicate the expectations per backend — exactly what this module exists to
  prevent.
- **Blast radius if wrong:** none to the code; the risk is *reporting*. An AC recorded as "met"
  when the test performed is a different (equivalent) one is an unverified claim hiding in
  supporting detail, which is why this entry exists rather than a checkmark.
- **Status:** CONFIRMED (2026-09-12) — see DECISIONS.md H-I-s #4.

## Group (a)'s oracle and group (e)'s seam check share `retrieve_visible`

- **Plan:** `plans/h-i-s-batched-visibility.md` (H-I-s Phases 2 and 4)
- **Assumed:** that a differential oracle plus a three-way seam check are independent guards.
- **They are not fully independent, and this states it rather than implying otherwise:** the
  N-call reference (`visible_by_n_calls`) and the trait default both apply `retrieve_visible`. A
  mutation of *that function* therefore moves the reference and the default together, so group (a)
  cannot detect an error in the Rust expression of the rule — only a disagreement between SQL and
  Rust.
- **What actually covers that gap:** the 13 unit mutations against `retrieve_visible` itself
  (phase 1), which pin it to §4.5 absolutely, plus the three absolute assertions inside the parity
  suite (audience-sees-private, contributor-does-not, retracted-still-visible) which do not go
  through the reference at all. A pure differential passes when both sides are wrong the same way;
  those absolute assertions are why this suite does not.
- **Blast radius if wrong:** a shared error in the Rust rule would be invisible to the
  differential. Bounded as above, and the absolute assertions are the load-bearing part.
- **Status:** CONFIRMED (2026-09-12) — see DECISIONS.md H-I-s #3.

## H-A / P7 — A3: extractor rejections get the §5 envelope at their original status

- **Plan:** plans/h-a-wire-surface-observability.md (phase P7)
- **Measured before changing anything** (real router, auth enabled): malformed body -> 400;
  missing/wrong `Content-Type` -> 415; schema mismatch -> 422; `?limit=abc` -> 400. All four
  already carried `application/acdp+json` over PLAIN TEXT with no `error.code`. Also confirmed
  `application/acdp+json` is ACCEPTED (200), so the RFC-mandated media type is not what 415s.
- **Chose:** a local `AcdpRejection` implementing `IntoResponse` directly, with `AcdpJson<T>` /
  `AcdpQuery<T>` newtypes using it as their `Rejection`. Envelope built from the public
  `WireError`/`WireErrorBody` so the shape -- including `details` ABSENT rather than `null` --
  cannot drift from `RegistryError::into_response`.
- **Alternatives:** (a) map onto `RegistryError` -- rejected, it has no 415-bearing variant so
  415 and 422 would both collapse to 400, which would be an artifact of the mechanism rather
  than a decision; (b) `impl From<JsonRejection> for RegistryError` -- the orphan rule forces it
  into `acdp-registry-types`, a different crate than the problem; (c) `axum-extra`'s
  `WithRejection` -- not a dependency, and adding one enters the `deny.toml` gate for something
  solvable in forty lines locally.

### DIVERGENCE: the plan's prescribed shape contradicts the plan's own acceptance criterion

The plan specifies `AcdpRejection(rej.status(), code_for(&rej), rej.body_text())`. Its
acceptance criterion 2 says **no response body may contain "Failed to parse", "Failed to
deserialize", or a serde type path**. `rej.body_text()` returns exactly those strings -- so the
prescribed shape guarantees the criterion fails. Implemented AC2; messages are this registry's
own and stable. Beyond AC2 there is an independent reason: axum's and serde's wording is theirs
to change, so echoing it onto the wire grows an accidental contract that breaks on a dependency
bump.

### OPEN — escalated, NOT decided here: the §5 `code` for a 415 *(re-verified U-507 2026-09-13: still open; `acdp_wire_code` emits 24 codes and none is a media-type failure, so minting one remains unavoidable. **Owner:** the spec/canon holder, not this repo.)*

- **The question:** enveloping a 415 requires a `code`, because `WireErrorBody::code` is a
  required `String` -- there is no "envelope without a code".
- **Why it is not mine to settle:** the canonical registry
  (`acdp_primitives::error::AcdpError::from_wire_error`) is 25 closed codes and none describes a
  media-type failure. This repo emits 24 codes and **every one is inside that set** -- verified
  by set difference, which is empty. So answering means minting a code the canon lacks: a policy
  question about this project's relationship to upstream, not a technical one.
- **Recommendation on record:** `unsupported_media_type`. An unrecognised code routes to
  `AcdpError::Registry(wire)`, documented as "for forward compatibility", preserving status and
  message and losing only the typed variant. The canonical alternative, `schema_violation`, is
  documented as "malformed body, missing field, schema mismatch" -- and on a 415 the body was
  never parsed, so it would state something false, make 415 indistinguishable from 400 at the
  code level, and be unfixable later without a breaking change once clients code against
  `AcdpError::SchemaViolation`. The canon also already has an `unsupported_*` family
  (`unsupported_algorithm`), so the name is in its own idiom.
- **Held, visibly.** The 415 keeps its existing un-enveloped body until the ruling.
  `marker_the_415_rejection_is_not_yet_enveloped_pending_a_ruling` pins the current behaviour and
  fails the moment the ruling is applied, with a failure message instructing its own deletion --
  so the hold cannot outlive the question by being forgotten.
- **Status:** UNCONFIRMED — the 415 code is with the project owner. Everything else in this
  phase is settled and shipped.


## H-A / P7 follow-up — the 415 ruling applied: `unsupported_media_type` minted

- **Resolves** the OPEN item recorded under "H-A / P7 — A3" above. That entry's status was
  UNCONFIRMED pending a ruling; the ruling landed and this entry closes it.
- **Decided by the project owner:** emit `unsupported_media_type` at the unchanged `415`, and
  file an upstream issue asking the canon to adopt it. Both done — acdp-rs#268, and the standing
  precedent is Decision 16 in `DECISIONS.md`.
- **The marker test did its job and is gone.**
  `marker_the_415_rejection_is_not_yet_enveloped_pending_a_ruling` pinned the un-enveloped
  behaviour and would have failed the moment the ruling was applied, with a failure message
  instructing its own deletion. Deleted deliberately, which is what it asked for. One assertion
  inside it was NOT about the ruling — that `application/acdp+json` is accepted rather than
  415'd — so it was rehomed as `the_acdp_media_type_is_accepted_not_rejected` rather than deleted
  with its host. Worth noting because deleting a marker wholesale is the obvious move and would
  have silently dropped a real guard.
- **One deviation from my own preference, flagged not acted on.** The ruling specified the
  message verbatim as `` Expected request with `Content-Type: application/json` ``. My P7 work had
  replaced that with wording naming `application/acdp+json` first. I implemented the ruling as
  stated. The recommendation stands and is raised with the coordinator: the ruled message names
  only `application/json`, while RFC-ACDP-0007 mandates `application/acdp+json`. Both are
  accepted, so the message is INCOMPLETE rather than false, and a client following it literally
  sends a media type that works but is not the one the RFC names. Deference was cheap here
  precisely because the message is not false; had it been false I would have raised it before
  shipping rather than alongside.
- **Status:** CONFIRMED (2026-09-12) — code minted per ruling, upstream issue filed, precedent
  recorded, marker deleted.


## H-A / P8 — A2: tenant-scoped search omits `total_estimate`

- **Plan:** plans/h-a-wire-surface-observability.md (phase P8)
- **Assumed:** that a caller asserting `X-Tenant-Id` must not learn the size of the population
  outside their tenant, and that `total_estimate` — computed in the store, before the handler's
  post-query tenant filter — disclosed exactly that.
- **Chose:** omit the key when `requested_tenant.is_some()`. Not recomputed: an honest
  tenant-scoped count needs the predicate in the store's SQL, a different change in a different
  crate. `skip_serializing_if` makes the key ABSENT rather than `null`/`0`, so "withheld" cannot
  be misread as "none found".
- **Deliberately not unconditional.** An un-scoped caller is entitled to the count, and omitting
  it for everyone passes the tenant test while breaking conformance's `want_total_estimate` and
  the vis-007 `total_estimate == 0` fixture. Pinned by
  `search_still_reports_total_estimate_without_tenant`.

### CORRECTION to the plan's description of the residual attack

> **This section is ITSELF corrected — see "CORRECTED — an earlier version of this entry
> overstated it" further down, under the P9 heading.** The measurement below is right about the
> fixture and wrong about the generalization: the refill loop has TWO exits, and this section
> only accounts for one. Kept unedited because the reasoning is the record.

The plan states a tenant-pinned caller "can walk cursors at `limit=1`" to recover foreign
`ctx_id`s. **Measured: at `limit=1` nothing leaks.** Every anchor returned at `limit=1` is one of
the caller's own rows.

The refill loop is why. A foreign anchor only escapes when `accumulated` reaches `target` on the
page whose last raw row is foreign. With `target == 1` a store page is one row, so a foreign row
is dropped by the retain, `accumulated` stays below `target`, the loop refills, and the
foreign-anchored cursor is consumed internally. **The filter that hides the row also hides the
anchor.** At `target >= 2` a page can hold an own row followed by a foreign one, `accumulated`
reaches `target` there, the loop stops, and that cursor is returned. Confirmed at `limit=2`;
`limit=3` returned no cursor at all for the fixture.

This matters beyond pedantry: anyone reproducing the finding at the size the plan names would
conclude the leak does not exist. The marker test pins `limit=2`.

- **Blast radius if wrong:** a client that depended on `total_estimate` under a tenant header now
  sees the key absent. That is the intended behaviour change and it is in the engineering log.
- **Status:** RESOLVED (U-507, 2026-09-13) — **A2 is closed, and this entry specified exactly how to tell.**
  It said A2 "must not be described as closed until the store-side predicate lands and that test is deliberately deleted". Both happened in the same commit: `f8a866d` (#259, *"scan inside the tenant so the cursor cannot anchor on a foreign row"*) landed the tenant-aware store search and deleted `search_cursor_oracle_remains_open_for_tenant_scoped_caller`. Established with `git log -S` on the test name, not by reading a changelog.
  **The `limit=1` claim in this section is superseded** by the correction under P9: a foreign
  anchor also escapes at ANY limit once the refill loop exhausts `SEARCH_REFILL_MAX_PAGES`,
  because `cursor` is assigned before that break. This fixture is too small to reach that exit.

## H-A P9 (A4) — `/log/entries` answers a page with one visibility query

- **Assumption:** passing `anonymous_public_reads: true` to `visible_ctx_ids` from
  `/log/entries` preserves the endpoint's existing behaviour, rather than ignoring the
  operator's configuration.
- **Why it holds, measured rather than reasoned:** the call this replaces went through
  `RegistryServer::retrieve`, which does not consult that flag. Probed on the wire with
  `auth.enabled = true` and `anonymous_public_reads = false`, an anonymous
  `GET /contexts/{ctx_id}` on a public context still returns 200. So passing the config value
  — the obvious-looking thing, and what a future reader will reach for — would make
  `/log/entries` STRICTER than the retrieve it is defined to mirror, and would break
  RFC-ACDP-0012 §8.3's own rule that `leaf` is present exactly where the requester could
  retrieve the context. The flag gates `search`/`list_contexts`, which is how
  `docs/ARCHITECTURE.md` and `docs/MULTI-TENANCY.md` describe it.
- **Blast radius if wrong:** public leaves would disappear from the log for anonymous
  auditors on a deployment that sets the flag — a silent transparency regression, not an error.
- **Status:** **RETRACTED — the claim above is FALSE and the probe that "confirmed" it was
  invalid.** Kept in full, unedited, because the reasoning is the record. The retraction is the
  section headed "RETRACTION of the `anonymous_public_reads: true` assumption above", two
  entries further down in this same P9 block.

- **Assumption:** the old handler-side tenant fallback
  (`tenant_of_ctx(...).unwrap_or_else(|| "default")`) had no behaviour to preserve.
- **Why it holds:** two independent reasons, both verified. (1) `tenant_id` is
  `TEXT NOT NULL DEFAULT 'default'` (`crates/acdp-registry-sqlite/migrations/007_tenant_id.sql:11`),
  so `tenant_of_ctx` returns `None` only for a row that does not exist — and such a row already
  failed the visibility check above it. (2) `"default"` is a RESERVED sentinel:
  `reject_reserved_tenant` (`handlers/context.rs:190`) refuses it from the header AND from a
  token claim, so `requested_tenant` is never `Some("default")` and the comparison was
  unreachable in the affirmative. Probed: `X-Tenant-Id: default` on `/log/entries` returns 400
  `schema_violation` "'default' is a reserved tenant sentinel".
- **Note on the plan:** this phase's plan asked for a test that a missing row "still resolves to
  `default`". Written to that premise it would have asserted nothing. The test that ships
  (`log_entries_rejects_the_reserved_default_tenant`) pins (2) instead — the property that
  actually makes `AND tenant_id = ?` safe, since the untenanted bucket cannot be named.
- **Status:** CONFIRMED.

- **Assumption (NOT an equivalence — recorded because it is a real behaviour change):** the
  reordering is boolean-identical on the **success path only**.
- **Why:** these are fallible store reads. Today a per-record error could surface only for rows
  that were already visible — the old loop never asked about a row it was about to hide. The
  batched call covers the whole page including invisible rows, so a store error can now surface
  on a page where it previously could not.
- **Blast radius:** a page that used to return 200 with some leaves omitted can now return 500.
  Strictly more honest, but it is a change, and calling this a pure refactor would be wrong.
- **Status:** **CORRECTED TWICE — the "Why" and "Blast radius" above are FALSE, and so was the
  first correction.** Both kept unedited; the corrected statement is in this bullet, not
  elsewhere.
  - The original claim — a store error could surface "only for rows that were already visible" —
    is false. `server.retrieve` **was** the visibility check, so the old path ran a full
    `RegistryStore::get` (events, `reconcile_retraction`, `body_json` decode) on EVERY record.
  - The first correction said the change therefore runs in **both** directions, with
    `tenant_of_ctx` newly reachable on hidden rows. Also false, and it was adopted without being
    checked against any implementation. There is none where it holds: SQLite and Postgres never
    call `tenant_of_ctx` from `visible_ctx_ids` (the predicate is `AND tenant_id = ?` in the same
    statement), and the default trait impl gates it behind
    `if !retrieve_visible(…) { continue; }` — the identical gate the old handler had.
  - **The correct statement:** the change is one-directional. Strictly **fewer** classes of store
    error can reach the caller than before, because the batched query is
    `SELECT ctx_id FROM contexts WHERE …` and never deserializes a body or reads events. The
    "strictly more honest" framing had it backwards.
  - **Scope, stated because the two failures above were both over-generalizations.** "Fewer" is a
    property of the SQLite and Postgres *overrides*, not of `visible_ctx_ids` as a trait method:
    the default impl still runs a full `get` per id. It holds for every backend that can serve
    `/log/entries` today (`MemoryStore` does not override `log_entries`, so the route is
    `NotImplemented` there), but not necessarily for an external implementor of this published
    trait that overrides `log_entries` and not `visible_ctx_ids`.
  - Worth keeping for its own sake: a correction is not self-verifying. The first one was written
    to fix a false claim and was itself false, and it read as more trustworthy *because* it was a
    correction.

### RETRACTION of the `anonymous_public_reads: true` assumption above

- **What was claimed:** that passing a literal `true` preserved `/log/entries` behaviour, and
  that passing the configured value would make the endpoint *stricter* than the retrieve it
  mirrors. Status was recorded as "CONFIRMED by wire probe".
- **What is actually true:** the opposite direction. `RegistryServer::retrieve` ->
  `can_retrieve` gates its public arm on `self.caps.anonymous_public_reads || requester.is_some()`
  — off the `CapabilitiesDocument` baked in at `try_new`, **not** off `RegistryConfig`. The
  binary copies `cfg.auth.anonymous_public_reads` into caps
  (`crates/acdp-registry-server/src/main.rs`), and `AuthConfig::default()` ships that flag
  `false` (`crates/acdp-registry-types/src/config.rs`). So on the shipped default a hardcoded
  `true` **discloses** every public `leaf` to an anonymous caller that the old code refused —
  and because `auth.enabled` also defaults to `false`, on that config every caller is anonymous.
- **Why the probe was invalid, which is the part worth remembering:** it flipped
  `cfg.auth.anonymous_public_reads` and observed a 200. The default test harness hardcodes
  `caps.anonymous_public_reads: true` (`tests/http_integration.rs`), and the caps/config split is
  *already documented in this repo* as GAP 3 (`tests/common/mod.rs`). The probe therefore
  measured the harness's own split and could not have returned anything else. A measurement that
  cannot fail is not evidence, and calling it a "wire probe" made it read as stronger than the
  reasoning it replaced.
- **How it was caught:** the pre-merge verification gate, reading the code rather than trusting
  the claim. Not by a test — no test could see it, because every test in the suite inherits the
  harness caps.
- **Fix:** the flag is read from `state.server.capabilities().anonymous_public_reads`, the same
  field `retrieve` reads, so the two are equal by construction rather than equal while config and
  caps agree. `log_entries_honours_anonymous_public_reads_from_caps` overrides the CAPS and fails
  against the hardcoded value; the disclosure was reproduced before the fix was written.
- **Status:** CONFIRMED (the corrected statement), with a regression test that has been shown to
  fail without the fix.

- **Assumption (CORRECTED — an earlier version of this entry overstated it):** the `/search`
  cursor oracle is closed at `limit=1`.
- **What is true:** the anchor escapes whenever the refill loop stops on a page whose last *raw*
  scanned row is foreign. At `limit=1` the loop keeps refilling, so the filter that hides the row
  also consumes its anchor — that much was measured correctly. But the loop also stops when it
  exhausts `SEARCH_REFILL_MAX_PAGES` (6), and `cursor = resp.next_cursor` is assigned *before*
  that break (`handlers/context.rs`), so with enough consecutive foreign pages a foreign anchor
  escapes at `limit=1` too. The honest statement is "at `limit>=2`, and at any `limit` once the
  refill budget is exhausted" — not "never at `limit=1`". Generalising "every time" from one
  six-row fixture was the error.
- **Status:** RESOLVED (U-507, 2026-09-13) — A2 is no longer PARTIAL; closed by `f8a866d` (#259), see the A2 resolution above. `docs/HTTP-API.md` and the call-site comment now
  state the corrected version.

- **Assumption (latent, accepted knowingly):** dropping the `CtxId::parse` guard is safe.
- **Context:** the old per-record path parsed each `ctx_id` and returned "not visible" for an
  unparseable one before any lookup. `visible_ctx_ids` matches raw strings in SQL, so a
  `log_leaves` row whose `ctx_id` is not a valid `CtxId` but which has a matching `contexts` row
  would now yield a `leaf` where it previously would not.
- **Why accepted:** unreachable today — `ctx_id`s are registry-minted through `CtxId`, and a leaf
  only exists for a row that was committed through publish. Re-adding the guard means parsing
  per record, which is the cost this phase exists to remove.
- **What would make it reachable:** a migration or import path that writes `contexts` rows
  without minting through `CtxId`. Anything of that kind must revisit this.
- **Status:** CONFIRMED (U-507, 2026-09-13) — verified unreachable today, and the trigger above is kept for whoever changes that. No path writes `contexts` rows outside the publish flow: the pg and sqlite migrations that appear to insert into `contexts` insert into **`contexts_fts`**, the FTS shadow table (`002_fts5.sql:15,33`, `013_fts5_porter.sql:44`), and no import or bulk-insert path exists. Recorded so it stays a known latent rather than a rediscovery.

- **Record that would otherwise not ship: how P9's planned acceptance criteria were actually
  met.** `plans/` is gitignored (the literal `plans/` entry in `.gitignore`; not cited by line,
  because the merge in this very PR moved it), so the plan's own status block is
  worktree-local and no reviewer sees it. The load-bearing part, in a tracked file:
  - *Criterion (2) as written* asked for `tenants_of_ctxs` "exactly once per tenant-scoped page".
    It is called **zero** times: the tenant predicate rides inside the single `visible_ctx_ids`
    query rather than beside it. That is better than the criterion asked for, so the criterion is
    superseded, not missed. The `CountingStore` counts `tenants_of_ctxs` anyway and pins it at
    zero — meaning a future variant that satisfies the criterion *as written*, with two queries
    per page, now fails. Falsified against a mutation that adds exactly that second query.
  - *Criterion (3)* — "a ctx_id absent from the map still resolves to `default`" — was **not met
    and no test was written**, because its premise is unreachable (see the two reasons in the
    tenant entry above). `log_entries_rejects_the_reserved_default_tenant` ships instead.
- **Status:** CONFIRMED.

## E1's cutoff is threaded through the trait, not the store constructors

- **Plan:** `plans/h-e-auth-webhook-quartet.md` (H-E Phase 1)
- **Assumed initially (WRONG):** that each `RevocationStore` implementation could take the leeway as
  a constructor parameter.
- **What checking found:** `SqliteRevocationStore::new` / `PgRevocationStore::new` /
  `InMemoryRevocationStore::new` are called from `crates/acdp-registry-server/src/main.rs:699,743,760`
  and `server/tests/http_integration.rs:4204` — **outside this lane's claim**, and inside lane-1's
  in-flight PR5/PR6. That design was a claim violation and a collision, discovered before writing it.
- **Chose:** change the **trait method signatures** instead — `is_revoked(jti, cutoff)` and
  `evict_expired(cutoff)`. `is_revoked` has exactly one production caller and `evict_expired` one,
  both in this crate; `server/` only constructs the stores and passes `Arc<dyn RevocationStore>`, so
  it is untouched and the workspace still builds.
- **Alternatives:** a defaulted trait method reading config itself — rejected, it re-derives the
  leeway independently of the validator, which is the exact defect being fixed one layer down.
  A claim-request for `server/src/main.rs` — disproportionate when an in-claim design exists.
- **Blast radius if wrong:** the trait is crate-local with three implementors, all in one file.
- **Status:** CONFIRMED (2026-09-12) — see DECISIONS.md H-E #3.

## The evictor takes its leeway from the signer, not from config

- **Plan:** `plans/h-e-auth-webhook-quartet.md` (H-E Phase 1)
- **Assumed:** that "the acceptance window" must have exactly one owner.
- **Chose:** `AuthService::spawn_evictor` reads `signer.leeway_seconds()`. `AuthConfig` also carries
  `token_leeway_seconds` and reading it there would look equivalent.
- **Why not config:** the bug being fixed IS a validator and a revocation layer disagreeing about the
  window. Deriving the value twice from a common source re-creates that class the moment any wiring
  changes which value reaches the signer. A new `leeway_seconds()` accessor is a smaller price.
- **Blast radius if wrong:** the two values are equal in every current wiring, so a mistake here is
  invisible today and would surface only after a config change — which is exactly why it is written
  down rather than left to inference.
- **Status:** CONFIRMED (2026-09-12) — see DECISIONS.md H-E #4.

## `safe_client` for the poller refuses private-range feed hosts, and that is accepted

- **Plan:** `plans/h-e-auth-webhook-quartet.md` (H-E Phase 2)
- **Assumed:** that matching webhook delivery's posture is right for the poller too.
- **Chose:** `safe_client(&SsrfPolicy::default(), 15s)`, the remedy the finding itself names.
- **The trade-off, stated because it can break a working deployment:** the finding is about
  *redirects* — the feed URL is operator-configured, so the SSRF risk is a hostile **redirect
  target**, not an attacker-chosen URL. `redirect(Policy::none())` alone would fix that. `safe_client`
  additionally installs a DNS resolver that refuses private/loopback/link-local hosts, so a peer
  registry on an internal hostname is now refused rather than polled. Adopted anyway, for consistency
  with webhook delivery, which already accepts that posture for operator-configured URLs.
- **Alternatives:** a hand-built client with `redirect(Policy::none())` and no SafeDnsResolver —
  fixes the finding with no collateral behaviour change, rejected because two different HTTP-client
  postures in adjacent crates is the drift this repo keeps paying for. A config knob to allow private
  ranges — scope creep for this unit; a legitimate follow-up if an operator hits it.
- **Also recorded:** `safe_client` consults the policy **only** for DNS. `allow_http` and
  `reject_ip_literals` are NOT enforced by it, so an `http://` feed still works and an IP-literal URL
  bypasses the resolver check. Anyone reading "safe_client" as "all policy fields enforced" would be
  wrong, here and in the webhook crate.
- **Status:** CONFIRMED (2026-09-12) — see DECISIONS.md H-E #1.

## E3 extends a surface this repo owns, additively, and does not claim to deliver protection

- **Plan:** `plans/h-e-auth-webhook-quartet.md` (H-E Phase 4)
- **Question raised to the leader:** whether "the registry now offers replay protection" is a product
  statement needing the human rather than a lane decision.
- **Settled by a fact, not a judgement:** the canon has no webhook concept at all (`grep -ril webhook`
  over `acdp-primitives` returns nothing) and `docs/WEBHOOKS.md` says the scheme "matches GitHub's
  exactly" — a choice this repo made. So the registry owns this surface outright; there is no
  external contract to diverge from. The inverse of the 415 case, which went to the human precisely
  because that code sat in a canonical closed registry the repo does not own.
- **Chose:** additive headers. `X-ACDP-Signature` stays byte-identical and is now pinned; a second
  header carries HMAC over `"<timestamp>." + body`.
- **Alternatives:** widen `X-ACDP-Signature` to cover the timestamp — breaks every deployed receiver.
  Send an unsigned `X-ACDP-Timestamp` only — worthless, since a replayer rewrites it.
  Make it config-gated — rejected: two schemes under one header name is worse than one additive pair.
- **What is deliberately NOT claimed:** the docs say the registry *offers* bound freshness and does
  not enforce it. A receiver ignoring the headers is as exposed as before. Writing "the registry now
  has replay protection" would have been false on the day it shipped.
- **Blast radius if wrong:** additive headers are ignorable; withdrawal would only affect receivers
  that had adopted them, which is why the adoption contract is documented rather than implied.
- **Status:** CONFIRMED (2026-09-12) — see DECISIONS.md H-E #2.

## The issuer guard asserts on `jsonwebtoken`'s Display string, not on an error kind

- **Plan:** plans/h-n-jwt-issuer-assertion.md
- **Assumed.** `rejects_token_from_a_different_issuer` proves WHICH guard rejected the token by
  asserting `err.to_string().contains("InvalidIssuer")`. That string comes from
  `jsonwebtoken::errors::ErrorKind::InvalidIssuer`'s `Display`, reached through
  `AuthError::TokenInvalid(e.to_string())` — so the assertion depends on a dependency's error
  *rendering*, which is not a stability contract.
- **Chose** the string anyway, because the alternative is worse here. `validate` deliberately
  flattens every decode failure into `AuthError::TokenInvalid(String)`, so by the time the error
  reaches a caller the kind is already gone. Recovering it would mean widening `AuthError` to carry
  a `jsonwebtoken` kind — leaking a dependency's type into this crate's public error enum, for the
  benefit of one test.
- **Alternatives rejected.** (a) Assert only `is_err()` — that is the defect this test exists to
  avoid: it passes when an earlier guard short-circuits, and M3 below shows a signer that rejects
  *everything* would satisfy it. (b) Add a typed `AuthError::IssuerMismatch` and check the issuer by
  hand before `decode` — a second issuer check next to the library's, which is the duplicate-source
  problem, and it would not even be exercised by the library's own path.
- **Blast radius if wrong:** a `jsonwebtoken` upgrade that renames the Display text turns this test
  RED with a message naming exactly what happened ("expected the ISSUER guard to reject this token,
  but it was refused by something else ... Got: <new text>"). It fails loudly and locally, and the
  fix is one string. That is the acceptable direction for this coupling to break.
- **Status:** CONFIRMED (2026-09-12) — see DECISIONS.md, unit H-N. Opus-settled; the caret
  requirement `"11"` means `cargo update` can break it without a manifest change, which is a wider
  exposure than first written, and it still fails loudly and locally.

## H-H-w — wiring the tenant predicate into `/contexts/search`

- **Plan:** unit H-H-w (lane-3), closing the residual E2 that H-H's `search_in_tenant` was built for.
- **Assumed:** that `search_in_tenant` existed, worked, and was dormant — i.e. that this unit was
  wiring rather than building.
- **Verified before designing, per Rule 87, rather than taken from the assignment.** All three cited
  call sites are correct by content: `sqlite/src/store.rs:164` and `pg/src/store.rs:127` both
  delegate to `search_inner(params, requester, anon, tenant)`; the default at
  `store/src/lib.rs:430` is **fail-closed** (a non-reserved `Some(tenant)` returns an empty response
  rather than falling through to an unscoped `search`). Dormancy confirmed too: every reference
  outside the definitions is a doc comment, a unit test, a parity test, or a test wrapper — no
  production caller. **Status: CONFIRMED.**

- **Assumption (the one that would have shipped a security regression):** that
  `RegistryServer::search` is a thin wrapper over `RegistryStore::search`, so swapping in the
  store's tenant-scoped entry preserves behaviour.
- **FALSE, and checked rather than assumed.** `acdp-server-0.13.1`
  (`src/registry/server.rs:937`) rejects an anonymous search with **403 `not_authorized`** when
  `caps.anonymous_public_reads` is false, *before* delegating — normative per RFC-ACDP-0008 §6.3 and
  fixture `vis-009`, and deliberately not an empty `200`, because an empty 200 still confirms the
  registry exists and that the query ran. `search_in_tenant` is a store entry point with no such
  gate, so the obvious wiring silently downgrades a normative 403 to an empty 200.
- **Chose:** leave the untenanted path on `server.search` **untouched** (no behaviour change at all
  for the common case) and replicate the gate on the new tenant path only, reading
  `anonymous_public_reads` from `state.server.capabilities()` — the same field `server.search`
  reads, so the two agree by construction. `state.config.auth.anonymous_public_reads` is the trap:
  right in the binary, which copies cfg into caps, wrong for any other wiring. That is GAP 3, and it
  already cost this repo one anonymous-disclosure bug on `/log/entries`.
- **Status:** CONFIRMED.

- **Assumption (checked because the default impl is fail-closed, which hides regressions as
  emptiness):** that routing tenant-scoped search through `search_in_tenant` does not break the
  `storage-memory` build, whose `MemoryStore` overrides none of the tenancy methods and therefore
  inherits a default that returns **zero rows** for any real tenant.
- **Why it holds, and it is not my doing:** #137 already refuses the combination at startup —
  `main.rs` bails when the memory backend is configured with tenancy, precisely so a registry does
  not boot answering every tenant-scoped read with zero rows. Independently, `MemoryStore` reports
  `"default"` for every row, `"default"` is `RESERVED_TENANT`, and `reject_reserved_tenant` refuses
  it from both a header and a token claim — so no caller can assert the only tenant that backend
  reports. A tenant-scoped search is therefore unreachable on a shipped memory deployment, and the
  fail-closed default is not reachable through the wire.
- **Blast radius if wrong:** tenant-scoped search on a memory build would silently return nothing
  rather than erroring — fail-closed, so no disclosure, but functionally broken and invisible.
- **Status:** CONFIRMED.

- **Assumption (recorded as an OPEN decision, deliberately not settled by this unit):** that
  `total_estimate` should remain omitted for a tenant-scoped request.
- **What changed under it.** It was omitted because it was **wrong** — the store counted before the
  handler applied the tenant predicate. `search_inner` puts `AND tenant_id = ?` in the same
  statement as `COUNT(*) OVER ()`, so once the request is served by `search_in_tenant` the count is
  **tenant-scoped and honest**. The original justification no longer exists.
- **Chose:** keep omitting it, and rewrite the *reason* in `docs/HTTP-API.md` to say it is now
  withheld **conservatively rather than necessarily**. Re-enabling it is wire-visible on an endpoint
  another lane documented this same wave, so it is a decision for the leader and not a side effect
  of a wiring unit. Documenting (b) without implementing it would have left the docs describing code
  that does not exist — the failure this unit's second half exists to remove.
- **Status:** UNCONFIRMED (re-checked U-507 2026-09-13 — deliberately NOT closed by inertia, which this entry explicitly forbids). **Settled by:** an explicit ruling. **Owner:** the leader. The behaviour is deliberate and the docs match the code as shipped, but
  the *decision* is open and should be closed explicitly rather than by inertia.
## H-A P10 (A10) — the route-classification guard names what it cannot parse

- **Assumption:** `NON_DATA_ROUTES` reverse containment is not redundant with the equality
  assertion that landed in PR3.
- **Verified, not reasoned:** removing `.route("/.well-known/jwks.json", ...)` from the core
  router while leaving its table row standing passes assertions 1, 2 and 3 and fails only the new
  one. Equality is blind to it because removing a route decrements the scanned count and the call
  count together; assertion 2 is blind because it runs mounted ⊆ classified, and a table row
  matching nothing subtracts nothing from `unclassified`.
- **Why it matters:** a row that matches no route reads as coverage. It also silently shrinks what
  assertion 2 can catch, because a later route reusing that path would be "already classified".
- **Status:** CONFIRMED by falsification.

- **Assumption (and the correction that produced it):** a source scanner for route registrations
  must scan the whole source, never line by line.
- **Why:** the first draft of `non_literal_route_forms` scanned per line and reported seven false
  positives. `crates/acdp-registry-core/src/lib.rs` registers seven routes with `.route(` at the
  end of one line and the path on the next; a per-line scan sees an empty argument and calls it
  non-literal. Whitespace between `(` and the path legitimately includes a newline.
- **This is the same defect that defeated a grep earlier in this unit** — a phrase spanning a line
  wrap, invisible to a line-oriented tool — reproduced in code written hours after that lesson was
  recorded. Knowing the failure mode did not prevent writing it again in a different medium.
- **Fix, and why this shape rather than a patched regex:** `non_literal_route_forms` is now the
  exact inverse of `mounted_route_paths` — same scan, same "is the first non-whitespace token a
  quote" test, opposite branch taken. Two functions that must agree about what a literal is now
  cannot disagree, rather than agreeing by inspection.
- **Status:** CONFIRMED — the seven false positives are gone and the real injection is still the
  only form reported.
## The caps/config invariant is made unrepresentable for new callers, not enforced for existing ones

- **Plan:** plans/h-o-caps-testability.md
- **Assumed.** `with_anonymous_public_reads` gives callers one input that sets both
  `cfg.auth.anonymous_public_reads` and `caps.anonymous_public_reads`. It does **not** enforce the
  invariant on the 75 existing construction sites that set the fields directly.
- **Chose** the constructive form because the enforcing form is not mine to ship. A `debug_assert`
  in `wire_server` / `build_harness_with_webhook` would make divergence fatal — and would redden
  `admin_list_returns_rows_under_the_shipped_disclosure_default` in `http_integration.rs`, where
  `config_shipped_disclosure_default` sets the config flag and leaves caps at `true`. That file is
  lane-1's for PR6. Breaking another lane's test to enforce an invariant that is available
  constructively to every new caller is not a trade this unit gets to make.
- **Alternatives rejected.** (a) The `debug_assert`, above — correct in kind, out of scope in
  practice; it is the right follow-up once the divergent site is resolved, and I have reported it as
  a claim-request rather than acting on it. (b) A newtype carrying both values, which would require
  editing all 75 call sites, 73 of them in files I do not own. (c) Doing nothing on the grounds that
  the one divergence is currently harmless — true today only because `admin_list` reads the flag
  from neither source, which is itself a latent defect rather than a guarantee.
- **Blast radius if wrong:** a caller can still hand-set one field and get the other's behaviour,
  which is the original defect class. The mitigation is that the correct path is now shorter than
  the incorrect one, and that `the_helper_sets_both_knobs_not_just_one` fails loudly if the helper
  itself ever stops setting both.
- **Measured, not assumed — the guards are asymmetric and the asymmetry is the evidence:** dropping
  the caps half reddens **both** the unit guard and the end-to-end test; dropping the config half
  reddens **only** the unit guard, because caps is the field the predicate actually reads. That
  directional result is what shows the end-to-end test measures caps rather than config, and it is
  why the config half needs its own assertion to be protected at all.
- **Status:** CONFIRMED (U-507, 2026-09-13) — still accurate: the helper exists, no enforcing `debug_assert` was added, and the divergent site remains.

## H-A2-w — `total_estimate` returns for tenant-scoped callers

- **Assumption:** the store's `COUNT(*) OVER ()` is tenant-correct, so returning `total_estimate`
  to a tenant-asserting caller discloses nothing.
- **Verified in the code, then on the wire:** `search_in_tenant` appends `AND tenant_id = ?` to the
  same statement that carries `COUNT(*) OVER ()` (`acdp-registry-sqlite/src/store.rs`,
  `acdp-registry-pg/src/store.rs`), so the count rides a scan that only ever sees the caller's
  rows. On the wire, a `tenant-a` caller against a fixture of 2 own rows and 3 foreign rows
  receives `2`.
- **Why the wire check is not redundant with reading the SQL:** the handler chooses between
  `search_in_tenant` and `RegistryServer::search` at `handlers/context.rs`. Correct SQL reached by
  the wrong branch would still report the registry-wide number, and only an end-to-end assertion
  distinguishes those.
- **Status:** CONFIRMED.

- **Assumption:** asserting the VALUE rather than the presence of `total_estimate` is necessary.
- **Why:** `Option<u64>` with `skip_serializing_if` makes absent and null identical on the wire, so
  `.is_some()` cannot tell "present and null" from "present with a number". More importantly,
  presence alone passes against two live bugs. The fixture separates all three readings — 2 is the
  tenant's count, 5 the registry's, 1 the page size at `limit=1` — and each was produced by a real
  mutation rather than predicted:
  - omission restored in the handler -> `None`;
  - tenant predicate removed from the store's WHERE clause (the pre-#259 world) -> `Some(5)`;
  - `total_estimate = matches.len()` -> `Some(1)`.
- **Status:** CONFIRMED by falsification, three ways.

- **Assumption (NOT acted on, recorded so the next reader does not "tidy" it):** the refill loop's
  tenant `retain` and its `tenants_of_ctxs` call are now redundant for a tenant-scoped request,
  because the predicate is in the statement.
- **Why it is left in place:** removing them is an optimisation with its own falsification burden,
  not part of this change. lane-3 left them deliberately for the same reason and said so at
  handover. Deleting them here would ship an unfalsified behaviour change inside a diff whose
  stated purpose is a one-field wire addition.
- **Status:** UNCONFIRMED (re-checked U-507 2026-09-13) — a separate unit if anyone wants it, with its own evidence. **Settled by:** scheduling that unit. **Owner:** the leader.

## H-U — the store parameter is renamed for what the predicate consumes

- **Assumption:** renaming the parameter is behaviour-neutral, and the two struct fields must not
  move with it.
- **The boundary, and the reason is not the one this unit was handed.** The hazard was described
  as: `types/src/config.rs`'s field is `#[serde(default)]`, so after a rename an old key would be
  **ignored rather than rejected** and the registry would boot at `false` while the operator
  believed they set `true` — a silent disclosure-posture change. **Measured, that is not what
  happens.** `AuthConfig` also carries `#[serde(deny_unknown_fields)]` (`config.rs:304`), so a
  config naming the old key **fails to parse and the registry refuses to start**, naming the
  unknown field. Still a breaking change for every deployed config, so the conclusion stands — but
  the failure is loud, not silent, and the severity is different.
- **Status:** CONFIRMED, with the mechanism corrected.

- **Assumption:** the serde key needed a guard, and the compiler was not one.
- **Evidence, from this unit's own diff:** the mechanical rename **did** rename the serde field.
  It compiled, `clippy --all-targets -D warnings` passed on **six** feature configurations, and 28
  test suites stayed green — because every other use in the repo sets the field programmatically
  and was renamed in lockstep. Nothing observed the wire contract with deployed `registry.toml`
  files. `anonymous_public_reads_is_a_stable_config_key` now asserts the KEY by deserialising it,
  which is the only way to observe the name serde matches on, and it fails against exactly the
  rename that produced this finding.
- **Status:** CONFIRMED by falsification.

- **Assumption:** `admin_sees_public_arm = true` is unchanged, and the test guarding it actually
  ran.
- **Why the second half needed checking:** the guard is `#[cfg(feature = "playground")]`, so
  `cargo test --workspace` never compiles it. A 28-suite green says **nothing** about it. Run under
  `--features playground` it passes, and flipping the value to `false` reddens it with an empty
  listing — the rejected first draft of #133.
- **Status:** CONFIRMED under the configuration named, which is the only configuration in which the
  claim means anything.

## U-501 (#242) — charging publishes that fail late

- **Assumption:** schema validation can be excluded from the identity oracle without weakening
  it.
- **Status:** SUPERSEDED (2026-09-22, acdp-registry-rs#336) — `publish_identity_proven_offline`,
  the hand-rolled oracle this assumption was about, is deleted. did:key/pinned now prove identity
  via the SDK's own `prove_publish_identity_did_key`/`prove_publish_identity_pinned` (acdp v0.14.0,
  acdp-rs#273), which bundles schema/size/hash/algorithm-binding validation (`validate_post_schema`)
  INTO the identity proof — there is no longer a way to prove hash+signature alone, first. The
  practical effect: a validly-signed but schema-invalid did:key/pinned publish, which the old
  oracle deliberately charged (per the reasoning this entry used to give, preserved below), is now
  an UNCHARGED rejection instead — the narrowing is real but small, since every such rejection now
  fails at or before the recomputed-hash check, i.e. before the one expensive step (the signature
  verify), so it cannot be used to burn CPU for free. Root cause: this repo no longer controls the
  boundary between "identity check" and "schema check" — the SDK does, and it drew that line
  differently. See DECISIONS.md's U-501 addendum (2026-09-22) for the full analysis (by a Fable
  agent under `/reconcile`, on the human's request) that surfaced this narrowing.
  Superseded text, kept because this file is cumulative: *"Reasoning: `publish_identity_proven_offline`
  recomputes `content_hash` and verifies the offline signature, but deliberately does not call
  `validate_publish_request`. Schema validity is not part of an identity proof: if the hash binds
  the body and the signature binds the hash to `agent_id`'s key, then `agent_id` signed this body
  whether or not the body is schema-legal. Excluding it makes the oracle broader (more publishes
  chargeable), not laxer in the dangerous direction, and a producer flooding signed-but-schema-invalid
  publishes is precisely the noisy producer the limiter exists to throttle. Status: CONFIRMED by
  construction. Reversible in one line if it is ever wrong — adding the call can only reduce what
  is charged, never permit an unproven charge."*

- **Assumption:** the oracle cannot cause a request to be rejected that is accepted today.
- **Evidence:** it returns `bool`, not `Result`, and every failure path inside it returns
  `false`. A `false` only skips arming; the request proceeds into the SDK unchanged and the SDK
  produces the authoritative error. The full 142-test `http_integration` suite passes unchanged,
  including every existing did:key, playground and production-path acceptance test.
- **Status:** CONFIRMED.

- **Assumption:** charging on panic and on client cancellation is correct, not a bug.
- **Reasoning:** both mean the verify work was really spent. Suppressing the charge unless the
  handler returned normally would hand a free channel to anyone able to induce either.
  `an_armed_charge_fires_when_the_scope_unwinds` pins the panic half.
- **Status:** CONFIRMED, and deliberate. Recorded because a future reader is more likely to
  read it as an oversight than as a decision.

- **Assumption:** the production `did:web` branch cannot be charged from inside this repo at an
  acceptable cost.
- **Status:** RESOLVED, not by accepting the trade (2026-09-22, acdp-registry-rs#336) — the seam
  this repo asked for in `plans/cross-repo/acdp-rs-publish-charge-seam.md` shipped as acdp-rs#273
  / acdp v0.14.0's `Proven`/`prove_publish_identity`/`commit_proven` split. The did:web branch now
  calls `prove_publish_identity(&req, &resolver).await?` (the same real DID resolution + signature
  verification `publish_verified_in_tenant` always did — no second resolution, no new SSRF
  surface), arms `PublishCharge` immediately on success, then `commit_proven`s the result — so a
  late failure (a store error, a duplicate-publish race) is now charged, closing the fourth and
  last branch of #242.
  `late_failures_are_charged_on_exactly_three_of_the_four_publish_branches` in
  `crates/acdp-registry-server/tests/http_integration.rs` is the regression test: it proves the
  did:web branch charges a genuine commit-time failure using a real (fixture) DID resolution, not
  merely that the code compiles against the new signature. This entry's original judgement — that
  the cost of closing this from the registry side was unacceptable — was correct and remains the
  reason the fix had to come from the SDK, not from this repo re-attempting its own resolution.
  Superseded text, kept because this file is cumulative: *"Evidence: `publish_verified_in_tenant`
  resolves the DID document over the network inside the SDK call. Establishing identity in the
  handler first would need a second resolution per publish — a second network round-trip, a second
  SSRF surface, and a cache that can disagree with the SDK's. Not attempted; the seam is designed
  in `plans/cross-repo/acdp-rs-publish-charge-seam.md` and filed upstream. Status: UNCONFIRMED
  (re-checked U-507 2026-09-13; unmeasurable by construction, so evidence cannot close it). Settled
  by: a reviewer accepting or rejecting the trade. Owner: the reviewer. This is a judgement about
  cost, not a measured fact. It is the one claim in this unit a reviewer should push back on if
  they disagree about the trade."*

- **Assumption:** when `prove_publish_identity_did_key`/`prove_publish_identity_pinned` (acdp
  v0.14.0) fails, the publish must be rejected, not accepted-uncharged.
- **Reasoning:** the human-reviewed cross-repo adoption plan
  (`acdp-rs/plans/cross-repo/acdp-registry-rs-publish-charge-seam-adoption.md`) specified the
  opposite as a bolded, hard acceptance criterion — reproduce the old `publish_identity_proven_offline`
  oracle's "false = don't charge, never reject" contract. A fresh Fable agent, asked to verify this
  independently (not asked to agree), found the premise was already false in acdp-server 0.13.1:
  the "real" SDK call the old oracle's `false` used to fall through to
  (`publish_verified_did_key_in_tenant`) ran a strict SUPERSET of the oracle's own checks, so
  nothing the oracle rejected was ever actually accepted — the existing tests
  `naming_a_victim_does_not_spend_their_budget` and
  `a_replayed_envelope_over_a_different_body_does_not_spend_the_budget` already pinned *rejected*,
  not accepted, for exactly these cases. In 0.14.0 the "real" call is now DEFINED as
  `prove_publish_identity_did_key` + `commit_proven`, so there is no more a second, more lenient
  path to fall back to — and the only 0.14.0 path that could honor the plan's literal instruction
  (`publish_unverified_in_tenant_for_tests`) would admit exactly and only forged, signature-invalid
  did:key publishes. See DECISIONS.md's U-501 addendum (2026-09-22) for Fable's full analysis.
- **Status:** CONFIRMED (2026-09-22, the human, presented with Fable's analysis) — reject on prove
  failure, matching did:web. The plan's acceptance criterion on this point is retired as
  inapplicable to the 0.14.0 API shape, not merely stale.

## U-505 — index of deferred work surfaced from this file and DECISIONS.md (2026-09-13, lane-1)

- **Index entry, not a status change.** No entry above was edited. U-505 enumerated the deferred
  items in this file and in `DECISIONS.md`, verified them against the tree, and filed the still-live
  ones as issues so they stop existing only as prose. Flipping any `UNCONFIRMED` above to
  `CONFIRMED` is a separate unit (U-507) — in-place status edits collide with the keep-both-sides
  resolution rule while other lanes are appending here.
- **Counts.** `ASSUMPTIONS.md`: **35** deferred items (25 `Status: UNCONFIRMED`, 1 `Status: OPEN`,
  9 whose bullet *is* the status and carry no `Status` line, plus 1 `###`-heading item and 1
  recorded only as an in-entry update). `DECISIONS.md`: **42** candidate follow-up blocks, a
  deliberate superset since prose mentions of a follow-up are indistinguishable from a record of
  one without reading each.
- **Filed as issues:** #265 (CI's feature-config steps run `clippy`, not `cargo build`), #266
  (`docker.yml` never boots the shipped quickstart stack), acdp-website#43 (public webhook docs
  list 2 of 4 event types).
- **Verified ALREADY DONE, not re-filed:** `/metrics` cache headers (`lib.rs:265-271`); the EdDSA
  PEM check and its ordering (`main.rs:111-117`, `:83` before `:677`); `acdp-playground`'s
  `WebhookType`; the cursor-literal dedup (`acdp-registry-store/src/cursor.rs`); `storage-memory`
  CI coverage; the `dtolnay/rust-toolchain` pin; `bump-spec.yml`.
- **Known stale pointer, reported not repaired:** `DECISIONS.md:1021-1022` pins
  `pg/src/store.rs:1644-1668` and `sqlite/src/store.rs:1740-1764`. Both ranges now hold unrelated
  code. Not repaired here because `DECISIONS.md` is append-only this wave.
- **The bound I could not close, stated plainly:** the 42 `DECISIONS.md` candidates were
  classified by reading, and the ones resolvable from this repo's tree were verified. Items whose
  resolution lives in a sibling repo's history (the spec-repo dispatch matrix in the `bump-spec`
  follow-up, and `acdp-ci/DELIVERY-STANDARD.md`'s status lines) were **not** verified to the same
  standard, because doing so means reading another repo's git history rather than its working tree.
  They are neither confirmed done nor confirmed live.
- **Status:** PARTIAL (U-507, 2026-09-13) — the `ASSUMPTIONS.md` count of 35 is CONFIRMED exact by independent re-derivation; the
  `DECISIONS.md` count of 42 is an upper bound on distinct items, not an exact count.
## U-503 — a shell script is the right home for a CI tag guard

- **Plan:** `plans/u-503-immutable-sha-tag.md` (Phase 1)
- **Assumed:** that `docker/assert-image-tags.sh` is an acceptable place for this guard even
  though **the repo contained no `*.sh` files at all** before it, and CI runs no `shellcheck`,
  `actionlint` or `yamllint` — so nothing lints it.
- **Chose:** the shell script, for two reasons that are not about convenience. First, the
  alternative that matches repo convention — a Rust test under `crates/**` reading
  `docker.yml`, the shape used for the route-documentation guard — is **outside this unit's
  path grant**, and needing it would be a `claim-request` rather than a judgement call.
  Second, a script taking the tag set as an argument is a better shape for this particular
  job: it is falsifiable in milliseconds against the real pre-fix data, with no CI round
  trip, which is what let Phase 1 demonstrate rejection instead of asserting it.
- **Mitigation for the absent linter:** `--self-test` is wired into `docker.yml` (Phase 2), so
  the script is exercised on every workflow run rather than trusted. A guard nobody runs is
  the failure mode this is guarding against.
- **Alternatives:** a Rust test under `crates/**` (out of grant, and would need a claim
  request); an inline `run:` assert in `docker.yml` matching the file's existing
  `assert semver tag` idiom — rejected because it cannot be executed locally, so the
  falsification requirement could not have been met; adding `shellcheck` to CI — rejected,
  `.github/workflows/ci.yml` is out of grant.
- **Blast radius if wrong:** low and local. The script is 1 file, invoked from 2 workflow
  steps; if the convention is unwelcome the logic moves to a Rust test in one commit, and the
  self-test table moves with it unchanged.
- **Status:** CONFIRMED (2026-09-13), Opus, at reconcile. The deciding point is not taste: the
  convention-matching alternative was out of path grant, so it was never this lane's to choose.
  Of the options actually available, this is the only one that could be falsified before merge,
  and it was — see `DECISIONS.md`, U-503 decision 3.

## U-503 — metadata-action honours `{{is_default_branch}}` in `enable=` for `type=sha`

- **Plan:** `plans/u-503-immutable-sha-tag.md` (Phase 2)
- **Assumed:** that `docker/metadata-action@dc80280` evaluates the `{{is_default_branch}}`
  handlebars expression in the `enable=` option of a `type=sha` rule, not only in the
  `type=raw` rule where this file already uses it (`docker.yml`, the `latest` rule).
- **Chose:** the handlebars form anyway, rather than the GitHub expression that carries no such
  question. Reason: `enable=${{ !startsWith(github.ref, 'refs/tags/') }}` evaluates **true for
  pull requests**, so `sha-` would still be computed in-PR and the invariant would have to
  weaken from an iff to "no `sha-` on tag pushes" — which is exactly the one-directional form
  that cannot fire on the PR that breaks it. Keeping the in-PR firing property is worth more
  than avoiding this question.
- **Why it is safe to leave unresolved:** it fails closed and fast. If the handlebars is not
  honoured, this PR's own `docker` run computes a `sha-` tag on a pull request and the new
  `assert image tags` step fails the job — at the assert step, about a minute in, before the
  20-minute build. There is no path where a wrong guess here publishes anything.
- **Named fallback:** `enable=${{ github.event_name == 'push' && github.ref == 'refs/heads/main' }}`
  — a plain GitHub expression with identical semantics that preserves the in-PR property.
- **Alternatives:** reading the action's README and believing it (rejected — the run output is
  the only evidence that counts here, and it is free); pinning a newer action version (not
  needed, and a version bump is a separate change).
- **Blast radius if wrong:** one failed CI step and a two-token edit. Nothing publishes.
- **Status:** CONFIRMED (2026-09-13) by run **34763580595** — PR #264's own `docker` run, which
  is the only thing that could settle it. `DOCKER_METADATA_OUTPUT_TAG_NAMES: pr-264` and
  `"tag-names":["pr-264"]`: **no `sha-` entry**, where the pre-fix PR run 34725795501 computed
  `["pr-262","sha-3617f76"]`. So the handlebars *is* evaluated in `enable=` for `type=sha`, the
  gate fires, and the same gate suppresses the tag on the release path for the same reason.
  `self-test the image-tag guard` and `assert image tags` both green; `build + push` skipped, as
  a pull request must. The fallback expression was not needed and was not applied.

## U-503 — the double build is KEPT; only the mutable tag is fixed

- **Plan:** `plans/u-503-immutable-sha-tag.md` (Open question 2)
- **Assumed:** that "the same commit gets built twice at all", which the unit brief named as
  part of the defect, is in fact correct behaviour and should survive this unit.
- **Chose:** to fix only the mutable tag, and to argue this in the PR body rather than quietly
  omitting half of what was asked. The evidence is in the two runs' `buildx` command lines:
  metadata-action stamps `org.opencontainers.image.version=main` on the main build and `=0.1.3`
  on the release build, `image.created` differs, and buildx attaches
  `--attest type=provenance,mode=max,builder-id=…/runs/<run-id>`. Labels and provenance live in
  the config blob, so the two digests differ **deterministically, by construction** — this is
  not flakiness. Therefore promoting the main digest to `:0.1.3` with
  `buildx imagetools create`, the obvious way to collapse the builds, would publish a release
  image whose own OCI `version` label reads `main` and whose provenance names the main run. That
  is a mislabelling regression traded for a cosmetic one, so the rebuild earns its keep: it is
  what stamps release identity into the release artifact.
- **Alternatives:** collapse to a retag (rejected, above); make the builds bit-reproducible
  (unreachable for the same reason, and the brief explicitly says reproducibility is not the
  acceptance bar); publish only from the tag path and drop the main-push publish (rejected — it
  would remove `:latest`/`:main`, which `docker/RAILWAY.md` documents and operators deploy).
- **Blast radius if wrong:** the leader overrules the call and the double build is collapsed in
  a follow-up unit. Nothing in this change forecloses that — the gate and the guard stay correct
  either way, since a single publishing path trivially satisfies the one-writer invariant.
- **Status:** CONFIRMED (2026-09-13), Opus, at reconcile — as the right call, *and* as one that
  must stay visible rather than be quietly absorbed. Reversible: nothing here forecloses
  collapsing the builds later, and a single publishing path satisfies the one-writer invariant
  the guard asserts trivially, so a follow-up unit would find the guard already correct. Kept
  visible by being argued in the PR body and carried in the lane's `done` report, not by
  spending a separate board message on something already written where the leader reads it.
  See `DECISIONS.md`, U-503 decision 1.

## U-502 — the mutation oracle for #216 (lane-2, 2026-09-13)

Plan: `plans/u-502-mutation-oracle.md`. Seven entries. Base `origin/main` = `ec9233d`,
merged in (never rebased).

### 1. The config lives at `.cargo/mutants.toml`, not `mutants.toml` at the repo root
**Status: CONFIRMED (2026-09-13)**
**Assumed.** That a config the tool reads *automatically* is safer than one behind a flag.
**Chose.** `.cargo/mutants.toml`. `cargo-mutants` reads it with no argument; a root file needs
`--config` on every invocation. `test_workspace` and `copy_vcs` are both load-bearing — omit
either and the verdicts are noise — so a config that cannot be forgotten is a correctness
property, not a preference.
**Alternatives.** Root `mutants.toml` + a documented flag (rejected: "ran it without the flag"
becomes a one-typo route to a wrong answer that looks right).
**Blast radius.** Low; a file move.

### 2. The ratchet scope is two files, and `handlers/context.rs` is excluded
**Status: CONFIRMED (2026-09-13)**
**Assumed.** That a budget keyed to a file another unit is actively editing is worse than no
budget, because it goes red for reasons unrelated to the property it guards, and a red check
nobody can explain gets disabled.
**Chose.** `receipt.rs` (9) + `handlers/log.rs` (65) = 74, measured at `5c401db`. Excludes
`handlers/context.rs` (134 here; `DECISIONS.md` #17 measured 132 one day earlier — the drift is
the argument). Leader ruling, recorded rather than re-decided.
**Alternatives.** Entry 17's 206-mutant set including `context.rs` (rejected above); the whole
workspace, 1398 mutants ≈ 2.4h (rejected: #216 names per-PR blocking as a non-goal).
**Blast radius.** Low, and reversible by editing two globs.

### 3. `copy_vcs = true`, because without it every verdict is suspect
**Status: CONFIRMED (2026-09-13)**
**Assumed.** That copying `.git` has no cost worth weighing against verdict validity.
**Chose.** `copy_vcs = true`. Without it `conformance_gate.rs`'s
`no_tracked_file_contains_a_conflict_marker` panics on `git ls-files` in the `$TMPDIR` copy;
`cargo test` stops at the first failing binary; and 41 of 48 "caught" verdicts were scored by
that panic. Here `.git` is a 4 KB worktree pointer, so the copy is free.
**Alternatives.** Making the hygiene test skip outside a repo (rejected: it adds another
self-skipping test, which is the hazard this same unit flagged as Rule 134); excluding
`conformance_gate` from the test command (rejected: discards real coverage).
**Blast radius.** Low, but the *absence* of it was high — it invalidated a published number.

### 4. The survivor budget is 2, and one of the two is budgeted rather than accepted
**Status: CHANGED (2026-09-13)** — budget is 1, not 2: the claim on `http_integration.rs` was
granted and survivor 1 was KILLED rather than budgeted. See `DECISIONS.md` #18 entry 4.
**Assumed.** That recording a real gap as a budgeted survivor with a filed follow-up is more
honest than either suppressing it or blocking the unit on a file this lane cannot edit.
**Chose.** Budget 2. `log.rs:117:19` (`!=`→`==` in `requester_can_retrieve`) is a REAL unasserted
security branch — the killing test belongs in `http_integration.rs`, outside the claim, and is
claim-requested. `log.rs:131:18` (`==`→`!=` in `root_for`) is an equivalent mutant: it guards only
`log.cache_root(...)` and no response changes.
**Alternatives.** Budget 0 by writing the test anyway (rejected: outside the path grant);
classifying #1 as "accepted" (rejected: it is a live disclosure branch, not an acceptable one).
**Blast radius.** Low. Ratchets to 1 when the test lands.

### 5. The AC8 concentration threshold is 50% of caught mutants
**Status: CONFIRMED (2026-09-13)**
**Assumed.** That a harness-wide failure concentrates on one sole-killer test and a genuine suite
does not.
**Chose.** Fail when one test is the sole failing test for more than half the caught mutants.
Measured on both real runs: broken 41/48 = 85%, healthy 8/46 = 17%. The threshold sits in an
order-of-magnitude gap, so it is not tuned to the data.
**Alternatives.** A workspace-scoped unmutated baseline (rejected as insufficient: `cargo-mutants`
runs its baseline PACKAGE-scoped, and the failure exists only in the build copy, so a green
baseline in either tree is compatible with every verdict being noise).
**Blast radius.** Low; a scheduled job's threshold.

### 6. The three-command CI-equivalent wrapper is declined, not overlooked
**Status: CONFIRMED (2026-09-13)**
**Assumed.** That tripling per-mutant cost (`DECISIONS.md` #17: ~6.2s → ~22s) to recover one test
is the wrong trade.
**Chose.** Run the default-feature workspace command plus `ACDP_SPEC_DIR`, and name the single
unreachable test: `playground_compiled_in_but_runtime_disabled_keeps_admin_route`.
**Alternatives.** A `--test-tool` wrapper running CI's three commands (deferred, not rejected in
principle — it becomes worth it if the scope grows to feature-gated code).
**Blast radius.** Low, and stated where the number is rather than in a footnote.

### 7. This unit's own unpublished engineering-log entry was corrected in place
**Status: CONFIRMED (2026-09-13)** — and note the licence has EXPIRED: the entry is now pushed,
so any further correction to it must be an append. See `DECISIONS.md` #18 entry 8.
**Assumed.** That the append-only rule protects *other* lanes' content and *published* entries,
and is not a reason to publish a retracted number and then publish its correction underneath.
**Chose.** Rewrote the U-502 entry in place. It existed only on `lanes/lane-2`, was never in
`main`, and no other session had read it — a draft fixed before publication, not history
rewritten. The retraction trail is in git (`fb5983b` → `52c0111` → the merge commit) and in the
entry, which now records the harness bug as its main finding.
**Alternatives.** Append a correction beneath the wrong entry (rejected for an unpublished draft;
it is the right choice for anything already in `main`, and the file's own precedent — "corrected
here rather than in place" — is about exactly that case).
**Blast radius.** Low, but it is a judgement about a shared-file rule, so it is recorded rather
than assumed.
## U-509 — #266: making "the documented quickstart boots" a CI property (2026-09-13, lane-1)

- **Correction to my own issue #266, stated before building on it.** #266 said `docker.yml`
  "never boots the stack the quickstart ships". The literal half was right — no `jwt_secret` /
  `JWT_SECRET` appeared anywhere in the workflow — but the wording overstated it. CI *did* boot a
  registry against the shipped `docker/config.docker.toml`. What it never did was read
  `docker/docker-compose.yml`, because the smoke test hand-rolls `docker run`. That distinction is
  the whole finding rather than a quibble: W3-U5's defect was a `changeme` placeholder **in the
  compose file's environment block** (fixed in `abfebf7`), so it lived in precisely the file CI
  never opened. Verified at `f658fd5`, not carried over from the earlier audit at `2199422`.
- **Status:** CONFIRMED, with the issue's wording corrected here rather than quietly relied on.

- **Assumption:** an overlay that pins the prebuilt image still exercises the recipe.
- **Evidence:** `compose.ci.yml` overrides only `image:` and `build:`. `docker compose config`
  shows the `environment:` block, the `config.docker.toml` mount, `depends_on` and the postgres
  service all resolving from the recipe unchanged. Those are the parts under test; the W3-U5 defect
  lived in the environment block, so an overlay that replaced it would have tested nothing.
- **Status:** CONFIRMED by reading the resolved model, not by assuming merge semantics.

- **Assumption (WRONG, caught by a negative control — recorded because the failure mode is the
  point):** that exporting `ACDP_REGISTRY_AUTH__ENABLED=true` before `docker compose up` would
  enable auth in the container.
- **What is actually true:** compose forwards **only** the variables named in a service's own
  `environment:` block. `ACDP_REGISTRY_AUTH__ENABLED` is not one of them, so it never reached the
  container. The first draft of `--check-auth-on` therefore booted the auth-**off** stack and
  reported success — a decorative check that could not have failed. It was caught only because
  negative control (2) refused to go red, and the control was right. A check that passes is not
  evidence; a control that fails to fail is.
- **Status:** CONFIRMED by `docker compose config`, which shows the variable absent from the
  resolved environment. Fixed with a CI-only overlay (`compose.ci-auth-on.yml`) whose effect is
  proven by control (2) now firing.

- **SUPERSEDED (U-507, 2026-09-13) — the recipe now DOES provide an environment path: `docker/docker-compose.yml:90` forwards `ACDP_REGISTRY_AUTH__ENABLED`. True when written:** The compose
  file's header tells operators to "set a real secret before enabling auth", but the recipe
  provides **no environment path to enable auth** — `ACDP_REGISTRY_AUTH__ENABLED` is not forwarded.
  An operator must edit `config.docker.toml`, which runs straight into the precedence caveat the
  same header documents for `jwt_secret`. The obvious fix — adding
  `ACDP_REGISTRY_AUTH__ENABLED: ${VAR:-false}` to the environment block — is **rejected as a
  security regression**: compose renders an unset variable as set-to-empty rather than absent, and
  env beats TOML, so an operator who set `auth.enabled = true` in the file would have it silently
  forced back to `false`. Turning someone's auth off to make a CI step convenient is not a trade
  worth making. Left to a unit that can design the passthrough safely.
## U-508 — "PR-blocking" means runs-and-can-fail, not listed-in-branch-protection

- **Plan:** `plans/u-508-lint-shell-and-workflows.md` (Open question 1)
- **Assumed:** that the assign's "wire both as PR-blocking checks" means the linters run on
  `pull_request` and can fail the check, rather than that the new `lint` context must be added to
  `required_status_checks`.
- **Chose:** deliver the former in full, and escalate the latter rather than perform it.
  `gh api repos/…/branches/main/protection` shows `required_status_checks.contexts` is enumerated —
  `["rustfmt","clippy","tests","conformance (spec fixtures)"]`, `strict: true`. So `lint` will run
  and can go red, but will **not** prevent a merge. `docker`/`build` and `coverage` are already
  non-blocking in exactly this way, which is why the narrower reading is also the one consistent
  with how this repo already works.
- **Why not just add the context:** it is not a path, so it falls outside a path grant entirely; it
  changes merge behaviour for **every** contributor, which is outward-facing rather than local; and
  a peer cannot authorize it on the human's behalf. The exact call is recorded in `lint.yml`'s
  header and in `done`, including that `contexts` is replaced wholesale so omitting an existing
  entry silently un-requires it.
- **Alternatives:** adding the linters as steps inside an already-required job (e.g. `rustfmt`) —
  **rejected**, it would report a shell failure under a check named `rustfmt`, which is the same
  mislabelling defect U-503 refused when it declined to retag a `main`-labelled image as a release;
  renaming a job to cover more ground — **rejected and dangerous**, the four job names are an
  external contract with branch protection, and a required context that stops reporting leaves
  every PR waiting forever.
- **Blast radius if wrong:** the gate advises instead of blocking until one API call is made. Loud,
  not silent: the limitation is in the workflow header, the log entry, the PR body and `done`.
- **Status:** **CONFIRMED** (2026-09-22, U-575, Opus under `/reconcile`) — the settings change
  landed. Live-verified via `gh api repos/…/branches/main/protection
  --jq '.required_status_checks.contexts | sort'`: `["cargo-deny","clippy","conformance (spec
  fixtures)","lint","rustfmt","tests"]`. `lint` is present; `.github/workflows/lint.yml`'s job
  publishes a check named exactly `lint` (its own header now states outright: *"THIS CHECK BLOCKS
  MERGES"*), and `ci.yml:42-46` corroborates the setting changed 2026-09-16. The original ask —
  "the new `lint` context must be added to `required_status_checks`" — is now literally true.
  Superseded text, kept because this file is cumulative: *"PARTIAL (U-507, 2026-09-13) — the
  technique is now proven by U-516: a checkout-only gate runs inside `ci.yml`'s `fmt` job
  (published `rustfmt`, a required context) and blocks merges with no settings change. `lint`
  itself is still a separate workflow publishing a non-required `lint` context, so the original
  ask is undischarged."*

## U-508 — a separate lint.yml rather than jobs inside ci.yml

- **Plan:** `plans/u-508-lint-shell-and-workflows.md` (Open question 2)
- **Assumed:** a new workflow file is better here than extending `ci.yml`.
- **Chose:** new file. The linters need no Rust toolchain and no cargo cache, so they share nothing
  with `ci.yml`'s matrix; `ci.yml` is ~500 lines and lane-1/lane-2 are both in adjacent CI surface
  this wave, so a separate file is the lower-conflict choice; and a distinct check name makes a
  shell finding legible rather than disguised as `rustfmt`.
- **Alternatives:** jobs in `ci.yml` — rejected for the conflict surface and the naming; one
  combined job — rejected, `shellcheck` and `actionlint` failing for unrelated reasons under one
  check name is harder to read, though they do share a job here since both are seconds long.
- **Blast radius if wrong:** one file moves. No behaviour depends on which file the steps live in.
- **Status:** CONFIRMED (U-507, 2026-09-13) — the stated reason still holds exactly: the linters need no Rust toolchain and no cargo cache, so they share nothing with `ci.yml`'s jobs. The *cost* of the separate file (a non-required context) is tracked as its own entry above and is not a defect in this choice.

## U-511 — #271: an empty env override is treated as absent (2026-09-13, lane-1)

- **Assumption:** this is a correction of wrong semantics, not a breaking change — so it is
  decided here rather than escalated as `blocked`.
- **Evidence, measured before deciding rather than argued from intuition.** What an empty override
  did *today* is not uniform; it depends on the field's type, and four of the five arms are a
  refusal to boot or a garbage value that no deployment can have depended on:

  | field type | behaviour with an empty env override, BEFORE this change |
  |---|---|
  | number | hard ERROR — `invalid type: string "", expected an integer` |
  | bool | hard ERROR — `expected a boolean` |
  | `Vec`, not a list-parse key | hard ERROR — `expected a sequence` |
  | `Vec`, list-parse key | `[""]` — a one-element list of nothing |
  | `String` | overrode with `""` |

  Only the `String` arm produced something an operator could lean on. No document in this repo
  promises that an empty env var clears a TOML value — the sole mention anywhere is U-509's own log
  entry describing it as a trap. `docker/config.docker.toml` keeps `jwt_secret` commented out, so
  the shipped recipe's behaviour is unchanged either way.
- **Why it is still made loud:** the one arm that genuinely changes is a `String` override, and a
  silent change of meaning is what turns into a support ticket. `empty_env_overrides_ignored()`
  plus a startup `warn!` in `main.rs` names every variable that was dropped.
- **Status:** CONFIRMED as a correction. Decided under the autonomy ladder, not escalated, because
  the enumeration shows the old behaviour was unusable in four of five arms and undocumented in the
  fifth.

- **Assumption (WRONG as first written, caught during implementation):** that the reporting helper
  and the loader could each decide "empty" independently.
- **What is actually true:** the first draft used `v.trim().is_empty()` in the helper and
  `v.is_empty()` in `load`. A whitespace-only value would then be **applied by `load` and reported
  as ignored by the helper** — the warning would have been misinformation. Both now use strict
  `is_empty()`, and `the_ignored_override_report_matches_what_load_actually_drops` asserts the two
  agree on exactly that case. The JSON escape hatches keep `trim()` deliberately, because
  whitespace is never valid JSON; that divergence is documented at its site.
- **Status:** CONFIRMED by falsification — reverting the helper to `trim()` reddens exactly one test.

- **CONFIRMED (U-507, 2026-09-13) — the hazard is real and is now documented where an upgrader will meet it (`docs/UPGRADING.md:110-124`), which is what this entry asked for: an upgrade-ordering hazard, stated rather than assumed away.** The recipe now
  passes `ACDP_REGISTRY_AUTH__ENABLED: ${...:-}`, which renders as an empty string. A registry
  binary from *before* this change rejects an empty boolean with a hard error, so pulling the new
  `docker-compose.yml` against an older image breaks the boot. Called out in README's Configuration
  section. Not mitigated in code: the alternative is omitting the passthrough, which leaves the gap
  #271 was filed about.
### 9. `mutants.yml` derives the spec pin from `ci.yml` instead of restating it
**Status: CONFIRMED (2026-09-13)** — decided and settled in the same pass, because the evidence
that forced it was a bot PR already in flight rather than a judgement call.
**Assumed.** That a second hardcoded copy of the spec pin would silently diverge forever.
**Chose.** A `pin` step that greps the 40-hex `ref:` out of `.github/workflows/ci.yml` and feeds
it to `checkout-spec` via `steps.pin.outputs.ref`.
**Why it is not DRY tidiness.** `bump-spec.yml:24` passes the bumper exactly ONE filename
(`file: .github/workflows/ci.yml`, singular), so a copy here would never be bumped: `ci.yml` would
move to the new spec and the scheduled mutation job would stay behind, with the per-PR conformance
job and this job measuring different spec versions and nothing reporting it. PR #272 (the bot
adopting `108ff76`) is exactly that, in flight. A comment saying "keep these in sync" cannot fix
it — the actor is a bot whose input is one filename. A machine-read signal needs a machine-read fix.
**The pinned property is preserved.** `ci.yml:400-402` wants "a spec-repo push cannot change this
repo's CI result without a commit here"; a commit here is still required, in one place instead of two.
**Alternatives.** A second bumper call in `bump-spec.yml` (unclaimed; rejected as more moving parts,
and `ci.yml:404-410` records that the bumper refuses to act on a file with two pin anchors, so a
file it silently declines to bump is worse than one never pointed at). Duplicate-and-document
(rejected: the mitigation would have to be a check that FAILS on disagreement, at which point
deriving is less work).
**Blast radius.** Low, and the extraction fails loudly: it asserts exactly one 40-hex `ref:`, exactly
one `checkout-spec@` `uses:` line, and that the ref follows it.
**SUPERSEDED (2026-09-14, U-536, PR #304).** The derivation described above no longer exists.
The pin moved OUT of `.github/workflows/ci.yml` into `.spec-pin` at the repository root, and
`ci.yml`, `mutants.yml`, the bumper and the conformance harness now all read that one file
through `.github/actions/read-spec-pin`. `bump-spec.yml` is pointed at `.spec-pin`, so the
one-filename constraint that forced the derivation is satisfied by the file the pin lives in.
The decision above is not reversed — its property ("a copy would never be bumped, so derive
rather than duplicate") is what made a single declarative source the next step. What it could
not see is that deriving coupled two workflows through the TEXT of one of them: nothing in
`ci.yml` said another workflow parsed it, so a valid reindentation of its spec step broke the
derivation silently, on the following Monday's cron, since `mutants.yml` has no `pull_request`
trigger. The "fails loudly" claim in the line above described the old `pin` step's three
assertions; the equivalent guarantees now live in `spec_pin_violations`
(`crates/acdp-registry-server/tests/conformance_gate.rs`), which asserts ten invariants and
falsifies each one.

## U-510 — the msrv job's `cargo check` steps stay `check` rather than becoming builds

- **Plan:** `plans/u-510-build-feature-configurations.md` (Open question 1)
- **Assumed:** that verifying the 1.88 toolchain *accepts* the language and API surface is the
  msrv job's purpose, and that linking at MSRV is not required once every configuration is linked
  at stable.
- **Chose:** leave both `cargo check` steps as `check`, and say so in the PR, the log and here
  rather than let a reader assume the gap was closed everywhere. `cargo check` shares the exact
  defect this unit fixes — it does not codegen or link — so this is a deliberately unclosed
  remainder, not an oversight.
- **Reasoning:** codegen divergence between 1.88 and stable *for identical source* is a much
  narrower risk than a configuration nothing ever links, and both msrv configurations are now
  linked at stable by this unit's new steps.
- **Alternatives:** convert to `cargo build` (closes it completely, costs MSRV-toolchain build time
  for the narrower risk); add a separate MSRV build job (a new non-required check, so non-blocking —
  the U-508 `lint` problem again).
- **Blast radius if wrong:** a codegen defect that only 1.88 exhibits would still pass CI. Narrow,
  and it would be caught by the stable build for any source-level cause.
- **Status:** PARTIAL (U-507, 2026-09-13) — verified unchanged: both msrv steps are still `cargo check --locked` (`ci.yml`, job `msrv`). The scope choice is defensible and the residual is real and unclosed — U-510 established that `cargo check` neither codegens nor links, so this job does **not** prove the workspace *builds* on 1.88, only that 1.88 accepts the surface. **Settled by:** converting both steps to `cargo build` if MSRV buildability is wanted. **Owner:** the leader. Not claimed as complete.

## U-510 — build steps inside the required `clippy` job rather than a new, honestly-named job

- **Plan:** `plans/u-510-build-feature-configurations.md` (Open question 2)
- **Assumed:** that coverage which actually blocks a merge is worth more than a job name that
  describes itself perfectly.
- **Chose:** inside the existing `clippy` job. It is one of the four contexts in
  `required_status_checks`, so the new builds gate merges immediately. A new job would be a check
  that is not required and therefore cannot prevent a merge — exactly where U-508's `lint` sits,
  still awaiting a decision. Shipping this unit's coverage in that state would have left the gap
  effectively open.
- **The inconsistency with U-508 is apparent, not real, and is argued in the PR rather than
  glossed:** U-508 refused to put shell linting inside `rustfmt` because that is a *different
  concern* wearing a Rust-formatting name. Building a feature configuration is the *same* concern
  this job already serves nine times over. The test is concern identity, not convenience.
- **Alternatives:** a new `builds` job (honest name, non-blocking — rejected); renaming `clippy` to
  something broader (**rejected and dangerous** — those four names are a contract with branch
  protection, and a required context that stops reporting leaves every PR waiting forever).
- **Blast radius if wrong:** a reader sees "clippy" fail on a build error. Mitigated by step names
  (`build (postgres)` etc.) making the failing step obvious, and by the comment in the job.
- **Status:** CONFIRMED (U-507, 2026-09-13) — corroborated by independent adoption: U-516 placed its
  own gate inside an already-required job for the same reason, so putting the build steps in `clippy` was the right shape, not merely the available one.

## U-510 — reconcile outcome for the two entries above (append-only, so their original wording stands)

Recorded as an appended resolution rather than by editing the two `Status:` lines in place. The board
rule for `ASSUMPTIONS.md`, `DECISIONS.md` and `docs/ENGINEERING-LOG.md` this wave is **APPEND-ONLY**,
and U-510's own acceptance criterion 6 enforces it mechanically (0 deletions). An in-place status
edit produces deletions and would have failed that check — which is how the criterion caught the
prose rule being broken. The entries above therefore keep the wording they had when the decision was
still open, and this is the outcome:

- **"the msrv job's `cargo check` steps stay `check`"** — **CONFIRMED (2026-09-13)** by Opus at
  reconcile, as a *bounded, stated remainder* rather than as complete closure. Both msrv
  configurations are now linked at stable by this unit's new build steps, so what goes unverified is
  only codegen divergence between 1.88 and stable for identical source. Stated in the PR body, the
  #265 closing comment and the engineering log, so it cannot be mistaken for the whole gap being
  shut. Full reasoning: `DECISIONS.md`, U-510 decision 2.
- **"build steps inside the required `clippy` job"** — **CONFIRMED (2026-09-13)** by Opus at
  reconcile. The deciding factor is that a separately-named job would not be a required context and
  therefore could not block a merge — the same position U-508's `lint` gate is stuck in, awaiting a
  human decision on repo settings. Reversible in one commit, and it becomes the better choice the
  moment that question is answered, since the same answer applies. Full reasoning: `DECISIONS.md`,
  U-510 decision 1.

## U-513 — n=5 supports a categorical latency claim but no numeric one

- **Plan:** `plans/u-513-correct-latency-claim.md` (Open question 1)
- **Assumed:** that 5 post-change CI runs are enough to say "`clippy` is *sometimes* the critical
  path" but not enough to quote any margin.
- **Chose:** make only the categorical claim. "Sometimes" needs a single instance and there are two
  (one on `main`); a margin needs the spread to be smaller than the difference, and here the spreads
  are 78s and 79s against per-run margins of 4-27s. So no headroom figure is quoted in either
  direction, and the corrected text says why rather than just omitting it.
- **Alternatives:** gather more samples first — rejected, because it would delay correcting a
  measurably false sentence that is on `main` right now, and because no realistic n rescues a margin
  an order of magnitude below the spread; quote a fresh single sample — rejected, that repeats the
  original error with a newer number, which is precisely what the assign forbids.
- **Blast radius if wrong:** a reader takes "sometimes the critical path" as settled when it is based
  on 5 runs. Mitigated by stating n in the table itself.
- **Status:** CONFIRMED (U-507, 2026-09-13) — **the prediction was right on both halves, measured at n=13.**
  Eight further CI runs (read 23:08:31Z) give clippy-minus-tests margins of `-92, -104, -70, -23, +4, -7, -9, -2`s. Combined with the original five (`+12, -18, -27, +26, -4`): **clippy led 3 of 13 runs**, so the frequency sharpened from 2-of-5 (40%) to 3-of-13 (23%) exactly as this entry said it would; and the categorical claim did not change, because clippy still leads sometimes and the widest per-run margin (+26s) remains far below the spreads (clippy 76-193s, tests 146-297s). The new set's single "lead" is **+4s**, which is noise, not headroom — quoting it as a margin would repeat U-510's original error with a fresher number.

## U-513 — the builds stay in the required `clippy` job rather than moving to a parallel job

- **Plan:** `plans/u-513-correct-latency-claim.md` (Open question 2)
- **Assumed:** that keeping feature builds merge-blocking is worth ~8s of mean added PR latency.
- **Chose:** keep them in `clippy`. The parallel-job form is **strictly better on every axis except
  one**: latency cost drops from ~8s to zero, and it *moves* the feature lists rather than copying
  them, so it avoids the two-sources-of-truth objection that kills the scheduled split. The one axis
  it loses on is decisive — a new job is a new check name, and `required_status_checks.contexts` is
  enumerated (`rustfmt`, `clippy`, `tests`, `conformance (spec fixtures)`), so the builds would stop
  blocking merges. U-510 put them inside `clippy` specifically to gain that property; trading it for
  ~8s is the wrong way round.
- **The coupling worth surfacing:** this is the same blocker as U-508's `lint`. One settings change —
  adding contexts to branch protection — would unblock **two** improvements, not one.
- **Alternatives:** scheduled split (rejected: duplicates the feature lists, and delays breakage
  detection by up to a day); swap clippy for build on the four never-linked configs to halve the cost
  (rejected: loses the lint coverage W3-U10 added for #200, trading one gap for another).
- **Blast radius if wrong:** ~8s per PR persists until the settings decision. Trivially reversible —
  moving the steps to their own job is one commit, and becomes correct the moment the contexts change.
- **Status:** **CONFIRMED as-is** (2026-09-22, the human, presented with U-575's evidence) — the
  human was shown the mechanism (a parallel job needs a new required-context settings change,
  which is outward-facing and not an agent's to make unilaterally) and the updated cost (build
  steps measure 2-10s each, off the `tests` job's critical path, so today's placement costs close
  to nothing) and chose to leave the builds inside `clippy` rather than spend a branch-protection
  settings change to chase a latency win that no longer meaningfully exists. Not left open for a
  future settings change — a real ruling, not a deferral.
  Superseded text, kept because this file is cumulative: *"UNCONFIRMED (re-examined 2026-09-22,
  U-575, Opus under `/reconcile`) — still open; do not conflate with U-508's discharge. The human
  did make a `required_status_checks.contexts` change on 2026-09-16, adding `lint` and
  `cargo-deny` — but that discharged U-508's blocker, not this one. Neither new context is a
  channel for a parallel feature-builds job: `lint` is shellcheck/actionlint, `cargo-deny` is the
  `audit` job's dependency/license/advisory check. Live-verified (`gh api
  .../branches/main/protection`) that no `build`-type context exists in the required list, and
  `ci.yml`'s `clippy` job (lines 147, 252, 262, 272, 282) still runs the five feature-config
  `build` steps inline — they have not moved, and the job's own comment still gives this entry's
  exact reasoning verbatim. New supporting evidence, not present when this entry was written: the
  build steps measure 2-10s each, cheaper than the `clippy` step beside them, and `clippy` itself
  (~39s) is not on the critical path (the `tests` job runs ~2m36s) — so today's placement costs
  close to nothing in practice, which further weakens the case for spending a settings change on
  this. Settled by: a `required_status_checks.contexts` change adding a feature-builds-specific
  context (not `lint`/`cargo-deny`). Owner: the human."*

## U-504 — #216: the mutation ratchet extended to `handlers/context.rs` (2026-09-13, lane-2)

Ten entries. All resolved in-context this unit (see the declared `/reconcile` deviation in
DECISIONS.md 19), so none is left dangling for a later pass.

1. **Assumed:** the survivor budget may rise from 1 to 8, rather than the unit being obliged to kill
   all 28 or keep the old ceiling.
   - **Chose:** raise it, with a per-survivor written argument and the honest comparison stated where
     the number lives (`MUTANTS_EXPECTED_SCOPE` tripled, 74 → 213; the comparable figure is 28, not 1).
   - **Alternatives:** hold at 1 (rejected: only reachable by narrowing the scope back, which is the
     defect the equality check on scope exists to catch); kill all 28 (rejected on evidence — 8 are
     not killable today, four being genuine equivalents).
   - **Blast radius:** a budget that is slack rather than a list would let a real survivor hide.
     Mitigated by naming all 8 in the env block and verifying by content that the measured set equals
     the documented set.
   - **Status:** CONFIRMED (2026-09-13) — measured 8/8, set-identical to the documented list.

2. **Assumed:** the ratchet's `timeout != 0` rule should become a named ceiling of 1.
   - **Chose:** budget the one provably non-terminating mutant (`context.rs:1277:12`); a second
     timeout still fails. The original rule was reasoned about *slow tests*, which remains right and
     is a different case.
   - **Alternatives:** leave `!= 0` (rejected: the scheduled job would go red on its first run for a
     mutant no test can convert into a fast failure); drop the timeout check (rejected: that is the
     category exemption the original reasoning warned about).
   - **Blast radius:** a genuinely slow test could hide one survivor. Bounded to one, and the error
     message now distinguishes the two causes and names the fix for each.
   - **Status:** CONFIRMED (2026-09-13) — measured `:1277` TIMEOUT at the full 300s, `:1223` MISSED
     in 10s, from the same targeted run.

3. **Assumed:** `-j1` is the right parallelism for the scheduled job, against the intuition that more
   jobs are faster.
   - **Chose:** `-j1`. Measured on one 18-mutant shard: `-j6`/`-j8` each exceeded 10 min with every
     mutant still building; `-j1` took 1m55s (~6.4s/mutant).
   - **Blast radius:** none beyond wall-clock; reversible in one line.
   - **Status:** CONFIRMED (2026-09-13); approved by the leader as an in-grant call.

4. **Assumed:** `parse_visibility`'s `"public"` and `"restricted"` arms (`:81`, `:82`) are equivalent
   mutants rather than coverage gaps.
   - **Chose:** argue them, on a measurement rather than a reading: search returns only PUBLIC rows to
     every requester (probed with anonymous, an audience member, and the row's own producer). The
     sibling `"private"` arm was CAUGHT, which is what a redundant-guard site looks like.
   - **CONTINGENT, and the contingency is recorded in the workflow beside the number:** both become
     real coverage gaps the moment search serves restricted rows to entitled requesters, and the
     budget must then drop to 6.
   - **Status:** CONFIRMED (2026-09-13) as equivalent *under current §4.5 search semantics*.

5. **Assumed:** `lifecycle_outcome`'s two survivors (`:1399`) should be deferred rather than asserted
   from `http_integration.rs`.
   - **Chose:** defer with a named follow-up. `/metrics` is deliberately not mounted in that harness
     (404, measured) and `metrics_integration.rs` is a separate binary precisely to isolate the
     process-global `metrics` recorder (its own module doc). Asserting there would put 158 tests
     behind shared mutable state.
   - **Alternatives:** mount `/metrics` in the http harness (rejected: breaks the isolation that file
     exists to maintain); claim-request `metrics_integration.rs` (rejected: low-value label, and the
     unit already carries enough surface).
   - **Status:** DEFERRED — settled by a rejected-transition label assertion in
     `metrics_integration.rs`, which is the named follow-up.

6. **Assumed:** the did:web lifecycle survivor (`:1542`) cannot be killed within this unit.
   - **Chose:** accept with a follow-up, having *verified* rather than assumed it: a did:web-signed
     retract was written and fails at `key_resolution_unreachable`, because `retract_verified`
     resolves through a real `WebResolver` and playground mode does not bypass it.
   - **Worth surfacing beyond this unit:** the whole did:web lifecycle branch has zero coverage, and
     structurally cannot have any until an HTTPS fixture serves `agents.test`'s did.json.
   - **Status:** DEFERRED — settled by that fixture, which would also unblock did:web publish.

7. **Assumed:** AC-8's fourth invariant should ban a hardcoded 40-hex ref in `mutants.yml`.
   - **CHANGED.** As proposed it would have been a broken guard: `mutants.yml` legitimately carries
     four 40-hex **action** pins (`rust-toolchain@`, `rust-cache@`, `install-action@`, its own
     `checkout-spec@`), because pinning actions by SHA is correct and required.
   - **Chose:** narrow it to a literal on a `ref:` line, plus a test asserting action pins are NOT
     flagged. A guard that fails against the correct file is a guard that gets deleted.
   - **Status:** CHANGED (2026-09-13) — narrowed before shipping, caught by running it against the
     real file rather than against the idea of the file.

8. **Assumed:** AC-8's invariants should be a pure function over both workflows' text rather than
   assertions against the real paths.
   - **Chose:** the pure function. Two reasons, one of them forced: `.github/workflows/ci.yml` is
     outside this unit's grant, so falsifying by editing it was never available; and each invariant
     then gets its own falsification against a valid-YAML restructuring.
   - **Status:** CONFIRMED (2026-09-13) — all four falsified, and the falsification test itself
     falsified by disabling each check in turn.

9. **Assumed:** the refill loop's six survivors can be killed by one test if it asserts *how far the
   scan got* rather than the matches.
   - **Chose:** discriminate on `next_cursor` — resume an unfiltered search from it and count what
     remains (10 of 70 when the cap holds, 60 when the loop gives up after one page, no cursor at all
     when it runs to exhaustion). The matches are empty either way and cannot tell them apart.
   - **Status:** CONFIRMED (2026-09-13) — all six killed, each by the number it should change.

10. **Assumed:** killing these survivors needs no claim on `crates/acdp-registry-core/src/handlers/context.rs`.
    - **Chose:** no claim-request. Mutating a file needs no write access, and every one of the 20
      kills is an additive test in the granted `http_integration.rs`. A surviving mutation means a
      missing test, not a wrong implementation — which held for all 20.
    - **Status:** CONFIRMED (2026-09-13); the leader confirmed no claim-request was needed.

## U-521 — retiring two accepted mutation survivors in `handlers/context.rs` (2026-09-13, lane-2)

Plan: plans/u-521-didweb-fixture-and-metrics.md

1. **Assumed:** the did:web TLS fixture should be a committed certificate, because generating one
   needs `rcgen` and that means a dev-dependency plus lock churn.
   - **CHANGED**, and the reason is policy rather than engineering. `.gitignore:39-43` forbids TLS
     material repo-wide (`*.pem`/`*.crt`/`*.key`) under an explicit `# TLS material` header, and
     `git ls-files` finds ZERO committed TLS material anywhere in the repo. The file's three
     negations are otherwise deliberately non-secret (`!.env.example`, `!plans/cross-repo/`), and the
     TLS block's own negation is an empty `.gitkeep` — there is no precedent for negating a
     secret-bearing pattern.
   - **Chose:** generate the CA+leaf chain per test process with `rcgen`. Committing it would have
     required an exception to a secret-guard, which is repo policy — the leader confirmed that is
     neither a lane's call nor the leader's, so (A) could only have been forwarded to the human.
     Generating needs no exception, so there was nothing to escalate.
   - **Also removes,** which is why this is a better outcome and not merely a permitted one: the
     fixture directory, its README, the secret-scanner allowlist entry it said would be needed, and
     `didweb_fixture_certificates_are_not_near_expiry` plus its helper (76 lines). Nothing persists,
     so nothing can lapse, so there is no expiry guard to maintain. A guard for a problem that no
     longer exists is worse than no guard: it implies something is being watched.
   - **Cost, corrected against my own estimate:** I argued this as "2 new lock entries". Measured
     **14** — `rcgen` declares `x509-parser` optional and Cargo.lock records optional deps whether or
     not they compile. Exactly **3** are actually built (`rcgen`, `yasna`, `pem`); the other 11 are
     lock-only. Neither `rcgen` nor `yasna` appears in the `no-dev` graph.
   - **Status:** CHANGED (2026-09-13) — granted as option B after the stop-work.

2. **Assumed:** "it's only a dev-dependency" is why adding `rustls` cannot affect the shipped binary.
   - **CHANGED.** That is a claim about the **resolver**, not about the section heading. The
     workspace sets `resolver = "2"` (`Cargo.toml:12`), under which dev-dependency features are not
     unified into the normal build; under `resolver = "1"` the same entry would have been a
     production change wearing a test-only label.
   - **Chose:** measure it rather than cite it. `cargo tree -e features,no-dev` is byte-identical
     with and without the entry (469-line inverted graph, 3406-line forward), **and** the same probe
     with dev edges does show the difference — which is what makes the identical result evidence
     rather than a blind diff.
   - **Status:** CHANGED (2026-09-13) — the reasoning came from the leader; the measurement and the
     control are mine. If `resolver` ever changes, both dev-deps need re-deciding, not re-testing.

3. **Assumed:** the rejected-transition witness should be `invalid_lifecycle_transition`, the wire
   code `lifecycle_outcome`'s own doc comment names.
   - **CHANGED.** That rejection needs a published context, and `metrics_integration.rs`'s counters
     are process-global with a single test owning the accumulation-sensitive assertions — including
     `publish_total{outcome="inserted"} == 2`. The suite said so: "9 passed, 1 failed, two accepted
     publishes: left 3.0, right 2.0".
   - **Chose:** retract a `ctx_id` that does not exist — a rejection needing **no publish at all**,
     whose error still flows through `lifecycle_outcome` because the metric wraps the whole
     `lifecycle_transition` call. It perturbs no series that test pins, and the retract route carries
     a different `route` label. Preserving that documented convention is worth more than the more
     quotable wire code.
   - **Rejected:** editing the other test's expected 2 to 3. It would have coupled two tests and left
     the next person adding a publish at the same wall.
   - **Status:** CHANGED (2026-09-13) — falsified by stubbing `lifecycle_outcome` to `""` and to
     `"xyzzy"`; both fire, with the other 9 tests unaffected.

4. **Assumed:** every assertion in the two new tests can be falsified individually.
   - **DEFERRED as partly false**, and named rather than papered over. The stray-series loop (no
     lifecycle event under `outcome=""` or `"xyzzy"`) cannot be fired by any mutation at its site,
     because the count assertion above it always fails first; it is shown live only by a contrived
     extra `("retract", "")` emission, and it guards double-recording rather than a gutted
     `lifecycle_outcome`. Likewise `code == "not_found"` and the scrape-status assertion sit behind
     the 404 precondition and are not independently falsifiable here.
   - **Chose:** keep all three and say so in the test's doc comment. An assertion that cannot be
     falsified at its site is not automatically decoration — but claiming it was falsified when it
     was not is the failure this discipline exists to prevent.
   - **Status:** DEFERRED (2026-09-13) — evidence that would settle it: a mutation that makes a
     transition both recorded and misrecorded.

5. **Assumed:** the production `bind_rustls` provider gap should be fixed in this unit, since the
   fix is one line and I am already touching TLS setup.
   - **Chose:** no. `src/main.rs` is outside the grant, and the leader filed it as U-530 with the
     board recording in my own words that my dev-dep grant fixes the **test binary only** — so
     approving the claim cannot later read as the finding having been handled.
   - **Exposure, stated because the alarming version was available:** zero today.
     `docker/config.docker.toml` disables in-process TLS deliberately (an edge terminates) and
     `config/registry.example.toml` ships `cert_path`/`key_path` commented out. It is latent on a
     documented configuration path, not a live outage. I could not establish that from inside this
     unit's grant; the leader checked it.
   - **Status:** CONFIRMED (2026-09-13) — deferred to U-530 by decision, not by omission.

## U-507 — reconciliation of this file's open entries (lane-3, 2026-09-13)

Resolutions are **appended here**; only the status *token* on each entry's own status line was
rewritten in place, per the narrow grant. Entries are addressed by line number as of base `f81013e`
so the mapping is checkable. Every resolution names its evidence; none says "reviewed and confirmed".

- **Scope, re-derived rather than inherited (read 17:08:08Z, base `5fd7cb5`).** **37 open items**
  carrying **46 open status declarations**. Three earlier figures existed and all three are unfit:
  `grep -c UNCONFIRMED` = **50** (counts prose — `:328` narrates a *past* status, `:379`
  cross-references a flip); an anchored `^\s*-?\s*\*\*Status:?\*\*\s*UNCONFIRMED` = **28**, which
  *undercounts* because this file's statuses also appear mid-prose-line (`:708`), with a
  parenthetical (`**Status (updated 2026-09-01):**`), spelled `**Status of the original
  assumption:`, with the **token wrapped onto the next line** (`:980`, `:1031`, `:1056`), and — 13
  times — as a **bullet or heading label with no `Status` word at all** (`- **UNCONFIRMED —
  awaiting human ruling:**`, `### OPEN — escalated, NOT decided here:`); and U-505's hand count of
  **35** (`:2544`), which excluded two shapes it listed separately and predates 8 entries added
  since. **U-505's enumeration and this one agree exactly** at 37 declarations before `:2537`
  (U-505: 25 + 1 status-line, 9 bullet-is-status, +1 `###`-heading, +1 in-entry update; here: 26
  status-labelled + 11 bullet-label). Two independent methods, same number.

### Resolved: `conformance (spec fixtures)` is now a required context — entry `:338`, status `:370`

**UNCONFIRMED → CONFIRMED.** The entry parked this on "awaiting a repo admin to action the
branch-protection change". It has been actioned. Evidence, `gh api
repos/agentcontextdistributionprotocol/acdp-registry-rs/branches/main/protection`, read
2026-09-13T17:17:51Z:

    required_status_checks.contexts = ["rustfmt","clippy","tests","conformance (spec fixtures)"]
    strict = true

- **A stale clause this unit could not fix.** `:370` now reads `CONFIRMED (awaiting a repo admin to
  action the branch-protection change)`, which is self-contradictory: the parenthetical is false and
  correcting it is a **prose** edit, outside the status-token grant. Flagged rather than silently
  exceeded — it needs either a one-line grant extension or a follow-up. The token is the
  machine-read fact and it is now true; the clause beside it is not.

### Resolved: `git_only = true` and `git_tag_name` are as recorded — entry `:800`, status `:815`

**UNCONFIRMED → CONFIRMED.** Read from the file, not from the entry's own prose:
`release-plz.toml:13` is `git_only = true`; `:36` is `git_tag_name = "{{ package }}/v{{ version }}"`.
The reasoning is documented in place at `:15-18` (why `git_only` alone is sufficient and not a stale
TODO) and `:29` (why changing the template orphans existing tags). `release-plz.toml` was **read
only** — U-507 is explicitly barred from modifying it or #278.

### Resolved: the new-shape tags were actually minted — entry `:817`, status `:830`

**UNCONFIRMED → CONFIRMED.** This was recorded as "the one link with no local evidence". There is
now remote evidence: `git ls-remote --tags origin 'refs/tags/acdp-registry-server/*'` returns four
tags in the new shape — `v0.1.0`, `v0.1.1`, `v0.1.2`, `v0.1.3` — each with its annotated `^{}` peel,
so they are real annotated tags pushed by the bot, not lightweight local artefacts.

### Resolved: GitHub's ref matcher accepts `acdp-registry-server/v*` — entry `:875`, status `:887`

**UNCONFIRMED → CONFIRMED**, by a real push event rather than by reading the docs. Run
**34734871991**: `event=push`, `head_branch=acdp-registry-server/v0.1.3`, workflow `docker`,
`conclusion=success`, against `docker.yml`'s trigger
`on.push = {branches: [main], tags: ['acdp-registry-server/v*']}`. The run existing is the proof —
the workflow cannot start unless the pattern matched the ref.

- **Method note, because the first attempt produced a false negative:** `gh run list --workflow
  docker.yml --limit 40` filtered on `headBranch` returned **nothing**, because 40 runs no longer
  reach back that far. The absence was an artefact of the window, not of the fact. Querying the run
  directly settled it.

### Resolved: `ACDP_BOT` holds `contents: write` + `pull-requests: write` — entry `:889`, status `:898`

**UNCONFIRMED → CONFIRMED**, demonstrated by exercised permission rather than by reading a settings
page. `app/acdp-deps-bot` has opened PRs **#225, #230, #236, #272, #278** (`pull-requests: write`),
and the four annotated release tags above were pushed by the same release-plz flow
(`contents: write`). A permission that has been used is better evidence than one that is listed.

### Addendum to batch 1 — two entries whose own bodies already answered them

Recorded because both findings are stronger than the evidence I first cited, and because the method
error that nearly hid them applies to the rest of this unit: **I flipped these on the status line and
the heading, without reading the entry's trailing bullets.** An entry's `Update:` / `Executed:` /
`Correction:` bullets sit *after* the status line, so a status-line-only read sees neither the
evidence that discharges it nor the caveat that would block it. The remaining entries in this unit
are read whole before any flip.

- **`:338` / status `:370` — the status line contradicted its own entry body, for twelve days.** The
  bullet immediately below it reads `**Executed (2026-09-01):** a repo admin actioned the recorded
  recommendation`, with the same read-only `gh api …/branches/main/protection` call and the same
  resulting `contexts`. So this was never "awaiting a repo admin": it was actioned on 2026-09-01,
  recorded one line below the status, and nobody flipped the token. My own read at 17:17:51Z
  re-confirms it independently, but the primary finding is the internal contradiction — which also
  explains the stale `(awaiting a repo admin …)` clause flagged above: it has been false since
  2026-09-01.

- **`:817` / status `:830` — the `:831` update is superseded by this unit's evidence, and it names
  the exact condition that supersedes it.** The update (2026-09-11) narrowed the claim to "the code
  path is demonstrated, the remote write is not" and stated the discharge condition itself:
  *"criterion 2a is still only dischargeable by observing the real post-merge run."* That run has
  since happened. `git ls-remote --tags origin 'refs/tags/acdp-registry-server/*'` returns
  `v0.1.0`, `v0.1.1`, `v0.1.2`, `v0.1.3`, each with its annotated `^{}` peel — the remote write,
  observed. The entry's own acceptance test ("a first run that is green and PR-less but mints NO tags
  is a FAILURE") is therefore passed, not merely argued. The `:831` text stays as written: it was
  true on 2026-09-11 and is superseded rather than wrong, and correcting its wording would be a
  prose edit outside this unit's grant.

- **Declaration count, stated because a reviewer comparing notes will hit it.** The leader's
  independent pass lists `:831` as a real status declaration; this unit's extractor classifies it as
  prose, because the token is not the bullet's leading label and because entry `:817` is already
  counted once via `:830`. Counting it either way gives **37 items** — the number the work is scoped
  by — and 46 or 47 declarations respectively. The divergence is a definitional one about whether an
  update restating a status is a new declaration, not a disagreement about what is open.

### Batch 2 — three resolved, one confirmed still-open, all anchored by quoted content

Anchored by **quoted content, not line pins**, per CHARTER rule 15 — and per `:900`'s own finding,
which this unit has now reproduced twice. Line numbers are given only as a convenience and are
correct at base `f093db7`.

**Resolved — `## REG-11 Phase 1 — extending #136's fix-forward past the planned two-file scope`**
(status line `:444`, **UNCONFIRMED → CONFIRMED**). The extension was the right call and the work
landed completely. A bound check rather than a spot check, because a partial migration would look
identical to a finished one at any single call site:

    thread_rng remaining in crates/ : 0 files
    OsRng     remaining in crates/ : 0 files
    SysRng    present in           : 2 files  (acdp-registry-auth/src/jwt.rs, acdp-registry-server/tests/http_integration.rs)
    Cargo.toml                     : rand = "0.10", hmac = "0.13"

All five call sites the entry enumerated are migrated, the direct dependencies are at the bumped
versions, and `main` is green. The entry's own falsified premise ("the literal '2 files' premise is
falsified") is exactly what the bound check confirms: it was five, and all five are done.

**Resolved — `### Rule-10 / rule-15 sweep: a FOREIGN pin went stale because of this branch`**
(status line `:911`, **UNCONFIRMED → CONFIRMED**). The observation is correct, and this unit has
produced a **third instance of it** while verifying the second. The entry `### Left standing
deliberately — docker/RAILWAY.md:57 and :68 are FALSE on main today` pins two lines that now land on
blockquotes added by U-503; the `ACDP_REGISTRY_AUTH__JWT_SECRET` row it meant to cite has moved to
`:112`. So the pattern is not "twice in two units" but at least three times, in three different
files, always because a docs edit shifted lines under a citation the citing lane could not edit.
**Its prescribed fix is adopted here:** this unit's own resolutions quote content. My batch-1
resolutions cited line numbers and will drift for exactly the reason this entry documents.

**Resolved — `### UNCONFIRMED: the four new steps run clippy, not cargo build`** (label form, no
flippable token under the current grant — see the `blocked` message of 17:31Z). **Superseded by
U-510 / #265.** The `clippy` job now runs **9 clippy steps and 5 `build (...)` steps**, so the
"two classes of configuration — one lint-checked, one not" that the entry weighed no longer exists:
every configuration is both linted and built. PR **#273**, merged `9df7c97`, closes **#265**. The
entry's reasoning was sound at the time and its own escape hatch ("reverses in one line per step if
the leader disagrees") was never needed — the gap was closed by addition, not reversal.

**Still open, deliberately, with a trigger — `## predecessor_admission enforcement: store-level
coverage, not end-to-end wiring`** (status line `:569`, stays **UNCONFIRMED**). Checked rather than
assumed, and the residual is intact:

- The only `predecessor_admission` references in `crates/acdp-registry-server/tests/` are three
  occurrences of **`predecessor_admission: None`** in `pg_integration.rs`. That is test setup which
  *disables* admission, not coverage of it — so those tests could not notice upstream dropping
  `Some(..)`, which is precisely the residual the entry recorded.
- `conformance.rs`'s `rev-001` fixture does now cite RFC-ACDP-0014 §4/§5, which could look like the
  missing coverage. It is not: its own docstring scopes it as *"a single-vector ACCEPT golden,
  structurally identical to sig-001/003"* for **key revocation**, plus a negative for §5 step 2. The
  predecessor-admission reject path is still uncovered.
- **What would settle it:** one end-to-end HTTP publish test that passes `Some(closure)` and asserts
  the closure's `Err` surfaces as the RFC-ACDP-0014 §4 rejection. **Who owns it:** it must live in
  `crates/acdp-registry-server/tests/conformance.rs`, which is **not** in U-507's grant — so it is a
  unit for whoever holds that file, not a thing this unit may fix. Upstream spec issue **#57** still
  governs whether a fixture will ever supply it.

### Batch 3 — the widened grant applied, and the contradiction batch 1 shipped is now repaired

The grant was widened at 17:37Z from "the status token" to "a complete status line — token plus its
reason clause", after measurement showed the token-only form fit 6 of 41 remaining declarations.
Append-only still governs every non-status line, so no `Assumed:` / `Chose:` / `Why:` / `Update:` /
`Correction:` text is touched anywhere in this unit.

- **`- **Status:** CONFIRMED (awaiting a repo admin to action the branch-protection change)` —
  repaired.** Batch 1 could only rewrite the token, which left a true status welded to a false
  clause. The line now records what actually happened: actioned 2026-09-01, recorded in the entry's
  own `Executed` bullet, re-verified independently 2026-09-13. This was flagged in the file rather
  than fixed by widening my own grant, which is why it survived to be repaired properly.

- **`### UNCONFIRMED: the four new steps run clippy, not cargo build` → `SUPERSEDED`.** Evidence
  recorded in batch 2 above; the heading is now flippable under the one-line treatment. Enumerated
  rather than counted by string match, because the two differ here: the `clippy` job holds **17
  steps**, of which **9** are named `clippy (…)` and **5** are named `build (…)`. The file contains
  6 occurrences of the string `build (`, so a string count answers a different question than a step
  count and cannot settle this one.

- **`**Status: UNCONFIRMED — blocked on the R3 ruling, not on evidence.**` → `SUPERSEDED`.** Only
  the bolded status span was rewritten; the sentence it shares a physical line with ("The evidence
  is …") continues onto the next line and is untouched. The entry's own fallback clause is what
  fired: *"If R3 is declined, these two lines still need a standalone factual fix"* — the standalone
  fix landed, so the item closed without the ruling. `grep -ci 'never validated'` on
  `docker/RAILWAY.md` is **0**, and the `ACDP_REGISTRY_AUTH__JWT_SECRET` row now states the opposite
  of what the entry reports as false there.

- **`## U-505 — index of deferred work` → `PARTIAL`, because only one of its two halves is closable
  here.** The status asserted two things and they have different fates:
  - *"the `ASSUMPTIONS.md` count of 35 is exact and bound-checked"* — **CONFIRMED**, by independent
    re-derivation rather than by agreement. U-507's parser and U-505's hand enumeration reach the
    same partition: 26 status-line declarations (U-505: 25 `UNCONFIRMED` + 1 `OPEN`) and 11
    label-form (U-505: 9 bullet-is-status + 1 `###`-heading + 1 in-entry update), for 37
    declarations before `:2537`. Two methods, built from opposite directions, same number.
  - *"the `DECISIONS.md` count of 42 is an upper bound"* — **still open, and correctly so.** The
    entry states the bound it could not close: items whose resolution lives in a sibling repo's
    history (the spec-repo dispatch matrix, `acdp-ci/DELIVERY-STANDARD.md`) were not verified to the
    same standard. **What would settle it:** classifying those 42 against the sibling repos'
    history — a cross-repo *read*, which is permitted, but a unit's worth of work scoped to
    `DECISIONS.md`. **Who owns it:** not U-507, whose grant is this file. Left as an upper bound
    with the reason attached rather than silently promoted to exact.

### Batch 4 — two verified against the tree, and the three AC6 entries U-516 touches

**`## H-A / P2 — 408 is not given an RFC-ACDP-0007 §5 envelope` → CONFIRMED.** The constraint the
decision rested on is unchanged, checked rather than assumed: `AcdpError` has **0** variants
mentioning `Timeout`, and `acdp_wire_code` (`crates/acdp-registry-types/src/error.rs:138`) still has
no timeout arm. The one `timeout` string in that file (`:528`, `"did:web timeout"`) is a did:web
resolution message, not a §5 wire code — a grep for `timeout` alone would have read as a hit. So
scoping P2 to 413 was right and remains right; emitting `internal_error` for a client-side timeout
would still attribute a client condition to a server fault. **Residual, unchanged:** a client parsing
envelopes uniformly still gets no `error.code` on a 408. **Settled by:** registering a
`request_timeout` code in the shared §5 registry. **Owner:** whoever holds `acdp-registry-types` and
the spec — not this unit.

**`## The caps/config invariant is made unrepresentable for new callers` → CONFIRMED.** Still
accurate in every part that matters: `with_anonymous_public_reads` exists
(`crates/acdp-registry-server/tests/common/mod.rs:390`), the guard
`the_helper_sets_both_knobs_not_just_one` is present in `caps_visibility.rs`, the divergent site
`config_shipped_disclosure_default` is still there (`http_integration.rs:1579`, used at `:1676`), and
**no enforcing `debug_assert` was added** — so the constructive-not-enforcing shape the entry
describes is exactly what ships. **One number I deliberately did not contradict:** the entry says 75
existing construction sites; a pattern for direct field assignment finds 39 lines today. The entry
does not state its counting method, so 39 and 75 may be answers to different questions. Recorded as
unverified rather than as a discrepancy — refuting a number requires matching how it was counted.

#### AC6 — what U-516 discharged, and what it did not

U-516's actual shape, measured rather than taken from its description:

    ci.yml      job=fmt    published='rustfmt'  required=TRUE   step if=${{ !cancelled() }}
    docker.yml  job=build  published='build'    required=FALSE  step if=${{ !cancelled() }}

The gate runs in **two** places, and the `ci.yml` one sits in a job whose published name is already a
required context — so it blocks merges **with no branch-protection change**. Both sites carry
`!cancelled()`, which closes the skip-chain defect where an upstream failure silently skipped the gate.

- **`## U-508 — "PR-blocking" means runs-and-can-fail, not listed-in-branch-protection` → PARTIAL.**
  The *technique* is now proven in production, which is more than the entry could claim when written.
  What is **not** discharged is the entry's own ask: `lint.yml` is still a separate workflow whose only
  job publishes the non-required context `lint`, and `required_status_checks.contexts` is still
  `["rustfmt","clippy","tests","conformance (spec fixtures)"]`. So `lint` still cannot block a merge —
  but there is now a demonstrated second route (move its steps into an already-required job) that needs
  no settings change at all. PARTIAL rather than CONFIRMED because the route is proven and untaken.

- **`## U-510 — build steps inside the required clippy job` → CONFIRMED.** Corroborated by
  independent adoption: U-516 reached for the same technique for the same reason. That turns the
  original choice from "the available option" into "the shape this repo converges on", which is
  stronger evidence than the entry could produce for itself.

- **`## U-513 — the builds stay in the required clippy job rather than moving to a parallel job` →
  stays UNCONFIRMED, deliberately.** This is the AC6 case that goes the *other* way, and it matters
  that the reasoning is written rather than the status copied: U-516's move works precisely because a
  checkout-only check has no parallelism to lose. These build steps do. Parallelism requires a
  separate job, a separate job publishes a new check name, and a new check name is not in an
  enumerated `contexts` list — so the blocker is untouched by U-516. **Settled by:** the
  branch-protection change. **Owner:** the human. Flipping this one on U-516's evidence would have
  been the exact error AC6 exists to prevent.

### Batch 5 — AC5: the four entries that are not this unit's to resolve

All four keep their original open token. **Zero flips in this batch, by design** — the job was to make
each ask precise, not to answer it. Each status line was rewritten to carry a **Settled by** and an
**Owner**, because an open item with no named owner is indistinguishable from a forgotten one.

- **`awaiting human ruling: whether docker/RAILWAY.md should require ACDP_REGISTRY_AUTH__ENABLED`** —
  still open. Owner named as **the human, not the leader**, which matters here: the leader has already
  said in this run that it holds no authority over repo settings or product recipes, so recording it as
  "the coordinator's" would park it with someone who cannot discharge it. Note the ask has **narrowed**
  since it was written: the factual defect in that file was fixed independently (see batch 3's
  `:1127`), so only the recipe change is still waiting.

- **`escalated to the human — a product judgement on a public API`** — still open, recommendation
  unchanged ("confirm as taken"). Verified that the situation it describes has not drifted:
  `PG_ENGLISH_STOPWORDS` is still a hand-written const (`crates/acdp-registry-store/src/fulltext.rs:46`)
  with a length check against Postgres at `:195`, so the trade the entry made — keep the table, verify
  it rather than trust it — is still the shape that ships.

- **`handed to the coordinator as a standalone decision`** — still open. **Owner: the leader**, and
  this is the one of the four where that is correct rather than a deflection; the other three are the
  human's. Worth distinguishing, because "escalated" has been used in this file for both.

- **`OPEN — escalated, NOT decided here: the §5 code for a 415`** — still open, and **re-verified
  rather than assumed**. `acdp_wire_code` emits **24** codes and **none describes a media-type
  failure**, so enveloping a 415 still requires minting a code the canon lacks, which is a policy
  decision for the spec holder. One precision: a filter for media-type-ish names flags
  `unsupported_algorithm`, which matches only on the substring "unsupported" and is about signature
  algorithms. It is not a media-type code, and a looser grep would have reported the gap as already
  closed.

### Batch 6 — the three entries this lane authored, resolved on fresh measurement

These are U-508's, U-510's and U-513's own assumptions. Resolving one's own entries is where the
temptation to confirm-by-familiarity is strongest, so each was re-measured rather than recalled.

**`## U-513 — n=5 supports a categorical latency claim but no numeric one` → CONFIRMED, at n=13.**
This entry made a falsifiable prediction — *"more runs will sharpen the frequency; the categorical
claim will not change unless clippy's distribution moves"* — and it can now be checked instead of
believed. Eight further CI runs, read 2026-09-13T23:08:31Z, give clippy-minus-tests margins of
`-92, -104, -70, -23, +4, -7, -9, -2`s. With the original five (`+12, -18, -27, +26, -4`):

    clippy led        3 of 13 runs        (original 2 of 5 = 40%; new 1 of 8 = 12%; combined 23%)
    clippy range      76-193s             tests range   146-297s
    widest margin     +26s                widest spread 151s

Both halves held: the frequency sharpened (40% → 23%) and the categorical claim is unchanged, because
clippy still sometimes leads and no margin is quotable. **The new set's one lead is `+4s`** — that is
noise, and quoting it as headroom would reproduce U-510's original defect with a fresher number, which
is the specific trap U-513 existed to fix.

**`## U-508 — a separate lint.yml rather than jobs inside ci.yml` → CONFIRMED.** The stated reason is
unchanged and still correct: the linters need no Rust toolchain and no cargo cache, so they share
nothing with `ci.yml`'s jobs — the choice was about *sharing*, not about blocking. U-516 does not
refute it; it only shows a required job is available if blocking is wanted. The cost of this choice (a
non-required `lint` context) is tracked as its own entry and is not a defect in this decision.

**`## U-510 — the msrv job's cargo check steps stay check rather than becoming builds` → PARTIAL, not
CONFIRMED.** Verified unchanged: both steps are still `cargo check --locked` in `ci.yml`'s `msrv` job.
The scope argument is defensible — MSRV asks whether 1.88 *accepts* the surface. But the residual is
real and this unit declines to paper over it: **U-510 itself established that `cargo check` neither
codegens nor links**, so this job does not prove the workspace *builds* on 1.88. Confirming it outright
would use my own unit's finding to justify ignoring my own unit's gap. **Settled by:** converting both
steps to `cargo build`. **Owner:** the leader.

### Batch 7 — five items, of which three closed themselves while nobody was looking

The pattern worth naming: three of these were not resolved by argument but by *other work landing*.
Nobody went back to flip them, which is the failure mode this whole unit exists to correct.

**`## W2-U1 — #185 pinned-keys guard hoist` → RESOLVED.** The entry left the placement of the shared
playground validator ("`acdp-registry-types` or `acdp-registry-core`") to "whoever takes them". They
were taken and it landed in **core**: `validate_playground_config` at
`crates/acdp-registry-core/src/playground.rs:259`, called from **two** doors —
`acdp-registry-server/src/main.rs:338` (startup) and `acdp-registry-core/src/handlers/admin.rs:182`
(the admin reload path, which is #192's). Both **#192 and #193 are CLOSED/COMPLETED**. So it is not
merely placed, it is genuinely *shared*, which was the point.

**`## W3-U1 — validating playground config at both doors` → CONFIRMED, and pinned by a test.** The
deliberate departure (refuse the five structural defects, warn loudly on an all-expired list) is still
what ships, and it is protected by an assertion that names the decision:
`main.rs:1874` reads `.expect("an all-expired list must WARN, not refuse — see DECISIONS W3-U1-b")`.
A deliberate deviation guarded by a test citing its own decision record is the strongest form this can
take — someone changing it has to delete a message telling them not to.

**`## Run-close sweep — #190/#191` → two of its three open declarations settled.**
- The `502`-for-a-local-data-fault mapping is **confirmed as a deliberate, documented non-decision**:
  `cross_registry_resolution_failed` is still the mapping (`error.rs:199`) and the 400-vs-502 reasoning
  is written beside it (`:155-175`). **Settled by:** a wire-contract change. **Owner:** the spec holder.
- The `private`-vs-`no-store` scope boundary is **RESOLVED**: it was split out to #205 by design, and
  #205 decided it — `private`, with the reasoning at `acdp-registry-core/src/lib.rs:107`
  ("`private` rather than `no-store`: the threat is shared caches"). Split-out items are exactly the
  ones that rot, because the split reads like a resolution.

**`## W3-U5 — the quickstart did not boot` → one half superseded, one half still open, and the
distinction is precise.**
- **SUPERSEDED by #270:** the claim that `docker.yml` sets no `jwt_secret` so "CI never exercised the
  stack the repo ships" is no longer true. `docker.yml` now runs *the documented quickstart boots*,
  *…with auth enabled*, and a self-test of the boot guard, with 4 `jwt_secret` references and
  `docker/assert-quickstart-boots.sh` shipped.
- **Still OPEN, and narrowed:** `validate_config` (`main.rs:123`) catches only an **empty** EdDSA PEM;
  it never parses one, so a **malformed** PEM still fails late — exactly as the entry said. Empty is
  not malformed, and confirming this from the presence of an EdDSA branch alone would have been wrong.
  **Settled by:** parsing the PEM in `validate_config`.

**`## #205 — Cache-Control posture` → one of four RESOLVED, three still open with owners.**
- **RESOLVED — `/metrics`.** #218 is **CLOSED/COMPLETED**, and this entry predicted its own resolution
  signal: *"the exemption is load-bearing in the test suite … deleting that line is how the fix
  announces itself."* The line is gone. `NON_DATA_ROUTES` now carries `/metrics` as *"`no-store`,
  overriding, on both the 200 and the 401 arm … Closed #218."* An entry that specifies how its own
  closure will be detectable is the best-designed thing in this file.
- **Still open, each re-checked rather than restated:** the CDN threat (unfalsifiable from this repo —
  **owner:** operators); `private` carries no validators (**verified the fix has not landed: no `ETag`
  anywhere in `acdp-registry-core`**); and `/log/checkpoint`'s inherited `private` (`if_not_present` at
  `lib.rs:121`, and `handlers/log.rs` sets no cache header of its own).

### Batches 8-9 — the last eleven items, and the two the equality caught

**`## H-A / P8 — A2: tenant-scoped search omits total_estimate` → RESOLVED, and this entry told us
how to check.** It said A2 *"must not be described as closed until the store-side predicate lands and
that test is deliberately deleted."* Both happened in **one commit**: `f8a866d` (#259, *"scan inside
the tenant so the cursor cannot anchor on a foreign row"*) landed the tenant-aware store search **and**
deleted `search_cursor_oracle_remains_open_for_tenant_scoped_caller`. Established with
`git log -S` on the test name rather than from a changelog. The `RETRACTION` entry's matching "A2
remains PARTIAL" line is resolved with it.

- **A stale comment this unit cannot fix, reported per U-505's precedent.**
  `crates/acdp-registry-core/src/handlers/context.rs:1262-1277` still says
  `search_cursor_oracle_remains_open_for_tenant_scoped_caller` *"asserts the residue so that is
  machine-checked rather than remembered"* — that test no longer exists — and still describes
  *"pushing the tenant predicate into the store's search SQL"* as what closing it **requires**, which
  `f8a866d` did. `crates/**` is outside U-507's grant, so this is reported, not repaired.

**`docker-compose.yml`'s two paired entries, which resolve in opposite directions.** `:2778` said the
recipe offers **no** environment path to enable auth; `:2867` said the recipe **now passes**
`ACDP_REGISTRY_AUTH__ENABLED`. Both cannot be current, and the second is: **`docker/docker-compose.yml:90`**
forwards it. So `:2778` is SUPERSEDED and `:2867` is CONFIRMED — the hazard it raises is real and is
now documented where an upgrader meets it (`docs/UPGRADING.md:110-124`).

- **Method note, because this nearly became a false finding against another lane's work.** A grep of
  `docker-compose.yml` returned nothing and read as "not forwarded" — the file is
  `docker/docker-compose.yml`, not at the repo root. The pattern was right and the path was wrong.

**`### The caps/config RETRACTION`'s latent → CONFIRMED unreachable, with its trigger kept.** The
assumption was that dropping the `CtxId::parse` guard is safe, reachable only via *"a migration or
import path that writes `contexts` rows without minting through `CtxId`"*. No such path exists: there
is no import or bulk-insert path, and the migrations that appear to insert into `contexts` insert into
**`contexts_fts`**, the FTS shadow table (`002_fts5.sql:15,33`, `013_fts5_porter.sql:44`). A grep for
`INSERT INTO contexts` matches `contexts_fts` as a **prefix** — that near-miss is why this was read
rather than counted.

**Four left open on their own terms, each with an owner:** the `challenge_per_agent` label rename
(unfalsifiable here — **operators**); `/contexts/search`'s untenanted default and `total_estimate`'s
return, both wanting an explicit ruling rather than closure by inertia, which one of them forbids in
its own text (**the leader**); and U-501's cost judgement, which is unmeasurable by construction so
evidence cannot close it (**the reviewer**). `H-A / P5`'s concurrency bound and #242 gap are
**CONFIRMED as deliberate trades**, since the entry classifies them that way itself and both still hold.

**The two the equality caught.** AC1's balance is not decoration: it surfaced two declarations this
unit had walked past — `:928` (the all-expired refusal, a **second** declaration inside an item whose
first one was already resolved) and the `CtxId` latent above. A floor (`>= 20 resolved`) would have
passed with both still open. `:928` stays **UNCONFIRMED deliberately**: the flag was raised in the done
report as the entry requires, no `playground.refuse_on_no_live_pin` key exists in the tree, and closing
it because nobody objected for three days is exactly the inertia another entry in this file forbids.
**Owner:** the leader.

## U-533 — driving required-but-unexercised from 6 to 1 (2026-09-13, lane-2)

Plan: plans/u-533-unexercised-fixtures.md

1. **Assumed:** the five retirable fixtures should be made *replayable* by widening the replayer's
   shape dispatch, since that is what U-527/U-528 did for the previous batches.
   - **CHANGED.** Measured per fixture: `pub-006`/`pub-009` carry 96-char signatures where ed25519
     needs 88, so widening Shape A to accept a 403 would have produced two green replays asserting
     nothing about `key_not_authorized` — the `pub-008` defect, recreated inside the unit meant to
     remove it. `pub-010` has no inline body. `pub-003`/`ret-002` need seeding shapes the seeder does
     not model.
   - **Chose:** direct tests that assert the *rule* with material that reaches it, and that assert
     each fixture's own blocker so the reasoning expires loudly if a spec bump changes it.
   - **Status:** CHANGED (2026-09-13).

2. **Assumed:** `pub-010`'s 201 makes it a second U-526 blocker, so the target is 6 -> 2.
   - **CHANGED, before escalating.** The `anc-001`/`idem-001` precedent already in this file handles a
     201-vs-200 divergence: assert the corrected status, assert the fixture's own literal separately,
     record it, neither fake nor fix. `pub-010`'s subject is `contributors[]`, so the status is
     incidental.
   - **Chose:** retire `pub-010` under that precedent; leave `pub-007`, whose subject *is* the response
     shape, with its row and an expanded reason.
   - **Status:** CHANGED (2026-09-13) — a false alarm caught by reading the file before reporting.

3. **Assumed:** `ret-002` cannot be retired without a production seam, because one scenario needs an
   all-superseded lineage.
   - **DEFERRED in part.** `expired` is producible via `expires_at`, so 2 of 3 scenarios drive. The
     third is unreachable over HTTP, and the fixture's own note says so.
   - **Chose:** drive two, and assert that the fixture still calls the third abnormal, rather than
     fabricating the state through a store insert that no client could reach.
   - **Status:** DEFERRED (2026-09-13) — evidence that would settle it: an admin path that can produce
     the state, or a spec bump dropping the note. Either reddens the assertion.

4. **Assumed:** retiring a fixture from `UNEXERCISED_FIXTURES` and updating the count is sufficient,
   since that is what the accounting test's doc comment describes.
   - **CHANGED.** That doc comment says "and nothing else", and nothing verified the retirement.
     Deleting the rows and changing the count goes green unaided.
   - **Chose:** `EXERCISED_FIXTURES`, compile-bound and runtime-checked. **And on testing it, found it
     closes only half the hole** — a retirement registering nothing is still green. Documented with
     the derived-partition follow-up named; a conservation law was rejected because it would redden on
     a legitimate spec bump.
   - **Status:** CHANGED (2026-09-13), with a named residual gap rather than an implied total fix.

5. **Assumed:** `../acdp-spec-pinned` is a safe source for fixture counts, its name implying it sits
   at the pin.
   - **CHANGED.** It is at `d1f06d0` with **143** fixtures; the pin has **144**; a nested clone has
     **145**. Neither checkout is the pin and the drift runs in both directions.
   - **Chose:** `git archive <pin> | tar -x` into scratch for every measurement, re-verified
     byte-identical after a mid-unit warning that the checkouts had moved.
   - **Status:** CHANGED (2026-09-13) — AC6 is load-bearing, not ceremony.

## U-542 — own the directory, not the file: the sqlite sidecar leak

Plan: `plans/u-542-sqlite-sidecar-leak.md`

### U-542-A1 — a scoped delta equality replaces the assign's `$TMPDIR`-wide count

**Assumed:** asserting the no-leak property on *one harness's own paths* is at least as strong as
counting every `acdp-*` file in `$TMPDIR` before and after a run, and is materially more reliable.

**Chose:** `tests/tmpdir_hygiene.rs` builds a `StoreMode::File` harness, records `db_path()`, drops
it, and asserts `== 0` of `{db}`, `{db}-wal`, `{db}-shm` still exist.

**Alternatives:** the assign's process-wide before/after count. Rejected because three lanes plus a
leader share one `$TMPDIR` on this machine, so a process-wide count is perturbed by unrelated
processes — it would fail randomly, which is how a guard gets ignored. It also cannot attribute a
delta to the test that caused it, and scanning 312k entries costs real time per call.

**Blast radius:** test-only, one file, reversible in a commit.

**Status:** CONFIRMED (2026-09-14) — the property was demonstrated in both directions on the same
binary: RED pre-fix naming `acdp-test-lI6bJ5.sqlite-{wal,shm}`, GREEN post-fix. The assign's
criterion was *additionally* satisfied as a measurement rather than discarded: 260 tests produced
`DELTA=0` against a pre-fix control of 1 test producing `+2`.

### U-542-A2 — the three self-cleaning sites are converted anyway

**Assumed:** `http_integration.rs:2690/2792/2859` do not currently leak — each calls
`store.pool().close()`, and a clean SQLite close checkpoints the WAL and removes both sidecars.

**Evidence:** 0 files in `$TMPDIR` for `acdp-livez-`, `acdp-degraded-`, `acdp-degraded-version-`,
*and* all three are plain `#[tokio::test]` with no `#[cfg]`/`#[ignore]` — so zero means "ran and
cleaned up", not "never ran". That distinction is the whole argument and was checked.

**Chose:** convert them to `TempDir` regardless.

**Alternatives:** leave them, documented as safe. Rejected: their safety is incidental to a
`pool().close()` call a later edit could remove, and nothing would catch that. Owning the directory
makes the property structural instead of a habit.

**Blast radius:** three small test edits.

**Status:** CONFIRMED (2026-09-14) — all three still pass.

### U-542-A3 — `acdp-reload-` is not a leak and is left alone

**Assumed:** `http_integration.rs:3682` writes a `.toml` config, not a database, so SQLite sidecars
are impossible.

**Evidence:** 0 `acdp-reload-*` files in `$TMPDIR`; the site's suffix is `.toml`; it is the only
surviving `.tempfile()` in the granted tree after this unit.

**Status:** CONFIRMED (2026-09-14).

### U-545-A1 — the leak class is `{server, sqlite, core}`, and the starting set was wrong in both directions

**Plan:** plans/u-545-hygiene-guard-spans-the-class.md

**Assumed:** the class of crates that open SQLite on a `tempfile`-guarded *file* path is exactly
`acdp-registry-server` (fixed by #309), `acdp-registry-sqlite` (lane-1, U-544) and
`acdp-registry-core` (fixed here).

**Chose:** re-deriving the class from the filesystem rather than accepting the handed-over list.

**Evidence, and it corrects the starting set in BOTH directions:**

- **Omitted:** `acdp-registry-core` carried a *live* leak at `src/witness.rs`'s `store_with_size`,
  inside a `#[cfg(test)]` module in `src/`. No `tests/` binary can observe that module — it is a
  different compilation target — and the crate had no `tests/` directory at all, so it appeared on
  nobody's candidate list.
- **Included wrongly:** `acdp-registry-pg` has zero `tempfile` references and is Postgres-backed.
  It is not in the class.

**Residual file-guard sites, both ARGUED as not leaking** (grep over all of `crates/`, run after a
positive control proved the pattern fires — the first attempt returned a false "none found" because
zsh glob-expanded an unquoted `--include=*.rs` and the grep never ran):

- `crates/acdp-registry-core/src/receipt.rs:194` — writes a base64 signing-key seed. Not a database;
  nothing creates siblings beside it. `NamedTempFile` is the correct tool here.
- `crates/acdp-registry-server/tests/http_integration.rs:3687` (`write_temp_config`) — writes a
  `.toml`. Same argument, and already CONFIRMED as U-542-A3.

**Blast radius:** low. A crate wrongly excluded keeps leaking; the scan in this unit is what makes
that detectable rather than a matter of who remembered which crate.

**Status:** CONFIRMED (2026-09-14) — derived from the tree, not from the list.

### U-545-A2 — the guard must key on the CALL SHAPE, never on a filename prefix

**Plan:** plans/u-545-hygiene-guard-spans-the-class.md

**Assumed:** a guard keyed on `acdp-*` filenames inherits the exact blind spot that let this class
survive #309, so the scan must look at what the code *does*.

**Evidence — this is a correction to my own #309 scope, not a hypothetical.** The `acdp-*` census
that sized U-542 could not see these sites *by construction*: they call `NamedTempFile::new()` with
**no prefix**, so the files land as `.tmpXXXXXX`. Measured: **17,538 `-wal` + 17,538 `-shm` =
35,076 files** invisible to that filter, against 174,003 `*-wal` in total. So #309 covered one crate
**and one prefix**.

Reproduced live during falsification: the deliberately-defective core fixture leaked
`.tmpFXt9XC-wal` and `.tmpFXt9XC-shm` — the no-prefix form, caught by name in the failure output.

**A tool default is a filter you never typed.** Re-reading my own pipeline could not reveal this,
because the omission was not in anything I wrote. The same shape nearly refuted the number from the
other side: an `ls -1` reading of the directory omitted dotfiles and returned a clean-looking 0.
Arithmetic settled it — 156,465 + 17,538 = 174,003, exactly.

**Consequence:** the scan matches a file-guard binding followed within 10 lines by `::connect(` on
that binding's `.path()`. No prefix appears anywhere in it. A no-prefix call is the *default* shape,
so it is the most likely form the next instance takes.

**Status:** CONFIRMED (2026-09-14).

### U-545-A3 — two mechanisms, because neither closes the class alone

**Plan:** plans/u-545-hygiene-guard-spans-the-class.md

**Assumed:** a workspace-wide source scan and a runtime guard answer different questions, and
shipping only one leaves a real hole.

**Chose:** both. The scan proves *every site takes the shape*; the runtime guard proves *the shape is
correct*. A runtime test can only exercise sites it calls, and an integration test cannot reach a
`#[cfg(test)]` module inside `src/` at all — which is precisely where core's leak lived. Conversely
a source scan fails on a pattern being *present* and can never fail on correct-but-absent behaviour.

**Both falsified independently, and that mattered:**

- Scan RED on the real defect reintroduced in core → `rc=101`, naming `witness.rs:400`.
- Runtime guard RED on a defective fixture → `rc=101`, leaking exactly **2** files, `-wal` and
  `-shm`. Two, not three: the guard cleaned the parent and orphaned both siblings — the
  14-parents-vs-286,894-sidecars signature reproduced on demand.

**Why separately:** the scan's red was **zero evidence** about the core guard, which at that moment
did not compile (`store.migrate()` is a trait method; `ExtendedRegistryStore` was not in scope). A
targeted `cargo test -p <pkg> --test <name>` builds only that package's target. I had described both
guards as working on the strength of one red. See U-545-A4.

**Status:** CONFIRMED (2026-09-14).

### U-545-A4 — what the guards do NOT catch, recorded so the next reader does not over-trust them

**Plan:** plans/u-545-hygiene-guard-spans-the-class.md

**Assumed:** a guard whose limits are undocumented will be read as covering the whole class, and
"this used to be broken and is now proven fixed" is exactly the sentence that stops people looking.

**The four blind spots, carried in the scan's own doc comment:**

1. **A guard that travels.** The pattern is a file guard bound and connected within ten lines. A
   `NamedTempFile` returned from a helper, stored in a struct, or passed across a function boundary
   is invisible. `write_temp_config` is a live example of the shape (benign here).
2. **Other sidecar-writing libraries.** It knows SQLite. Any library that writes siblings beside a
   path it is handed has the identical defect and is unchecked.
3. **Non-Rust callers, and anything outside `crates/`.**
4. **It is a source scan.** It fails on the pattern being present, never on a correct-but-absent
   test. Deleting a fixture outright leaves it green.

Guard-the-guard assertions are in both files so a broken walk fails loudly instead of passing
vacuously: the scan asserts `crates/` resolves to a directory and that the walk found > 50 `.rs`
files; the runtime guard asserts the database **and both sidecars exist while the store is open**,
before any claim is made about their removal.

**Status:** CONFIRMED (2026-09-14).

### U-554-A1 — the release PR breaks CI because three rules contradict, not because of version pinning

**Plan:** plans/u-554-release-pr-breaks-three-checks.md

**Assumed:** `tests`, `coverage` and `conformance (spec fixtures)` all fail on #286 for a single
reason, and that reason is structural rather than a stale pin.

**Evidence, read from each job's own log separately rather than inferred from matching symptoms:**
all three report `13 passed; 1 failed`, and the one failure is `root_changelog_stays_a_pointer`
(`conformance_gate.rs:852`) in every case. `coverage` has no threshold failure — llvm-cov died on the
test run before reaching that step. Each job emits exactly one `##[error]`.

**The hypothesis I was given, and refuted three ways:** hard-coded version strings such as
`--package=acdp-registry-sqlite@0.1.3`. (1) No `@0.1.x` or `=0.1.x` package spec exists anywhere in
`.github/workflows/**` or `release-plz.toml`. (2) The `mutants.yml` line cited is
`--package=acdp-registry-core` **inside a comment**, carrying no version at all — the `@0.1.3` came
from a log line, not from source. (3) No failing check involves a version-pinned command.

**The actual cause — three rules, any two compatible, all three not:**
1. Every crate is `version = { workspace = true }`, so a release bumps all 8.
2. release-plz writes a changelog section only for crates with commits in their own directory —
   confirmed from its documentation, and **no configuration option exists to change this**
   (`changelog_update` only toggles updating; `changelog_include` pulls *other* packages' commits in,
   which is the wrong semantics and would fill one crate's notes with another's).
3. `root_changelog_stays_a_pointer` requires every crate to carry the workspace version's heading.

The three failing crates are **exactly** the three with zero commits since `v0.1.3`.

**Status:** CONFIRMED (2026-09-14) — reproduced locally at the PR head: `rc=101`, same message, same
three crates.

### U-554-A2 — it recurs on every release with an unchanged crate, and that is measured

**Plan:** plans/u-554-release-pr-breaks-three-checks.md

**Assumed:** this is a release-process defect, not a one-off.

**Evidence — per-crate commit counts, anchored on real tags.** Note `git tag -l 'v*'` matches
**nothing** here: `git_tag_name = "{{ package }}/v{{ version }}"`, so tags are `<package>/v<version>`.
Anchoring on a bare `v*` glob produces a degenerate range and confident zeros for every crate.

| window | counts | result |
|---|---|---|
| `v0.1.2..v0.1.3` | all 8 crates ≥ 2 (auth 3, webhook 2, types 3, …) | every crate got a section; guard passed |
| `v0.1.3..ce1ce77` | auth **0**, pg **0**, webhook **0**; core 5, sqlite 4, server 20 | three gaps; guard fails |

Cross-checked two ways: anchored on `acdp-registry-server/v0.1.x` (36 commits in the previous
window, so non-degenerate) **and** on each crate's own tag. All eight `v0.1.3` tags point at the same
commit `f8b6d9e`, so the two methods are equivalent and they agree.

**Conclusion:** the guard has never been exercised against "a crate did not change". It fires on every
release where at least one crate is unchanged, and that becomes more likely as the workspace grows.

**Status:** CONFIRMED (2026-09-14).

### U-554-A3 — document the non-change rather than weaken the guard

**Plan:** plans/u-554-release-pr-breaks-three-checks.md

**Assumed:** the right fix adds information rather than removing a check.

**Chose:** a step in `.github/workflows/release-plz.yml` that writes an explicit "no changes" section
for any crate the release bumped but did not change. **Rejected:** relaxing
`root_changelog_stays_a_pointer` to skip unchanged crates.

**Why.** Under the rejected option a consumer upgrading `acdp-registry-auth` 0.1.3 → 0.1.4 finds **no
record at all** and cannot distinguish "nothing changed" from "someone forgot to write it up". The
chosen option answers the question the version bump raises. It also makes the guard's stated
invariant *true* rather than narrowing it — and that file was hardened in this exact area by U-538
("A FLOOR CANNOT CATCH UNDERCOUNTING"), so softening it now would undo a fix for the very case the
hardening anticipated.

**Status:** CONFIRMED (2026-09-14) — approved in principle by the leader, both its constraints met.

### U-554-A4 — release-plz abandons its branch rather than force-pushing, so the fix must land on main first

**Plan:** plans/u-554-release-pr-breaks-three-checks.md

**Assumed:** a hand-edit on the release PR branch would be destroyed by the next release-plz run.

**Evidence from this repo's own history, not from documentation:** release-plz opens a **new
timestamped branch and a new PR** each run and abandons the previous one.

| PR | branch | version | state |
|---|---|---|---|
| #213 | `release-plz-2026-09-11T03-47-15Z` | v0.1.1 | MERGED |
| #225 | `release-plz-2026-09-11T20-33-01Z` | v0.1.2 | MERGED |
| #230 | `release-plz-2026-09-12T01-54-09Z` | v0.1.3 | MERGED |
| **#278** | `release-plz-2026-09-13T16-15-07Z` | **v0.1.4** | **CLOSED — superseded** |
| **#286** | `release-plz-2026-09-13T17-44-55Z` | **v0.1.4** | OPEN |

**#278 and #286 are two PRs for the same version**, the first closed in favour of the second. That is
an observed regeneration event, not a hypothetical one.

**Consequence:** editing the three changelogs on `1854b2f` would strand the fix on a branch that gets
abandoned, and the replacement PR would be red again with nobody watching. The fix therefore lands on
`main` first and release-plz regenerates the release PR with the sections already present.

**Status:** CONFIRMED (2026-09-14).

---

## U-552 Phase 1 — the survivor classifier: every classification still FAILS the job
- **Plan:** `plans/u-552-widen-mutation-scope.md`
- **Assumed:** a disappeared survivor line should never turn the ratchet green, not even
  when the classifier can PROVE the mutant was killed.
- **Chose:** every branch exits non-zero and names the edit to make. `MUTANTS_SURVIVORS` is
  a SET EQUALITY; if a proven kill passed green, the committed list would sit disagreeing
  with `missed.txt` and nothing would force it back into agreement — re-opening the hole
  U-551's equality closed. The classifier's job is to say WHICH edit to make, not to excuse
  the reader from making it.
- **Alternatives:** exit 0 on a proven kill (rejected: silently decays the list); warn-only
  (rejected: `.cargo/mutants.toml:126-132` already records that a red check nobody can
  explain gets disabled, which is why every branch here prescribes a remedy).
- **Blast radius if wrong:** a weekly scheduled job stays red one cycle longer than needed.
  It blocks no PR — `mutants` is not in `required_status_checks.contexts`. Reversible in a
  one-line diff.
- **Status:** **CONFIRMED** (2026-09-19, Opus under `/reconcile`) — holds in code and in test. Every branch returns EXIT_CLASSIFIED (10) and the workflow's outer `rc=1` fires regardless, so a proven kill still reddens the job; the 36 unit tests assert the non-deletion branches explicitly (`assertNotIn("DELETE the line", out)`).

## U-552 Phase 1 — pairing drifted lines needs TWO identity keys, required to agree
- **Plan:** `plans/u-552-widen-mutation-scope.md`
- **Assumed:** one line-independent identity is not enough to re-find a mutant that moved.
- **Chose:** compute both an offset key (`line − function_start_line`, plus column) and an
  ordinal key (position within the function by line, then column). Pair when both agree; pair
  on whichever key resolves when only ONE does, saying which; escalate only when both resolve
  to *different* mutants, or when either matches several.
- **CORRECTED DURING THIS PHASE.** The first implementation required BOTH keys to resolve,
  and this entry claimed "they fail on different edits, so requiring agreement costs
  nothing". That was wrong in direction, and the phase verifier demonstrated it: requiring
  agreement makes pairing the INTERSECTION of the two keys, so it succeeds only where both
  survive — i.e. only for whole-function shifts, the one case a single key already handled.
  Every edit the second key was added to cover was being dumped into the coarse fallback and
  mislabelled "the two identity keys DISAGREE" when in fact one had simply found nothing.
  Single-key pairing is now its own labelled outcome.
- **On injectivity, stated precisely.** The offset key's injectivity is a real measurement:
  213/213 on the core ledger, 138/138 on store.rs, 351/351 on the union — and dropping a
  component measurably collapses it (without `replacement`, 213 → 123; without `column`,
  213 → 204). The ordinal key's injectivity is **true by construction**, since
  `(file, function, ordinal-within-function)` is unique by definition; quoting "351/351
  measured" for it, as an earlier draft did, described a check that could not fail.
- **Alternatives:** the naive `(file, description)` key — measured to collapse 351 mutants to
  312, merging the three `delete ! in run_search_with_refill` mutants which hold three
  DIFFERENT verdicts (Caught/Missed/Timeout); re-running the isolated mutant with
  `-F '^…$'` — works under the committed config, but costs a cold build per line and
  cannot find a mutant that drifted at all (0 matches), which is the one case that matters.
- **Blast radius if wrong:** a drift is reported as ambiguous and a human reads
  `mutants.out/diff/`. The failure direction is deliberately "ask", never "guess".
- **Status:** **CONFIRMED** (2026-09-19, Opus under `/reconcile`) — and it earned the confirmation the hard way. The first fixtures were degenerate (one file, one function, one replacement), so both keys were trivially satisfiable and 9 of 10 verifier mutations survived. Rebuilt with multi-file/function/replacement fixtures, then a second layer where only ONE key can succeed, because a passing replacement test was still being rescued by the other key. Phase 3 added a third independent check: 25/25 mutations caught by 36 tests.

## U-552 Phase 1 — MUTANTS_PRIOR_LEDGER names ONE file, never a glob
- **Plan:** `plans/u-552-widen-mutation-scope.md`
- **Assumed:** the pairing basis must be a single named ledger.
- **Chose:** a `MUTANTS_PRIOR_LEDGER` env var beside `MUTANTS_EXPECTED_SCOPE`, currently
  `docs/mutation-runs/u551-core-scope-213-outcomes.json`, with `conformance_gate.rs`
  asserting it is not a glob and that the file exists. Measured: globbing
  `docs/mutation-runs/*-outcomes.json` matches 13 files / 417 records — it picks up the VOID
  `u548` ledger (taken with `copy_vcs` lost, so every "caught" in it is an artifact) and
  three SUPERSEDED `u549` shards — yielding 66 duplicate names, 11 with CONFLICTING verdicts.
  That would trip the classifier's own duplicate-name assertion on every run.
- **Alternatives:** a glob (measured broken, above); deriving the CURRENT set by parsing
  `docs/mutation-runs/README.md` prose (rejected: a prose index is not a machine-readable
  contract, and parsing it would be a second untested extractor).
- **Blast radius if wrong:** points at a stale ledger → drifts degrade to the coarse
  fallback and get reported as ambiguous. Phase 3 must repoint it at the 351-scope ledger;
  if it forgets, drift pairing silently weakens rather than failing loudly. **That is the
  sharpest residual risk in this phase** and is why the existence check is a test.
- **Status:** **CONFIRMED and STRENGTHENED** (2026-09-19, Opus under `/reconcile`) — `conformance_gate.rs` falsifies it against the REAL mutants.yml (glob substitution must produce a violation), and Phase 3 added `the_declared_prior_ledger_actually_exists`, which requires the pointer to resolve AND to be git-TRACKED. The tracked check was not pedantry: `is_file()` alone passes locally on an unstaged ledger and fails only in CI's checkout.

## U-552 Phase 1 — the classifier's exit codes are 10/11, not 1/2
- **Plan:** `plans/u-552-widen-mutation-scope.md`
- **Assumed:** a classifier that CRASHES must not be readable as one that classified cleanly.
- **Chose:** `EXIT_CLASSIFIED = 10`, `EXIT_UNSOUND = 11`. An unhandled Python exception exits
  1 and an argparse error exits 2, so those values would have made a dead script
  indistinguishable from a normal classification — the workflow would have printed the
  routine message over a script that never ran. The workflow's `case` treats every other
  non-zero code as "the classifier itself failed", and prints the raw removed lines
  BEFORE invoking it so a crash still tells the operator which lines vanished.
- **Alternatives:** 1/2 (rejected: collides with the interpreter's own codes); parsing the
  script's stdout for a sentinel (rejected: a crashed script produces no stdout, so the
  absence of a sentinel and the absence of a problem look identical).
- **Blast radius if wrong:** none beyond this workflow; the outer `rc=1` already fails the
  job regardless, so the exit code only governs which message the reader gets.
- **Status:** **CONFIRMED** (2026-09-19, Opus under `/reconcile`) — the workflow's `case` handles 10, 11 and a catch-all that says the classifier itself failed and that the listed lines have NOT been judged. Asserted by the unit tests via EXIT_CLASSIFIED/EXIT_UNSOUND.

## U-552 Phase 1 — a SHARDED report is refused rather than classified
- **Plan:** `plans/u-552-widen-mutation-scope.md`
- **Assumed:** the classifier must never run against one shard of a sharded run.
- **Chose:** pass `--expected-scope "$MUTANTS_EXPECTED_SCOPE"` and refuse when the report's
  `total_mutants` disagrees. `cargo mutants --shard N/K` writes a report whose
  `total_mutants` is that shard's share, so `len(records) == total_mutants` holds PER SHARD
  and the truncation check passes — while every committed survivor belonging to another
  shard looks like a line naming no mutant in scope, which classifies as
  `NO CANDIDATE → delete the line`, for nearly every survivor at once. The scope pin is the
  only thing that can tell a shard from a whole run.
- **Alternatives:** reading a shard field from the report (cargo-mutants does not record one);
  accepting shards and merging them in the classifier (rejected: the merge would have to be
  right before the check that validates it, and a wrong merge reintroduces duplicate names).
- **Blast radius if wrong:** if Phase 3's 351-mutant run must be sharded, this gate makes the
  ratchet hard-fail until the shards are merged into one report. That is the intended
  direction — refusing to judge beats judging wrongly — but it is a real constraint on
  Phase 3 and is recorded in the plan's Phase 3 edge cases.
- **Status:** **CONFIRMED, and its stated blast radius did NOT materialise** (2026-09-19, Opus under `/reconcile`). This entry warned that if Phase 3's run had to be sharded, the gate would hard-fail until the shards were merged. Phase 3's run was NOT sharded: one invocation produced all 351 (`total_mutants` 351, `end_time` set, 219/7/124/1 summing to 351), so the constraint never bound. Recording that the risk was real, priced, and then simply did not occur — rather than deleting the entry as if it had never been a risk.

## U-552 Phase 2 — `994:35` `<` and `==` are KILLABLE; U-544's EQUIVALENT label was wrong
- **Plan:** `plans/u-552-widen-mutation-scope.md`
- **Assumed:** a committed `EQUIVALENT` label backed by a probe can still be wrong, and the only
  way to find out is to run the mutant.
- **Chose:** wrote `a_keyed_superseding_publish_replays_instead_of_failing_as_already_superseded`
  in `crates/acdp-registry-sqlite/tests/store_contract.rs` and ran both mutants in isolation with
  `cargo mutants -F '^<escaped --list line>$'`. Both come back **CaughtMutant**, that test the
  SOLE failure (26 passed, 1 failed). U-544's probe was correct but generalised from a
  NON-superseding request: step 7's `ON CONFLICT` replay fallback (`store.rs:1284-1318`) is
  reached only AFTER step 2, so a superseding replay with step 1 skipped hits the coherence check
  at `store.rs:1118-1125` and returns `Err(SupersededTarget{AlreadySuperseded})`.
- **Alternatives:** accept the committed label (rejected — it is a claim about behaviour, and
  behaviour is measurable); argue it in review without running it (rejected — the repo's label was
  itself backed by a probe, so only a stronger measurement settles it).
- **Blast radius if wrong:** the survivor list would carry two entries that are actually killed,
  and the ratchet would fail on the next run with the classifier reporting them as proven kills.
  Self-correcting by design.
- **Status:** CONFIRMED (2026-09-16) — measured twice, the second time after correcting a
  poisoned harness (below).

## U-552 Phase 2 — local cargo-mutants verdicts were VOID until safe.directory was injected
- **Plan:** `plans/u-552-widen-mutation-scope.md`
- **Assumed:** a `CaughtMutant` verdict means the mutation was detected. **It does not, on its own.**
- **Chose:** re-ran with `GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=safe.directory GIT_CONFIG_VALUE_0='*'`
  exported for the process only — no `git config --global` write, no repo change — and confirmed
  the killing test by name in `mutants.out/log/<mutant>.log` before believing the verdict.
- **What happened:** the first run ALSO reported "2 caught", but the killers were
  `no_tracked_file_contains_a_conflict_marker`, `every_docs_page_is_listed_in_the_docs_index` and
  `every_directly_read_env_var_is_documented` — workspace scans unrelated to the mutation. This
  worktree is owned by uid 501 while the session runs as uid 502, so git refuses cargo-mutants'
  temp copy with `fatal: detected dubious ownership`; `cargo test --workspace` stops at the first
  failing binary, and `conformance_gate` sorts before `store_contract`, so **the new test never
  executed.** Right conclusion, no evidence behind it.
- **Alternatives:** `git config --global --add safe.directory '*'` (rejected — persists a
  security-relevant setting machine-wide to fix one run); running as uid 501 (not available).
- **Blast radius if wrong:** every local mutation measurement in these worktrees is untrustworthy,
  including Phase 3's. CI is unaffected (single-user runner), which makes it worse rather than
  better: the poisoned run is the one a human uses to decide what to commit. This is exactly the
  class `mutants.yml`'s AC8 harness check exists to detect, and it reproduced live.
- **Status:** CONFIRMED (2026-09-16)

## U-552 Phase 2 — the four uncalled trait methods ARE given tests (judgement REVERSED)
- **Plan:** `plans/u-552-widen-mutation-scope.md`
- **Assumed, first:** "a test can be written that goes red" is not the same as "this is a coverage
  gap worth closing", so `568:9 put`, `821:9 mark_superseded`, `832:9 first_version_ctx_id` and
  `923:9 idempotency_evict_expired` should be carried as budgeted survivors under a new label,
  `UNREACHED-BY-PRODUCTION`, rather than killed. The worry was real: a test whose only purpose is
  to call an otherwise-uncalled method converts a true finding — *this surface has no production
  caller* — into a permanently green line that hides it.
- **REVERSED, and the phase verifier is why.** It pointed at a counter-precedent two rows above in
  the same table: `store.rs:380:9 lifecycle_events_of_ctx -> Ok(vec![])` is recorded **KILLED
  (U-543)** by a direct-call test, and that method has no originating production caller either —
  three impls, one delegating wrapper, plus U-543's own two calls. Carrying these four while
  counting that one a win would have left the document asserting both positions at once.
- **Chose:** kill all four with direct contract tests in
  `crates/acdp-registry-sqlite/tests/store_contract.rs`. Three reasons, in order of weight:
  (1) `.cargo/mutants.toml`'s governing rule is "PAY FIRST, WIDEN LAST" — a survivor a test CAN
  kill gets the test; (2) the repo's own precedent on the identical shape; (3) these are genuine
  backend-CONTRACT surface, not dead code — `acdp-registry-pg/src/store.rs` implements all four and
  `parity.rs` exists to compare backends, which is the phase rubric's own "contract surface both
  backends implement → KILL" branch.
- **Measured:** all four come back **CaughtMutant**, each killed by its own named test as the SOLE
  failure (30 passed, 1 failed). The killer was confirmed by name in each
  `mutants.out/log/<mutant>.log`, not inferred from the verdict.
- **The finding is not lost**, which was the whole objection: the call-site census stays in
  `docs/MUTATION-SCOPE-CANDIDATES.md`'s U-546 section and in a block comment above the four tests
  saying why they exist and why the first judgement was reversed.
- **Blast radius if wrong:** four contract tests pinning behaviour that currently has no production
  caller. Cheap either way, and a future originating caller now inherits a tested contract.
- **Status:** CONFIRMED (2026-09-16)
## U-557 — local `cargo-deny` binary differs from the one CI gates on
- **Plan:** plans/u-557-clear-yanked-crates.md
- **Assumed:** cargo-deny 0.19.9 (this worktree) and 0.20.2 (the pinned
  `EmbarkStudios/cargo-deny-action` image at `3c63498`) agree on the `yanked` check and on
  the summary-line wording, so a local green predicts a CI green.
- **Chose:** proved criterion 3 locally with 0.19.9 and recorded the version beside the
  output, rather than installing 0.20.2 to match. Reproducing the gate's *command* (and its
  argument ordering, `--workspace` before `check`) is what the criterion asks for; matching
  its *binary* is a stronger claim than the criterion makes, and CI is the authority that
  actually blocks.
- **Alternatives:** install cargo-deny 0.20.2 locally (slow, and still not the action's
  container); skip the local run entirely and rely on CI (gives up the fast falsification
  that caught the non-discriminating criterion in the first place).
- **Blast radius if wrong:** CI's cargo-deny goes red on a PR that was green locally. Now
  that `cargo-deny` is a required context this blocks the merge — visible immediately, fixed
  by reading CI's output. No silent failure mode; cost is one round trip.
- **Status:** RESOLVED (U-559, 2026-09-16) — **the skew was immaterial.** CI's cargo-deny
  0.20.2 agreed with the local 0.19.9: the `cargo-deny` job passed in 26s on PR #322
  (merged `181df1f`), under `yanked = "deny"`, and `main`'s own post-merge run passed it
  too. The local green did predict the CI green. Recorded rather than deleted because the
  reasoning — that reproducing a gate's *command* is not reproducing its *binary* — stays
  true and will apply to the next tool-version gap.

## U-557 — the Postgres test step was not run locally
- **Plan:** plans/u-557-clear-yanked-crates.md
- **Assumed:** the lockfile bump does not break the Postgres-backed tests, which are step 3
  of CI's required `tests` context (`ci.yml:362-370`). That job (`ci.yml:332-480`) has **7**
  named steps and 7 `cargo test` command lines (`:357, :360, :367, :368, :389, :434, :442`) —
  not the 5 an earlier draft of this entry claimed, which counted only steps whose *name*
  begins "cargo test".
- **Chose:** ran CI's step 1 (`cargo test --locked --workspace`, 708 passed / 0 failed /
  0 ignored) and left steps 2-5 to CI. **This step is skipped, not covered** — no Postgres is
  reachable from this worktree (port 5432 closed, no client installed), so there is no local
  evidence either way. Stating it as skipped rather than folding it into "the suite is green".
- **Alternatives:** stand up a local Postgres via Docker to run it here. Rejected as
  disproportionate: neither bumped crate is reached *through* `sqlx-postgres` (its dependency
  list contains neither `flume` nor `spin`; `wnaf` is on the P-256 signature path). Note this
  is narrower than "not linked into that binary": `spin` has a **second** parent, `lazy_static`
  -&gt; `tracing-subscriber`, so it IS compiled into the Postgres test binary. The argument rests
  on the 708-test sqlite run exercising `spin` heavily, not on its absence. Also
  CI runs the step on every PR with a service container, blocking.
- **Blast radius if wrong:** a Postgres-specific regression reaches CI instead of being
  caught locally. CI blocks it. Cost is one round trip, not a bad merge.
- **Status:** RESOLVED (U-559, 2026-09-16) — **CI ran the step and it passed.** The `tests`
  context on PR #322 (merged `181df1f`) passed in 3m12s with the `postgres:16-alpine`
  service container, covering the Postgres step this worktree could not run. No
  Postgres-specific regression existed. (This entry's reasoning about `spin` was already
  corrected in place by U-557 — see the "Alternatives" bullet above, which records that
  `spin` IS compiled into that binary via a second parent. Nothing further to add here.)

## U-556 — TLS startup handshake probe

- **Plan:** `plans/u556-tls-startup-handshake.md`
- **Assumed:** `GET /livez` is the right endpoint to prove an HTTPS exchange — it takes no
  `State` (`crates/acdp-registry-core/src/handlers/meta.rs:169-175`), so it returns 200 on a
  cold, empty store under both `storage-sqlite` and `storage-memory`, and no middleware
  authenticates it.
- **Chose:** `/livez`, asserting `status == "ok"` rather than the version string (which carries
  a build sha and moved 0.1.3 → 0.1.4 mid-unit, vindicating the choice).
- **Alternatives:** `/healthz` — rejected, it has a 503 arm and is strictly more fragile on a
  cold store. `/` — does not exist; bare 404 fallback.
- **Blast radius if wrong:** the test would fail loudly on a green build; no production impact.
- **Status:** CONFIRMED (2026-09-16, Opus under `/reconcile`). Independent analysis found the
  endpoints' own behaviour already covered in-process over plain HTTP (`http_integration.rs:220-257`,
  `:2680`; `conformance_gate.rs:305,330`), so a richer endpoint buys no coverage and imports a
  storage-init failure mode into a test whose red must mean "TLS broke". The rejection reason is now
  recorded at the `PROBE_PATH` constant, where a maintainer tempted to "strengthen" it will meet it.

## U-556 — cipher suite deliberately not asserted

- **Plan:** `plans/u556-tls-startup-handshake.md`
- **Assumed:** the negotiated suite (`TLS13_AES_256_GCM_SHA384`) is a rustls default, not a
  property this repo chose.
- **Chose:** capture it in `TlsProbe` and print it in failure messages, but do not assert it.
  The negotiated protocol *version* is asserted, because that is the property U-556 is about.
- **Alternatives:** asserting the suite — rejected: it would turn an upstream default change
  into a red build whose message points at this repo.
- **Blast radius if wrong:** a suite downgrade within TLS 1.3 would go unnoticed by this test.
- **Status:** CONFIRMED (2026-09-16, Opus under `/reconcile`), on a stronger argument than the one
  logged. An AEAD *floor* was considered and rejected as **unfireable**: the TLS 1.3 suite registry
  is AEAD-only by construction, and the client is pinned to 1.3, so such a check cannot fail on any
  run where the version assertion passes — it would read as coverage and prove nothing. Noted at the
  `suite` field so nobody adds it later believing it buys something.

## U-556 — ALPN assertion added beyond the written plan

- **Plan:** `plans/u556-tls-startup-handshake.md`
- **Assumed:** asserting the negotiated ALPN is worth a line, since the server advertises
  `["h2","http/1.1"]` and a client offering the wrong thing would make the HTTP/1.1 request
  line meaningless.
- **Chose:** assert `alpn == Some("http/1.1")`. This was **drift** — the plan's phase 1 said
  "assert on version / status / body" and named no ALPN criterion. It was caught by the phase
  verifier, not self-flagged, and it shipped only after being falsified (client offering only
  `h2` → `left: Some("h2") right: Some("http/1.1")`).
- **Alternatives:** dropping it — rejected once falsification proved it discriminates rather
  than being decorative.
- **Blast radius if wrong:** a spurious red if the server's ALPN list ever legitimately changes.
- **Status:** CONFIRMED — KEEP (2026-09-16, Opus under `/reconcile`). The "near-unfireable"
  objection is right on mechanics and wrong on conclusion: the reachable `None` branch is what
  happens if anyone replaces `axum-server`'s acceptor with a hand-rolled `rustls::ServerConfig` — a
  plausible migration here given this repo's documented provider fight with that crate — and this is
  the **only** observation of the server's ALPN anywhere in the repo. Its failure mode was deletion,
  not a false pass, so the assertion message now argues for its own existence and states its blind
  spot (a list narrowing from [h2, http/1.1] to [http/1.1] loses h2 and still passes).

## U-556 — child stdout is not drained during the probe

> **Superseded — see "U-560 — resolution of U-556's 'child stdout is not drained' entry" below.**
> The reasoning in this entry held up; two figures in it did not, and they were corrected by two
> different units. The `~16 KiB` pipe buffer was already corrected **by this entry's own
> `Status:` block below**, under U-556's `/reconcile` — U-560 did not re-verify it and must not be
> read as its source. What U-560 corrects is the separate *"truncated 64 KiB"* wording, which
> misdescribes how a pipe fails. This entry is left as written because the file is cumulative.

- **Plan:** `plans/u556-tls-startup-handshake.md`
- **Assumed:** exactly one HTTP request is ever issued, so `TraceLayer`'s two log events
  (`crates/acdp-registry-core/src/lib.rs:300-302`) cannot fill the child's ~16 KiB pipe buffer.
- **Chose:** do not drain. **This holds only because `Exchange`-stage probe failures are
  non-retryable** — retrying the whole exchange would re-issue a real request every 100ms for
  up to 50 iterations. If that retry policy is ever relaxed, this decision must be re-made with
  it; the two are one decision, not two. **Measured, since a decision resting on an unverified
  property is the thing this log exists to prevent:** forcing an `Exchange`-stage failure fails
  in 0.93s versus 5.32s for a retryable handshake failure that burns all 50 iterations. The gap
  is the evidence that `Exchange` is not retried.
- **Alternatives:** draining stdout on a reader thread — rejected as unnecessary complexity for
  one request.
- **Blast radius if wrong:** the child could block on a full pipe and the test would hang until
  the 5s socket timeout, then report `[exchange]`.
- **Status:** **RESOLVED** (2026-09-19, U-561) — the follow-up landed; see the U-561 section at
  the end of this file for the evidence. Superseded text, kept because this file is cumulative:
  *"**NEEDS-CHANGE** (2026-09-16, Opus under `/reconcile`) — does **not** block shipping
  U-556; filed as a follow-up."* The analysis refuted this entry's own premise on two counts.
  (a) The ~16 KiB figure is the *initial* pipe allocation; macOS grows it to **64 KiB**, measured at
  65,531/65,536 B. At the default log filter 50 retried requests emit ~40 KB and the test **still
  passes** — so the hazard documented here does not fire on the variable it blames.
  (b) The variable that does fire is **`RUST_LOG`**, which the child inherits (the test sets only
  `ACDP_REGISTRY_CONFIG`): one probe at `trace` already uses ~20 KB, and 50 would wedge.
  The right fix is structural and severs the coupling rather than documenting it — give the child
  file-backed stdio in the existing tempdir instead of pipes: a file never blocks, has no ceiling,
  and is immune to request count, retry policy and log level at once, while *improving* diagnostics
  (today a wedged child yields a truncated 64 KiB). **It cannot be done inside U-556:** it requires
  editing `tls_startup.rs:206-208`, which is inside the byte-identical protected range this unit
  pre-registered (criterion 6) and which the assign's criterion 3 calls load-bearing. Widening scope
  to take it would break a criterion this unit's PR claims. Corrected comments are in the code now;
  the structural fix is a successor unit.

## U-552 Phase 3 — a file leaving `examine_globs` is diagnosed as a SCOPE change, not a dead expression
- **Plan:** `plans/u-552-widen-mutation-scope.md`
- **Assumed:** the Phase 1 `--expected-scope` pin covered every way a committed survivor can
  stop naming a mutant. It does not, and the gap was found by running Phase 1's own classifier
  as a NEGATIVE CONTROL during Phase 3 — feeding it the six store.rs lines against the
  213-scope ledger, a report that never contained that file.
- **Chose:** classify a removed line whose file contributed **zero** mutants to the report as
  `FILE NOT IN SCOPE`, with its own action, instead of `NO CANDIDATE → delete the line`.
  **Why the existing pin cannot catch it:** `--expected-scope` compares TOTALS. Narrow
  `examine_globs` and update `MUTANTS_EXPECTED_SCOPE` in the *same* commit and
  `total == expected` still holds — the guard stays silent while a whole file stops being
  watched, and every survivor in it is reported as an expression that "no longer exists",
  prescribing exactly the deletion that locks the narrowing in. Dropping a file is the one
  way to empty `missed.txt` without writing a single test, so it is the one disappearance
  that must never read as a kill.
- **Alternatives:** parsing `examine_globs` out of `.cargo/mutants.toml` and comparing
  (rejected: re-implements glob semantics in a second place, and would disagree with the
  run that actually happened — the report is the ground truth for what was examined);
  leaving it to the human (rejected: the prior text actively argued for the wrong edit).
- **Falsified, not merely tested:** three mutations of the new branch — deleted, condition
  inverted, and `cur_files` forced empty — produced 2, 5 and 3 test failures respectively,
  against 36 green on the restored file. One pre-existing test
  (`test_the_coarse_fallback_does_not_match_across_FILES`) had a fixture in which FILE_A
  contributed no mutants at all; it was passing for a reason it did not intend, and now
  carries a FILE_A mutant of its own so it still tests the cross-file property.
- **Blast radius if wrong:** a false `FILE NOT IN SCOPE` would tell a reader to restore a glob
  that was never removed. Bounded: the branch is reached only when the report contains zero
  mutants for that file, which the second control above confirms does not fire while the file
  is still examined.
- **Status:** **CONFIRMED** (2026-09-19, Opus under `/reconcile`) — implemented, and falsified three ways (branch deleted / condition inverted / `cur_files` forced empty → 2, 5, 3 failures against 36 green). The independent Phase 3 verifier reviewed it and called it a real gap-closure.
## U-560 — the manifest scan is kept, for one property only

- **Plan:** `plans/u560-honest-test-infrastructure.md`
- **Assumed (REFUTED by measurement, 2026-09-19):** that normal-vs-dev-only is unobservable
  except from the manifest, and that the scan is what stands between this repo and a dev-only
  `rustls`. Both halves are wrong. Since U-530, `src/main.rs:100` names
  `rustls::crypto::ring::default_provider()` in the **bin**, and Cargo does not expose
  `[dev-dependencies]` to bins. Measured in a detached scratch worktree of `ff9b3fe` by moving
  the line into `[dev-dependencies]`: `cargo build --bin acdp-registry` →
  `error[E0433]: cannot find module or crate rustls --> crates/acdp-registry-server/src/main.rs:100`
  (rc=101), and `cargo test --test tls_startup` → the **same** E0433, rc=101, with **zero**
  `test result` lines. The dev-only state never reaches a test binary, so the scan's own dev-only
  message is unreachable in the one state it was written for.
- **Chose (decision unchanged, stated reason replaced):** keep the `[dependencies]` scan and its
  `axum-server` anti-vacuity control — not for normal-vs-dev-only, which the bin's own compile
  enforces harder than any test could, but for the **`ring` assertion**, which is live and which
  no compile gate can reach. Measured in the same worktree: drop `features = ["ring"]` and the
  bin still **compiles** (rc=0) — `ring` resolves anyway through
  `reqwest`/`hyper-rustls`/`tokio-rustls`/`sqlx-core` feature unification — while this test goes
  red at `tls_startup.rs:162` with its D-W5-105 message. That is the property worth ~25 lines:
  this crate must keep making its own DIRECT request for its recorded provider choice rather
  than inheriting `ring` by accident of a graph where one unrelated bump could remove it.
- **Alternatives:** delete the scan wholesale now that a runtime assertion exists — rejected on
  the measurement above: the `ring` assertion is the only check of D-W5-105 anywhere, and it is
  unreachable from any runtime observation in this process.
- **Blast radius if wrong:** low and test-only. ~25 lines, of which the dev-only half is now
  known to be defensive-only; nothing ships differently.
- **The same refuted claim was carried in two other places and is corrected in this PR:** the
  `rustls_is_a_normal_dependency` docstring and `docs/ENGINEERING-LOG.md`. Correcting only this
  entry would have left the repo's permanent log asserting the falsehood.
- **Status:** CONFIRMED (2026-09-19, Opus under `/reconcile`) — decision kept, stated reason
  replaced after measurement. See `DECISIONS.md`.

## U-560 — `custom-provider` is left unruled-out, deliberately

- **Plan:** `plans/u560-honest-test-infrastructure.md`
- **Assumed:** `ClientConfig::builder()` panicking with the process-level-CryptoProvider message
  is worth asserting even though it does not identify *which* of three feature states caused it.
- **Chose:** assert the panic and its message, narrow the stated invariant to what all three
  states share — rustls cannot select a provider unaided, so `main` must install one — and rule
  out only the "no providers" reading, via a compile-time reference to
  `rustls::crypto::ring::default_provider` (`crypto/mod.rs:25-26` gates the module on the
  feature). `custom-provider` stays unruled-out and the comment says so.
- **Measured:** `rustls-0.23.45/src/crypto/mod.rs:259-263` documents all three states returning
  `None` from `from_crate_features()`; `:249` is the `.expect` that panics. **Strengthened on
  reconcile:** `:266-282` guards *both* `Some(...)` returns with `not(feature = "custom-provider")`
  and falls through to `None` at `:284-285`, so enabling `custom-provider` returns `None`
  unconditionally. The blast-radius argument below is therefore not merely plausible but forced
  by the `cfg` guards — the assertion staying green and the operational invariant holding cannot
  come apart in that state.
- **Alternatives:** (a) assert the provider *count* — not reachable from a test crate, which
  cannot `cfg!` on another crate's features; (b) name `rustls::crypto::aws_lc_rs` to detect the
  second provider — rejected, its absence is a desirable end state so that turns a legible red
  into a hard compile error; (c) scrape `cargo tree` — rejected by the test's own docstring.
- **Blast radius if wrong:** low. Enabling `custom-provider` in this workspace would leave the
  assertion green for a reason it does not name — but that state also requires an explicit
  provider install, so the operational invariant the test protects would still hold.
- **Status:** CONFIRMED (2026-09-19, Opus under `/reconcile`) — as-is; all three citations
  verified exact. See `DECISIONS.md`.

## U-560 — phase 1's docstring cites a record phase 4 creates

- **Plan:** `plans/u560-honest-test-infrastructure.md`
- **Assumed:** a citation in tracked code must resolve in tracked files. Measured: `U-563` and
  `D-W5-105` each appeared in exactly one tracked file — the test asserting them — because
  `plans/*` is gitignored (`.gitignore:62`). A self-referential citation is not a citation.
- **Chose:** point the docstring at the U-560 entry in `docs/ENGINEERING-LOG.md` (created by
  phase 4, same PR) for the keep-both-providers decision, and at
  `crates/acdp-registry-server/Cargo.toml:41-43` for `D-W5-105`, which carries its reasoning
  inline and is tracked today.
- **Alternatives:** write the decision into `DECISIONS.md` from phase 1 — rejected, that file is
  outside phase 1's declared scope and `/reconcile` owns it; or drop the citation — rejected,
  criterion 7 exists precisely so a reader does not re-open the manifest question.
- **Blast radius if wrong:** the reference dangles until phase 4 lands. **DISCHARGED:** phase 4
  landed as `6427d89`; the anchor is `docs/ENGINEERING-LOG.md`'s `### U-560` heading, and
  `D-W5-105`'s REASONING is inline at `crates/acdp-registry-server/Cargo.toml:41-43` —
  the id string itself is not in that file, which is why the test's message points at the lines
  rather than at the id. All three
  ENGINEERING-LOG citations in `tls_startup.rs` reach a tracked record.
- **One gap found in the cited ARTIFACT, not in this decision:** `tls_startup.rs:16` promises the
  log is "where to read the reasoning", and the log carried the decision and the reversal
  mechanism but not the post-quantum trade behind it. Corrected by adding the reasoning to the
  log rather than by weakening the comment — a citation that resolves to a record missing the
  thing it was cited for is the same defect this unit exists to remove, one level out.
- **On `U-563`:** before this PR it appeared in no tracked file at all. It now appears in four,
  all added here — but only as a pointer to an off-repo board, never as the authority. The log
  says so outright and carries the substance itself (the reversal needs BOTH enablers removed),
  so this entry's own rule — a self-referential citation is not a citation — is satisfied by the
  record existing, not by the id resolving. `tls_startup.rs` no longer cites the id at all.
- **Status:** CONFIRMED (2026-09-19, Opus under `/reconcile`). See `DECISIONS.md`.

## U-560 — resolution of U-556's "child stdout is not drained" entry

- **Plan:** `plans/u560-honest-test-infrastructure.md`
- **Resolves:** the `NEEDS-CHANGE` entry above (*"U-556 — child stdout is not drained during the
  probe"*). This is an APPENDED resolution, not a rewrite: that entry's `Status:` block carries
  measurements worth keeping, and this file is cumulative.
- **Done:** the child now writes stdout and stderr to files in the tempdir the test already owns,
  so the decision that entry recorded — *whether* to drain — no longer exists to be made. Request
  count, retry policy and `RUST_LOG` stop mattering simultaneously. The coupling that entry warned
  about ("the two are one decision, not two") is severed rather than documented.
- **One sentence in that entry is wrong and could not be corrected in place without rewriting it,
  so it is corrected here:** it says the structural fix improves diagnostics because *"today a
  wedged child yields a truncated 64 KiB"*. A pipe does not truncate — it **blocks the writer**.
  And on the early-exit branch the child has already exited, so `wait_with_output()` drains to EOF
  and gets everything. The real gap is the opposite one: a genuinely wedged child never exits, so
  `try_wait` never returns `Some`, that call is never reached, and the test falls through to the
  probe assertion carrying **zero** child bytes. That is what phase 3 of this unit fixes, and it
  is a larger gain than the pipe swap.
- **That entry's central reasoning was RIGHT, and this unit's plan briefly talked itself out of
  it.** It said the no-drain decision "holds only because `Exchange`-stage probe failures are
  non-retryable... the two are one decision, not two". Measured directly rather than reasoned
  about, 2026-09-19: a child with an **unread stdout pipe** at `RUST_LOG=trace` stops responding
  without exiting, and the caller hits its 5s read timeout. The test issues at most one request,
  and measured (below) it passes with a margin of exactly **one** spare request. That is a genuine
  bound, exactly as the entry claimed; a draft of U-560's plan called it "an accident of ordering,
  never a bound", which was wrong and is retracted here.
- **The byte figures.** The entry's ~16 KiB / ~20 KB-per-probe figures are U-556's and were not
  re-verified. Measured fresh 2026-09-19 (debug binary, default features, TLS on, sqlite,
  `RUST_LOG=trace`): forced early exit **~200 B**, all of it stderr, stdout 0 — indicative only,
  since that line embeds the tempdir path and moves with its length (196 / 237 B for a short and a
  long path); successful TLS startup **~35.6 KB** (35,576 / 35,588 / 35,600 / 35,610 B across
  runs); startup plus one probe by the test's own rustls client **46,708 / 46,710 / 46,743 B**,
  i.e. ~11.1 KB per probe, which **supersedes** the inherited ~20 KB-per-probe figure for this
  client. Per-request cost is constant to within two bytes across eight requests, so the ~29.9 KB
  left after startup holds **two** probes and the **third** wedges. Startup alone never wedges —
  served requests do.
- **On the 64 KiB ceiling, which the line above used to claim both ways.** U-556 measured it
  (65,531 / 65,536 B) and this unit re-measured it independently, observing the unread pipe stop
  at exactly **65,536 B**. So it is *not* one of the inherited-and-unverified figures, and listing
  it as such alongside a fresh measurement of the same quantity was a contradiction four lines
  wide. The genuinely inherited-and-not-re-verified figures are the ~40 KB-at-default-filter one
  and the ~20 KB-per-probe one.
- **RETRACTION, and it is a retraction of a retraction — held to the standard of the claim it
  overturns.** An earlier draft of this entry asserted "three requests served, the fourth wedges"
  and "by the moment a single probe returned, **96,573 B**", and built on the second of those a
  paragraph asking why a pipe-bound child keeps serving *past* the 64 KiB ceiling. **All three are
  withdrawn.** 96,573 B was never a one-probe figure: one probe leaves the child at 46.7 KB (real
  client) or 51.6 KB (a heavier OpenSSL client), both **under** the ceiling, so nothing ever serves
  past it and the phenomenon I set out to explain does not occur. The "three requests" figure came
  from a Python/OpenSSL client costing ~16.1 KB per request, not the ~11.1 KB the real probe costs;
  that client's own wedge point is **one** request served, the second stalling. Three instruments
  gave three different answers (3, 2, 1) because each measured a **different client**, which the
  original claim never named — the request count is not a property of the server alone. The figure
  that belongs here is the one for the client this test actually uses.
- **Status:** CONFIRMED (2026-09-19) — superseded by the change; nothing further to decide.

## U-552 Phase 3 — the AC4 kill-vs-drift proof, anchored to SYMBOLS not line numbers
- **Plan:** `plans/u-552-widen-mutation-scope.md`
- **Assumed:** every survivor line that vanished from the store.rs tranche was KILLED by a
  test, not merely displaced by a line-number shift. The set check cannot tell those apart.
- **Chose:** prove it three independent ways rather than trust the count.
  1. **Arithmetic on the diff.** The only commit touching
     `crates/acdp-registry-sqlite/src/store.rs` since the shard ledgers is `1070c33`; its
     14 hunks all start at or below line 1986 (lowest `@@ -1986`), and every survivor site
     is at or above 1306 in the file, so no survivor could shift.
  2. **The classifier** returns `KILLED (proven by this run)` for all six against
     `docs/mutation-runs/u552-union-scope-351-outcomes.json`, matching by exact name.
  3. **A negative control:** the same six against the 213-scope ledger, which never
     contained store.rs, return `NOT A KILL` — so the verdict discriminates.
- **THE SYMBOLS, because a line number written today decays tomorrow.** Carried from
  U-560's finding (lane-2 cited `tls_startup.rs:149` for an assertion the same commit moved
  to `:162`). The six killed mutants are identified here by the symbol each one mutates, so
  this record survives any future reformatting of store.rs:
  `<impl RegistryStore for SqliteStore>::put`, `::mark_superseded`,
  `::first_version_ctx_id`, `::idempotency_evict_expired`, and the two non-`>=` comparison
  replacements on the `expires_at > now` guard inside `::commit_publish`.
  The two CARRIED survivors are, likewise by symbol: the `>=` replacement on that same
  `expires_at > now` guard (equivalent — step 1 already DELETEd everything at or before
  `now` in the same transaction under `BEGIN IMMEDIATE`), and the `!=` -> `==` replacement
  inside `::commit_publish`'s `if inserted == 0` branch (unreachable by design).
- **The line numbers above are NOT a counter-example to this rule.** They are an arithmetic
  claim about ONE named commit's diff (`1070c33`), which is immutable; they are not
  citations into a moving file. The distinction is the whole point: cite a symbol when you
  mean "this code", cite a line when you mean "this diff".
- **Blast radius if wrong:** deleting a survivor line that was never killed drops the budget
  for nothing and loses a live survivor silently. That is why three proofs, not one.
- **Status:** **CONFIRMED** (2026-09-19, Opus under `/reconcile`) — all six classify `KILLED (proven by this run)` against the committed 351 ledger, each matching by exact name at the exact site. Independently re-derived by the Phase 3 verifier, which also replayed the prior 8 survivors against the new ledger and got 6 CaughtMutant / 2 MissedMutant — positive presence, not inference from absence. The harness was separately shown healthy (top sole-killer 3.7% vs the 50% ceiling), without which no CAUGHT verdict would have been evidence at all.
## U-561 — open entries whose named trigger has already fired (lane-2, 2026-09-19)

Scope: this file's open status declarations, plus the `docs/ENGINEERING-LOG.md` cross-references
into it. **Everything here is appended.** The single in-place edit is the status *token* on
U-556's entry above — lane-2's own entry, per U-507's precedent of rewriting the token only — and
the superseded text is preserved inline there rather than deleted.

### The census, and why three earlier counts were all unfit

**Measured on `723fba8`: 4,229 lines, 17 open declarations** — 14 `UNCONFIRMED`, 2 `OPEN`,
1 `NEEDS-CHANGE`. Three other figures were in play and none survives:

- **37** — `grep -c UNCONFIRMED`. **28 of the 37 are prose.**
- **9** — an anchored `^- \*\*Status:\*\* UNCONFIRMED`. A **2.9x undercount**, and it failed for
  the reason U-507 already documented at length in this file: statuses here also appear
  mid-prose-line, parenthesised (`**Status (updated …):**`), spelled `**Status of the original
  assumption:`, with the token **wrapped onto the next line**, and as a bullet or heading label
  with **no `Status` word at all**. The pattern was derived from the entries lane-2 had written
  itself, which is exactly the shape that misses what somebody else wrote.
- **26** — a first corrected extractor. It **overcounted**: a 400-char window bled into the
  following line, and U-507's `**UNCONFIRMED → CONFIRMED.**` resolution headings — which are
  *closures* — read as open.

The extractor that produced 17 resolves `A → B` transitions to `B`, reads only the status span,
and was **self-tested against 9 known-closed and 17 known-open fixtures before its number was
quoted**. Completeness was then checked from the other side: every one of the 31 lines carrying an
open token that the census did *not* flag was read individually.

**Method note for whoever counts next.** Do not write a fresh pattern. U-507's census saved this
unit from shipping a 2.9x undercount, and this entry exists to do the same again: the count is
only as wide as its pattern, and agreement between two methods that share an extractor is
structural, not corroboration.

### Settled — trigger demonstrably fired (1 of 17)

- **U-556 — "child stdout is not drained during the probe"** (`NEEDS-CHANGE` → `RESOLVED`). The
  structural fix that entry specified — *"give the child file-backed stdio in the existing tempdir
  instead of pipes"* — **landed in U-560** (`723fba8`, PR #327), together with the probe-path
  diagnostic. The resolution entry appended by U-560 already states *"Resolves: the
  `NEEDS-CHANGE` entry above"*; what was stale was the status line itself, 110 lines above it,
  which still read `NEEDS-CHANGE` with nothing pointing forward.

### Verified still open — the trigger has NOT fired (16 of 17)

Evidence recorded so the next pass does not re-derive it. **Two of these are near-misses that a
single grep would have closed wrongly:**

- **`#205` — "there is no `ETag` anywhere"**: `git grep -il etag` **does** return a file. It is a
  **comment** at `handlers/meta.rs` (*"the 404 carries no ETag"*) which **corroborates** the entry.
  No ETag is emitted. Positive control: `cache-control` matches three files, so the search works.
- **`predecessor_admission` — "the conformance fixtures do not cover the RFC-ACDP-0014 §4 reject
  path at all"**: conformance now cites *"RFC-ACDP-0014 §4/§5"* in several places. But `rev-001`
  is a **single-vector ACCEPT golden** for §5 step 2 (a revocation must not be signed by the key it
  revokes); it is **not** the §4 **reject** path for predecessor admission. Same section label,
  different path. The entry's upstream citation
  `acdp-server-0.10.0/src/registry/server.rs:716-727` is **exact**, and immutable — it names a
  pinned version, so it cannot decay.

Also verified as still-true: `playground.refuse_on_no_live_pin` does not exist (`PlaygroundConfig`
carries exactly `enabled`, `pinned_keys`, `pinned_only`; positive control `pinned_keys` matches
three files); `validate_config` still only checks `.trim().is_empty()` on the EdDSA PEM and never
parses one; `/log/checkpoint` still inherits `private` via `if_not_present`, and `handlers/log.rs`
still sets no cache header.

The remaining eleven are gated on a human ruling, an operator observation, a coordinator decision
or the scheduling of another unit. None has fired. One of them (`U-513`) waits on the settings
decision that is U-562 — which is itself blocked on a token nobody has granted, so it cannot fire.

### Restated — an open entry that named nothing which could close it

- **`predecessor_admission` enforcement: store-level coverage, not end-to-end wiring.** This entry
  carried **no trigger at all** — no `Settled by:`, no owner, no closure condition — so it was open
  by construction rather than by evidence. That is the same defect as a status whose trigger has
  fired, pointed the other way: in both cases the record asserts a state the evidence does not
  support. **Settled by:** either a conformance fixture exercising the RFC-ACDP-0014 §4 *reject*
  path (upstream spec issue #57), or an end-to-end HTTP test in
  `crates/acdp-registry-server/tests/conformance.rs` superseding a key-revocation context.
  **Owner:** whoever takes spec issue #57. **Falsifiable now:** if either exists, this closes.

### Citation decay — corrected by appending, never by editing another unit's lines

Line numbers into a **moving file** decay. Lane-1's refinement, adopted: a line number is still
correct when it cites **one named commit's diff**, which is immutable; cite a **symbol or a quoted
string** for live code.

| site | cites | actually at | note |
|---|---|---|---|
| `docs/ENGINEERING-LOG.md` U-507 section | `ASSUMPTIONS.md:2872` | **`:2881`** | `:2872` is unrelated JSON-whitespace prose |
| `docs/ENGINEERING-LOG.md` U-507 section | `ASSUMPTIONS.md:315` | **`:313-314`** | lands in the right entry, wrong sentence |
| U-507's resolution of `predecessor_admission` | "status line `:569`" | **`:578`** | `:569` is prose |
| the quickstart entry | `validate_config` (`main.rs:123`) | **`main.rs:160`** | the function moved |

### One entry asserts two statuses at once

**`W2-U3`** carries `- **Status:** CONFIRMED` and, on the **next line**,
`- **Update, 2026-09-11 — PARTIALLY narrowed, still UNCONFIRMED.**` Both are the entry's own
words. Not corrected here — it is another lane's entry and the right token is a judgement its
owner should make — but recorded so it is not read as settled. **Settled by:** that entry's owner
choosing one.

- **Status:** CONFIRMED (2026-09-19) — census and triage are evidence, not judgement calls.

## U-575 — U-508 and U-513, re-examined after U-562 (2026-09-22, Opus under `/reconcile`)

**Trigger.** U-561's census (immediately above) named U-513 as waiting on "the settings decision
that is U-562 — which is itself blocked on a token nobody has granted, so it cannot fire." U-562
merged as PR #332 (`e9e8f39`, 2026-09-21): `acdp-deps-bot`'s installation was granted
`administration: read`, and `.github/workflows/branch-protection-drift.yml` now reads
`main`'s live `required_status_checks.contexts` on a schedule and fails on drift from a pinned
baseline. That baseline is `["cargo-deny","clippy","conformance (spec fixtures)","lint","rustfmt",
"tests"]` — a superset of the four in force when U-508 and U-513 were written. Both entries were
re-examined against that fact, each by a fresh, independent Opus agent instructed to re-derive the
live setting itself rather than trust this framing. Both are reversible, code-only calls — no
public contract, no schema, no auth model, no external dependency — so both are Opus's to settle,
not the human's.

**U-508 — CONFIRMED.** The added context is `lint`, which is exactly what this entry named as its
unmet ask. Independently re-verified: `lint.yml`'s job publishes a check named `lint`, that name
is in the live required list, and the workflow's own header now says outright that the check
blocks merges. The settings change happened 2026-09-16 (`ci.yml:42-46`) — six days before this
reconciliation pass, undocumented until now because nothing had re-read this entry since.

**U-513 — stays UNCONFIRMED, and this is the finding worth keeping.** The two entries share one
sentence in U-513's own text — *"one settings change... would unblock two improvements, not
one"* — which reads today as a trap: 2026-09-16's change added **two** new contexts, `lint` and
`cargo-deny`, and it would have been easy to see two new contexts land and mark both entries
settled by the same event. They are not the same event. `cargo-deny` is the `audit` job's
dependency/advisory check; neither it nor `lint` is a channel a parallel feature-builds job could
publish to. The build steps U-513 is about are still inline inside `clippy` (`ci.yml` lines 147,
252, 262, 272, 282), unmoved, and the job's own live comment still states U-513's reasoning
verbatim — a required context that stops reporting leaves every PR waiting forever. The
independent analysis also surfaced evidence that did not exist when U-513 was written: the build
steps measured at 2-10s each, cheaper than the `clippy` step beside them and off the critical path
(`tests` runs ~2m36s), so the ~8s figure the original entry weighed against is now known to
overstate the cost of doing nothing. That strengthens, not weakens, the case for leaving U-513
exactly where U-507 left it.

**Blast radius if this reconciliation is wrong:** none beyond documentation accuracy — the
underlying settings and code were not touched by this pass, only the record of them.
