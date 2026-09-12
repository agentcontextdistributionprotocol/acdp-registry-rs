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
- **Status:** UNCONFIRMED (awaiting a repo admin to action the branch-protection change).
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
- **Status:** UNCONFIRMED

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
- **UNCONFIRMED — awaiting human ruling:** whether `docker/RAILWAY.md` should require
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
- **UNCONFIRMED — deliberately not decided here:** whether the shared playground validation
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
- **Status:** UNCONFIRMED

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
- **Status:** UNCONFIRMED
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
- **Status:** UNCONFIRMED

### The `ACDP_BOT` App has `contents: write` + `pull-requests: write` on THIS repo
- **Assumed:** yes, from `repository_selection: all` on the org App and its use in three existing
  workflows here.
- **Why it is NOT proven:** verifying needs a JWT; org secret listing needs admin.
- **Mitigation that made it safe to proceed:** failure is loud and immediate — token minting
  fails before any side effect — and the token is now explicitly narrowed with
  `permission-contents` / `permission-pull-requests` rather than inheriting every installation
  permission (which would have included `workflows: write`).
- **Blast radius if wrong:** the release-plz job fails at the mint step. Reversible in two lines.
- **Status:** UNCONFIRMED

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
- **Status:** UNCONFIRMED

## W3-U1 — #192/#193: validating playground config at both doors (2026-09-10, lane-1)

- **UNCONFIRMED — a deliberate departure from a written acceptance criterion.** The unit
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
  plainly wrong for the lax case. **Status: UNCONFIRMED.**
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
- **UNCONFIRMED and deliberately left alone:** that path answers `502`, which blames an
  upstream for a local data fault. It is defensible (the wire code is registered to
  RFC-ACDP-0012 §11's federation meaning) and changing it is a wire change. Noted next to the
  mapping in `error.rs` rather than fixed inside a docs pass. **Status: UNCONFIRMED —
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
  original claim, pointing the other way. **Status: UNCONFIRMED — split out by design.**

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
  case still fails late. Every document it touches is scoped to say so. **Status: OPEN, owned
  by nobody, reported in this lane's `done`.**
- **UNCONFIRMED — reported, not acted on:** that `.github/workflows/docker.yml` sets no
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
- **Status: UNCONFIRMED — blocked on the R3 ruling, not on evidence.** The evidence is
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
- **UNCONFIRMED — a CDN in "cache everything / ignore origin headers" mode defeats both
  `private` and `Vary`.** No origin header can fix this. It stays an operator note in
  `RECEIPTS.md`, downgraded to defense-in-depth rather than deleted. Not testable from here.
- **UNCONFIRMED — `private` carries no validators.** No `ETag`, no `Last-Modified`, no
  `max-age`, so a requester's *own* cache may briefly reuse a context retracted since. Accepted
  deliberately: they already held those bytes. **The future fix is ETags plus explicit
  freshness, NOT `no-store`** — reaching for `no-store` would trade a real client-caching
  capability for protection against caches that ignore directives anyway.
- **UNCONFIRMED — `/log/checkpoint` inherits `private` it does not need.** It is hash-only and
  requester-invariant (`handlers/log.rs`, `State` only). It gives up shared cacheability it has
  never used. `if_not_present` was chosen precisely so an explicit short public TTL can be added
  later without touching the layer.
- **UNCONFIRMED — `/metrics` was left out of scope, and the dismissal deserves revisiting.** It
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

### UNCONFIRMED: the four new steps run `clippy`, not `cargo build`
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
- **Status:** UNCONFIRMED — **escalated to the human**. Reversible, but a product judgement on a public API taken against a defensible alternative. Recommendation: confirm as taken. See DECISIONS.md H-B #10.

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
- **Status:** UNCONFIRMED

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
- **Status:** UNCONFIRMED — handed to the coordinator as a standalone decision with this
  evidence rather than actioned here.


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
- **Status:** UNCONFIRMED — the label rename is a deliberate, documented break of an existing
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
- **Status:** UNCONFIRMED — the concurrency bound and the #242 gap are deliberate, documented
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

### OPEN — escalated, NOT decided here: the §5 `code` for a 415

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
- **Status:** UNCONFIRMED — A2 ships PARTIAL by design. The cursor oracle remains open and is
  asserted by `search_cursor_oracle_remains_open_for_tenant_scoped_caller`; A2 must not be
  described as closed until the store-side predicate lands and that test is deliberately deleted.
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
- **Status:** UNCONFIRMED — A2 remains PARTIAL. `docs/HTTP-API.md` and the call-site comment now
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
- **Status:** UNCONFIRMED — recorded so it is a known latent rather than a rediscovery.

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
- **Status:** UNCONFIRMED
