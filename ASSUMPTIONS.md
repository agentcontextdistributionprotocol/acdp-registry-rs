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
