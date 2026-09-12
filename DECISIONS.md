# Decisions log

Durable record of `/reconcile` outcomes. Each entry: the original assumption, the
recommending agent's verdict, the human decision, and the resulting status. `/ship` and
future `/reconcile` passes read this file instead of replaying the conversation.

## 2026-08-29 — `reg3-anchors` Phase 4 (RFC-ACDP-0016 version-gated anchors, PR B in flight)

Reconciled pre-ship, per `/drive`'s own sequencing (run once every phase is `DONE`,
before the closing `/ship` pass). One `UNCONFIRMED` entry, a genuine one-way door
(public wire-contract change to the capabilities advertisement).

### 1. Make `acdp_version: "0.5.0"` reachable in the capability ladder
- **Assumption:** without this phase, RFC-ACDP-0016 §10's version gate is dead on
  arrival — the pre-existing ladder in `build_capabilities`
  (`crates/acdp-registry-server/src/main.rs`) topped out at `"0.4.0"`, so no
  configuration of the shipped binary could ever advertise `>= 0.5.0`, and every
  anchored publish would be rejected forever in production.
- **Chosen implementation:** the `max()`-over-per-feature-version-claims refactor
  (`main.rs:866-957`) — `ladder_claims`/`ladder_rung_claim`/`acdp_version_claim` — with
  an unconditional `ANCHORS_VERSION_CLAIM: (5, "0.5.0")` folded in, so every reachable
  deployment now advertises `acdp_version >= "0.5.0"`, no config gate to opt out. This
  executes the prior wave's OQ2 follow-up (`DECISIONS.md`, `reg2-reg5-reg6-reg8-reg9-wave4`
  entry) rather than superseding it.
- **Recommending agent:** fresh Fable pass (one-way-door tier, per `/reconcile`'s own
  tiering rule), independent of the Fable pass already run during `/implement`'s Phase 4
  verification gate (that one checked implementation correctness; this one checked
  whether the choice is still the strongest long-term call now that the code exists).
- **Fable's recommendation:** **CONFIRM as-is.** Re-verified the "dead on arrival" claim
  directly against `crates/acdp-registry-core/src/handlers/context.rs:338-347` (the
  accept gate keys on `state.server.capabilities().acdp_version`). Found the
  implementation-time framing of alternative (b) — a config opt-in flag gating only the
  *advertisement* — was inaccurate: because the accept gate keys on the advertised
  version, a default-false flag would de facto gate anchors *acceptance itself*,
  shipping the feature broken-by-default for any operator who never finds the flag, and
  would reintroduce the exact version-regression hazard (advertised version dropping
  back down when a flag is toggled off) that the one-way-door analysis most wanted to
  avoid. The unconditional constant is the only shape where the advertised version can
  never regress. Residual risk (an operator with no interest in anchors has no opt-out)
  is unchanged from before this phase either way, since anchor handling is unconditional
  code regardless of the advertisement mechanism.
- **Human decision:** **Confirm as-is**, per Fable's recommendation.
- **Status:** CONFIRMED (2026-08-29).

---

## Summary (`reg3-anchors` Phase 4)

1 entry, confirmed as recommended — no code changes needed. PR B (Phases 2-7) is now
clear to proceed to the closing `/ship` pass; this was the only `UNCONFIRMED` entry
blocking it.

---

## 2026-08-29 — `reg1-reg7-conformance-deny` (REG-1 PR #94, REG-7 PR #93, both merged)

Reconciled post-ship, per `/drive`'s own procedure (both PRs already merged; this pass
closes out `ASSUMPTIONS.md`'s 8 `UNCONFIRMED` entries logged during implementation).

### 1. `checkout-spec@v1` vs inline checkout
- **Assumption:** shipped an inline `actions/checkout@v4` for the spec pin in
  `.github/workflows/ci.yml`'s `conformance` job, diverging from
  `acdp-ci/DELIVERY-STANDARD.md:64-71`'s stated "MUST use `checkout-spec@v1`."
- **Recommendation (Opus):** confirm as-is. Decisive finding: the `v1` tag in `acdp-ci`
  (`8e99405`) is six commits behind `main` and does not contain the `checkout-spec`
  action at all (added later in `22dd548`) — a workflow referencing
  `acdp-ci/actions/checkout-spec@v1` today would fail to resolve. Zero repos in the
  family use the shared action; `acdp-rs` itself still does inline checkout, confirming
  DELIVERY-STANDARD.md's claim about `acdp-rs` is stale. The document describes an
  intent, not a current state.
- **Decision:** Confirm as-is. File a GitHub issue in `acdp-ci` flagging: (a) `v1` needs
  re-tagging to include `checkout-spec`, (b) DELIVERY-STANDARD.md:64-71's status line
  needs correcting for both `acdp-rs` and this repo.
- **Status:** CONFIRMED (2026-08-29). **Follow-up owed:** file the `acdp-ci` issue
  (tracked below, not yet filed as of this entry — see Follow-ups).
- **Correction (2026-09-01):** the `acdp-ci` issue was filed and resolved —
  `acdp-ci#9` ("Move the v1 tag — CI-1/CI-6/CI-7 hardening is merged to main
  but unreachable at v1"), now CLOSED. `acdp-ci`'s `v1` tag has been re-tagged
  to the current `main` HEAD (`015910153b61c32abbe018afe85d44868897bf3b`,
  verified via `git rev-parse v1^{commit}`/`git rev-parse main` in a local `acdp-ci`
  checkout) and now contains `actions/checkout-spec` (`git ls-tree -r v1`
  lists `actions/checkout-spec/action.yml`). The premise that motivated
  "confirm as-is" (an unusable `v1`) no longer holds; this repo still does
  inline checkout rather than adopting `checkout-spec@v1`, which is fine —
  nothing requires the change — but the DELIVERY-STANDARD.md staleness this
  entry flagged for `acdp-ci` is only PARTIALLY fixed: `acdp-ci#9` (closed) was
  scoped solely to re-tagging `v1` and never mentions the status-line text, and
  a byte-for-byte diff of `DELIVERY-STANDARD.md`'s status paragraph between
  `22dd548` and current `acdp-ci` HEAD (`0159101`) shows no change — it still
  reads "As of 2026-08-28: `acdp-rs` pins via this action; `acdp-verifier-py`
  (PY-2) and `acdp-registry-rs` (REG-1) are scheduled to adopt it — until they
  do, their CI does not enforce this rule." That line is now factually wrong
  about this repo: `acdp-registry-rs` already pins the spec at a 40-hex SHA
  inline (`.github/workflows/ci.yml:161`), just not via `checkout-spec@v1`.
  Part (b) of the original follow-up (correcting the status line) was never
  filed and remains outstanding.
- **Superseded (2026-09-06, `#155`):** this repo now uses the shared action.
  `.github/workflows/ci.yml`'s `conformance` job calls
  `agentcontextdistributionprotocol/acdp-ci/actions/checkout-spec@015910153b61c32abbe018afe85d44868897bf3b # v1`,
  so the inline `actions/checkout` spec step this entry describes — and the
  `.github/workflows/ci.yml:161` line the 2026-09-01 correction cites — no
  longer exist. The original "confirm as-is" is left standing above on purpose:
  it was correct on 2026-08-29, when `v1` genuinely did not contain the action.
  Only its premise changed, and the 2026-09-01 correction had already recorded
  that change. The pinned spec ref is untouched
  (`d1f06d0d49b73d411a3983d3877321ccaccd38e7`); the action itself is pinned at
  the commit `v1` dereferences to — `v1` is an annotated tag, so `refs/tags/v1`
  → tag object `82b2a25…` → commit `0159101…`, which is why `@v1` would not
  have been a pin at all. That matches `acdp-verifier-py`'s call site verbatim
  and `REG-8`/`REG-10`'s rule that non-`actions/*` refs resolve at an immutable
  SHA. Part (b) above is unchanged by this and is now *worse* than stale:
  `DELIVERY-STANDARD.md`'s status line says this repo is "scheduled to adopt"
  the action and that "until they do, their CI does not enforce this rule" —
  flatly false once `#155` lands. Still not filed; still owed.
- **Correction to the quotation above (2026-09-06, `#155` review).** The two
  paragraphs above quote `DELIVERY-STANDARD.md` as saying `acdp-registry-rs` is
  "scheduled to adopt" the action and that "until they do, their CI does not
  enforce this rule". **That sentence no longer exists in `acdp-ci`.** It was
  replaced on 2026-09-05 by `566e561` (`acdp-ci#14`) — verified with
  `git log -S "scheduled to adopt" -- DELIVERY-STANDARD.md`, whose diff removes
  that exact line. (It was *not* `60564f6`/`acdp-ci#17`, which changed the
  `secrets: inherit` adoption paragraph in a different section and touches
  nothing here — a misattribution caught while checking this correction.)
  The current text reads: `acdp-rs` and `acdp-registry-rs` "satisfy the pinning
  rule using the inline pin shape instead — neither has adopted the action."
  The **conclusion is unchanged and part (b) is still owed** — that replacement
  sentence is itself falsified by `#155`, which adopts the action. Only the
  wording of what is wrong has moved.
  **Method note, and the reason this rotted:** quoting another repo verbatim in
  a durable file here is the same hazard as the `bump-spec.yml` comment fixed
  in `#139` — mirroring external state locally, where this repo cannot observe
  the source changing. Both were true when written. Prefer a citation
  (repo + path + commit) to a quotation whenever the source is outside this
  repo; the citation stays valid when the text moves, the quotation does not.

### 2. `bump-spec.yml` scope
- **Assumption:** no `bump-spec.yml` in this repo; the pinned spec SHA will never
  auto-refresh via the family's `repository_dispatch: spec-released` mechanism.
- **Recommendation (Opus):** change — add it now. This repo's inline checkout shape is
  compatible with the shared `bump-spec-ref.yml@v1` caller (its matcher handles both the
  inline `repository:`/`ref:` shape and the `checkout-spec@` shape), so no
  `checkout-spec@v1` adoption is required first. But found a deeper gap: the spec repo's
  own `notify-spec-consumers.yml` dispatch matrix is hardcoded to `[acdp-rs,
  acdp-verifier-py]` — adding `bump-spec.yml` here alone doesn't close the loop; this
  repo also needs adding to that matrix, a cross-repo edit in the spec repo.
- **Decision:** Add `bump-spec.yml` here as a near-term follow-up (inert until
  dispatched, zero CI-time risk). File the spec-repo dispatch-matrix addition as a
  separate, paired cross-repo item.
- **Status:** NEEDS-FOLLOWUP (2026-08-29). Not a one-way door, not blocking anything
  already shipped. **Follow-up owed:** (a) add `bump-spec.yml` to this repo, (b) file an
  issue/PR against the spec repo's `notify-spec-consumers.yml` matrix + update
  DELIVERY-STANDARD.md's status line for this repo.
- **Correction (2026-09-01):** part (a) shipped — `.github/workflows/bump-spec.yml`
  exists (commit `87e4127`, "ci: add bump-spec.yml for spec-pin bumps (#110) (#119)").
  Part (b) is still outstanding: the spec repo's
  `.github/workflows/notify-spec-consumers.yml` dispatch matrix (line 25) is still
  hardcoded `repo: [acdp-rs, acdp-verifier-py]` — `acdp-registry-rs` has not been added,
  verified directly against that file. **Status:** part (a) DONE, part (b) still
  NEEDS-FOLLOWUP.
- **Note (2026-09-06, `#155`):** the compatibility argument in the recommendation
  above still holds, but this repo now exercises the *other* branch of that
  matcher. `bump-spec-ref.yml@v1` anchors on either a `repository: <spec>` line
  or an `acdp-ci/actions/checkout-spec@` line; since `#155` this repo matches on
  the latter. Verified by running that workflow's own anchor-count `awk`, its
  `perl` rewriter and its post-rewrite assertion against the new `ci.yml`: one
  anchor, and a simulated bump rewrote exactly one line, the spec `ref:`. Note
  for future editors: adding an explicit `repository:` to the `checkout-spec`
  step would create a second anchor and make `bump-spec.yml` fail every run.

### 4. REG-1 acceptance criterion — the "as applicable" reading
- **Assumption:** REG-1's acceptance criterion named six families (`pub-, vis-, idem-,
  caps-, lc-, fed-`) as ones that should execute "as applicable." Shipped reading: only
  `pub`/`ret` genuinely replay; the other five are accounted-for skips.
- **Recommendation (Opus):** the shipped disclosure is honest, but the "as applicable"
  reading doesn't hold uniformly across all five skipped families. `lc`/`fed` (disjoint
  advertised profile) and `caps` (non-HTTP, document-schema fixture) are legitimately
  "not applicable." `vis` and `idem` are different: both are core-*required* by the
  spec's own `acdp-registry-core` profile (confirmed via `required_fixtures`/
  `conditional_fixtures`), and the ratchet's own excuse rule would mechanically reject
  excusing them if asked — so calling them "not applicable" is inconsistent with the
  ratchet's own logic. Closing the gap needs a "stateful replay" capability (pre-seed a
  golden `sig-001` context, advertise more profiles, substitute `{ctx_id}` templates) —
  real new work, not a quick fix; priced in the plan as "roughly a phase of its own,"
  reaching ~19 fixtures (`vis`/`idem`/`lc` together).
- **Decision:** REG-1 as shipped stands (already merged, honestly disclosed in the PR
  body) — no rework of merged code. Schedule the stateful-replay phase as a concrete
  near-term follow-up item (not indefinite backlog), specifically to close `vis`/`idem`
  coverage.
- **Status:** NEEDS-FOLLOWUP (2026-08-29). Shipped code is sound; a scheduling gap, not
  a code defect. **Follow-up owed:** schedule "stateful replay" as a new REG-item
  (numbering TBD by whoever next touches `plans/00-overview.md`'s status board).
- **Correction (2026-09-01):** the "`lc`/`fed` and `caps` are legitimately not
  applicable" claim above does not match the shipped `EXCUSED`/`DEFERRED` split.
  Verified directly against `crates/acdp-registry-server/tests/conformance.rs`: `EXCUSED`
  (~6585-6618) is exactly `fp`, `data-ref-ssrf`, `fed`, `rot` — only `fed` of the three
  named here actually got the "not applicable" treatment. `caps` is `DEFERRED`
  (~6714-6719) with the reason "required-but-uncovered: ... mechanically inexcusable" —
  the opposite of not-applicable. `lc` is `DEFERRED` (~6730-6735), reason: "plausibly
  excusable, but that has never been declared, so it stays DEFERRED rather than silently
  assumed EXCUSED" — i.e. this entry's own "not applicable" framing is precisely the
  unearned assumption that DEFERRED status was created to refuse. The stateful-replay
  follow-up this entry scheduled did ship (REG-10 Phases 5-11): `vis` and `idem` are now
  both `COVERED` in the same file (Replayed + Direct for `vis`; Direct for `idem`),
  closing the gap this entry flagged for those two families. `caps` and `lc` remain
  open, tracked as `DEFERRED` under issue #115, not closed by this correction.

### 3. `can`/`lin` deliberately not excused from the coverage ratchet
- **Assumption:** `can` (12 fixtures) and `lin` (1 fixture) stay in `KNOWN_FAMILIES`
  with zero coverage rather than being added to `EXCUSED`, despite looking like pure
  library golden-vectors — because both are in `acdp-registry-core.required_fixtures`.
- **Recommendation (Opus):** confirm the policy — it's mechanically self-enforcing
  (`no_excused_family_is_required_by_our_profile` would reject excusing either by
  construction) and independently spec-verified twice this session. Separately: `can`
  and `lin` appear in zero Rust source (no `acdp-jcs` golden-vector re-assertion exists
  in this workspace either) — the recommender suggests `can` specifically might be
  cheaply closeable via a direct content-hash-path test, independent of and much cheaper
  than the expensive `vis`/`idem`/`lc` stateful-replay work.
- **Decision:** Confirm the policy as-is (no change to `EXCUSED`). Confirmed as
  recommended in the batch check — `can`'s cheap-closure finding is noted for whoever
  schedules the coverage-gap follow-up work (see #4 above), not separately scheduled by
  this pass.
- **Status:** CONFIRMED (2026-08-29).

### 5. `h2` CVE fix bundled into the REG-7 PR
- **Assumption:** `RUSTSEC-2026-0258` (h2, unrelated to REG-7's actual ask) was found
  already blocking `cargo-deny` on `main`, independent of the `all-features` flip, and
  fixed as its own commit/phase within REG-7's PR (#93, merged) rather than filed
  separately.
- **Recommendation (Opus):** confirm as-is. No policy in `DELIVERY-STANDARD.md` or
  `CONTRIBUTING.md` against bundling; REG-7's acceptance was literally unreachable
  without the fix; it was reported not silenced (version bump, no `ignore` entry, per
  REG-7's own instruction); kept as a separate commit so the flip's green run stays
  attributable to the flip. Already merged — reverting would undo a safe security fix
  for no benefit.
- **Decision:** Confirm as-is.
- **Status:** CONFIRMED (2026-08-29).

### 8. Plan-text overclaim ("yields exactly four")
- **Assumption:** the plan's Phase 4 prose claims its two-part excuse rule "yields
  exactly four" excused families — not mechanically true (ten families satisfy the
  stated two-part rule; a third, unstated criterion — "server doesn't implement it" —
  is what narrows ten to four). Zero shipped-code impact; the code's own doc-comment
  states the rule correctly.
- **Recommendation (Opus):** confirm, no edit needed — the plan file already carries a
  self-correction block (added during this session) stating the exact finding, sitting
  *before* the overclaimed line in reading order, so a future reader hits the caveat
  first. Editing the original line would just add a third copy of the same disclosure.
- **Decision:** Confirm as-is, no further plan edit.
- **Status:** CONFIRMED (2026-08-29).

### 6. Stale `deny.toml` entries (REG-9 scope)
- **Assumption:** an unused `allow-git` entry for `acdp-rs` and a stale "consumed from
  git" comment in `deny.toml`, left untouched (REG-9's separately scheduled item).
- **Recommendation (Opus):** confirm as deferred. Verified dead (workspace pulls `acdp`
  from crates.io, confirmed via `Cargo.toml`); benign `unmatched-source` warning is the
  only cost; folding into REG-7's PR would have been scope creep for zero risk
  reduction.
- **Decision:** Confirm, deferred to REG-9.
- **Status:** CONFIRMED (2026-08-29).

### 7. `storage-memory` uncovered by CI
- **Assumption:** a third storage-backend feature (`storage-memory`) is exercised by
  zero CI jobs today; noticed in passing, not added to any REG-1/REG-7 phase.
- **Recommendation (Opus):** confirm as flagged, but treat as a real gap rather than
  purely informational — unlike the deny.toml entries, this gates actual compiled code
  (`crates/acdp-registry-server/src/memory_ext.rs`, `#[cfg]` branches in `main.rs`) and
  a documented user-facing config option (`docs/CONFIGURATION.md:112`), so zero CI
  coverage means a silent compile break is possible for anyone selecting it. Cheap fix
  (one clippy matrix entry) argues for scheduling, not doing it unscheduled here.
- **Decision:** Confirm as recommended — file as a trackable backlog item (not just a
  passive note), owner TBD ("whoever owns CI-matrix completeness").
- **Status:** NEEDS-FOLLOWUP (2026-08-29). **Follow-up owed:** file a backlog item (new
  REG-item or a note on `plans/00-overview.md`'s status board) for
  `storage-memory` CI coverage.
- **Correction (2026-09-01):** shipped — commit `57d74e9` ("ci: build and lint
  storage-memory in required checks (REG-10, #109) (#118)"). `.github/workflows/ci.yml`
  now runs a `clippy (memory)` step (`--no-default-features --features storage-memory`,
  lines 56-60) and a `cargo test (memory)` step (lines 135-138) inside the `test` job
  (`test:` at line 97, display name `tests`) — not the `msrv` job (lines 64-80), which
  only runs `cargo check (sqlite default)` (line 74) and `cargo check (postgres)`
  (line 76) and never touches `storage-memory`. **Status:** DONE.

---

## Summary

7 entries confirmed, 3 of those confirmed-with-a-scheduled-follow-up (#2 bump-spec.yml,
#4 stateful-replay phase, #7 storage-memory CI coverage), 1 additional follow-up
(#1's `acdp-ci` issue). Zero entries changed the already-shipped code — both PR #93 and
PR #94 stand as merged. Zero one-way doors were in play. **Follow-ups still owed, not
yet done as of this reconcile pass:**
1. File a GitHub issue in `acdp-ci` re: `v1` tag missing `checkout-spec`, and
   DELIVERY-STANDARD.md's stale status lines (entry #1).
2. Add `bump-spec.yml` to this repo (entry #2, part a).
3. File/pair a cross-repo item for the spec repo's dispatch-matrix + DELIVERY-STANDARD
   status line (entry #2, part b).
4. Schedule a "stateful replay" REG-item to close `vis`/`idem` coverage (entry #4).
5. File a backlog item for `storage-memory` CI coverage (entry #7).

None of these are blocking anything already merged. They are new, separately-scoped
work items for a future session.

**Correction (2026-09-01) — status of the five follow-ups above, re-verified against
current `main`:**
1. PARTIAL — `acdp-ci#9` filed and closed; `acdp-ci`'s `v1` tag now points at `main` and
   contains `checkout-spec`. But `acdp-ci#9`'s scope was the `v1` tag only — it never
   mentions DELIVERY-STANDARD.md's status lines, which remain byte-identical to
   `22dd548` and are now factually wrong about this repo (see entry #1's correction
   above). The stale-status-line half of this follow-up was never filed and is still
   owed.
2. DONE — `.github/workflows/bump-spec.yml` shipped (commit `87e4127`, #119).
3. STILL OWED — the spec repo's `notify-spec-consumers.yml` dispatch matrix (line 25) is
   still `[acdp-rs, acdp-verifier-py]`; `acdp-registry-rs` has not been added.
4. DONE — `vis`/`idem` are both `COVERED` in
   `crates/acdp-registry-server/tests/conformance.rs` as of REG-10 Phases 5-11 (see entry
   #4's correction above). Note `caps`/`lc` were never claimed closeable by this
   follow-up and remain `DEFERRED` (#115).
5. DONE — `storage-memory` runs in `.github/workflows/ci.yml`'s required checks (commit
   `57d74e9`, #118; see entry #7's correction above).

Item 3 is fully open; item 1's status-line half is also still open (see above).

---

## 2026-08-29 — `reg2-reg5-reg6-reg8-reg9-wave4` (REG-2, REG-5, REG-6, REG-8, REG-9 — PRs #95, #96, #97, #99, #101, all merged)

Reconciled post-ship, per `/drive`'s own procedure. 5 `UNCONFIRMED` entries logged during
`/implement` (the plan's own Open Questions section already proposed a defensible default
for each, so `/implement` proceeded without stopping — this pass converts those into
confirmed decisions). Ranked by blast radius: OQ2 (public wire contract) first, OQ1
(design-honesty of a test-coverage substitution) second, OQ3–OQ5 (low/near-zero blast
radius, already realized as merged code) batched per reconcile's own norm for trivial
items.

### OQ2 — advertise `acdp_version: "0.4.0"` when aggregating witness cosignatures
- **Assumption:** a registry with `[[witnesses]]` configured should stop under-claiming
  `acdp_version: "0.3.0"`, since it already serves the 0.4.0 `witness_signatures` wire
  member (`main::build_capabilities`, gated on `!cfg.witnesses.is_empty()`, ordered
  before the 0.3.0 rung).
- **Recommendation:** confirm. This item already received a dedicated **Fable**
  one-way-door pass during `/implement`'s own phase-verification gate (not the default
  Opus) — Fable independently confirmed `validate_config` genuinely runs before
  `build_capabilities` on every real startup path with no hot-reload escape hatch, the
  §6.1 witness-aggregation implementation fulfills its obligation in full, and no
  consumer can be made worse off (spec min-version gates have no upper bound). This
  reconcile pass added: (1) the version bump is the *only* wire-visible signal available
  — `acdp-log-witness` is still Draft and explicitly not for registries, so there's no
  profile-based alternative; (2) checked all 4 downstream family repos
  (`acdp-control-plane`, `acdp-playground`, `acdp-ui-console`, `acdp-rs`/
  `acdp-verifier-py`) for consumers that could be surprised — none exist,
  `acdp-playground` already accepts `"0.4.0"` (added weeks before this wave, unrelated
  commit); (3) confirmed the inverse also holds — claiming 0.4.0 imposes no new
  obligation, since the only 0.4.0-version-conditional spec rule is a *permission* gate
  (`invalid_witness_cosignature` MUST NOT be emitted below 0.4.0), not a MUST the
  registry would need to newly satisfy. One nuance recorded, not a defect: the gate is on
  config (`witnesses` non-empty) while the wire member is on data (verified cosignatures
  present) — a freshly-started registry can advertise 0.4.0 before its first cosignature
  arrives. This is correct (config-gating is the right axis; `build_capabilities` runs
  once at startup and can't track live data without a redesign).
- **Decision:** Confirm as-is.
- **Status:** CONFIRMED (2026-08-29). Optional, non-blocking follow-up noted for a future
  wave: if a 5th `acdp_version` rung is ever added, consider replacing the ordered
  if/else ladder with an order-independent `max()` over per-feature version claims —
  value is low at 4 rungs, not worth doing now.

### OQ1 — accept the wit-002/wit-004 vacuous-pass substitution
- **Assumption:** REG-2's literal acceptance text ("wit-002 and wit-004 pass in this
  repo's harness") was already true, vacuously, before any of this work — both fixtures
  skip as non-HTTP vectors, and a skip counts as "pass, no failures."
- **Recommendation:** confirm. Independently re-verified against the merged code on
  `main`: the new `wit004_key_mismatch_...` test genuinely exercises real Ed25519
  verification (the asserted failure message `"signature verification failed"` is
  produced by exactly one call site in `acdp-crypto`, traced to confirm it can't be
  produced by any other failure mode in the function under test); the positive control
  (`wit-001`'s golden) isolates exactly one variable (the signature bytes) via a
  same-key/same-body cross-check; the test genuinely runs in CI under require-mode, not
  skipped. The strengthened registry-side fork-refusal tests
  (`cosignature_over_wrong_root_is_rejected`, `cosignature_beyond_current_head_is_rejected`)
  discriminate on non-overlapping message substrings, confirmed against the actual
  upstream `acdp-types` source producing those strings. The cost (≈200 lines across two
  files, one sitting, zero production-code risk) was proportionate: the alternative (a
  bare skip-line claim) would have been actively misleading for a security-adjacent
  claim (witness cosignature / fork detection), in the exact way this repo's own REG-1
  `KNOWN_FAMILIES`/`EXCUSED` ratchet already exists to prevent.
- **Decision:** Confirm as-is. Log two optional follow-ups as backlog (neither blocking,
  neither in this wave's scope):
  1. Split `verify_and_store` at the DID-resolution boundary (`crates/acdp-registry-core/src/witness.rs:133`)
     so the post-resolution half is directly testable, converting the current
     non-persistence assertions from a forward guard (they test a function with no write
     calls) into proof against the actual reject-then-no-write path. ~15 lines of
     production refactor, no behavior change.
  2. Reword the comment at `crates/acdp-registry-server/tests/conformance.rs`'s quorum
     assertion (near the `report_both.witnesses == vec![witness_id]` check) — it implies
     the assertion discriminates wit-001's witness from wit-004's, but the test already
     proves those are the same DID; the actually-discriminating assertions are the
     `witnessed_count` checks just above it.
- **Status:** CONFIRMED (2026-08-29). **Follow-ups owed:** the two items above, both low
  priority, not scheduled.
- **Correction (2026-09-01):** both follow-ups shipped in commit `0d26107` ("refactor
  (core): split verify_and_store for testability; state what the quorum assertion proves
  (#112, #113) (#121)"). (1) `verify_and_store` is now split at the DID-resolution
  boundary into a private `verify_and_store_resolved` in
  `crates/acdp-registry-core/src/witness.rs`, with two new tests exercising the
  post-resolution store path directly against a real `SqliteStore`. (2) The quorum
  assertion's message and surrounding comment at
  `crates/acdp-registry-server/tests/conformance.rs:4893` now states the honest
  limitation — it proves consistency with `witnessed_count` but cannot discriminate
  which of the two cosignatures verified. **Status:** DONE.

### OQ3 — file a spec-repo issue for the assumed `rev-001` profiles.md/profiles.json divergence
- **Assumption:** at spec pin `31cf874`, `profiles.md`'s `acdp-registry-core` row lists
  `rev-001` among its fixtures, but `profiles.json`'s `required_fixtures` (72 entries)
  doesn't contain it — assumed to be a spec-side documentation inconsistency worth an
  upstream issue.
- **Recommendation:** the assumption's premise doesn't hold — checked directly against
  `profiles.json` and found `rev-001-revocation-context-golden` **is** present, in
  `conditional_fixtures` (`required_when: "acdp_version >= 0.3.0"`), which the ratchet
  correctly reads. This is the exact same required-vs-conditional distinction this
  repo's own REG-1 coverage ratchet was built to handle (`no_excused_family_is_required_by_our_profile`
  checks both lists for the same reason). No spec bug exists; filing an issue would
  misreport a non-bug to the spec maintainer.
- **Decision:** No issue filed. Confirmed the premise was wrong, not the original
  "file an issue" plan.
- **Status:** CONFIRMED (2026-08-29) — closed, not deferred; nothing further owed.

### OQ4 — REG-8's reach: also SHA-pin `peter-evans/repository-dispatch`
- **Assumption:** `notify-website.yml` carries a credential-adjacent third-party action
  the wave's literal scope (`docker.yml`, `release-plz.yml`) didn't name, but `acdp-rs`
  already pins it.
- **Recommendation:** confirm. Independently re-verified on `main`: the pin is byte-exact
  parity with `acdp-rs`'s own pin (same 40-hex SHA, same version comment). Swept every
  workflow for any other credential-bearing action that might have been missed — none
  found; all three secret-consuming workflows (`docker.yml`, `release-plz.yml`,
  `notify-website.yml`) have every third-party action SHA-pinned, with only the
  deliberate first-party carve-outs (`actions/checkout`, `actions/create-github-app-token`)
  left on tags.
- **Decision:** Confirm as-is.
- **Status:** CONFIRMED (2026-08-29). Two follow-ups noted, both out of this wave's
  scope, not scheduled: (a) `acdp-registry-rs`'s `ci.yml` still floats several
  non-credential third-party actions that `acdp-rs` SHA-pins repo-wide — a policy gap
  between the two repos, not a defect in this decision; (b) `ci.yml:63`'s
  `dtolnay/rust-toolchain@master` is a mutable branch ref, the loosest pin in the repo —
  cheapest single hardening pickup if a future pass wants one.
- **Correction (2026-09-01):** both follow-ups shipped, commit `9313267` ("ci(security):
  SHA-pin third-party actions and replace two unreachable pins (#116)"). (a) Every
  third-party `uses:` line in `.github/workflows/ci.yml` is now SHA-pinned — 15 of 15
  (`grep -c 'uses:.*@[0-9a-f]\{40\}' .github/workflows/ci.yml` → 15); only the deliberate
  first-party `actions/checkout@v4` / `actions/upload-artifact@v4` carve-outs remain on
  tags, matching the posture this entry already treated as acceptable. (b)
  `dtolnay/rust-toolchain` is now pinned to
  `6c977a6ca4077a0ceb28ffbe03f59d46e9ac8772 # master` (a real commit SHA, not a floating
  branch ref); `ci.yml:63` no longer refers to that line at all — the file has grown, and
  line 63 is now a comment ("instead of aspirational. Checks both shipped storage
  backends."). **Status:** DONE.

### OQ5 — PR count: kept PR C (axum-server 0.8) and PR D (axum 0.8 migration) separate
- **Assumption:** isolating the axum-server security fix from the larger HTTP-stack
  migration means a router regression in the latter can't block the former.
- **Recommendation:** confirm, and record a standing policy. Verified the split paid off
  in practice, not just in theory: PR #97 (the advisory fix) merged and closed
  `RUSTSEC-2025-0134` a full 31 minutes before PR #99 (the full migration) was even
  opened — the security fix was never gated behind code that didn't exist yet. The split
  was also near-free: the two PRs' source-file diffs are disjoint, overlapping only in
  the manifest spine (`Cargo.toml`/`Cargo.lock`/`CHANGELOG.md`), trivial to sequence.
- **Decision:** Confirm as-is. Adopt as a standing policy for this repo: a change that
  closes a security advisory ships in its own PR and is never bundled into an adjacent
  larger migration, even when both touch the same crate family and even under time
  pressure — conditioned on the split being cheap (confined to manifest-file overlap); if
  a future advisory fix genuinely cannot compile without the larger migration, the split
  stops being free and this policy should yield.
- **Status:** CONFIRMED (2026-08-29).

---

## Summary

5 entries, all confirmed as recommended — no changes to shipped code, no genuine
one-way-door ambiguity found on the one item (OQ2) that warranted a dedicated look.
**Follow-ups owed, not yet started, none blocking:**
1. `verify_and_store` resolver-boundary refactor + real persist-skip test (OQ1).
2. Comment reword at `conformance.rs`'s quorum assertion (OQ1).
3. `acdp-playground/playground/conformance.py`'s stale docstring (OQ2, cross-repo,
   cosmetic — surfaced by the OQ2 recommender while checking downstream consumers).
4. `ci.yml`'s non-credential third-party actions left unpinned, unlike `acdp-rs`'s
   repo-wide posture (OQ4).
5. `ci.yml:63`'s `dtolnay/rust-toolchain@master` — the loosest pin in the repo, a mutable
   branch ref (OQ4).

All five are new, separately-scoped, low-priority items for a future session — none
require action before anything already merged is considered done.

**Correction (2026-09-01) — status of the five follow-ups above, re-verified against
current `main`:**
1. DONE — shipped in `0d26107` (#112). See OQ1's correction above.
2. DONE — shipped in `0d26107` (#113). See OQ1's correction above.
3. Not re-verified by this pass — `acdp-playground` is a sibling repo outside this
   correction's scope; status unchanged.
4. DONE — `ci.yml`'s third-party actions are now all SHA-pinned (`9313267`, #116). See
   OQ4's correction above.
5. DONE — `dtolnay/rust-toolchain` is SHA-pinned, not a floating `@master` ref (`9313267`,
   #116). See OQ4's correction above.

---

# RECONCILED (2026-09-01) — `reg10-conformance-and-ci-hygiene`

Four `UNCONFIRMED` entries from the REG-10 plan, walked in blast-radius order. Each was
given to a fresh **Opus** recommender (Opus substituted for Fable per standing
instruction); every recommendation was input only. All four verdicts are the human's.

## 1. First-party reusable workflows trusted by mutable `@v1` (Phase 3)

- **Assumption:** #111's SHA-pinning mandate scopes to third-party actions;
  `agentcontextdistributionprotocol/*` reusable workflows are trusted by major tag.
  `bump-spec.yml:18` uses `bump-spec-ref.yml@v1` with `secrets: inherit`. Ranked first:
  the only entry touching a trust boundary, and the only one that can change without a
  commit in this repo. **This assumption had never been logged** — it surfaced from
  review of the phase, not from `ASSUMPTIONS.md`; it is now recorded there retroactively.
- **Recommendation (Opus):** CONFIRM as-is. It is an existing convention, not a new risk —
  all three first-party refs in this repo use `@v1`, and the convention is stated upstream
  at `acdp-ci/.github/workflows/auto-merge.yml:10-11`. SHA-pinning the outer hop would be
  partly illusory, since the callee itself consumes `actions/checkout@v7` and
  `create-github-app-token@v3`. Its own strongest counter: the ruleset's admin bypass makes
  it a speed bump, and "we already do it elsewhere" is precedent, not justification.
- **Correction to that recommendation (found on the ship gate, after the decision):** the
  upstream citation does not carry the weight it was given. `auto-merge.yml:10-11` says
  first-party `actions/*` — GitHub's own namespace — are trusted by major tag; it says
  nothing about `agentcontextdistributionprotocol/*` reusable workflows, and the same file
  SHA-pins the third-party `dependabot/fetch-metadata`. So the "existing convention"
  support reduces to the three in-repo `@v1` refs, which the recommendation itself already
  labelled precedent rather than justification. The verdict is unchanged — it was confirmed
  on the reachability and trigger analysis, not on this citation — but the citation is
  narrower than the recommendation implied and should not be leaned on again.
- **Two of its claims were independently re-verified before the decision, because both
  changed the answer:**
  - An earlier check in this session found `acdp-ci` had **no** rulesets. That is now
    stale. Ruleset `21899019` `protect-v-tags` exists, `enforcement: active`, created
    `2026-08-30T21:36:03-07:00`, covering `refs/tags/v*` with
    `creation`/`update`/`deletion`/`non_fast_forward`, `bypass_actors` = `RepositoryRole 5`
    (admin) only. The force-move vector the assumption was written about closed between
    the two checks.
  - **The recommender was corrected on one point.** It treated `bump-spec.yml` and
    `auto-merge.yml` as comparably exposed and concluded auto-merge should be fixed first.
    They differ in kind: `bump-spec.yml` has `secrets: inherit` (org bot App key +
    `CARGO_REGISTRY_TOKEN`) but only `repository_dispatch`/`workflow_dispatch` triggers, so
    it is not fork-reachable; `auto-merge.yml` runs `on: pull_request` but has **no**
    `secrets: inherit`, receiving only `contents: write`/`pull-requests: write`, which
    GitHub downgrades to read-only for fork PRs. High-privilege/low-reach vs.
    lower-privilege/high-reach — its "fix auto-merge first" conclusion does not follow.
- **Decision:** **Confirm as-is.** No code change. The `auto-merge.yml` observation was
  reviewed and deliberately left as-is — no issue filed, recorded here instead so it is not
  rediscovered as novel.
- **Status:** CONFIRMED (2026-09-01).

## 2. Memory `test` leg ships without an anti-vacuity guard (Phase 2)

- **Assumption:** the load-bearing half of #109's fix is `clippy (memory)`'s `--all-targets`
  compile, which cannot go vacuous; the `cargo test (memory)` leg therefore ships with no
  assertion on its own test count.
- **Recommendation (Opus):** CONFIRM the entry as written — and file the *real* gap
  separately rather than amending this one. The feared regression is near-unreachable:
  `tests/anchors_uri_never_dereferenced.rs` is a source-grep test that never constructs a
  store, so gating it on `storage-sqlite` would defeat its own purpose; even if someone did,
  the leg drops to 38, not 0 (`conformance_gate.rs` is ungated and survives; the "37" in the
  recommendation and in `ASSUMPTIONS.md` was off by one, corrected in both).
  The count is read off the sources, not off a run — see the evidence caveat below. It cannot go vacuous, only thinner. The
  `ACDP_REQUIRE_CONFORMANCE` precedent does not transfer — that guards a suite compiling to
  literally zero, and there is no such cliff here. Its own strongest counter: under memory
  the harness exercises `MemoryStore` zero times, so the run half is substantively
  decorative either way.
- **The larger gap it identified:** `MemoryStore` overrides only `migrate`/`health`/
  `list_contexts` (`crates/acdp-registry-server/src/memory_ext.rs:98-119`), so tenancy runs
  on trait defaults. Traced through, tenancy **fails closed** on memory — a non-default
  tenant sees nothing — which is a broken demo, not a data leak, on a backend documented as
  ephemeral. Its proportionate fix is not a behavioral suite but one startup refusal
  mirroring the existing guard at `crates/acdp-registry-server/src/main.rs:317-323`.
- **Evidence limits, stated rather than papered over:** the recommender could not run
  `cargo test` (sandbox denied `target/` writes). Neither could this pass — cargo fails to
  write `.d` files in *any* directory in this environment, including a freshly created one,
  though it succeeded earlier in the session. **Entry 2 therefore rests on static
  verification of the cfg gates, not on a measured test count.** Gates confirmed from
  source: `conformance.rs:383`, `http_integration.rs:26`, `metrics_integration.rs:8` are
  `#![cfg(feature = "storage-sqlite")]`; `pg_integration.rs:20` is `storage-pg`;
  `anchors_uri_never_dereferenced.rs` and `conformance_gate.rs` carry no crate-level cfg.
- **Decision:** **Confirm, and file the real gap as its own issue.** The count guard stays
  unbuilt — it is a tripwire for a door nobody uses, priced in false positives on every new
  test. The startup-refusal fix is tracked separately, not folded into a CI-plumbing phase.
- **Status:** CONFIRMED (2026-09-01). Follow-up filed as #137; not blocking.

## 3. Pin durability generalized to `taiki-e/install-action` (Phase 1)

- **Assumption:** the human's ruling on `dtolnay/rust-toolchain` — prefer a default-branch-
  reachable SHA plus an explicit input over a convenient-but-unreachable ref-selector SHA —
  is a principle that generalizes, not a one-off.
- **Recommendation (Opus):** CONFIRM. Verified: the pin `1ed6d7be` is a true ancestor of
  `main` (`compare` → `behind 14`); the `cargo-llvm-cov` tool tag is off `main`'s history
  entirely (`ahead 1, behind 0`), and the tag has since moved off the entry's earlier
  `ea647c55` to `2af88edc` — which is the point, these commits are regenerated per release.
  Stated precisely, because an earlier draft of this line overstated it: `ea647c55` still
  resolves (HTTP 200, `ahead 1, behind 14`); it is dangling — referenced by no tag or
  branch — not deleted. Upstream's own framing of the hazard is a commit *not present on
  the repository*, which is a stronger condition than this one and is not what happened
  here. Upstream's own security section
  *discourages* hash-pinning tool tags and routes pinners to a version tag, so the chosen
  shape follows vendor guidance rather than deviating from it. The "version bumps now need a
  deliberate commit" objection is answered in-repo by `.github/dependabot.yml:13-19`.
- **Decision:** **Confirm as-is.** No code change.
- **Status:** CONFIRMED (2026-09-01).

## 4. Amended acceptance criterion 4 (Phase 1)

- **Assumption:** AC4's wording ("the coverage job's install-action pin still defaults
  `tool: cargo-llvm-cov`") encoded a *means*, not the *end*, and was amended to "the coverage
  job installs `cargo-llvm-cov`, via an explicitly passed `tool:` input on a `main`-reachable
  pin."
- **Recommendation (Opus):** CONFIRM. AC4's letter was unsatisfiable — `tool` is required
  with no default on `v2`/`main`; a `default:` exists only in the generated tool-tag commit,
  i.e. only on the ref the phase existed to stop using. AC4 thus encoded "keep the orphan
  pin" as a hidden premise. What makes the amendment legitimate rather than
  criterion-shopping: the *end* was preserved verbatim, only the *means* clause moved, and it
  was logged rather than quietly applied. Its own strongest counter: amendment-by-implementer
  erodes if unpoliced.
- **Decision:** **Confirm as-is.** Adopted as the standing rule for future phases: **amend a
  criterion only when the new wording is strictly narrower than or equal to the original on
  the outcome; escalate when it would weaken what is being verified. Always log the
  amendment.**
- **Status:** CONFIRMED (2026-09-01).

## Standing rule adopted this pass — extending a prior human ruling

Entry 3 raised a question larger than itself: a ruling given on one action was extended to a
second without re-asking. Put to the human directly, since it sets precedent. **Ruling: a
prior decision may be extended to a new instance without re-asking only when all three
hold —**

1. the **reason given** applies unchanged, not merely the outcome;
2. the second instance sits inside the **same unit of work** already under review;
3. the blast radius is **bounded and fails loudly**.

Anything failing one of the three gets asked. Logging to `ASSUMPTIONS.md` as `UNCONFIRMED`
remains mandatory either way — that log, not the good outcome, is what made this entry
reviewable at all.

## Summary

4 entries: **4 confirmed, 0 changed, 0 deferred.** No code follow-up blocks the next
`/ship`. One non-blocking issue filed (#137 — entry 2's `MemoryStore` startup refusal). One
previously-unlogged Phase 3 assumption recorded retroactively in `ASSUMPTIONS.md`. Two
standing rules adopted: the criterion-amendment rule (entry 4) and the ruling-extension bar
(above).

---

## 5. `secure_compare::ct_eq` is `pub(crate)`, not `pub` (2026-09-10)

**Plan:** `plans/u003-metrics-ct-eq.md` (U-003 — landing PR #171, `/metrics` constant-time
bearer, #168). **Decided by:** Opus, reversible tier — not escalated.

**The assumption as logged.** PR #171 extracted the private `fn ct_eq` out of
`handlers::admin` into a new `secure_compare` module and exported it `pub`. `/implement`
logged this UNCONFIRMED, reasoning that `acdp-registry-core` is an internal crate with no
external consumers and that `pub` preserved the option of another workspace crate calling
the helper later.

**Analysis (fresh Opus agent, independent).** The option being preserved does not exist,
and cannot:

- **Six of the seven sibling crates could never call it.** The dependency graph is
  `types ← store ← {pg, sqlite}`, `types ← auth`, `types ← webhook`, with
  `core → {auth, store, types, webhook, sqlite}` and `server → core`. `acdp-registry-core`
  sits second from the top, so `auth`, `webhook`, `store`, `types`, `pg` and `sqlite` are
  all *below* it — calling up would be a dependency cycle. `pub` cannot serve them at any
  point; only relocating the helper to `acdp-registry-types` would, and nothing down there
  needs it.
- **The one eligible crate does not need it.** `acdp-registry-server`'s only
  credential-adjacent comparisons are `main.rs:78`/`:93` (`jwt_signing_alg` vs `"EdDSA"` —
  not secret) and `main.rs:153` (`token != token.trim()` — startup validation of the
  operator's own configured value, no attacker-controlled input, no request-path timing
  channel).
- **No latent second consumer anywhere.** `acdp-registry-webhook` only *produces*
  `X-ACDP-Signature` (`lib.rs:273`) and never verifies one, so there is no MAC compare to
  protect; `acdp-registry-auth` verifies via `jsonwebtoken::decode`, so the signature
  compare is inside RustCrypto, not our code; replay protection is an atomic
  `ChallengeStore::take(&nonce)` lookup, not a byte compare; and the `prior_hash`
  comparisons in the pg/sqlite stores are over a public content/idempotency identifier.

**Two facts that decided it.**

1. **`publish = false` is weaker than the assumption treated it as.** It is a *release-plz*
   key (`release-plz.toml:5`), not Cargo's. No `crates/*/Cargo.toml` carries Cargo's own
   `publish = false`, so a manual `cargo publish -p acdp-registry-core` is unblocked today,
   and that line's own comment states the flip to `true` is intended. When it flips, every
   `pub` item silently becomes a semver commitment.
2. **On `main` this helper was *private*.** The `pub` is a widening newly introduced by the
   extraction, incidental to its actual purpose (deduplication) — not a posture being
   preserved. Framing it as "keep what #171 chose" obscured that the refactor itself
   created the exposure.

`ct_eq` is also a poor thing to commit to publicly: `secure_compare.rs:14-16` documents that
it deliberately does **not** hide token length, so an external caller taking it for a
general-purpose constant-time compare would be misled.

**Decision: narrowed to `pub(crate)`.** Two lines —
`crates/acdp-registry-core/src/secure_compare.rs:17` and
`crates/acdp-registry-core/src/lib.rs:13`. No `Cargo.toml`/`Cargo.lock` change. Compiler-
verified safe: both call sites are `crate::`-rooted, the unit tests are an in-file
`mod tests` on `super::*`, and the only other mention of `ct_eq` outside the crate is a
comment. Docs cite the module by **file path**, never by Rust import path, so no doc edit
was needed.

**Rejected middle options.** `#[doc(hidden)]` on a still-`pub` item (hides it from docs
while leaving it callable and semver-relevant — the worst of both); moving the helper to
`acdp-registry-types` (the only change that would genuinely unlock the six lower crates,
but speculative restructuring for a need that does not exist, and it would require a
Cargo edit that is out of scope for this unit).

**Why this is not a violation of the "land #171 as written" constraint.** The merge commit
`a1af331` is untouched and its tree remains bit-identical to the pre-merge trial — that
property was always a property *of that commit* and stays true permanently. The constraint
forbade smuggling a tidy-up *inside* the landing; this is a separate, analyzed, reviewed
follow-up, which is exactly what the UNCONFIRMED entry was logged to produce. The approach
#171 chose — one shared helper, both gates calling it, same implementation, same location —
is unchanged. Acceptance criterion 1 ("exactly one constant-time helper, both gates calling
it") holds identically under `pub(crate)`.

---

5 entries: **4 confirmed, 1 changed, 0 deferred.** The change is applied in full, not
deferred — no code follow-up blocks `/ship`.

### Entry 5 — two corrections from the pre-merge gate (2026-09-10)

Appended rather than edited in place, so the original reasoning stays auditable. Neither
correction changes entry 5's conclusion; both are inaccuracies in its supporting statements
and are recorded because a decision record that overstates its own evidence is worth less
than one that doesn't.

1. Entry 5 says *"Docs cite the module by file path, never by Rust import path."* Not quite:
   `CHANGELOG.md:2107` writes `acdp_registry_core::secure_compare`, which is import-path
   form. It reads there as a statement of where the module lives rather than a claim that
   external crates may import it, so nothing in that entry is falsified by the narrowing —
   but "never" was too strong.
2. Entry 5 says *"the only other mention of `ct_eq` outside the crate is a comment."* There
   are **two**, not one: `crates/acdp-registry-server/src/main.rs:126` and
   `crates/acdp-registry-server/tests/metrics_integration.rs:497`. Both are comments, so the
   conclusion — no code outside `acdp-registry-core` references the helper — still holds.

---

## 6. U-001 — `predecessor_admission` enforcement: four settled calls (2026-09-10)

Plan: `plans/u-001-acdp-0.10.0-predecessor-admission.md` (issue #174, supersedes PR #175).
All four are Opus calls under the autonomy ladder — consequential but reversible, none a
one-way door. Recorded because each shaped the diff.

**6a. Where the admission call sits — settled by upstream, not by preference.**
The plan initially framed this as a choice between two contract-legal slots (the "contract
floor" right after the ownership gate, versus after `AlreadySuperseded`) and defended the
later one on error-taxonomy grounds. The plan review found that framing wrong. The reference
implementation's own comment (`acdp-server-0.10.0/src/registry/store.rs:773-780`) says the
hook "Runs AFTER producer-continuity, lineage/version coherence, and AlreadySuperseded have
all passed — never earlier", and `check_revocation_supersession` puts arm 5 (already
superseded) explicitly out of scope. **The contract floor would violate the documented caller
contract.** Not a judgement call; the plan was corrected and a test now guards the position.

**6b. A corrupt predecessor `body_json` is `RegistryInternal`, not `SchemaViolation`.**
Parity with `row_to_context`, which already reports every other decode failure in both stores
that way. Rejected `SchemaViolation` — it blames the producer for the registry's own bad data.
Accepted wart, recorded rather than hidden: `RegistryInternal` reports `is_transient() == true`
(`acdp-primitives-0.10.0/src/error.rs:297-306`), so a *permanently* corrupt row is advertised
as retryable. That is pre-existing and repo-wide (every `row_to_context` decode has it); fixing
it belongs in its own unit, not smuggled into a dependency bump. Reversible in one line.

**6c. Supersede PR #175 rather than rebase it.**
#175 changed only `Cargo.toml` + `Cargo.lock` and failed 7 of 9 CI jobs — the code half was
never written, which is what U-001 supplies. This branch carries the same pin change plus the
code, so once it merges #175 is empty. Rejected pushing to `deps/acdp-0.10.0` from this lane's
checkout to make it empty: identical outcome, more moving parts, and it is a bot-owned branch
belonging to `bump-acdp.yml`.

**6d. The pg `commit()` test helper was NOT widened — and the first stated reason was wrong.**
The tests build their `PublishCommit` inline instead. The rationale originally recorded was
that `spawn_blocking`'s `'static` bound made widening *impossible*; verification disproved that
by compiling the alternative (an owned
`Option<Box<dyn Fn(&Body) -> Result<(), AcdpError> + Send + Sync>>` is `Send + 'static`, and
`.as_deref()` yields the field's type). The real reason is preference: less churn, no change
to five existing call sites, and it matches what the sqlite tests already do. Corrected here
because a decision record that misstates its own reasoning is worth less than one that admits
the reasoning was thinner than claimed.

### Note on evidence quality

Two claims in this unit's own records were found to overstate their evidence and were
corrected rather than quietly dropped:
1. A `content_hash` assertion was described as proving the JSONB decode faithful. It compared
   two values that both round-trip through the same decoder — decode-vs-decode, which a lossy
   decode satisfies on both sides. It now compares against the hash captured from the
   in-memory request. **And the first correction of it was itself incomplete:** it was applied
   to the pg suite only, while this file and `PROGRESS.md` both claimed it was fixed outright.
   The sqlite twin kept the circular form for two further commits. Fixed in both, and recorded
   here rather than silently completed, because "corrected" was itself an overstatement.
2. A mutation table recorded an observation for a test that did not yet exist when that
   mutation was run. Re-run against the full suite; the corrected result is stronger than the
   one first recorded.
# RECONCILED (2026-09-10) — `u-002-webhook-duplicate-event-id` (#179)

Three `UNCONFIRMED` entries from `plans/u-002-webhook-duplicate-event-id.md`. All three
**decided by Opus** at `/reconcile`, each from an independent fresh-agent analysis given the
entry plus the code it is baked into. All three confirmed; none is a one-way door needing a
human call. Two had their *reasoning* corrected — the decisions were right, the recorded
arguments were not, and an argument that does not hold is worse than none because the next
maintainer will cite it.

## 6-bis. `WEBHOOK_SCHEMA_VERSION` stays `"1.0"` across the `event_id` wire rename (2026-09-10)

**Decided by:** Opus. **Verdict: CONFIRMED, rationale replaced.**

The original argument was that the constant is emitted on all five event types while only two
changed shape, so bumping would misreport the other three. **That argument is wrong.** Both
downstream consumers parse all five types through a single open union
(`acdp-control-plane/src/contracts/acdp.ts:29-102`; `acdp-playground/acdp_client/models.py:255-288`),
so "only some variants changed" is a distinction that exists in this repo's Rust enum and in
no receiver's model.

The decision stands on a sounder argument: **`schema_version` is stamped per delivery**, so it
describes that delivery's envelope, not the stream. A `search_executed` body carrying `"1.1"`
would assert that something about *that delivery* changed when nothing did — bumping is not a
blast-radius trade-off, it is emitting a falsehood on every delivery, since the envelope it
describes changed for none of them. Independently
corroborated by **RFC-ACDP-0009 §2.10**, which reserves this profile's version field as the
schema version of the event *envelope*, independent of `acdp_version`: the narrowed scope
matches what the spec already reserved rather than being a carve-out invented to fit this case.
The next real move is expected to come from §2.10 promotion (reserved name `event_version`),
not from a variant edit.

Also corrected: the plan claimed "the only known consumer". There are **two**; neither reads
the field (verified by a grep across all sibling repos), so the conclusion is unchanged.

The doc comment at `crates/acdp-registry-webhook/src/lib.rs` now carries the corrected
argument and resolves the edge the original left open — a change touching *every* variant is
still not an envelope change and still does not bump.

## 7. `#[serde(rename)]` is symmetric on the two lifecycle fields (2026-09-10)

**Decided by:** Opus. **Verdict: CONFIRMED, reasoning strengthened, test added.**

The entry argued symmetry avoids read/write skew. True, and **understated**: symmetric is the
only *correct* form of the three, and both alternatives fail silently or totally.

- `#[serde(rename(serialize = ...))]` would read `event_id` — a key still present on the wire,
  holding the **envelope delivery id** — straight into the lifecycle field. No error, wrong
  value: precisely the `event_id` confusion #179 removes, resurrected on the read side.
- `#[serde(alias = "event_id")]` maps both names to one field slot, so serde rejects **every
  current body** with a duplicate-field error — and rejects old duplicate-key payloads too.

On an old payload the shipped form fails loudly with `missing field lifecycle_event_id`, which
is the correct outcome: that payload's `event_id` is genuinely ambiguous, and last-wins
recovery is exactly what #179 declared unsafe. With `schema_version` deliberately frozen
(entry 6), this error is also the only automatic detector an old payload has.

**Gap closed.** Nothing asserted the read side, so the entry rested on behaviour no test
exercised — the argument was unfalsifiable. `the_emitted_body_round_trips_into_the_lifecycle_field`
now deserialises a real emitted body and asserts the lifecycle field receives the actor-minted
id and **not** the envelope id. Reversal remains a two-word edit: the crate is unpublished
(`release-plz.toml:5`) and no Rust consumer exists in any sibling repo.

## 8. Notifying `acdp-control-plane`: issue on merge, never an edit (2026-09-10)

**Decided by:** Opus. **Verdict: CONFIRMED disposition and authorization; content and blast
radius corrected.**

**Authorization.** Filing an issue in a sibling repo *is* a cross-repo write — a plan review
was right about that taxonomy. But CHARTER rule 6 is a **routing** rule: cross-repo writes go
to the human, and *the leader cannot authorize them*. It does not say the permission cannot
exist. The human granted exactly this mechanism in advance ("only this repo changes; for
cross-repo changes file GitHub issues and ping that repo's Claude session"), so rule 6 is
**satisfied, not bypassed**. The reviewer was right that the earlier plan prose asserted the
exemption without naming its source; that wording now cites the source. Editing that repo
remains forbidden outright.

**Two corrections to the planned action.**

1. The standing instruction has two halves and the second was dropped: **ping that repo's
   Claude session**, not merely file the issue.
2. The issue must **not** say the control plane "can key on `lifecycle_event_id`". Read as
   dedup advice, that would put their body fallback back into disagreement with
   `X-ACDP-Event-Id` — re-creating the divergence #179 removes. `lifecycle_event_id` is actor
   provenance, **not** a dedup key. The correct ask is narrow: update the stale comment at
   `acdp-control-plane/src/contracts/acdp.ts:81-85`; no code change required.

**Blast radius corrected.** "The fallback keeps working either way" was too strong. It holds
for delivery retries — its documented purpose. It stops collapsing *distinct deliveries of the
same logical lifecycle event*: the SDK's lifecycle commit has an idempotent-replay outcome and
this registry emits the webhook unconditionally, so a producer resubmitting a byte-identical
signed event produces a second delivery with a new envelope id and the same actor-minted id.
**With the `X-ACDP-Event-Id` header present — the normal path — nothing changes.** With the
header stripped, the old fallback keyed on the actor-minted id and collapsed the pair; the new
one keys on the delivery id and does not, costing a duplicate ingest row, a double SSE emit and
a double outbound webhook. The lifecycle projection stays correct (its upsert guard makes
re-applying a transition a no-op). This is the fallback becoming *consistent with* the primary
path rather than accidentally stronger than it — defensible, but it belongs in the issue rather
than in their on-call's lap.

**Timing.** After merge and **only** after merge: a pre-merge issue would tell another team a
wire format changed when it has not, and if they act and the PR is reverted they have made a
wrong change on our word. If the PR never merges, nothing is owed and nothing is filed.

## Found while reconciling — not this lane's to fix

Both are cross-repo and outside this unit; recorded so they are not lost.

- **`acdp-playground/acdp_client/models.py:248-252`** types webhooks as a closed `Literal` of
  **three** event types, so `context_retracted` / `context_republished` fail `model_validate`
  and are logged-and-dropped at `playground/api/webhooks.py:50-56` — before the control-plane
  forward. Pre-existing and unrelated to this rename, but it means one hop in the chain
  currently discards exactly the two event types #179 is about.
- **`acdp-website/content/registry-server/webhooks.mdx`** documents three event types and no
  lifecycle events, so the public docs now lag this repo's `docs/WEBHOOKS.md`.

## 9. U-005 — three settled calls on the operator-docs sweep (2026-09-10)

**Decided by:** Opus (lane-1), during `/drive` on `#180`. Ruled on by the lane leader.

### 9a. A claim grants paths, not authority over what a change means

`docker/**` was in this unit's claim, so adding `ACDP_REGISTRY_AUTH__ENABLED = true` to
`docker/RAILWAY.md`'s required env vars would have been strictly inside the grant. It was
**not** done. Changing what a deployment recipe *instructs operators to deploy* is an
operator-visible posture change — the class `#180` says must not change silently in a docs
pass — and it merely happens to live in a granted file. The gap is **documented** instead:
`RAILWAY.md` now states that the recipe leaves auth off, that the JWT secret is therefore
never validated, and that `ALLOW_PUBLIC_BIND = true` waives a guard whose stated precondition
includes an authenticating proxy. The note names the knob without mandating it. The row itself
awaits a human ruling.

The general rule, adopted: **a path grant is not a mandate to make every change that path
would permit.** Scope is decided by what a change *means* to an operator, not by which file it
lands in.

### 9b. Wording rule for the `changeme` corrections

Every site scopes the claim to `auth.enabled`, adds **HS256** wherever the surrounding context
does not already establish it (`docs/AUTHENTICATION.md`'s callout is HS256-framed in its own
opening line, so it names only the `auth.enabled` half), notes that the match is
case-insensitive after trimming, and — where the file describes a shipped stack — states
plainly that its own default does not trip the guard. Hedging alone was rejected: a reader of `docker-compose.yml` needs to know *that stack*
boots with the placeholder, not merely that the rule has conditions. HS256 is load-bearing —
under EdDSA `jwt_secret` is never examined, and `docs/CONFIGURATION.md:35` sits one sentence
after an EdDSA discussion.

Correspondingly for claim 1: every correction carries **both** guards (`main.rs:259` and
`:267`). Documenting the first without the second would have sent an operator who followed the
corrected doc into a different startup failure — the same defect class the unit exists to
remove.

### 9c. Line-pins into edited files are reported, never re-pointed

Two live pins point into `docs/CONFIGURATION.md` (`DECISIONS.md:267` → `:112`,
`CHANGELOG.md:2054` → `:242`), both drifting `+5`. Re-pointing them means editing existing
lines in files this unit treats as additive-only, which contradicts its own zero-deletions
constraint. Drift is recorded in the CHANGELOG entry and the PR body instead. This extends the
same historical-record principle already applied to stale pins in CHANGELOG history.

### 9d. A doc may not inherit a false rationale from the code it documents

Three sites in the first draft said that `pinned_only = true` with an empty `pinned_keys`
"would reject every publish outright." **That is false, and it is false in the dangerous
direction.** `crates/acdp-registry-core/src/playground.rs:109-111` returns
`PinOutcome::Skipped` when `pinned_keys` is empty — *before* `pinned_only` is consulted — and
`Skipped` falls past the `if let PinOutcome::Verified` branch
(`handlers/context.rs:464-474`) into the unverified publish path. `config.rs:739-740` states
it plainly: "Has no effect when `pinned_keys` is empty." So that combination does not lock the
registry down; it silently leaves it wide open.

The wording was copied in good faith from the startup guard's own bail message
(`crates/acdp-registry-server/src/main.rs:271-273`), which says the same wrong thing. **The
error message is the defect; the docs merely inherited it.** Corrected wording now gives the
real rationale — the guard exists because the configuration *looks* locked down and is not.

Rule adopted: **an error message is not a source of truth about behaviour.** Verify a
rationale against the code path, not against the string the code prints. This unit's whole
premise is that documentation drifted from behaviour; quoting a stale error message is the
same failure with a shorter feedback loop. The `main.rs` bail text is filed as a follow-up —
`crates/**` is read-only for this unit.

## 10. U-004 — `cur-002`'s message-leak "gap" is not a gap (2026-09-10)

**Decided by:** Opus, via an independent reconcile analysis. Low blast radius, reversible,
no one-way door — so settled without escalating, per the Autonomy ladder.

**The assumption** (`ASSUMPTIONS.md`, "`cur-002`'s message-leak rationale is documented, not
satisfied"): that asserting only `cur-002`'s machine-checkable `expected` block is sufficient,
even though its prose `rationale` says a registry "MUST NOT leak why a cursor failed to parse
beyond the registered code" — this registry answers `"invalid cursor: cursor is not valid
base64"`, which names the reason.

**Verdict: CONFIRMED, on stronger grounds than the entry itself claimed.** The entry logged
this as a tolerated gap. It is not a gap at all:

1. **The clause has no normative backing.** `RFC-ACDP-0005-discovery.md:153-161` (§2.5.4, the
   section `cur-002` cites) lists the cursor MUSTs: 1-hour validity; no *client-decodable
   visibility information*; `cursor_expired` on result-set change; `invalid_cursor` for
   unparseable; re-scope to the current requester every page. **None concerns parse-failure
   detail.** The visibility MUST is about the cursor *payload*, not the error message. Verified
   by reading the section, not inferred.
2. **`rationale` is corpus-wide descriptive and never asserted** — stated at
   `conformance.rs:1178-1180` and listed in `RECOGNIZED` at `:1206`. 74 of 143 fixtures carry
   one; `cur-002` gets no special treatment. Asserting this one would be inventing a
   requirement.
3. **No disclosure in substance.** All the message literals are static; none echoes caller
   input. The cursor is unsigned plaintext base64 of `mint:anchor:ctx_id`, so one `base64 -d`
   on any legitimately issued cursor reveals more about the format than the messages do.

**Two corrections to the original entry's body.** `ASSUMPTIONS.md` is append-only (board rule
D-005), so they are recorded here rather than edited in place:

- **The entry named the wrong file.** It says the fix would be "a one-line message change in
  `error.rs`". The message literals are not in `crates/acdp-registry-types/src/error.rs` at
  all — that file maps the *variant* to a wire code (`:190`) and status (`:210`). The strings
  live in the two store crates, duplicated byte-for-byte:
  `crates/acdp-registry-sqlite/src/store.rs:1740-1764` and
  `crates/acdp-registry-pg/src/store.rs:1644-1668` — **14 literals across two crates**, not one
  line in one. The scope conclusion (don't touch `src/`) was right; the reasoning behind it was
  asserted rather than verified.
- **The entry examined only the base64 arm.** Six others name internal cursor fields —
  `"cursor missing mint"`, `"cursor missing anchor"`, `"cursor missing ctx_id"`,
  `"cursor mint not int"`, `"cursor anchor not int"`, `"cursor anchor out of range"`. They
  disclose the cursor's field structure, which is more than the entry's analysis considered.
  It does not change the verdict — that structure is already recoverable from any issued
  cursor — but the entry overstated how narrow the behaviour was.

**One change applied**, in-scope, in `conformance.rs`: a self-invalidating **tripwire** now
pins the current behaviour. The doc note quotes a string literal owned by another crate with
nothing binding them, so tightening that message would leave the test green while the note
silently became a false claim that a gap still exists — a truthful file made to lie. That
hazard is already on the books one boundary out: `DECISIONS.md` entry 4's rule prefers a
citation to a quotation whenever the source is outside this repo. Same hazard, crate boundary
instead of repo boundary. The tripwire fails loudly and tells the reader to retire the note and
this entry rather than "fix" the code. No ratchet churn —
`EXPECTED_CUR_ASSERTION_COUNT` counts one increment per vector, not per assertion.

**Filed as #187, not fixed:** collapsing the 14 literals to a bare `"invalid cursor"` and
deduplicating the byte-identical cursor codec between the sqlite and pg stores. Genuinely outside this unit's
granted paths, and the test's own scope note already flags the duplication.

## W2-U1 — #185: hoisting the pinned-keys guard (2026-09-10)

**Decided by:** Opus (lane-1) during `/drive`, under leader direction that the behaviour
change is lane-decidable. Keyed by unit rather than a sequential integer per CHARTER 16 —
`main` already carries two `## 6.` from N lanes numbering against a shared base.

### W2-U1-a. Hoist the startup guard; do NOT make the runtime deny-all

The obvious alternative was to make `enforce_pinned_signature` reject when `pinned_keys` is
empty and `pinned_only` is true — fixing it where the danger actually lives. Rejected:
`crates/acdp-registry-types/src/config.rs` documents "Has no effect when `pinned_keys` is
empty" as the contract, and `PinOutcome::Skipped` is defined as "no policy active"
(`playground.rs`). Callers rely on both. Changing that is a semantic change to a shared
type's behaviour; refusing a self-contradictory config at startup is not.

**A justification used in the first draft and withdrawn:** that the startup bail makes the
state "unreachable through the real binary." **False.** `POST /admin/pinned-keys/reload`
(`handlers/admin.rs`) re-reads config and swaps `state.playground` with no validation at all,
and the publish path reads that live cell per request. The decision stands on the contract
argument alone; the reachability claim does not survive. Filed as **#192**.

### W2-U1-b. Bail A stays inside the receipts block

`playground.enabled && !pinned_only` is genuinely receipts-specific — an unverified playground
is incompatible with *advertising receipts*, not with running a registry. Hoisting it too
would refuse a legitimate standalone open playground, which is a declared, self-consistent
configuration and not what #185 is about.

### W2-U1-c. The error message names the fall-through and stops at two remedies

The message offers "add a pinned key" or "set `playground.enabled=false`", and deliberately
**omits a third exit that exists**: `pinned_only = false`. That would silence the error while
leaving the registry in exactly the unverified state the guard is trying to surface. An error
message that offers "or disable the check" undercuts the check. `docs/CONFIGURATION.md`
documents `pinned_only`'s full semantics for an operator who genuinely wants an open
playground; the startup error is not the place to advertise it.

The message says "every **non-`did:key`** agent", not "every agent". `did:key` publishes take
their own verified route (`handlers/context.rs`) *before* the playground gate. An unqualified
"any agent" would be false — and it is the same `did:key` nuance a review caught in U-005,
regressed here while drafting fresh prose and caught again. Recorded because the pattern is
the point: precision that exists in the text being replaced can be lost by rewriting it.

### W2-U1-d. Proof standard for a validator behaviour change: mutation, not coverage

A test asserting the new refusal would pass whether or not the hoist happened, if the guard
already fired for some other reason. So the standard applied was: run the new test against the
**unhoisted** guard and record the actual failure output; confirm the failure signature is the
right one (`expect_err` panicking on `()`, i.e. `validate_config` returned `Ok`) and not a
compile error; then re-mutate after the fix and confirm the new test fails while the
pre-existing receipts test stays green. That last step is what discriminates "pins the hoist"
from "pins the guard's existence". Full transcript in `PROGRESS.md`.

### W2-U1-e. Line-pins are reported, never re-pointed — and one was protected by edit shape

Consistent with the prior ruling on stale pins in historical records. Recomputed from the
final tree: bail A `:259` → `:282`, rationale `:249` → `:272`, public-bind `:475` → `:488`;
the pins at `:267`/`:271-273` have no surviving target at all. All reported, none re-pointed.

Separately, `DECISIONS.md`'s own citation of `config.rs:739-740` lands *inside* the doc
comment this unit rewrote. It survives because the new text was **appended after** the cited
sentence rather than inserted above it — a deliberate constraint carried from plan review into
the edit. Worth generalising: when an edit lands inside a cited range, the edit's *shape*
decides whether the citation survives, and that is cheaper than re-pointing afterwards.

## 11. W2-U2 — #187: one payload for every cursor parse failure (2026-09-10)

**Decided by:** Opus, under the lane's "approach choices are not escalations" rule. Recorded
here because it changes an observable wire string, and because it corrects entry 10.

**The decision.** Every `InvalidCursor` arm in the now-shared codec
(`crates/acdp-registry-store/src/cursor.rs`) carries one payload, the module-level constant
`CURSOR_MALFORMED = "malformed"`. `AcdpError::InvalidCursor` renders as
`#[error("invalid cursor: {0}")]`, so the wire message is exactly
**`invalid cursor: malformed`**. A bare `invalid cursor` was considered and is not
reachable without editing the `thiserror` attribute in `acdp-registry-types`, which is
outside this unit's granted paths and would change every other caller of that variant.

**Why one payload rather than a tidied set.** `cur-002`'s rationale asks a registry not to
"leak why a cursor failed to parse beyond the registered code". Any per-arm string describes
the cursor's internal field layout (`missing anchor`, `mint not int`), which is precisely
what the clause names. Nothing operational is lost: `error.code` still separates
`invalid_cursor` from `cursor_expired`, which is the distinction callers actually branch on,
and a client already holds the cursor it sent. The two codes were verified to stay distinct
by the conformance run, not assumed.

**Corrections to entry 10 — appended, per the entry 9c rule that pins are reported and never
re-pointed.**

1. **Entry 10's count is superseded.** It says **14 literals across two crates** and "six
   others" beyond the base64 arm. The tree had **8 arms per store, 16 total** — entry 10 and
   #187 both missed `"cursor is not utf-8"`. Of the 8, one (`"cursor missing mint"`) was
   **unreachable**: `splitn` always yields a first element, so its `ok_or_else` could never
   fire. It is deleted rather than collapsed, and the code now says so at the call site. So
   the true accounting is 16 literals → 1 constant, with one dead branch removed.

2. **Entry 10's two line-pins are DANGLING as of this commit, and are reported, not
   re-pointed.** `DECISIONS.md:1021-1022` cites
   `crates/acdp-registry-pg/src/store.rs:1644-1668` and
   `crates/acdp-registry-sqlite/src/store.rs:1740-1764`. This unit deleted exactly those
   ranges, so:
   - `pg/src/store.rs:1644-1668` is now **past end of file** — the file is 1636 lines.
   - `sqlite/src/store.rs:1740` now lands on `assert_eq!(fts5_escape("hello"), "\"hello\"");`,
     an unrelated full-text-search assertion.

   Both were verified by reading the files at this commit, not inferred from the diff. Entry
   10 is left byte-for-byte intact: the correct reading of it is "the literals that were at
   those lines when entry 10 was written", and the content-addressed replacement is the
   single `CURSOR_MALFORMED` constant named above. This is the failure mode CHARTER rule 10
   exists to surface, and it is a genuine argument against citing line ranges in an
   append-only file at all — a follow-up worth taking up separately from this unit.

## 12. W2-U2 — the shared cursor codec is plainly `pub`, not hidden (2026-09-10)

**Decided by:** Opus. The lane assignment named this call explicitly as mine and explicitly
NOT an escalation. Recorded because entry 5 sets the opposite-looking precedent and a future
reader would otherwise read the two as inconsistent.

**The apparent conflict.** Entry 5 narrowed `secure_compare::ct_eq` from `pub` to
`pub(crate)`. This unit makes `encode_cursor`/`decode_cursor` `pub`. Those look contradictory
and are not, for one material reason: **`ct_eq`'s callers were all inside
`acdp-registry-core`, so narrowing it compiled. This codec's callers are in two *different*
crates** — `acdp-registry-sqlite` and `acdp-registry-pg` — so `pub(crate)` in
`acdp-registry-store` does not compile at all. `pub` here is forced by the crate boundary,
not chosen over a narrower alternative. Deduplicating across crates and keeping the item
crate-private are mutually exclusive; #187 asked for the former.

**The live choice was therefore only whether to hide it**, via `#[doc(hidden)]` or a
deliberately-internal module name. Kept plainly `pub` and documented:

- Entry 5 already considered and rejected `#[doc(hidden)]` on a still-`pub` item as a middle
  option — it hides from docs but not from the type system, so it buys undiscoverability
  rather than encapsulation. That reasoning applies unchanged here, and adopting it now would
  be the real inconsistency with entry 5, not this.
- A cursor codec is a coherent thing for this crate to expose. `acdp-registry-store` is where
  the store contract lives; a third backend would need exactly this and should find it.
- The wire format is documented at the module level, which matters more than visibility: the
  format is what any future backend must agree on, and an undocumented-but-reachable item is
  how two implementations silently diverge — which is the very failure #187 exists to end.

**Blast radius, and when this stops being reversible.** `release-plz.toml:5` currently sets
`publish = false`, with the comment "flip to true once crates are ready for crates.io". So
today this is an internal workspace detail and narrowing it later costs one commit. **When
that flag flips, this becomes a public API commitment.** That is the moment to revisit — not
because the decision is wrong, but because its cost changes. Flagged here rather than left
for someone to discover at publish time.

**What would change my mind:** a second consumer appearing that wants a *different* cursor
format. Then the right shape is a trait with the codec behind it, not a free function — and
that is a bigger change than visibility.
## W2-U3 — release-plz un-stall, docker tag pipeline, Railway auth posture (2026-09-10)

Unit-scoped slug, matching the precedent lane-1 set with
`## W2-U1 — #185: hoisting the pinned-keys guard`. Decided by Opus under `/drive`; the
auth-posture item was ruled by the human and is recorded here as ratification, not as a
lane call.

> **On the `D-0xx` and "CHARTER rule N" identifiers used below.** They refer to a
> multi-session coordination board that lives *outside this repository* and is deliberately
> not in git, so they are **not resolvable from this tree**. Every one of them is therefore
> stated with its substance inline; the identifier is provenance, never the argument. This
> file already carried that convention before this entry (two such references predate it),
> but it is worth naming: a citation a reader cannot follow has to carry its own content.

### 1. The duplicate `## 6.` heading is disambiguated as `## 6-bis.`, not renumbered

`DECISIONS.md` carried two `## 6.` entries — `U-001 — predecessor_admission …` and
`WEBHOOK_SCHEMA_VERSION stays "1.0" …` — because two lanes numbered against the same base
with no lock. That is the collision CHARTER rule 16 was written for.

The `WEBHOOK_SCHEMA_VERSION` entry's heading becomes `## 6-bis.`. **A cascading renumber was
forbidden and would have been wrong anyway**: entries 7-10 are cited elsewhere, including
`ASSUMPTIONS.md`'s pointer to entry 10.

A unit slug (`## U-002 — …`) was considered for the renamed entry and **rejected**: that
entry's body carries no unit id, and neither does its neighbour, so assigning it to U-002
would invent provenance that cannot be verified from the tree — the exact failure CHARTER
rule 15 exists to prevent. `6-bis.` asserts only what is certainly true: a sibling of entry 6
from the same wave, sorting between 6 and 7. New entries, whose provenance *is* known, get
slugs.

### 2. `.gitignore` gains root-anchored `/PROGRESS.md` and `/.drive.lock`

The CHARTER rule-13 grant. Root-anchored on purpose: unanchored patterns would also hide a
future `crates/*/PROGRESS.md`, which the grant does not cover.

> **Tense warning.** Sections 3-6 record decisions taken for this unit and describe the
> state *after* all of its phases land. At the moment this entry was appended, only the two
> items above were in the tree. Where a statement below is a prediction rather than an
> observation, it says so explicitly.

### 3. release-plz is un-stalled with `git_only = true`, NOT by publishing to crates.io

Root cause established from evidence, not hypothesis. release-plz resolves "what was last
released" from the **cargo registry** even when `publish = false`; nothing is on crates.io;
every crate reads as never-released; it proposes the current version `0.1.0`; the existing
tag then blocks it. A self-sustaining deadlock. The DEBUG log names the path outright
(`Processing 8 packages from registry` -> `downloading packages from cargo registry crates.io`
-> `Package acdp-registry-types@*.*.* not found`), with zero git-tag lookups.

Flipping `publish = true` would have "fixed" it by starting to publish eight crates to
crates.io. **Rejected outright** as a one-way door and a cross-boundary publish, not a lane's
call. The constraint was standing before the analysis began, so no escalation was needed —
the fix below never approached it. `git_only = true` un-stalls versioning with no publish anywhere.

### 4. The dead `*-v0.1.0` tags are retired by changing `git_tag_name`, not by deleting them

`git_only` alone still fails: it makes release-plz `cargo package` the June tag's tree, which
cannot resolve because at that commit `acdp` was a **git** dependency, and across the surrounding
range it was a `path = "../acdp-rs"` dependency with a `[patch.crates-io]` — neither of which
`cargo package` can rebuild from a tarball. (The path-dependency period runs through
2026-07-05 and is not cleanly "after" the git-dependency one; both simply predate the current
crates.io dependency.) The baseline must therefore be
re-based at a modern commit. **Deleting the eight tags was rejected — it would orphan eight
published GitHub Releases.** Changing the tag template retires them non-destructively: they
simply stop matching.

Consequence, accepted knowingly — and this is a **prediction from a local simulation, not an
observation**: the first post-merge run is expected to mint new-shape tags and create eight
duplicate `0.1.0` GitHub Releases, moving the "Latest release" marker. The evidence is a
throwaway-clone run with synthetic tags planted 12 commits back, not a real CI run; a tag
event cannot be staged before merge. Cosmetic and
reversible by a human with `gh release delete <tag>` — which must **not** delete the tag.

### 5. This is a TWO-STEP bootstrap and the first post-merge run is deliberately PR-less

**Predicted, from the same local simulation.** The merge that lands this unit is expected to
mint the tags and produce no release PR. **The merge after it is the one that produces the
first release PR.** A green, PR-less bootstrap run is success, not failure. Recorded here because the
obvious misreading — "still broken" — is the one a future reader is most likely to make.

### 5-bis. release-plz runs as a GitHub App, with its permissions pinned down

Tags and PRs created with the default `GITHUB_TOKEN` **do not start workflow runs** — GitHub
suppresses them to prevent recursion, and release-plz documents this for `on: push: tags`
specifically. So the corrected `docker.yml` trigger would have been right and still never
fired, and CI would never have run on the release PR. release-plz therefore mints a GitHub App
installation token (the App this repo already runs in `notify-website.yml`, `bump-acdp.yml` and
`bump-spec.yml`) and uses that as its `GITHUB_TOKEN`.

`owner` and `repositories` are omitted so the token scopes to this repository. Copying
`notify-website.yml`'s shape would have been wrong twice over: it sets
`repositories: acdp-website` because it dispatches into a different repo, which would leave
release-plz with no permission here; and setting `owner` alone widens the token to every
repository in the installation.

**`permission-contents: write` and `permission-pull-requests: write` are load-bearing, not
hygiene.** An App token ignores the job's `permissions:` block and otherwise inherits every
permission the installation holds — for this App that includes `workflows: write`. Without
those two lines the change would have handed a third-party action, and the binary it
downloads, the ability to rewrite `.github/workflows/*` — authority the default token it
replaces never had. Scoping the repository is not scoping the grant; that distinction was
missed on the first pass and caught at the verification gate.

Consequences a future reader should know: the App's token lives exactly 60 minutes, which is
why the job is bounded at 45 rather than 60 — a 60-minute bound would fail on an opaque 401
instead of a clean timeout. `persist-credentials: false` on checkout keeps the default token
out of `.git/config`, so release-plz's `git push` fallback paths cannot silently re-acquire an
identity that cannot trigger workflows. And because the release PR is now authored by the App
rather than by `github-actions[bot]`, anything keyed on the old author — auto-merge rules,
CODEOWNERS review requirements — is keyed on the wrong identity. If tags are ever configured
to be GPG-signed, release-plz falls back to `git push` and this arrangement needs revisiting.

### 6. Only `docker/RAILWAY.md` turns authentication on; the compose stack does not

Human ruling (D-016/D-018 escalation). Railway and the shipped compose stack are different
deployment classes and only Railway was ruled on; `docker/config.docker.toml` and
`docker/docker-compose.yml` keep `auth.enabled = false` per D-016 item 1.

Enabling auth falsifies three statements the previous unit shipped, so all three move in the
same commit: the `JWT_SECRET` row's "never validated", the note's "This recipe leaves
authentication OFF", and the `ALLOW_PUBLIC_BIND` argument. The `ALLOW_PUBLIC_BIND` row is
**kept and re-documented, not dropped** — removing it would change the recipe a second time
beyond what was authorized.

## W3-U1 — #192/#193: refusing bad playground config at both doors (2026-09-10)

**Decided by:** Opus, under the lane assignment. Recorded because three of the four calls
below either depart from a written acceptance criterion or cut against an existing entry,
and a future reader would otherwise read them as drift.

### W3-U1-a. The shared validator lives in `acdp-registry-core`, not `acdp-registry-types`

The assignment named the structural constraint and it holds: `validate_config` is in
`crates/acdp-registry-server/src/main.rs`, and that crate is bin-only (`[[bin]]`, no
`[lib]`), so `acdp-registry-core` — where the reload handler lives — cannot call it. One
copy of the rules has to move somewhere both doors can see.

**This is the same question entry 5 answered the other way, and the difference is real.**
Entry 5 narrowed `ct_eq` to `pub(crate)` and explicitly rejected moving it to
`acdp-registry-types` as "speculative restructuring for a need that does not exist". The
need was absent there because every caller was inside one crate. Here two crates genuinely
both need the validation *today* — that is precisely the condition entry 5 said was
missing. Entry 12 already drew this same line for the cursor codec.

**But the destination is core, not types**, which is where this departs from both prior
entries. The rules being enforced are not properties of the `PlaygroundConfig` *shape* —
they are properties of what the *runtime* will accept, and the authority for that is
`PinnedAlgorithm::parse` and the two key decoders, all private to
`crates/acdp-registry-core/src/playground.rs`. Putting the validator in types would mean
restating the algorithm list and the byte-length rules a crate away from the code that
enforces them: exactly the drift this unit exists to close, reintroduced one layer up. The
binary already calls into core from `validate_config` in three places, so this extends an
established direction rather than opening one.

Consequence, stated deliberately: `acdp-registry-types` ends this unit with **zero diff**,
so nothing here becomes a semver commitment when `release-plz.toml`'s `publish` flag flips.
One new `pub` item in core, not a new public config API.

### W3-U1-b. An all-expired pinned-key list WARNS; it does not refuse — a departure from AC3

The assignment's AC3 says an unusable list "MUST" be refused at startup and that
"all entries expired" must count as unusable. **I refuse all five structural defects — the four
key-material ones and the impossible window — and I warn on the all-expired
list.** Flagged to the leader rather
than taken silently, and logged `UNCONFIRMED` in `ASSUMPTIONS.md`.

Two reasons, both about what the state actually *is* rather than what it looks like:

1. **Expiry is time-dependent, and the others are not.** A typo'd algorithm is wrong in
   every possible present. An all-expired list is a config that was valid yesterday and is
   identical today. Refusing it makes *bootability a function of the wall clock* — the same
   file starts a registry at 09:00 and refuses to start it at 09:01, and the failure lands
   on whoever happens to restart next, which is usually an unrelated incident. That turns a
   key-rotation lapse into a second outage during the first one.
2. **Under `pinned_only = false` the state is not even an error.** Lax-with-expired-pins is
   behaviourally identical to lax-with-no-pins, which is a supported configuration. Refusing
   it would refuse a state the registry otherwise permits.

The warning therefore carries the whole weight, and it **branches on `pinned_only`**,
because the two modes do opposite things: strict rejects every `did:web` publish; lax
**accepts them with no signature check**. A single message could only have been right about
one. See W3-U1-d.

**What would change my mind:** a strict-mode deployment that would rather fail to start than
run rejecting every publish. That is a legitimate preference and the right shape for it is a
config key (`playground.refuse_on_no_live_pin`), not a change to this default — because the
default has to be right for the lax case too, where refusing is plainly wrong.

### W3-U1-c. A rejected reload must leave the live cell untouched, and the test proves the cell

AC2 asked for the observable state, not the status code, and the distinction turned out to
be load-bearing rather than pedantic. Two mutations were run against the fix:

- Remove the rejection entirely → both refusal tests fail. Expected.
- **Swap first, validate after** → the status assertion still **passes** (the request is
  still rejected with `400`), and the test fails only at the cell assertion:
  `left: ["did:web:agents.test:mallory"]`, `right: ["did:web:agents.test:alice"]`.

The second mutation is the one that matters. A status-only test would have been **green**
against a handler that corrupts the running configuration on every rejection — a reload that
half-applies is strictly worse than one that fails, because the operator is told "rejected"
and left running the rejected config. Validation is therefore ordered before the write lock
is taken, not inside the critical section: the swap is not merely undone on failure, it is
never reached.

The reload validates against the **running** receipt posture
(`state.config.receipt.is_configured()`), not the freshly-loaded one, because `[receipt]` is
not hot-swappable — only the `[playground]` section is. Validating the new playground section
against a receipt setting that will not take effect until restart would refuse configs that
are correct for the process actually running.

### W3-U1-d. No error string is written until its branch is read

Adopted as a rule after the third occurrence of one failure. In `U-005` and again in `W2-U1`
I wrote prose that dropped precision the text it replaced already had (the `did:key` carve-
out: `did:key` identities are self-verifying and never reach the playground gate). The third
was in this unit's own first draft — a warning reading "Pinned agents will be rejected until
a key is rotated in", which is **false in the lax branch**, where they are accepted
unverified. That draft would have told an operator their registry was closed at the exact
moment it was silently open.

The rule is mechanical, because judgement is what failed: **every new error or warning
string is checked against the code path that emits it, by reading that path, before the
string is written.** Applied here to the doc examples too — the `400` body quoted in
`docs/HTTP-API.md` is the byte-exact string the handler produces, and the claim that a
request-time failure is opaque was checked against
`crates/acdp-registry-types/src/error.rs:125`, which redacts every `internal_error` to the
static message `"internal error"`.

### W3-U1-e. Exhaustiveness is enforced by the compiler, not by a test

The gap `#193` closed was a validator narrower than the hazard it was written for. The
obvious way to reintroduce it is to add a field to `PinnedAgentKey` tomorrow and not validate
it — and every by-example test in this unit would still pass, because none of them know the
new field exists.

`validate_playground_config` therefore destructures both `PlaygroundConfig` and
`PinnedAgentKey` **exhaustively and deliberately without `..`**, with each ignored field
bound explicitly and commented with where it *is* validated. Adding a field is then a compile
error in this function, not a silent coverage hole. Mutation-proven rather than asserted:
adding `pub key_usage: String` to `PinnedAgentKey` produced

```
error[E0027]: pattern does not mention field `key_usage`
  --> crates/acdp-registry-core/src/playground.rs:281:13
```

The code carries a comment saying not to "fix" this with `..`, since that is what a reader
tidying warnings would reach for first. Chosen over a self-inspecting test (the shape lane-3
used elsewhere) for this specific property: a test that enumerates fields can go vacuous
without anyone noticing, whereas this cannot compile wrong.

---

## W2-U3 addendum — the eight duplicate GitHub Releases are NOT accepted after all (2026-09-11)

**This supersedes the "Consequence, accepted knowingly" paragraph in this unit's section 4**,
which is where both the acceptance and the "eight GitHub Releases" prediction live. Those paragraphs are left in place —
`DECISIONS.md` is append-only — but they no longer describe what shipped, and a reader who stops
there gets the opposite of the truth.

**What changed.** Section 4 recorded, as accepted, that the bootstrap run would create eight
duplicate `0.1.0` GitHub Releases and move the repository's "Latest release" marker, on the
grounds that this is cosmetic and a human can reverse it with `gh release delete`. The
companion plan (`plans/w2-u3-release-ci-plumbing.md`, gitignored) went further and recorded that
toggling `git_release_enable` off and on across two merges **was rejected as the worse trade**,
because it needs two merges and risks the flag never being restored. That phrase is the plan's,
not this file's — an earlier draft of this entry misattributed it here.

**The human overruled that**, in full knowledge of the objection — the "flag never restored" risk
was put to them in writing, by me, as the reason not to do this. They chose the two-merge route
anyway. It is their repo. The decision stands and is implemented.

**Decided by:** the human, overruling both this lane's recommendation and the leader's.

**What shipped instead:**

- Merge A (PR #202) carries `git_release_enable = false`. The bootstrap run mints the eight
  new-shape tags and creates **no** Releases. Nothing touches the eight 2026-06-13 tags or their
  existing Releases, and the "Latest release" marker does not move.
- Merge B restores `git_release_enable = true` immediately after, and removes the `TEMPORARY`
  comment block with it.

**The load-bearing check that made this safe.** Merge A's entire purpose is minting the tags, so
the route collapses if the flag also suppresses tagging. It does not — verified against the
0.3.160 source (two independent `if` blocks in `create_git_tag_and_release`, distinct config
fields, one read site repo-wide) and by running `release --dry-run` both ways. Recorded in full
in `ASSUMPTIONS.md` under "Disabling GitHub Releases for the bootstrap merge does not disable git
tags", now **CONFIRMED**.

**The risk this decision knowingly takes, stated plainly so the record carries it.** If merge B
does not happen, GitHub Releases are disabled for this project indefinitely, and the failure is
silent: release-plz keeps reporting success and keeps minting tags. Four independent carriers
exist against that — issue #204, a `TEMPORARY` comment above the flag, a section at the top of
PR #202's body, and this entry. A fifth was considered and not built: a CI check that fails while
the flag is off. It is the only carrier that does not rely on a human reading something, and it
is the right long-term answer; it is not in scope for a lane under stand-down, and is recorded
here and on #204 as the recommended follow-up.

**Restore precondition — do not restore blind.** Merge B should land only once the eight
new-shape tags are confirmed present on `main`. If the bootstrap run fails to mint them, restoring
the flag means the *next* run mints tags **and** the eight duplicate Releases — the exact outcome
this route exists to prevent.

### W3-U5-a. The root cause was an inference, not a sentence

`U-005` was mine. It set out to correct two operator-facing claims about the `changeme`
placeholder, and it did correct them. It also introduced a new false claim — that
`docker compose up` with the shipped default "boots cleanly" — and shipped it in the very
entry announcing that the old claims were wrong.

What happened: I located the literal-`changeme` guard, verified its gating, found it correctly
nested inside `auth.enabled && jwt_signing_alg != "EdDSA" && !jwt_secret.is_empty()`, and
concluded from that verified fact that the *boot path* was gated on `auth.enabled`. It was not.
`serve_with_store` has always passed any non-empty `jwt_secret` to `JwtSecret::from_base64`,
which imposes a 32-byte floor, with no `auth.enabled` gate anywhere. `changeme` is valid base64
of six bytes, so the stack died on the LENGTH FLOOR — a door I never looked at — while I was
confirming the literal guard could not fire.

**The inference is the defect; the prose was only where it surfaced.**

This is the same shape as verifying a branch against `git diff origin/main` while `origin/main`
moves: *verified against A reference and concluded it was THE reference*. The check performed
was sound. The set it was performed over was incomplete, and nothing in the method would ever
have revealed that, because a passing check on a subset looks identical to a passing check on
the whole.

The rule adopted is therefore not "read more carefully". It is: **a claim about what a stack
DOES is backed by an observed boot with captured output, or it does not ship.** Every
behavioural sentence in this unit's diff traces to a transcript in `PROGRESS.md`. That is a
control rather than an intention, which is the distinction `CHARTER 29` draws — and which this
unit went on to test three more times before it was finished (`W3-U5-e`).

### W3-U5-b. Ship no secret, rather than a working one or a better error

Two directions were offered: (a) commit a real 32-byte base64 default so the quickstart works,
at the cost of a fixed secret in a tracked file; or (b) keep failing, but fail with a message
telling the operator what to generate. The leader leaned (a) and explicitly declined to rule it,
because neither of us had read `docker/docker-compose.yml` and `config/` for this question.

Reading them produced a third option that dominates both: **ship no secret at all**
(`${ACDP_REGISTRY_JWT_SECRET:-}`).

It works because of a fact neither (a) nor (b) accounted for: an EMPTY `jwt_secret` with
`auth.enabled = false` is already a supported, working configuration. The empty-secret check is
gated on `auth.enabled` and *stays* gated — the deliberate asymmetry now commented in `main.rs`
— while only a NON-EMPTY secret is examined unconditionally. `docker/config.docker.toml` ships
`auth.enabled = false`. So removing the placeholder did not require a replacement; it required
nothing.

Against (a): a 32-byte secret in a tracked file is a real if low hazard whose failure mode is
that it gets copied into something that is not a disposable demo, and the documentation calling
it throwaway does not travel with the value. Against (b): an honest error on a quickstart that
still does not run is a 100% failure rate for every new user. "Fail well" is right when no
working configuration exists; here one did.

The decisive evidence that this had gone unnoticed: `.github/workflows/docker.yml` sets no
`jwt_secret` at all. **CI was green because CI exercised a different stack than the one the repo
ships.** The compose placeholder was never on any tested path. Filed, not fixed — `.github/**`
is another unit's.

One hazard the choice does introduce, recorded rather than discovered later: compose renders the
variable as *set-to-empty* rather than absent, and an env var outranks the TOML file, so a
`jwt_secret` uncommented in `config.docker.toml` is silently discarded. With auth off that is
harmless and with auth on it fails loudly — except with `allow_ephemeral_secret = true`, where
it downgrades to a process-lifetime key after one startup `warn!` (`main.rs:838-844`) and no
further signal. Option (a) has the same property plus a secret
in git, so this does not change the ranking; it earns a caveat in the compose header.

### W3-U5-c. Four documents, one commit, because a truth-flip has no safe halfway point

`docker/docker-compose.yml`, `config/registry.example.toml`, `SECURITY.md` and
`docs/CONFIGURATION.md` all described the `jwt_secret` checks as gated on `auth.enabled`. Two
said so in the direction of false confidence ("starts cleanly with the placeholder", "NOT
validated in this stack"); `SECURITY.md` said so *inverted*, warning that a placeholder "can
survive there unnoticed" — asserting a weakness that could not exist, since a placeholder stops
the stack from booting.

They land in one commit. The leader's ruling, quoted: *"Splitting those into two PRs creates a
window where the repo's security document describes a hazard the repo has just removed. One unit
makes that window impossible."*

The general rule promoted from it — **a doc your change falsifies moves in your commit; a doc
that is already wrong is someone's backlog item** — also decided what this unit did NOT touch.
`docs/OPERATIONS.md`, `docs/AUTHENTICATION.md` and `docker/RAILWAY.md` carry the same false
gating claim. They are named in this lane's `done` with verbatim quotes and content anchors, and
left to their owners. Being able to see a defect is not a claim on it.

### W3-U5-d. The corrected changelog entry is appended, and the brief's premise about it was wrong

`AC3` required appending rather than rewriting, and said that if append-only should bend, that is
a `blocked` and not a judgement call. It did not need to bend.

One premise correction, derived from the artefact rather than relayed: the brief called the false
sentence **released** and pinned it at `CHANGELOG.md:2637`. Neither holds. `CHANGELOG.md`
contains exactly one `##` header — `## [Unreleased]` — so nothing in the file sits in a released
version section, and the sentence was at `:2686`, moving to `:2727` under a mid-unit rebase.
Calling it "released" would have been this unit repeating the error it exists to fix. Appended
either way; `D-008` keeps history pins stale on purpose, and zero deleted lines against the
merge-base is verified mechanically before shipping.

The correction also retracts a second sentence from that entry that nobody had flagged: its
explicit audit instruction to operators — *"if you relied on that documented check with auth
disabled, it never ran"* — is false. The check that actually stopped the boot, the ≥32-byte
floor, always ran with auth disabled. A wrong instruction aimed at operators outranks a wrong
description aimed at readers, so it is corrected by name.

### W3-U5-e. Three more instances of the same error, inside the unit written to fix it

Every one was caught by a fresh-Opus gate and none by me, which is the whole argument for
`CHARTER 29`: a rule you write for yourself is not a control until something external checks it.

**One.** Phase 1's first gate found three comments asserting the Phase-2 world as present fact —
that an auth-off stack with no secret "is what the docker-compose quickstart ships", at a commit
where the quickstart still shipped `changeme`. True one commit later; false where it stood.
Closed by deleting the cross-references rather than forward-tensing them, so the commit that
makes them true is the commit that adds them.

**Two.** Phase 1's second gate found I had declined to write a test on the grounds that it was
not writable inside my granted paths — and that this was simply false. `main.rs` already
imported `build_router` and `AppStateInner`, `tower` was already a regular dependency,
`SqliteStore::connect_in_memory()` ships under the default feature. **I asserted infeasibility
instead of checking it**, and it pointed in the direction that saved me work.

**Three, and this is the one that matters.** Phase 2's gate found that
`docs/CONFIGURATION.md` claimed the EdDSA `jwt_private_key_pem` check "still requires auth to be
enabled". True of `validate_config`. False of the binary: auth off + EdDSA + an empty PEM still
refuses to boot, from the serve path, after migrations. My evidence for that sentence was four
`validate_config` line cites and nothing else — **the serve path was never consulted.** That is
`W3-U5-a`'s error, with the same two code paths, committed inside the commit that fixes it. The
same gate found a second instance in `SECURITY.md`, where dropping the HS256 qualifier the old
text carried made "a placeholder cannot survive unnoticed" false under EdDSA — the algorithm the
next bullet recommends.

**A process defect worth more than any of them.** Phase 1's gate reported that the worktree
changed under it mid-run: I was making Phase 2 edits while it read. Its results survived by luck
— no test it ran touches those files — not by design. The rule is **no worktree edits while a
verification gate is running**; I broke it once more within the same phase and disclosed the
advancing HEAD to the running phase-2 gate in flight rather than letting it surface as a
confusing finding.

The pattern across all of them: the failure is never the sentence. It is a check performed over
a set smaller than the claim, by someone who did not notice the difference. Three fresh gates
caught what three careful readings by the author did not, and the cost of each catch was one
commit instead of one release.

## H-B — storage parity & correctness (unit H-B, lane-2, reconciled 2026-09-12)

Ten `UNCONFIRMED` entries from `plans/h-b-storage-parity.md`. All five PRs (#228, #229, #231,
#232, #233) were already merged when this ran, so nothing here gated a release; the pass
settles the record.

**Method deviation, recorded not waived.** `/reconcile` §3 calls for a fresh Opus subagent per
entry (and Fable for one-way doors). This session is instructed not to spawn agents, so the
analyses ran in-context. What that costs is analyst independence — a self-analysis cannot catch
an error rooted in a misreading still held — so every entry below is settled on a **measurement
or a code fact**, not on a re-reading of my own prose, and the one entry that turns on product
judgement rather than fact is escalated rather than settled.

### Settled by Opus — 9 entries

1. **`ACDP_REQUIRE_PG` inherits `is_ok()` semantics (`=0` enables require-mode).**
   **CONFIRMED.** Matches `ACDP_REQUIRE_CONFORMANCE` (`conformance.rs:2704-2708`) byte-for-byte
   and carries that gate's own "do not improve this to a truthiness check" comment. Two
   require-flags in one repo disagreeing about `=0` is a worse trap than one being surprising.
   Reversal is one line.

2. **Phase 1 gates 23 of 34 pg tests.** **RESOLVED — the entry was stale.** lane-3's #227
   applied the same helper to `crates/acdp-registry-server/tests/pg_integration.rs`. Verified in
   the tree rather than taken on report: that file now has `require_pg()` present, 11
   `#[tokio::test]` and 11 gated call sites; `acdp-registry-pg/tests/store_contract.rs` has 23
   and 23. **Gating is 34 of 34.** The caveat stated in #228's body is closed, and it closed
   without either lane editing the other's file — the shape went through the coordinator.

3. **`unixepoch(…, 'subsec')` rather than a canonical stored column.** **CONFIRMED, with the
   upgrade path recorded rather than taken.** Query-side only, so no stored bytes change —
   which matters more than it first appears: `body_json`'s exact bytes are the `content_hash`
   preimage, so normalizing them would break signature verification. The `data_period` filter
   remains a scan; a generated canonical column plus an index is the upgrade, deferred because
   no volume has been measured. Revisit when search latency is actually observed, not before.

4. **B2 split out of Phase 2.** **RESOLVED — stale.** The split is complete; B2 shipped in
   #231. Its open question was which semantics to adopt, which entry 5 records.

5. **The stopword table is verified against Postgres rather than trusted.** **CONFIRMED.** The
   pg suite asserts every one of the 127 entries is still a stopword per the live server, with a
   negative control so it cannot pass vacuously. This is the entry most worth keeping: a
   hand-copied table whose staleness nobody notices is the failure mode, and the check converts
   it into a verified one. The uncheckable direction — Postgres *gaining* a stopword this list
   lacks, unenumerable from SQL — is documented on the constant.

6. **B3 fixed by reconciliation, not a read snapshot.** **CONFIRMED.** Two preconditions were
   verified in the code before relying on the cheaper fix, and the plan had rejected this
   approach on the second: `lifecycle_events` has no `DELETE` in either backend (append-only),
   and the event and denormalized flag are written in one transaction. So the pruning hazard the
   plan feared does not exist here. Reconciliation also beats a snapshot on shape — it makes the
   served pair self-consistent by construction, so a future read path that forgets a transaction
   cannot reintroduce the contradiction, and it is one shared helper rather than two
   hand-mirrored per-backend transactions (Postgres would have needed `REPEATABLE READ`
   explicitly, since `READ COMMITTED` re-snapshots per statement).

7. **B5's busy timeout is a constant, and has no behavioural test.** **CONFIRMED as an accepted,
   documented gap — recommendation: do NOT add a test.** This is the only unguarded change in
   the unit and it is not being quietly confirmed away. Reddening it means holding
   `BEGIN IMMEDIATE` across a slow callback and racing a second writer against a wall clock;
   that test is timing-dependent, and a flaky guard gets deleted, which leaves a worse record
   than an honest gap. The change configures an existing mechanism explicitly instead of
   inheriting sqlx's implicit 5s. Making it tunable needs a field in `acdp-registry-types`,
   outside this unit's path scope — filed as a follow-up rather than smuggled in.

8. **B7 is a parity fix, not a live exploit closed.** **CONFIRMED, and it drops out of the
   critical tier on measurement.** The schema change is a widening, so no value can fail to fit
   and nothing needed migrating. Whether it is a one-way door was measured, not argued: `MAX(version)`
   in the live database is **2** and **zero** rows exceed `i32::MAX`, so `BIGINT` → `INTEGER`
   would succeed today. It is therefore reversible in practice, at the cost of another table
   rewrite, and is settled here rather than escalated.
   **Two corrections stand in the record, one of them mine.** The finding called the narrowing
   unreachable "because `put()` has no production callers" — false, the casts were in
   `commit_publish` and the row INSERT. I then concluded it was reachable — **also false**:
   measured, a publish carrying `version = 3_000_000_000` is refused on both backends because
   the request builder requires `version == 1` for a first publish and `prev + 1` after.
   Reaching 2^31 needs ~2 billion sequential supersessions.

9. **`lineages` is write-only and was deliberately NOT dropped.** **CLOSED as a finding handed
   onward, not a decision implemented.** Verified exhaustively: `INSERT` only, and zero
   `SELECT`/`JOIN`/`UPDATE` anywhere in the repository — src, tests and migrations swept, not
   just the two store files. Two independent reasons not to act: dropping a table is a one-way
   door this unit had no mandate for, and `crates/acdp-registry-server/tests/pg_integration.rs`
   TRUNCATEs it, so removal requires an edit outside this unit's path scope. The evidence is
   handed to the coordinator as a standalone decision. Cost of inaction, measured: one extra row
   INSERT per publish, inside a transaction that already writes several.

### Escalated to the human — 1 entry

10. **`q=` semantics: Postgres wins; SQLite raised to it via `porter` + a verified stopword
    list.** **LEFT UNCONFIRMED pending the human.** Not because it is irreversible — the FTS
    index is derived data and the stopword filter is one query-side line, so reverting restores
    the prior behaviour exactly — but because the choice is a **product judgement** on a public
    API, and I decided it against a defensible alternative on grounds that are the human's
    domain more than mine.
    - **Taken:** SQLite adopts Postgres. Keeps stemming on the production backend. Parity is
      pinned per-mechanism, because FTS5 `porter` and snowball `english` are different
      implementations and will not agree on every word.
    - **Rejected:** Postgres adopts SQLite (`simple` config). Would have given **exact
      structural** parity — literal token matching both sides, no word list, no stemmer mismatch
      possible — at the cost of removing stemming from the backend real users search.
    - **My recommendation: confirm as taken.** Paying in production search quality to buy a
      cleaner testing property is the wrong trade, and the residual gap is documented rather
      than hidden. But the reverse is arguable and the human should get the choice.
    - **User-visible effect already shipped:** on SQLite, inflected queries now match
      (`q=running` finds "run report") and stopword-only queries now match nothing. Postgres
      behaviour is unchanged. No documentation was falsified — `docs/` describes no `q=`
      behaviour and makes no backend-equivalence claim (checked).

## 13. Version strings in docs: placeholder vs literal, decided per site (H-D / D3)

**Context.** `docs/HTTP-API.md`'s build-identity table and prose pinned the package version at
`0.1.0`, stale since `0.1.2`. The first fix replaced the stale literals with current ones. That
was wrong in a way worth recording: the divergence list this unit is working through is made
almost entirely of literals that were correct when written. Replacing a stale literal with a
fresh one schedules the same defect for the next release.

**Decision.** Prefer *shape* over *literal* for fact claims and example payloads; keep literals
where the literal IS the content. Judge per site, never per file.

Applied:

| site | kind | outcome |
|---|---|---|
| `HTTP-API.md` build table, both rows | illustrative | `<version>` placeholder, **both cells** |
| `HTTP-API.md` "currently a placeholder 0.1.0…" | **fact claim**, false at 0.1.2 | rewritten to name no version at all |
| `HTTP-API.md` `/admin/status` example | illustrative | version placeholdered; **`+g83de685c2f26` and `"commit"` left byte-identical** |
| `README.md` `/healthz` example | illustrative | `<version>` |
| `HTTP-API.md` "absent `acdp_version` is treated as `0.1.0`" | **protocol floor** | **UNTOUCHED** |
| `README.md` "v0.1.0 through v0.5.0" | **protocol range** | **UNTOUCHED** |

**Why not literals in the table.** Its job is to show that two builds of one release share a
version and are told apart only by `+g<shortsha>`. With `0.1.0` in both cells the reader must
*notice* the two numbers are equal; with `<version>` in both cells they are equal by
construction. That only holds if the doc says so, so it now asserts it in bold directly beneath
the table.

**Revert invariant.** If a future reader wants real numbers back, the property to preserve is
that *both cells show the same value*. A partial revert — one row literal, one row placeholder —
is worse than either consistent state, because it silently destroys the contrast the table
exists to teach.

**The decoys are the finding.** Five `0.1.0` hits in `HTTP-API.md`; only three were targets. The
protocol-version floor at "absent `acdp_version` is treated as `0.1.0`" is a deliberate rejection
threshold for old payloads — a mechanical sweep would have bumped it and shipped a **wire-behaviour
change disguised as a docs cleanup**. Re-verifying each site individually is what this unit's
method buys, and this is the case that pays for it.

## 14. ARCHITECTURE's dependency diagram: replaced with a verifiable edge list (H-D / D3)

**Context.** The hand-drawn box diagram asserted two dependency edges that do not exist:
`-store → -auth` and `-sqlite → -webhook`. Both crates depend on `acdp-registry-types` alone.
Confirmed by `cargo metadata --no-deps` and by the absence of those entries in each
`Cargo.toml`. The prose crate-map table further down was correct throughout, so the two
representations had been contradicting each other.

**Decision.** Replace the drawing with a textual edge list, plus the one-line `cargo metadata`
command that regenerates it — and run that command to confirm it reproduces the documented
edges verbatim before shipping.

**Reasoning.** The diagram was wrong *because* it was a drawing: nothing could check it, so it
drifted silently while the table beside it stayed right. A representation that a single command
can verify is the only form that does not rot, and it is the same pattern used elsewhere in this
unit (a CI step that boots the shipped example; mutations that must redden a named test).
Prettiness is not worth an unverifiable claim about how the workspace is wired.

**Rejected:** redrawing the boxes correctly. It would have been correct on the day and
unverifiable forever after — the exact property that produced the defect.

## 15. The root `CHANGELOG.md`: retired to a pointer, not split by version (H-D / D4, #220)

**Context.** 3,596 lines, one `## ` heading — `## [Unreleased]` — while `0.1.0`, `0.1.1` and
`0.1.2` have all shipped. The file therefore asserted that nothing in it had been released,
which was false for roughly 90% of its content. Its own header also claims the file "follows
Keep a Changelog"; a single perpetual Unreleased section does not.

**What the evidence actually showed.** Attribution is derivable, not a matter of opinion:
`git blame` each line, then bucket its commit by the earliest release tag containing it.

```sh
git blame --line-porcelain -- CHANGELOG.md | awk '/^[0-9a-f]{40} /{print $1}'
# then, per commit: git merge-base --is-ancestor <commit> acdp-registry-server/v0.1.<n>
```

That yields 2,938 lines from `0.1.0`, 324 from `0.1.1`, 334 unreleased, and **zero** from
`0.1.2` — consistent with the file being 2,939 lines at the `v0.1.0` tag and 3,264 at both the
`v0.1.1` and `v0.1.2` tags.

**Decision.** Retire the root file to a pointer at the authoritative records, and move the
narrative to `docs/ENGINEERING-LOG.md` unchanged. Do **not** split it into version sections.

**Reasoning — why splitting was rejected, and it is not the effort.** The `0.1.1` content is not
appended, it is *interleaved*: seven separate runs inside the `0.1.0` body, because entries were
inserted under pre-existing `### Category` headings rather than prepended wholesale. Two of those
runs make a version split ill-defined rather than merely laborious:

- Lines 2974–2979 are a `0.1.1` amendment written **inside a `0.1.0` paragraph**. There is no
  assignment of those six lines to exactly one version section that leaves the paragraph intact.
  The acceptance constraint for a split — every non-blank line lands in exactly one section — is
  unsatisfiable there without rewriting prose that documents already-released behaviour, i.e.
  editing the historical record to fit the format.
- Line 3552 opens `<!-- W3-U5 (lane-1) — correcting the U-005 entry above -->`. The file contains
  deliberate cross-version corrections. Splitting by version tears each correction away from what
  it corrects, making the record *less* accurate, not more.

**Reasoning — why a pointer is sufficient.** Release truth already has an authoritative home and
it is in good order: `release-plz` owns the eight per-crate `CHANGELOG.md` files
(`[workspace] changelog_update = true`), each of which carries correctly dated and linked
`0.1.0`/`0.1.1`/`0.1.2` sections, and the GitHub Releases mirror them. Verified: all eight have a
current `## [0.1.2]` section. The root file was duplicating that job badly and was the only
artefact stating the falsehood. Removing the duplicate removes the contradiction.

**Rule 48.** Neither the pointer nor the narrative restates a fact by hand. The per-release
attribution is not written down at all — the command that derives it is, because a number written
into prose here would be stale at the next release, which is precisely the defect being fixed.
`root_changelog_stays_a_pointer` enforces the outcome: the root file must not reacquire a version
heading, must keep pointing at both authoritative sources, and every crate must still carry a
section for the current workspace version.

**Convention change, and it affects other lanes.** New entries go to `docs/ENGINEERING-LOG.md`,
not to `CHANGELOG.md`. Relayed to the leader as an `fyi` so it reaches lane-1 and lane-2 at their
next phase boundary rather than as a merge conflict.

**Rejected:** leaving the narrative in place under a disclaimer. The file would still be named
`CHANGELOG.md`, still be read as the changelog by anyone arriving at the repo root, and still
fail the format its own header claims. A disclaimer that contradicts the filename is the same
class of defect as the diagram in decision 14 — correct text that the surrounding artefact
undermines.
## H-H — tenant-aware search at the SQL layer (unit H-H, lane-2, reconciled 2026-09-12)

Seven entries tagged `plans/h-h-tenant-aware-search.md`, reconciled **before** the PR rather
than after it — `/drive`'s ordering for a one-PR feature, so code cannot ship carrying an
unresolved one-way-door assumption.

**Tier: the critical tier is empty, and that is a finding rather than a convenience.** No entry
changes a schema, runs a migration, alters a public HTTP contract, or adds a dependency to the
shipped artifact (`tokio` is dev-only). Every one is reversible in a commit. So none went to
Fable and none needs the human — but the three security-shaped entries (1, 2, 6) were analyzed
on evidence rather than confirmed by re-reading, and one of them made me reopen a design
alternative I had not logged.

**Method deviation, same as unit H-B:** `/reconcile` asks for a fresh subagent per entry and
this session is instructed not to spawn agents, so the analyses ran in-context. What that costs
is analyst independence, which is why each entry below rests on a code fact or a banked
measurement, and why entry 1 records a residual risk instead of claiming none.

### 1. `search_in_tenant`'s default treats the backend as untenanted — CONFIRMED (Opus)

The question worth asking was not "does it work" but "can a tenant-recording backend reach this
default and silently disclose?" Reconciling it surfaced a **fourth option I had not logged**:
fail closed for *every* `Some`, including `RESERVED_TENANT`. That is strictly safer for an
unknown future backend — it can never over-return — and `reject_reserved_tenant` means the
`Some("default")` case is unreachable from HTTP anyway, so the "wrongness" would never be
observed in production.

**Rejected, on the ground that decided the original design too:** it would make the memory
backend answer `Some(RESERVED_TENANT)` with an empty page while both SQL backends answer it with
the untenanted bucket (`WHERE tenant_id = 'default'`). That is a cross-backend divergence in the
trait's own contract — precisely the defect class unit H-B existed to remove, reintroduced in the
name of safety. One contract, one answer: `Some(t)` means the rows whose tenant is `t`, on every
backend.

**Residual risk, recorded rather than argued away:** a *future* tenant-recording backend that
neither overrides the method nor runs the parity suite would inherit the default and
over-return. That risk is inherent to any defaulted trait method and cannot be closed by the
type system. It is mitigated twice: the doc comment states the override obligation in the same
words `tenant_of_ctx` uses, and the parity suite **catches a missing override** — verified in
finalization by deleting SQLite's override, which reddened 2 guarantees rather than passing.

### 2. The store does not re-enforce the reserved-tenant rejection — CONFIRMED (Opus)

Logged with the premise "cannot arrive from the HTTP path". That premise is now **verified
rather than assumed**: the search handler resolves tenancy through `tenant_for_request`
(`crates/acdp-registry-core/src/handlers/context.rs:939`), which calls
`reject_reserved_tenant` (`:123` and `:180`); `tenant_for_publish` does the same at `:220`/`:264`.

Defence in depth was weighed and rejected for a reason stronger than layering purity:
`list_contexts` already applies its predicate for **any** `Some`, including `RESERVED_TENANT`.
Adding a rejection to `search_in_tenant` alone would make the two sibling methods disagree about
the same input — a divergence inside one trait. Adding it to both would change the behaviour of
a shipped method, and arguably wrongly, since an admin listing may legitimately want the
untenanted bucket. The rule keeps one enforcement point.

### 3. `tokio` as an unconditional dev-dependency — CONFIRMED (Opus)

Dev-dependencies never reach a downstream build, the workspace already pins
`features = ["full"]`, and the alternative (gating the tests behind `test-support`) would have
meant the default impl's guards do **not** run in a plain `cargo test` — the one place they
matter most.

### 4. No new index for the tenant-scoped search path — CONFIRMED (Opus)

On the banked measurements, not on argument. SQLite over 2000 rows / 20 tenants after `ANALYZE`:
`SEARCH ... USING INDEX idx_ctx_tenant` against `SCAN contexts` on the tenant-spanning path.
Postgres chooses **per selectivity** — `idx_ctx_tenant` for a selective tenant (2 of 506 rows, 4
buffers), `idx_ctx_created` plus a filter for `default` (367 of 506) — which is the planner being
right, not a gap. The `ORDER BY` temp sort is pre-existing on both paths because
`COUNT(*) OVER ()` must materialize the matching set, so no index can remove it. The assign
predicted `idx_ctx_tenant_created` would be the index used; it is not, and a third index would
be write cost for no measured gain.

### 5. Fixture isolates by unique tenant name, not cleanup — CONFIRMED (Opus)

This one earned its entry by being a bug I shipped into phase 2 and caught in phase 3: an exact
count assertion against a **persistent** Postgres accumulated 3+3+3 rows across runs. Cleanup
was rejected because a failing assertion skips it and poisons the next run — how a flake becomes
permanent. Asserting `>=` was rejected because it would stop detecting the count oracle, which
is the A2 finding. Verified by three consecutive green pg runs **and** a re-falsification
confirming the isolation had not neutered the guard.

### 6. `cursor.rs`'s claim is per-dimension, not restored — CONFIRMED (Opus)

The risk to weigh was whether per-dimension wording could mislead the very reader it targets — a
future author deciding where to put a filter. It does not, because it does not stop at
enumerating dimensions: it states the general rule (*a cursor discloses nothing beyond what the
scan that produced it was allowed to see*) and the consequence (*any future filter that must not
leak positions belongs in the query, not in Rust*). Restoring the original sentence was rejected
as the worse outcome — it would be false for the deployed path, and a subtly-false comment reads
as verified where a known-false one is at least discoverable.

### 7. Per-assertion falsification via accumulation — CONFIRMED (Opus)

CHARTER rules 51/52, applied to this unit's own guards and finding real gaps: only 2 of phase
1's 4 assertions had ever failed, and `matches.is_empty()` was **structurally unfalsifiable**
against an empty sentinel. Unit tests were split one-per-guarantee; the shared parity assertion
accumulates instead, because splitting it would multiply the per-backend caller boilerplate its
own module docs warn against. Result: 6/6 guarantees fire on both backends, and the two-mutation
contrast is now the unit's clearest evidence — the deployed shape leaves guarantee (a) green and
reddens (b), page clean and cursor leaking.

**Summary: 7 confirmed, 0 changed, 0 deferred, 7 settled by Opus, 0 needing the human. No code
follow-up blocks the ship.** The unit still ships PARTIAL by design — that is scope, not an
unresolved assumption.

## Unit H-I-s — batched retrieval visibility for `/log/entries` (lane-2, 2026-09-12)

Six `UNCONFIRMED` entries from `plans/h-i-s-batched-visibility.md`, ranked by blast radius.

**Tiering, stated up front:** none of the six is a genuine one-way door. No schema change, no
migration, no public HTTP contract change, no auth-model change; the new method is on a
**crate-local** trait, is defaulted, and has **no callers at all** until H-I-w wires the handler.
Every entry is reversible in a commit, so all six are Opus-tier and **none needs the human**. Two of
them are security-*shaped* and were given the most scrutiny anyway, because "reversible" is not
"unimportant" — a disclosure boundary that moves silently is cheap to revert and expensive to
notice.

**Deviation from `/reconcile`'s method, declared:** it asks for a fresh subagent per entry. This
session is instructed not to spawn agents, so the analyses ran in-context. Compensation: every
entry below is settled against evidence produced *this run* — a `file:line`, a measurement, or a
named mutation — rather than against recollection, and the two entries that could have been waved
through are the two that exist specifically to stop me overstating the result.

### 1. The §4.5 rule is expressed a third time, in Rust — CONFIRMED (Opus)

Highest blast radius in the unit: three expressions of one disclosure rule, and a drift between them
moves a security boundary with no error. Examined hardest, and the finding is that the duplication
is **forced rather than chosen** — `RegistryServer::retrieve` is `store.get()` + `can_retrieve`, and
`can_retrieve` is `pub(crate)` in `acdp-server-0.13.1` (`src/registry/server.rs:1257`), while
`RegistryStore` — the trait this crate can actually reach — exposes only a raw `get`. There is no
call available to make.

Rejected: have the SQL backends call `retrieve_visible` per row (that is precisely the N+1 this unit
removes); vendor or fork upstream (disproportionate, and it is an upstream change).

Confirmed because the containment is a **test, not a comment**: `DefaultOnly` wraps a real backend
and declines to override the method, so the Rust default answers for the same rows the SQL override
just answered for, and mutating the default *alone* reddens both backends' suites (E1, E2 — verified
`ran==1`, matched by name, on sqlite and pg).

**Follow-up, recorded and deliberately NOT actioned:** the better long-term fix is for upstream to
make `can_retrieve` `pub`, which would collapse three copies to one. Upstream is
`github.com/agentcontextdistributionprotocol/acdp-rs`. An issue there is a tracked ask rather than a
cross-repo write, but it is still an outward-facing artifact in another repository and this unit
ships correctly without it, so it is flagged for a human to authorize rather than filed
unprompted. It blocks nothing.

### 2. The default impl is behaviour-preserving (N calls), not fail-closed — CONFIRMED (Opus)

The method had to be defaulted — `ExtendedRegistryStore` has three implementors and one is
`MemoryStore` in `crates/acdp-registry-server/`, outside this lane's claim, so a required method is
a compile break in a file this unit may not edit. That makes what the default *does* a security
decision rather than a formality.

Settled in H-H and **not re-derived**: fail-closed makes an untenanted backend disagree with both
SQL backends about rows it can see perfectly well — the H-B divergence defect reintroduced in the
name of safety. `Err(NotImplemented)` turns a working backend into a 500 on a read path. The chosen
failure mode is **cost**, which is observable, over **silence**, which is not; and the cost is
measured rather than asserted (991µs batched vs 21.9ms per-id over a 256-entry page on SQLite).

Specifically checked, because it is the question that matters: can a tenant-recording backend reach
this default and silently mis-answer? No — both SQL backends override, and deleting either override
reddens its parity suite.

### 3. Groups (a) and (e) share `retrieve_visible` — CONFIRMED (Opus)

A coverage *limitation*, logged so it is not mistaken for coverage. The N-call reference and the
trait default both apply `retrieve_visible`, so a mutation of that function moves both together and
the differential cannot detect an error in the Rust expression of the rule — only a SQL-vs-Rust
disagreement.

Confirmed because the gap is covered elsewhere and the compensations are named: the 13 unit
mutations pin `retrieve_visible` to §4.5 absolutely, and three **absolute** assertions inside the
parity suite (audience-sees-private, contributor-does-not, retracted-still-visible) do not route
through the reference at all. A pure differential passes when both sides are wrong the same way;
those three are why this suite does not.

### 4. AC1 is satisfied transitively, not by direct comparison — CONFIRMED (Opus)

The assign asked for a test proving both backends return identical sets for the same fixture. What
exists is not a direct two-backend comparison and **cannot be**: `parity.rs` deliberately does not
depend on either backend crate — they depend on it — because importing both would invert the
dependency graph, as its own module docs state. Both backends are compared against the same third
implementation, so `sqlite == reference` and `pg == reference` yields `sqlite == pg`.

Confirmed as a *reporting* decision rather than a code one. The alternative (a new test crate
depending on both backends) buys a literal side-by-side comparison and nothing else. The risk this
entry exists to kill is an AC recorded as "met" when the test performed was a different, equivalent
one — an unverified claim hiding in supporting detail.

### 5. `visible_ctx_ids` returns a set, not a mask — CONFIRMED (Opus)

`HashSet<String>`: order-free, duplicate-safe, reads correctly at the call site. A `Vec<bool>`
parallel to the input silently mis-associates if any caller reorders, filters or de-duplicates
between building the slice and reading the mask, and no type would catch it. Reversible as a compile
error, not a silent behaviour change; crate-local and unreleased.

### 6. SQLite chunks at 900 ids; Postgres does not chunk — CONFIRMED (Opus)

SQLite's `IN (?,?,…)` meets a default 999-parameter ceiling, leaving headroom for five disclosure
binds plus the tenant bind; Postgres binds the list as one array and has no ceiling. Chunking is
kept even though the caller's page cap is 256, because `LOG_ENTRIES_PAGE_CAP` bounds the *handler*,
not this public method — relying on it would be a correctness bug waiting for a second caller. An
oversized chunk would fail loudly as a SQL error, not silently. The asymmetry between backends is
documented rather than smoothed over.

**Summary: 6 confirmed, 0 changed, 0 deferred, 6 settled by Opus, 0 needing the human. No code
follow-up blocks the ship.** One optional upstream follow-up is recorded above and flagged for human
authorization; it blocks nothing. The unit ships **PARTIAL by design** — nothing calls
`visible_ctx_ids` until H-I-w — which is scope, not an unresolved assumption.


## 16. This repo may mint a §5 wire code when the canon lacks an honest one (H-A / P7, #245, acdp-rs#268)

**Standing precedent, decided by the project owner. Anyone minting a second one should find this
entry first.**

**The rule.** `acdp-registry-rs` may emit an `error.code` outside the canonical RFC-ACDP-0007 §5
registry **when, and only when:**

1. the canon has no code that is *honest* for the condition — not merely none that is convenient;
2. the new name follows the canon's own idiom (here `unsupported_*`, as in
   `unsupported_algorithm`); and
3. an issue is filed upstream asking the canon to adopt it, so the divergence has a closing path
   instead of becoming permanent.

**The case that set it.** Enveloping the `415` from a missing or wrong `Content-Type` requires a
code, because `WireErrorBody::code` is a required `String` — there is no envelope-without-a-code.
`AcdpError::from_wire_error` recognises 25 codes and none describes a media-type failure. Before
this, every one of the 24 codes this repo emitted was inside that set; the set difference was
empty.

**Why not the nearest canonical code.** `schema_violation` is documented as "malformed body,
missing field, schema mismatch". On a `415` the body was never parsed — the media type was
rejected first. Using it would state something false, make `415` indistinguishable from `400` at
the code level, and be unfixable later without a breaking change once clients had coded against
`AcdpError::SchemaViolation`. The canon has **no code→status mapping anywhere**, so a code's name
is its only semantic content and nothing else corrects a wrong one.

**Why minting is cheap here and would not always be.** Unrecognised codes route to
`AcdpError::Registry(wire)`, explicitly "for forward compatibility": the client keeps the status
and the message and loses only the typed variant. That property is what makes condition (1)
bearable — it is not a licence to mint freely, because every minted code is a divergence someone
must later reconcile, and condition (3) exists so that someone is us.

**Emitted:** `{"error":{"code":"unsupported_media_type","message":"Expected request with
`Content-Type: application/json`"}}` at `415`. **Upstream:** acdp-rs#268.

**Process note worth keeping.** The question was held open for ~8 hours awaiting this ruling, and
cost nothing but time, because the gap was pinned by a marker test that failed the moment the
ruling was applied and whose failure message instructed its own deletion. A gap held behind a
failing-on-resolution marker cannot outlive the question by being forgotten; a gap held in a
comment can.

## 17. `cargo-mutants` is viable here, but only with `--test-workspace=true` (H-M, #216)

**Status: measured, not adopted.** This entry records numbers so the next lane inherits a
measurement instead of a guess. No CI job was added — see "Why no workflow" below.

**The finding that matters is not the timing.** `cargo-mutants` defaults to testing each mutant
with a **package-scoped** command. Mutating `acdp-registry-core` produces:

```
cargo test --verbose --package=acdp-registry-core@0.1.2
```

That runs core's own 80 unit tests and nothing else. This repo's coverage does not live there: it
lives in `acdp-registry-server`'s integration suites. So the default configuration reports
survivors for code that **is** covered, by tests it never ran.

Demonstrated on one mutant, both ways:

| scope | command | `receipt.rs:58:21` `replace \|\| with && in validated_fragment` |
|---|---|---|
| default (package) | `cargo test --package=acdp-registry-core@0.1.2` | **MISSED** |
| `--test-workspace=true` | `cargo test --verbose --workspace` | **CAUGHT** |

The killing test is `malformed_retired_receipt_key_fails_startup` in
`crates/acdp-registry-server/src/main.rs`, which asserts a `'#'` in a retired key fragment fails
startup. It is in a different package from the mutant, so package scoping cannot see it. Under
`--test-workspace=true` the whole of `receipt.rs` comes back **7 caught, 2 unviable, 0 missed** —
the file has no coverage gap, and the lone survivor was an artifact of scope.

**A naive baseline would therefore have been worse than no baseline**, because a survivor list
full of false positives is indistinguishable from a real one without re-deriving each entry.

**Measurements** (this machine, 2026-09-12, `cargo-mutants 27.1.0`, warm `target/`):

| quantity | value | how obtained |
|---|---|---|
| install cost | 23s, +7.8 MiB in `~/.cargo/bin` | `cargo install cargo-mutants` |
| mutants, whole workspace | 1383 | `cargo mutants --list --workspace` |
| mutants, `acdp-registry-core` | 472 | `--list -p acdp-registry-core` |
| mutants, `handlers/log.rs` | 65 — note three different `log.rs` files exist (8 / 15 / 65) | `--list --file …` |
| mutants, the bounded scope below | 206 (`receipt.rs` 9, `handlers/log.rs` 65, `handlers/context.rs` 132) | `--list --file …` |
| `receipt.rs`, workspace-scoped | 9 mutants in 82s, baseline build 26s | run below |
| build directory on disk | **2.3 GiB, per `-j` job** | `du -sh $TMPDIR/cargo-mutants-*.tmp` mid-run |
| free disk during runs | never below 30 GiB (from 32 GiB) | `df -g /` before/after each run |
| CI's playground suite alone | 16s warm, 305 tests | `cargo test --locked -p acdp-registry-server --features storage-sqlite,playground` |

Reproduce the run the per-mutant cost comes from:

```
cargo mutants --file 'crates/acdp-registry-core/src/receipt.rs' --test-workspace=true --timeout 900
```

**Extrapolation — an estimate, and labelled as one.** Basis: **9 mutants from one 273-line leaf
module**, n=7 viable. Subtracting the 26s baseline build leaves 56s for 9 mutants, so **~6.2s per
mutant** marginal under `--test-workspace=true` (9.2s if the one-off baseline build is amortised
over this small a run; it vanishes over a large one). Applied to 1383 workspace mutants that is
**~2.4 hours**, and applied to the 206-mutant bounded scope below **~21 minutes**. Treat both as
**lower bounds**, for three reasons: `receipt.rs` is a leaf module whose mutants trigger the cheapest possible rebuild, where
`handlers/context.rs` (132 mutants, the largest single file in scope) forces its dependents to
rebuild; mutants that induce a hang cost the full `--timeout` each and their number here is
unmeasured; and a faithful test command is more expensive than the one measured — see next.

**No single `cargo test` reproduces CI, and `cargo-mutants` runs exactly one command per mutant.**
CI's `test` job runs three, with different package scopes and feature sets:

```
cargo test --locked --workspace                                             # default features
cargo test --locked -p acdp-registry-server --features storage-sqlite,playground
cargo test --locked -p acdp-registry-pg  (+ a --features storage-pg server run)
```

`--test-workspace=true` covers only the first. `--features storage-sqlite,playground` cannot be
added while mutating core — `cargo-mutants` applies `--features` to the *mutated* package's
baseline build, which fails outright (`the package 'acdp-registry-core' does not contain this
feature: storage-sqlite`). So reaching CI-equivalent coverage requires a custom `--test-tool` or a
wrapper script running all three. Adding the playground suite's measured 16s to each mutant puts
the estimate nearer **~22s/mutant**, i.e. **~8.5 hours** for the workspace and **~76 minutes** for
the bounded scope. Until that wrapper exists, **survivors in code reachable only under the
`playground`, `storage-sqlite` or `storage-pg` features are expected false positives.**

**Verdict: viable now, as a scheduled job over a bounded file set — not per-PR, and not
workspace-wide.** Disk is not the constraint (2.3 GiB per job against 30+ GiB free); wall-clock is.
Concretely, for whoever picks up #216 steps 1–3:

1. `--test-workspace=true` is **mandatory**, not an optimisation. Without it the baseline is noise.
2. Scope the first baseline to a bounded set rather than the workspace. `receipt.rs` +
   `handlers/log.rs` + `handlers/context.rs` is 206 mutants, ~21 min estimated (~76 min with the
   wrapper in point 3). That set is a choice made here for being the densest core logic, **not**
   a scope #216 names — #216 names only `conformance.rs`, and its step 1 is fault injection over
   `src/` generally, which is the full 1383.
3. Build the three-command wrapper before trusting any survivor in feature-gated code, or restrict
   the scope to code the default-feature workspace suite actually reaches.

**Why no workflow file.** The unit deliberately excluded one. A scheduled `cargo-mutants` job whose
survivor baseline had never been produced is the same mistake #249 avoided: an issue closed by an
unverified artifact is worse than an open issue. The measurement above is the prerequisite that was
missing, and #216 stays open with it recorded.

## Unit H-E — the auth/webhook quartet (lane-2, 2026-09-12)

Four `UNCONFIRMED` entries from `plans/h-e-auth-webhook-quartet.md`, ranked by blast radius.

**Tiering.** No genuine one-way door: no schema change, no migration, and the one public-surface
change is **additive and withdrawable**. The leader settled E3's tier explicitly on a fact rather
than a judgement — the canon has no webhook concept (`grep -ril webhook` over `acdp-primitives`
returns nothing) and the scheme "matches GitHub's exactly" by this repo's own choice, so the registry
owns the surface outright; the inverse of the 415 case, which needed the human precisely because that
code sat in a canonical registry the repo does not own. So all four are Opus-tier and **none needs
the human**. One carries real operator impact and got the most scrutiny anyway.

**Method deviation, declared:** `/reconcile` asks for a fresh subagent per entry; this session does
not spawn agents, so the analyses ran in-context. Every entry below is settled against evidence
produced this run — a `file:line`, a measurement, or a named mutation.

### 1. `safe_client` refuses private-range feed hosts — CONFIRMED (Opus), with the cost named

Highest blast radius because it can break a **working** deployment, not just a wrong one. Examined
hardest for that reason.

The finding is about **redirects**: the feed URL is operator-configured, so the SSRF risk is a
hostile *redirect target*, not an attacker-chosen URL. `redirect(Policy::none())` alone closes it.
`safe_client` does more — it installs a DNS resolver that refuses private/loopback/link-local hosts —
so a peer registry reachable only on an internal hostname is now refused rather than polled.

Confirmed for consistency, not for strength: webhook delivery already accepts exactly this posture
for operator-configured URLs, and two different HTTP-client postures in adjacent crates is the drift
this repo keeps paying for. The rejected alternative (a hand-built client with redirects off and no
resolver) would have fixed the finding with zero collateral change and is the right fallback if an
operator hits this; a config knob to allow private ranges is a legitimate follow-up, not this unit.

Also recorded, because "safe_client" reads stronger than it is: it consults its policy **only** for
DNS. `allow_http` and `reject_ip_literals` are unenforced, so an `http://` feed still works and an
IP-literal URL bypasses the resolver check entirely. That is true of the webhook crate too.

### 2. E3 is additive and does not claim to deliver protection — CONFIRMED (Opus)

`X-ACDP-Signature` stays byte-identical and is now **pinned by a test**, so the additive property is
enforced rather than promised. Rejected: widening that header (breaks every deployed receiver); an
unsigned timestamp alone (rewritable, therefore worthless); a config-gated scheme under one header
name (two dialects, one name).

Confirmed specifically including what is *not* claimed: the docs say the registry **offers** bound
freshness and does not enforce it. A receiver that ignores the headers is as exposed as before.
"The registry now has replay protection" would have been false on the day it shipped — and writing it
would have manufactured a tenth false doc claim in the same wave that removed nine.

### 3. E1's cutoff threads through the trait, not the store constructors — CONFIRMED (Opus)

Settled by a claim boundary discovered **before** the code was written: the store constructors are
called from `server/src/main.rs:699,743,760`, outside this lane and inside lane-1's in-flight PRs, so
a constructor parameter was both a claim violation and a collision. `is_revoked` and `evict_expired`
each have exactly one production caller, both in this crate, so the trait signatures changed instead
and `server/` is untouched — verified by the workspace still building.

Rejected: a defaulted method reading config itself, which re-derives the window independently of the
validator and so re-creates the defect one layer down.

### 4. The evictor reads leeway from the signer, not config — CONFIRMED (Opus)

The two values are equal in every current wiring, so this is invisible today — which is exactly why
it is written down. The bug being fixed *is* a validator and a revocation layer disagreeing about the
acceptance window; deriving the value twice re-opens that class the moment wiring changes which value
reaches the signer. A `leeway_seconds()` accessor is the cheaper guarantee.

**Summary: 4 confirmed, 0 changed, 0 deferred, 4 settled by Opus, 0 needing the human.** No code
follow-up blocks the ship. Two items are recorded as follow-ups that block nothing: a config knob if
an operator needs private-range revocation feeds, and the `conformance_gate` false positive that
flags `#[cfg(test)]` env reads inside `src/` as operator configuration.

## Unit H-N — the JWT issuer assertion (lane-2, 2026-09-12)

One `UNCONFIRMED` entry from `plans/h-n-jwt-issuer-assertion.md`. Low blast radius, test-only,
reversible in a commit — **settled by Opus, not brought to the human.**

**Timing deviation, declared:** `/drive` reconciles *before* the single PR so code cannot ship
carrying an unresolved one-way door. This entry was reconciled **after** #251 merged. Stating it
rather than letting the ordering imply otherwise. It is defensible only because the entry is not a
one-way door — no schema, no wire contract, no production code, and the failure mode is a red test —
but the reconcile was still owed and the sequence was wrong.

### The issuer guard asserts on `jsonwebtoken`'s Display string — CONFIRMED (Opus)

`rejects_token_from_a_different_issuer` proves *which* guard rejected the token by asserting
`err.to_string().contains("InvalidIssuer")`, coupling a test to a dependency's error *rendering*.

**Premises re-verified rather than reasoned about:**
- The error kind is genuinely unrecoverable at this surface — `AuthError::TokenInvalid(String)`
  (`lib.rs:64`), reached through the single `map_err(|e| AuthError::TokenInvalid(e.to_string()))`
  at `jwt.rs:246`. There is no typed channel to assert on instead.
- `jsonwebtoken = "11"` (workspace `Cargo.toml:59`), locked at `11.0.0`.

**Confirmed, with the coupling's real exposure stated rather than minimised.** `"11"` is a caret
requirement, so `cargo update` can move within `11.x` **without any manifest change** — this can
break on routine dependency maintenance, not only on a deliberate major bump. That is a larger
surface than "a future upgrade might rename it."

It is still the right trade, because of *how* it breaks: the test goes red locally with a message
naming exactly what happened and quoting the new text, and the fix is one string. Nothing ships
wrong; a maintainer is told. The alternatives are worse in kind, not just in cost — asserting only
`is_err()` is the very defect the test exists to avoid (mutation M3 showed a signer that rejects
*everything* would satisfy it), and widening `AuthError` to carry a `jsonwebtoken` kind would leak a
dependency's type into this crate's public error enum for the benefit of one test.

**Summary: 1 confirmed, 0 changed, 0 deferred, settled by Opus, 0 needing the human.**
