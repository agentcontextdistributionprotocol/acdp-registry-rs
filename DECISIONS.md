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

## Unit U-503 — the mutable `sha-` container tag (lane-3, 2026-09-13)

Three `UNCONFIRMED` entries from `plans/u-503-immutable-sha-tag.md`, reconciled before ship and
ranked by blast radius rather than by how consequential they sound. None is a one-way door —
there is no schema, public API contract, auth model, data migration or external dependency
among them, and every one is a workflow-config or file-location choice reversible in a single
commit — so all three sit in Opus's tier and none was escalated. The analysis ran in-thread
rather than in a fresh subagent (this session is under a standing instruction not to spawn
agents unrequested); that is weaker than an independent read, so each decision below rests on
an executed check or on primary evidence, not on agreement.

### 1. The double build is KEPT; only the mutable tag is fixed — CONFIRMED (Opus)

**Assumed:** that "the same commit gets built twice at all, producing two non-reproducible
digests" — which the unit brief named as part of the defect — is in fact correct behaviour.

**Analysis.** The two digests differ *deterministically*, not flakily, and both runs' `buildx`
command lines say why: metadata-action stamps `org.opencontainers.image.version` as `main` on
the main-push build and `0.1.3` on the release build, `image.created` differs, and buildx
attaches `--attest type=provenance,mode=max,builder-id=…/runs/<run-id>`. Labels and provenance
live in the config blob, so the manifests cannot agree. The obvious collapse — promoting the
main digest with `buildx imagetools create` — would therefore publish a release image whose own
OCI `version` label reads `main` and whose provenance names the main run. That trades a
cosmetic problem (two digests for one source) for a substantive one (a release artifact that
misreports its own release identity).

**Verdict:** confirm. Ranked highest here not because it is expensive to reverse — it isn't —
but because it **declines half of what a peer asked for**, which is the kind of thing that
should never be settled silently. It stays visible by being argued in the PR body and carried
in the lane's `done` report. A separate `fyi` to the leader was considered and rejected: the
information is already going where the leader reads it, and `done` is the next scheduled
contact, so a fourth message would spend coordination budget to say the same thing earlier.

**Status:** `CONFIRMED (2026-09-13)`. Reversible — a follow-up that collapses the builds would
find the guard already satisfied, since one publishing path trivially meets the one-writer
invariant.

### 2. metadata-action honours `{{is_default_branch}}` in `enable=` for `type=sha` — DEFERRED (Opus)

**Assumed:** that the handlebars expression is evaluated in the `enable=` option of a
`type=sha` rule, not only in the `type=raw` rule where this file already uses it.

**Analysis.** Deliberately *not* resolvable at reconcile, and that is the judgement worth
recording rather than papering over. The only evidence that counts is whether this PR's own
`docker` run computes a `sha-` tag on a `pull_request` event, and that run does not exist until
the branch is pushed. Reading the action's README would produce belief, not evidence. Deferring
is safe because the assumption **fails closed and fast**: if unsupported, the PR's run computes
a `sha-` tag on a pull request and the new `assert image tags` step fails the job at the assert,
about a minute in, before the twenty-minute build — and nothing publishes on a pull request in
any case, since `docker.yml` gates both the registry login and the push on
`github.event_name != 'pull_request'`. Named fallback if it fires:
`enable=${{ github.event_name == 'push' && github.ref == 'refs/heads/main' }}`, a plain GitHub
expression with identical semantics that preserves the in-PR firing property.

**Verdict:** defer, explicitly, with the settling evidence named. This does **not** block the
ship — it is not a one-way door, and the mechanism that would catch it is the same mechanism
this unit exists to add.

**Status:** `UNCONFIRMED` — resolve from the PR's `docker` run output at CI watch, specifically
whether the computed `tag-names` contain a `sha-` entry on a pull request.

### 3. A shell script is the right home for a CI tag guard — CONFIRMED (Opus)

**Assumed:** that `docker/assert-image-tags.sh` is an acceptable home for this guard even
though the repo contained no `*.sh` files before it and CI runs no `shellcheck`, `actionlint`
or `yamllint`.

**Analysis.** The convention-matching alternative — a Rust test under `crates/**` reading
`docker.yml`, the shape used for the route-documentation guard — is **outside this unit's path
grant**, so it was never this lane's to choose; taking it would have required a
`claim-request`. Among the options actually available, the script is also the only one that
could be falsified before merge, and was: `--self-test` rejects the real pre-fix tag sets from
runs 34734871991 and 34725795501, and it caught a genuine defect in the guard's own input
normaliser (a dropped final token) that had made it *pass* the release run it was written to
reject. The absent linter is mitigated structurally rather than promised — the self-test is
wired into `docker.yml` and runs on every workflow event.

**Verdict:** confirm. Lowest blast radius of the three: one file, two workflow steps, and the
case table survives a move to a Rust test unchanged if the convention is later unwelcome.

**Status:** `CONFIRMED (2026-09-13)`.

### 2 (resolved). `{{is_default_branch}}` in `enable=` for `type=sha` — CONFIRMED (Opus), by run 34763580595

The deferral above is closed by the evidence it named, not by a later opinion. PR #264's own
`docker` run computed `tag-names: ["pr-264"]` — no `sha-` entry — against the pre-fix PR run
34725795501's `["pr-262","sha-3617f76"]`. metadata-action does evaluate the handlebars in
`enable=` for a `type=sha` rule; the gate fires; the fallback expression was never needed.
`self-test the image-tag guard` and `assert image tags` both green, `build + push` skipped as a
pull request requires. Recorded here because the next reader should not have to re-derive which
run answered it.

## 18. U-502 — the mutation oracle for #216 (lane-2, 2026-09-13)

Seven `UNCONFIRMED` entries from `plans/u-502-mutation-oracle.md`, ranked by blast radius.

**Tiering.** No genuine one-way door: a config file, a new scheduled workflow, doc comments,
additive tests, and an issue comment. No schema change, no migration, no public HTTP contract
change, nothing irreversible. So **all seven are Opus-tier and none needs the human.** One
(#3) is recorded as the highest-consequence entry anyway, because its *absence* invalidated a
published number.

**Method deviation, declared:** `/reconcile` asks for a fresh subagent per entry; this session
does not spawn agents, so the analyses ran in-context. Every entry below is settled against
evidence produced this run — a `file:line`, a measurement, or a named mutation — never reasoning
alone.

### 1. Config at `.cargo/mutants.toml`, not root — CONFIRMED (Opus)

Verified by mechanism rather than preference: `cargo mutants --help` documents
`.cargo/mutants.toml` as the default read path, and a bare `cargo mutants --list` in this repo
returns exactly the 74-mutant scope with no `--config` argument. Since `test_workspace` and
`copy_vcs` are both load-bearing — omit either and the verdicts are noise — a config that cannot
be forgotten is a correctness property. Rejected: a root file plus a required flag, which makes
"ran it without the flag" a one-typo route to a wrong answer that looks right.

### 2. Ratchet scope is two files; `handlers/context.rs` excluded — CONFIRMED (Opus)

Leader ruling, and the measurements confirm it rather than merely permitting it: `context.rs` is
**134** mutants here against the **132** recorded in entry 17 one day earlier, and the workspace
is **1398** against **1383**. A budget keyed to a file another unit is rewriting would go red for
reasons unrelated to the property it guards, and a red check nobody can explain gets disabled.
The other 134 are tracked separately rather than lost.

### 3. `copy_vcs = true` — CONFIRMED (Opus), and this is the entry that earned its place

Not a preference; its absence produced a **false baseline that was published and retracted**.
Without it, `conformance_gate.rs`'s `no_tracked_file_contains_a_conflict_marker` panics on
`git ls-files` in the `$TMPDIR` build copy, `cargo test` stops at the first failing binary, and
**41 of 48 "caught" verdicts were scored by that panic** rather than by any mutation. Confirmed
by reading which test failed in all 74 logs, then by re-running: survivors went 0 → 2 once the
harness was honest. Cost here is a 4 KB worktree pointer.

Rejected: making the hygiene test skip outside a git repo — it would add another self-skipping
test, which is the exact hazard this unit flagged elsewhere; and excluding `conformance_gate`
from the test command, which discards real coverage to hide a harness fault.

### 4. The survivor budget — **CHANGED** (Opus): 2 became 1, because the survivor was killed

Recorded as changed rather than confirmed, because the decision itself moved. The entry assumed
budget **2** with `log.rs:117:19` *budgeted* on the grounds that its killing test lay outside the
claim. The claim was granted, so it is **killed, not budgeted**: two tests in
`http_integration.rs`, one per direction, landed in the same commit as the ratchet. Budget is
**1**, and that 1 is entry 5's equivalent mutant.

Two tests rather than one for a method reason worth keeping: inverting `if stored != tenant`
breaks **both** directions, so a single test asserting both would stop at whichever assertion ran
first and never evaluate the other.

**And the finding is stated accurately, which matters more than its severity.** The code is
CORRECT — `!=` is the right operator and no cross-tenant disclosure ships. The oracle found an
*unguarded correct property*: nothing executed the branch, so nothing would have noticed if it
stopped being correct. "Mutation testing found a cross-tenant disclosure" would be false.

### 5. `root_for`'s cache mutant is accepted as equivalent — CONFIRMED (Opus)

`handlers/log.rs:131:18`, `== -> !=`, guards **only** `log.cache_root(...)`. `root_for` returns
the same `root` on both branches and append-only makes any `(size → root)` pair immutable, so a
cached historical root is still correct and an uncached current root is merely recomputed. Only
*which* sizes get cached changes. No assertion over a response can observe it; killing it needs
instrumentation counting merkle computations — observable work rather than a result — which is a
performance harness dressed as a correctness one. Accepted **with that reason recorded in the
repo**, so a future reader does not see an accepted survivor and assume nobody looked.

### 6. AC8's 50% concentration threshold — CONFIRMED (Opus)

Measured on both real runs rather than chosen: the broken run's sole killer accounts for
**41 of 48 = 85%**, the corrected run's largest for **8 of 46 = 17%**. A 50% line sits in an
order-of-magnitude gap, so it is not tuned to the data.

Rejected as insufficient: relying on a green unmutated baseline. `cargo-mutants` runs its baseline
**package-scoped even under `test_workspace`** (`baseline.log` says
`--package=acdp-registry-core`; the mutant logs say `--workspace`), so it never builds the binary
that was failing — a green baseline is compatible with every verdict being noise. That is a
property of the tool, not of this repo, and it is stated in the workflow comment so the check is
not later deleted as redundant.

### 7. The three-command CI-equivalent wrapper — CONFIRMED as declined (Opus), deferral noted

`cargo-mutants` runs one test command per mutant where CI runs three. Adding the playground suite
would take the marginal cost from ~6.2s to ~22s per mutant (entry 17) to recover **one** test:
`playground_compiled_in_but_runtime_disabled_keeps_admin_route`. Declined deliberately, with the
single unreachable test named rather than a vague class waved at. **Deferred, not rejected in
principle** — it becomes the right trade if the scope ever grows to include feature-gated code.

### 8. Correcting this unit's own unpublished log entry in place — CONFIRMED (Opus), **and the licence has now expired**

The `docs/ENGINEERING-LOG.md` entry carrying the retracted 48/0 number was rewritten in place
rather than corrected underneath. Confirmed: it existed only on `lanes/lane-2`, had never been in
`main`, and no other session had read it — a draft fixed before publication, not history
rewritten. The append-only rule protects other lanes' content and published entries; it is not a
reason to publish a number already retracted and then publish its correction beneath.

**The condition that justified it no longer holds.** That entry is now committed and pushed, so
the next correction to it must be an append, exactly as the file's own precedent does
("corrected here rather than in place"). Recorded explicitly because this is precisely the kind of
narrow ruling that gets over-extended: "lane-2 edited ENGINEERING-LOG in place once" is not a
precedent, and the three-part condition — own entry, never in `main`, unread by anyone else — is
the whole of it.

**Summary: 7 confirmed, 1 changed, 0 deferred, 0 needing the human** — eight entries because the
budget decision is recorded separately from the equivalent-mutant reasoning it now rests on. All
settled by Opus against evidence produced this run. No code follow-up is outstanding before
shipping.

### 9. `mutants.yml` derives the spec pin rather than duplicating it — CONFIRMED (Opus)

Appended to entry 18 after the reconcile pass, because the fact that forced it arrived afterwards:
bot PR #272 bumps the spec pin at `ci.yml:411` (`d1f06d0d` → `108ff76`), and `bump-spec.yml:24`
passes the bumper **one** filename — `ci.yml`, singular. A hardcoded copy in `mutants.yml` would
therefore never be bumped, and the per-PR conformance job and the scheduled mutation job would pin
different spec versions permanently, with nothing reporting the divergence.

So the ref is now derived: a `pin` step reads the 40-hex `ref:` out of `ci.yml` and passes it via
`steps.pin.outputs.ref`. The property the pin exists for is unchanged — a spec-repo push still
cannot move this repo's result without a commit here, just in one place instead of two.

**Falsified against five fixtures, and it caught a real defect in itself.** The first version
counted `grep -cE 'checkout-spec@'`, which matches **two** lines in `ci.yml` — the real `uses:` and
a comment at `:407` discussing it — so it failed on the actual file and would have reddened the
scheduled job on its first run. Anchoring the pattern on `uses:` fixed it. Fixtures: the real
`ci.yml` → `rc=0` yielding exactly `d1f06d0d…`; two 40-hex refs → `rc=1`; zero refs → `rc=1`; a ref
placed before the `checkout-spec` usage → `rc=1`; `checkout-spec@` present only in a comment →
`rc=1`.

Rejected: a second bumper call in `bump-spec.yml` (more moving parts, and `ci.yml:404-410` records
that the bumper refuses to bump a file carrying two pin anchors at all — a file it silently declines
is worse than one it was never pointed at); and duplicate-and-document, whose only honest mitigation
is a check that fails on disagreement, which is more work than deriving.

## U-512 — #267: `sha-` tags will NOT be made immutable by a CI check (2026-09-13, lane-1)

**Decided by:** Opus (lane-1), under the autonomy ladder. Consequential but decidable and reversible
in a commit; the assign named both outcomes as complete units. **Outcome: won't-do, with the
residual risk accepted explicitly and a cheaper answer documented.**

### What was asked

Give `sha-<short>` registry-level immutability via a pre-push existence check that fails the job when
the tag already exists, so a hand-run re-run of a `main` build cannot repoint it.

### Why not

**1. "Registry-level" is not available on this registry, so the name overstates whatever we build.**
Checked rather than assumed: GHCR's package API for
`orgs/agentcontextdistributionprotocol/packages/container/acdp-registry` exposes
`created_at, html_url, id, name, owner, package_type, repository, updated_at, url, version_count,
visibility` — and nothing for tag immutability, tag protection, or retention. There is no registry
setting to turn on. Anything we ship is a **workflow** check.

**2. A workflow check binds the workflow, not the tag.** The package is repo-scoped, so anyone who
can push to the repo (or holds a PAT with `write:packages`) can `docker push` over a `sha-` tag
directly, never touching this workflow. Stated as one sentence with its limit inside, per the
assign's own instruction, the guarantee would read: *"this workflow will not repoint a `sha-` tag,
though anyone with package write access still can."* That is materially weaker than what #267 asks
for, and shipping it under the name "immutability" is the overclaim class this board keeps catching.

**3. The immutable identifier already exists, costs nothing, and is the mechanism registries
actually provide.** Every published image is addressable by digest
(`...acdp-registry@sha256:<64-hex>`). Verified end-to-end: `docker buildx imagetools inspect
:sha-33bb3a3 --format '{{.Manifest.Digest}}'` returns
`sha256:002469d2dc7d6f1263a058010976f0ec7e4a2ba52c6db3cdece1e866a9a29df3`, which matches the digest
the packages API records for that tag. A digest cannot be repointed by anyone, including us. The
real problem #267 names — that the `sha-` prefix *invites* being read as content-addressed — is a
documentation problem, and it is now fixed in `docker/RAILWAY.md` where operators choose a tag.

**4. The fail-closed cost lands on the worst day.** The check fires on the re-run after an infra
flake, which is exactly when the pipeline needs to move, and recovery means deleting a published tag
by hand. We would be trading "a re-run can move a tag" for "a flake wedges publication until someone
does registry surgery."

**5. An escape hatch would make the guarantee nominal while keeping the complexity.** The obvious
mitigation for (4) is a `workflow_dispatch` input permitting overwrite. But anyone who can re-run the
job can also dispatch it with the input set, so the guarantee degrades to "immutable unless someone
chose otherwise" — which is what we already have, with more moving parts. It is auditable, which is
a real but small gain; it does not change who can move a tag.

### The honest counter-argument, and why it does not carry

A CI check **would** prevent the realistic accident: a maintainer clicking *Re-run all jobs* on a
`main` build. That is the actual failure mode, not a malicious insider. Declining still seems right
because the harm from that accident is bounded — the rebuilt image is from the same commit by
construction (the tag encodes it), so what changes is build metadata, `image.created`, and the
provenance attestation, not which source was built. The exposure is reproducibility and audit, not
behaviour. And the fix for "someone pinned a mutable identifier" is to pin the immutable one, which
now costs a documented one-liner.

If that judgement is wrong, it is wrong cheaply: re-opening is a commit.

### Residual risk, ACCEPTED not missed

A hand-run re-run of a `main` build rebuilds that commit and moves its `sha-<short>` tag to a new
digest. Anyone who pinned `sha-<short>` and expected content-addressing gets a different digest of
the same source. This is **unchanged** by this unit and is stated in `docker/RAILWAY.md` in the same
paragraph that describes the single-writer guarantee, so a reader choosing a tag meets the limit and
the alternative together rather than discovering it later.

### What is NOT reopened

D-W5-11's double-build ruling stands and was not revisited.

### What this unit changed

`docker/RAILWAY.md` only: digest pinning documented as the answer, with the command, the reason a CI
check is not it, and the cost of pinning a digest (it never picks up a fix). No workflow change, so
U-503's `type=sha` single-writer gate, `flavor: latest=false`, `assert image tags` and `--self-test`
are untouched.
## Unit U-510 — CI builds the feature configurations it checks (lane-3, 2026-09-13)

Two `UNCONFIRMED` entries from `plans/u-510-build-feature-configurations.md`, reconciled before
ship. Neither is a one-way door — both are workflow-file placements reversible in a single commit —
so both sit in Opus's tier and neither was escalated. Analysis ran in-thread (standing instruction
against unrequested subagents), so each rests on an executed measurement rather than on agreement.

### 1. Build steps inside the required `clippy` job, not a new job — CONFIRMED (Opus)

**Assumed:** coverage that actually blocks a merge is worth more than a job name that describes
itself perfectly.

**Analysis.** `required_status_checks.contexts` is `["rustfmt","clippy","tests",
"conformance (spec fixtures)"]`. A new `builds` job would be a check that is **not** required, so a
failing feature build could not prevent a merge — which is precisely where U-508's `lint` gate sits,
still awaiting a human decision on the same settings change. Shipping U-510's coverage in that state
would have closed the gap on paper and left it open in practice.

The apparent inconsistency with U-508 — which refused to put shell linting inside `rustfmt` — is not
one. The test is **concern identity**: shell linting is a different concern wearing a
Rust-formatting name; building a feature configuration is the same concern the `clippy` job already
served nine times over. Mitigated further by step names (`build (postgres)` …) so a failing build is
legible rather than arriving as a mysterious `clippy` failure.

**Verdict:** confirm. Recorded with the reasoning because the next reader will otherwise see only
the surface inconsistency.

**Status:** `CONFIRMED (2026-09-13)`. Reversible — moving the steps to their own job is one commit,
and would become the better choice the moment `lint`'s required-context question is answered, since
the same answer would apply.

### 2. The msrv job's `cargo check` steps stay `check` — CONFIRMED (Opus) as a bounded remainder

**Assumed:** the msrv job's purpose is that 1.88 accepts the language and API surface, not that it
links.

**Analysis.** `cargo check` shares the exact defect this unit fixes — it does not codegen or link —
so leaving it is knowingly leaving a remainder, and that is why it is recorded rather than skipped.
The reason it is acceptable: both msrv configurations (`sqlite default`, `storage-pg`) are now linked
at stable by this unit's new steps, so what goes unverified is narrowly *codegen divergence between
1.88 and stable for identical source*. Converting them would buy that narrow case at the cost of
MSRV-toolchain build time on every PR.

**Verdict:** confirm as a bounded, stated remainder. The PR body, the `#265` closing comment and the
engineering log all say it, so it cannot be mistaken for complete closure.

**Status:** `CONFIRMED (2026-09-13)` as scope. The leader may prefer full closure; that is a
`cargo check` → `cargo build` swap in two lines.

## U-514 — #276: operator hazards travel in `docs/UPGRADING.md`, not in the changelog template (2026-09-13, lane-1)

**Decided by:** Opus (lane-1), under the autonomy ladder. The assign offered two outcomes and named
both as complete. **Outcome: option 2 — a stated convention with a gate — and the reason is a
verification limit, not a preference.**

### The gap, restated after measuring it

Release notes are per-crate and generated by `release-plz` from commit **subjects**. A subject is one
line and cannot carry "upgrade these two things together or the stack will not boot". So the #271
version-coupling hazard existed in a commit body, `ASSUMPTIONS.md`, the README and an issue, and
reached nobody performing an upgrade.

### What was checked before pricing anything (Rule 147)

- `release-plz generate-schema` — the tool's own schema — confirms `[changelog]` accepts `body`,
  `commit_parsers`, `commit_preprocessors`, `protect_breaking_commits`. **The mechanism for option 1
  exists.** It was not rejected as impossible.
- The schema carries **no default for `body`**, so adopting a custom template means authoring a
  complete replacement for release-plz's built-in one without having that built-in text
  authoritatively. The current generated format (grouped sections, PR links) is good, and silently
  regressing it is the likely failure.
- **Squash-merge commit bodies on `main` are multi-commit concatenations.** `0e5bd14`'s body is
  **1920 lines**; `30ba447` carries three units' full narratives. Rendering `commit.body` is not a
  design with a drawback, it is unusable. Any viable option-1 design is therefore a *targeted footer*
  convention — which is a convention needing enforcement, exactly like option 2, plus a template
  rewrite.

### Why option 2

The assign's own constraint decided it: *"If your change only takes effect at release time, say how
it was verified without releasing. 'It will work at the next release' is an undischarged claim."*

A `body` template only renders during changelog generation. Verifying it locally requires
`release-plz update`, which runs `cargo package --verify` across eight crates — slow, and it failed
outright on this machine's toolchain. So a template rewrite would have shipped on an undischarged
claim, with the release pipeline as the blast radius, **while a release PR (#278) is open**. A
template change would regenerate that PR with an unverified template.

Option 2 has no release-time component at all: nothing it touches runs during a release.

### The convention, and what enforces it

`docs/UPGRADING.md` is the operator's pre-upgrade read: one section per version, newest first, and a
version with **no** operator-visible change says so explicitly — an absent section is
indistinguishable from one nobody wrote.

Named from the three places a reader or contributor actually lands: `CHANGELOG.md`'s lookup table
*and* its "Where new entries go" section, `README.md`'s Configuration section, and `docs/README.md`'s
index.

`docker/assert-upgrade-notes.sh --check` fails the build when the workspace version has no section.
It sits in `docker.yml` because that workflow builds the operator-facing artifact — same audience,
same moment — and because it runs on `pull_request`, so the **release PR**, where the version bump
actually happens, is gated before it merges.

Four negative controls, wired as `--self-test` rather than asserted in prose: a missing section is
rejected, a present one accepted (or the gate would reject everything and (1) would still pass), a
**prefix** match is rejected (`## 0.1.40` must not satisfy 0.1.4 — during a release these differ by
one trailing character), and a missing file is rejected distinctly from a missing section.

### What this does not claim

It does not put the hazard text inside the GitHub Release body. A reader who reads only the rendered
release notes and never follows a link still does not see it. That is the residual limit of this
choice, and option 1 remains available: if someone later has a way to exercise a changelog template
without a release, the footer design is written up above and the convention this unit establishes is
what such a template would render.
## Unit U-513 — the falsified latency claim, corrected (lane-3, 2026-09-13)

### 1. The correction is a distribution, not a better number — CONFIRMED (Opus)

U-510's entry quoted "18 seconds of headroom" from **one sample of each job**, and paired it with a
trigger that had **already fired before the sentence was written** (run 34766261172: clippy 98s vs
tests 86s). Measured across n=5 post-change runs, clippy spans 98-176s and tests 86-165s — spreads of
78s and 79s against per-run margins of 4-27s. **A margin smaller than the run-to-run spread is not a
margin**, so the fix could not be a fresher number; any single-sample margin here is meaningless.

The corrected text makes a categorical claim instead: before the change clippy was never near the
critical path (slowest 54s vs tests' fastest 142s); after it, the distributions overlap and clippy led
in **2 of 5** runs, one on `main`. Derived cost, mean of `max(0, clippy - tests)`: **~8s** against a
~150s critical path.

Edited **in place** under the narrow exception the assign granted, with the superseded figure left
visible and the deletion count stated (**7** in `ENGINEERING-LOG.md`, 0 elsewhere). The normal
append-only rule exists to stop lanes destroying each other's entries, not to preserve a defect —
appending would have left the false sentence as the first thing a reader meets.

### 2. The split: declined again, on re-derived grounds — CONFIRMED (Opus)

The original decline rested on *"it relieves a job that is not the bottleneck"*, which is false in 2
of 5 runs. Re-derived rather than restated: the scheduled split would buy ~8s and cost two things
that survive — the feature lists become two sources of truth (this repo has already had a
written-out list go stale silently, and `ci.yml` says so), and breakage detection is delayed by up to
a day.

**A third option is strictly better than both and is recorded rather than taken:** move the five
build steps to a separate job running *in parallel*. Added latency becomes zero, and it *moves* the
lists rather than copying them, so neither surviving objection applies. It is not taken because a new
job is not a required context, so the builds would stop blocking merges — the property U-510 chose the
`clippy` job to obtain. **The blocker is identical to U-508's `lint`: one branch-protection change
unblocks both.**

**Status:** declined, with the trigger recorded as fired and weighed so no reader concludes it went
unnoticed.

---

## Decision: a CI gate that is not a required status check is a report, not a gate (U-516)

**Context.** U-514 added `docker/assert-upgrade-notes.sh` to `docker.yml` and its `done` said the
workflow "now blocks a release PR whose version has no `UPGRADING.md` section". Measured, that word
was wrong:

```
$ gh api repos/.../branches/main/protection/required_status_checks
  contexts: ["rustfmt","clippy","tests","conformance (spec fixtures)"]
$ gh api repos/.../rules/branches/main   -> []
$ gh api repos/.../rulesets              -> []
$ gh pr checks 280 | grep -E '^build'
  build   pass   2m47s   ...
```

`docker.yml` publishes the check named `build`. `build` is not a member of that four-element list,
and no branch rule or ruleset supplies another. So a release PR missing its section gets a red
`build` and a green merge button — #276's failure with a check-mark in front of it.

This is the **third** site of the class already recorded for U-508 (`lint`) and U-510 (`clippy`'s
feature-matrix builds): a check authored as a guard, published under a name nobody required. The two
prior entries concluded "one branch-protection change unblocks both" and escalated. The class then
produced a third instance *while the escalation was outstanding* — which is evidence that waiting on
the settings change was the wrong remedy to depend on, not that it needed restating.

**Decision.** Run the assertion inside a job whose check name is **already** required, and change no
repo settings. `required_status_checks.contexts` is identical before and after this unit; no lane and
no lanes leader has authority over it, and a design needing a fifth context would have been the wrong
design for this unit.

**Host job: `fmt` (check name `rustfmt`).** Chosen on dependencies, not convenience. The gate needs a
checkout and nothing else — no feature-set toolchain, no services, no registry login — which is also
`fmt`'s entire dependency set, so it introduces no new failure surface into a required job. It is the
fastest required job (~6s vs ~2m30 `clippy`, ~2m57 `tests`), so a missing section reddens in seconds.
And a bug in the step is diagnosable as itself there, rather than being read as a real test failure
inside `tests`.

The cost is that `rustfmt` now reddens for a reason unrelated to formatting. That is accepted: the
job name is a required context and renaming it would violate the unchanged-contexts constraint, and
the script's own failure output names the actual problem.

**The `docker.yml` copy stays**, and for a measured reason rather than caution: `docker.yml` also
triggers on the `acdp-registry-server/v*` tag push, which `ci.yml` never runs on. The two copies
cover different windows — `ci.yml` the pull request, `docker.yml` the tag. Both call the same script,
so they cannot drift in substance.

**Status:** applied. The false "blocks" claim is corrected in `docker/assert-upgrade-notes.sh`'s
header, in `CHANGELOG.md`, and here.

### Addendum (U-516, AC8): a gate with no `if:` can silently not run

Added mid-unit after lane-3 named the mechanism unprompted. A step with no explicit `if:` defaults to
`if: success()`, so **any** earlier failing step skips it — and a skipped step reads like a passed one.
In `docker.yml` three steps that can fail precede the gate (`checkout`, `setup-buildx-action`,
`metadata-action`), plus `assert semver tag`, which is `if: startsWith(github.ref, 'refs/tags/')`, on
the tag path. So the accurate description of the pre-U-516 state is not "reports but does not block";
it is **"can silently not report, and still not block."**

The first version of this unit reproduced the same defect in its new home: the gate was placed after
`cargo fmt`, where a formatting error would have skipped it. Position alone was not the fix.

Two defences, covering different failure modes:

- **position** — immediately after `checkout`, so nothing that can currently fail precedes it;
- **`if: ${{ !cancelled() }}`** — so that remains true when someone later inserts a step above it,
  which position alone cannot guarantee. `!cancelled()` rather than `always()` because a cancelled
  run should stop rather than press on.

The same `if:` is applied to the `docker.yml` copy. That copy still cannot block, but it can now no
longer fail to *report* on the tag-push path.

**Control, run 34769860335, job `rustfmt` (103757349450).** A deliberate `exit 1` before the gate, and
a copy of the gate with no `if:` beside the real one — two outcomes in one run:

```
3  failure  deliberate failure before the gate
4  skipped  CONTROL - gate without if, must be SKIPPED
5  failure  assert the version has operator upgrade notes   <- ran, and reported
6  success  self-test the upgrade-notes gate
```

Same run, same preceding failure, opposite outcomes: that isolates the `if:` as the cause instead of
assuming it.

Also taken from lane-3: the self-test now runs **before** the assert in both workflows, matching
U-503's ordering, so a vacuously-passing guard is caught before its verdict is trusted.

**What was checked and is NOT a defect:** on the tag path, `assert semver tag` failing also skips
`build + push` at `docker.yml`'s step 16, which likewise requires `success()`. So a skipped gate there
never let a publish escape — the job goes red and nothing is pushed. What was lost was log legibility.
The exposure case is the `pull_request` one above.

### Correction to the addendum above (U-516): AC8 bought legibility, not exposure

The addendum ends "The exposure case is the `pull_request` one above." **That is wrong, and it is my
sentence, not the reviewer's.** Verified rather than reasoned:

```
$ gh api .../required_status_checks -q 'if (.contexts | index("build")) then "MEMBER" else "NOT" end'
NOT
```

On a pull request the two paths are indistinguishable in outcome:

| | job result | `build` required? | merge |
|---|---|---|---|
| gate **skipped** by an earlier failure | red | no | proceeds |
| gate **ran and failed** | red | no | proceeds |

So the `if: success()` default produced **no exposure at all** — in either direction, the merge was
never blocked. The exposure came entirely from `build` not being a member of the contexts list, and
would have persisted whatever the `if:` chain did.

**The two halves of U-516 are orthogonal, and AC8 is the smaller one.** Moving the assertion into an
already-required job is the whole of what closes the exposure; `!cancelled()` buys independence
between two unrelated guards and a legible log — a skipped step reads like a passed one — and nothing
more. Had AC8 shipped alone, the gate still could not have blocked a merge.

Recorded because the error is instructive in shape: a correct mechanism (`if: success()` really does
skip the step) carried a wrong consequence (that this let something through). The harmful step is the
merge, and the merge's condition is the contexts list — which had been measured, correctly, twice the
same day, and was not re-read when the consequence was written. Naming the mechanism is not the same
as tracing it to the harm. Credit to lane-3 for the catch, on both ends.

## 19. U-504 — extending the mutation ratchet to `handlers/context.rs` (lane-2, 2026-09-13)

Ten assumptions from `plans/u-504-context-ratchet.md`, all resolved here. **8 CONFIRMED, 1 CHANGED,
2 DEFERRED with named follow-ups** (entry 5 and 6 are the deferrals; the count is 8+1+2 = 11 because
entry 4 is confirmed with a recorded contingency, listed under CONFIRMED).

**Declared deviation, same as U-502:** `/reconcile` calls for fresh subagents to analyse each entry;
this session does not spawn agents, so every entry was resolved in-context. Compensation: each
verdict below rests on a measurement, a `file:line`, or a named mutation — not on reasoning about
the code. Where a verdict rests on a claim that could expire, the claim was re-measured at the final
sha rather than carried forward.

**None of the eleven needed the human**, and none is a one-way door: the budget numbers, the job
count, and the guard are each one commit to reverse.

### Settled by Opus, with the evidence

| # | assumption | verdict | the evidence, not the argument |
|---|---|---|---|
| 1 | budget may rise 1 → 8 | **CONFIRMED** | final run 8/8, and the measured survivor set is **content-identical** to the 8 named in the workflow |
| 2 | `timeout != 0` becomes a ceiling of 1 | **CONFIRMED** | targeted run: `:1277` TIMEOUT at the full 300s, `:1223` MISSED in 10s |
| 3 | `-j1` over `-j2` | **CONFIRMED** | same shard: `-j6`/`-j8` > 10 min still building, `-j1` 1m55s |
| 4 | `:81`/`:82` are equivalent, not gaps | **CONFIRMED** (contingent) | search serves only public rows to anonymous, audience member **and the producer**; sibling `"private"` arm CAUGHT |
| 7 | AC-8 invariant 4 bans any 40-hex in `mutants.yml` | **CHANGED** | the real file carries four legitimate 40-hex *action* pins; narrowed to `ref:` lines |
| 8 | AC-8 as a pure function over text | **CONFIRMED** | all four falsified; then the falsification test falsified by disabling each check (4/4) |
| 9 | one refill test kills all six | **CONFIRMED** | six mutations, each changing the measured count (10 → 60, or cursor absent) |
| 10 | no claim on `context.rs` | **CONFIRMED** | all 20 kills are additive tests in the granted file; leader confirmed |

### Deferred, with what settles them

| # | assumption | verdict | settled by |
|---|---|---|---|
| 5 | `:1399` ×2 deferred rather than asserted from `http_integration.rs` | **DEFERRED** | a rejected-transition label assertion in `metrics_integration.rs`. `/metrics` is not mounted in the http harness (404, measured) and that file is a separate binary *precisely* to isolate the process-global recorder — asserting there would put 158 tests behind shared mutable state |
| 6 | `:1542` (did:web retract) unkillable in this unit | **DEFERRED** | an HTTPS fixture serving `agents.test`'s did.json. Verified unreachable, not assumed: a did:web-signed retract was written and dies at `key_resolution_unreachable`, because `retract_verified` resolves through a real `WebResolver` and playground does not bypass it |

Both deferrals are **real gaps, named as such**, not dismissals. The did:web one is the more
significant: that entire verification branch has no coverage and structurally cannot until the
fixture exists.

### The decision that changed shape, and why it is the instructive one

Entry 7 is the only CHANGED, and it would have shipped a guard that fails against the correct file.
The proposal — "no hardcoded 40-hex ref anywhere in `mutants.yml`" — was written from the *idea* of
the file. Run against the actual file it reports four violations, all of them correct behaviour
(pinning actions by SHA). **The lesson generalises past this guard: a sweep must be run against a
known positive AND a known negative before it is trusted, and the known negative here was the file
it is meant to protect.** A guard that cries wolf on the correct state is a guard someone deletes.

### The method finding worth carrying forward

**A survivor is a fact; "a survivor means a missing test" is an inference.** Eight of the 28 were not
gaps, and the alternative branch — the mutation changes nothing observable — is *manufactured on
purpose* by defence-in-depth. So the redundant-guard case is commonest in the most-hardened code,
which is exactly where a tenant-isolation audit points an oracle first.

The discriminator is cheap and mechanical: **two mutations at one site with opposite verdicts is
positive evidence of equivalence.** Seen twice here — `t == tenant` CAUGHT beside `delete !`
SURVIVED, and `"private"` CAUGHT beside `"public"`/`"restricted"` SURVIVED. Each time the caught
sibling proves the site is reachable and the survivor unobservable.

Recorded because I got it wrong first: `:1223` was reported to the board as an uncovered tenant gate
before `search_filters_by_tenant` — a **green** test that should have reddened — turned out to be the
evidence rather than the noise. Noticing that a passing test is the signal is the hard direction, and
the retraction is the reason the other four equivalence claims were measured rather than argued.
---

## Decision: hold acdp at 0.13.1 — 0.13.2's regression has no correct downstream fix (U-518)

**Symptom.** #285 (`deps/acdp-0.13.2`, Cargo.toml + Cargo.lock only) fails seven checks with:

```
error[E0277]: the trait bound `fn(...) -> ... {retrieve::<...>}: Handler<_, _>` is not satisfied
  --> crates/acdp-registry-core/src/lib.rs:79:42
  note: required by a bound in `axum::routing::get`
```

**Cause — established, not inferred.** Two hypotheses were tested and both were wrong before the
real one was found, which is worth recording because the error names a route line and no cause:

1. *`FullContext` lost `Serialize`.* **False** — `acdp-types`' `src/` is byte-identical between
   0.13.1 and 0.13.2, and `assert_ir::<Json<FullContext>>()` compiles.
2. *`RegistryError` lost `IntoResponse`.* **False** — `assert_ir::<Result<Json<FullContext>,
   RegistryError>>()` compiles. The return type was never the problem.

The real cause is the **other** half of axum's `Handler` bound: the handler's *future* must be
`Send`. `acdp-client` 0.13.2's issue-#264 refactor extracted a shared `discover_revocations`
(`revocation.rs:453`) taking

```rust
keep: &dyn Fn(&KeyRevocation) -> bool,
on_drop: &dyn Fn(&KeyRevocation, &CtxId, DropSite),
```

with no `Sync` bound. `&dyn Fn(..)` is `Send` only if the `dyn Fn` is `Sync`; both are held across
awaits, so the future is `!Send`, and that propagates through the public
`find_revocations` / `find_registry_attested_revocations` → `verified::verify_retrieved` →
`cross_registry::resolve` → our `retrieve` (`context.rs:913`). `acdp-client` 0.13.1's
`revocation.rs` contains **zero** `dyn Fn`; the refactor introduced them.

**Proven by construction rather than by reading:** `+ Sync` added to those two parameters in a local
copy of 0.13.2, wired in via `[patch.crates-io]`, makes `acdp-registry-core` compile clean —
including an explicit `fn is_send<T: Send>(_: T)` assertion on `retrieve`'s future.

**Decision: do not adopt 0.13.2. Stay on 0.13.1. Filed upstream as acdp-rs#279** with the diagnosis,
the verified one-line fix, and a suggested regression guard.

**Why there is no downstream fix.** The `!Send` is baked into upstream's public future types. The
three options and why each is wrong here:

- *Restructure `retrieve` to run resolution off-task* (dedicated single-thread runtime + channel) —
  a real architecture change to accommodate a bug that a one-line upstream patch fixes.
- *Carry a `[patch.crates-io]` fork* — forks a signature-verification dependency for a routine
  version bump. Kept in reserve if 0.13.2 ever becomes necessary before a fix lands; it is not
  necessary, because —
- *Nothing in 0.13.2 is needed here.* Its headline change types the `unsupported_media_type` wire
  code as `AcdpError::UnsupportedMediaType`. This repo already emits that code as a string
  (`crates/acdp-registry-core/src/extract.rs:143`, from #247) and never matches the typed variant,
  so holding costs no behaviour. The spec-pin half of 0.13.2's changelog is tracked separately in
  #272 and does not depend on the crate bump.

**No diagnostic guard was added, deliberately.** The obvious one — a `Send` assertion per handler —
means hand-listing eight signatures that drift as handlers change and that silently fail to cover a
ninth. A hand-maintained table cannot catch an omission. The reproduction recipe is recorded in
`docs/ENGINEERING-LOG.md` instead, which is drift-free and gets the next reader to the cause in
minutes. The guard that would actually work belongs upstream, in `acdp-client`'s own suite, and is
proposed in acdp-rs#279 — the breakage is invisible to `acdp-client`'s build and surfaces only in a
downstream axum consumer.

**Status:** #285 should be closed unmerged; the bump bot will re-propose once upstream ships the fix.

---

## Decision: gate `POST /contexts`'s media type without routing it through `AcdpJson` (U-520, PR A)

**The defect.** `publish` took `body: Bytes` and never read `Content-Type`. Measured across all five
scenarios of spec fixture `err-002-unsupported-media-type.json`, same body, only the header varied:

```
A  application/acdp+json                 -> 400 schema_violation
B  application/acdp+json; charset=utf-8  -> 400 schema_violation
C  text/plain                            -> 400 schema_violation   <-- MUST be 415
D  application/json                      -> 400 schema_violation
E  (absent)                              -> 400 schema_violation
```

Scenario C is a violation and the fixture rules out precisely what we emitted: *"the body MUST NOT be
parsed: `schema_violation` is NOT conformant here, because it asserts a structural validation that
never ran and is pinned to 400."* `extract.rs`'s own comment had written the same harm down while the
extractor was being built — and `publish` was then never routed through it.

**Found only because the fixture was asserted where it points.** `err-002` is replayed by nothing, and
`extractor_rejections_return_the_rfc0007_envelope` pinned C and E on `/auth/challenge`, which routes
through `AcdpJson` and was correct all along. U-519 measured that green as uninformative about the 415
path; it was concealing a live wire defect.

**Rejected: routing `publish` through `AcdpJson`** — the first thing tried, and the obvious reuse.
Measured, it moves a wrong-shaped body from **400 to 422**, because `JsonRejection::JsonDataError`
carries 422. **RFC-ACDP-0007 §5's status table pins `schema_violation` to 400.** So that fix would
have traded one conformance violation for another, on a far wider path — every malformed publish,
rather than only the ones with an unacceptable media type. This is the "turn one violation into
another" trap named in the assign, arriving from an unexpected direction.

**Adopted: a new `AcdpBytes` extractor** in `extract.rs` — the media-type gate, then the raw `Bytes`.
It reuses `AcdpRejection`, the envelope shape and the minted `unsupported_media_type` code, and leaves
the 400, the raw bytes and the 413 path exactly as they were. The whole non-comment change to
`context.rs` is two lines — the import and the parameter type — so `from_slice`, the content-hash
recomputation and signature verification are byte-identical downstream. **Nothing about what `publish`
hashes or verifies changed.**

**Scenario E (absent `Content-Type`) is ACCEPTED, and that is measured rather than preferred.** The
fixture declares E "either" and asks only that a registry document its choice. Gating absent headers
reddened **104 of 159** tests in `http_integration.rs`, every one a publish path sending no
`Content-Type` — the shape of real client breakage, not a test artefact. `/auth/*` rejects absent
headers and continues to; the fixture makes this a per-route choice.

**The duplication is pinned, not trusted.** `media_type_accepted` re-implements axum's private
`json_content_type` predicate (`mime` is not a direct dependency here, and adding one enters the
`deny.toml` gate for six lines). Two implementations of one accept-set is how routes drift, so
`the_two_media_type_gates_agree` asserts both paths agree across ten content types; breaking the
`+json` suffix rule reddens it and names the drift direction.

**Falsified, not asserted:** removing the gate reddens scenario C specifically (415 -> 400); breaking
the accept-set reddens the agreement test at `application/acdp+json`.

**Status:** applied. `/admin/*` has the same ungated shape on three handlers and is deliberately NOT
fixed here — it is U-522, so that wire change is reviewed on its own terms rather than riding in on a
publish fix.

---

## Decision: count fixtures, not families — a parallel list, not a re-keyed partition (U-520, PR B)

**The measurement.** At spec pin `16211e6` there are 144 fixtures. **11** are HTTP-replayed, **76** are
requested by a direct test, and **57 are requested by nothing**. `err-002` — the fixture U-519 found —
is one of the 57. It was never special; it was the one that happened to get noticed.

Split against `registries/profiles.json` → `acdp-registry-core`, the profile this registry advertises:
**12 required** (`pub-001/002/003/006/007/009-014`, `ret-002`), **3 conditional** (`dk-003`, `err-002`,
`idem-007`), 42 neither. `pub` claims `CoverageMechanism::Replayed` on 3 replayed fixtures while the
profile requires 14; `ret` claims it on `ret-001` alone.

**The diagnosis is "an unasked question", not "a unit that is too coarse", and the distinction picks
the fix.** The family partition answers *does this family have a coverage mechanism at all*, which
genuinely must be asked per family: `anc`/`can`/`idem`/`wit` are covered by direct in-process tests and
produce **zero** replayed exchanges, so a per-fixture replay-derived rule would brand all four
uncovered — the exact design `:443-451` records as considered and rejected. Re-keying the partition to
answer the second question would have destroyed the answer to the first.

**`DEFERRED` rejected as the vehicle** — it is family-keyed, so listing `pub` there would (1) move
`pub` out of `COVERED`, deleting the only guard that its tests still exist, (2) assert `pub` is
uncovered while 3 of its fixtures replay — false in the opposite direction, (3) break `PARTIAL_DIRECT`,
whose invariant is membership in `DEFERRED` ∪ `EXCUSED`, and (4) fail the partition test's issue
allow-list outright.

**Adopted: `UNEXERCISED_FIXTURES`, a fixture-level list ALONGSIDE the family partition**, following
`PARTIAL_DIRECT`'s precedent — which exists because *"the partition above buckets by family, which
leaves a gap once a family is only partly closable."* Same shape, one level finer. The partition is
untouched and `DEFERRED` stays empty. Anchored to **#291**.

**Every count is an equality, never a floor.** `TOTAL_FIXTURES_AT_PIN = 144`,
`REPLAYABLE_FIXTURES_AT_PIN = 11` (derived in-test from the same `extract()` the replayer dispatches
on, so it cannot drift), and 15 = 12 + 3. **A floor passes the very scanner that is silently missing
items** — which is how a fixture arriving inside an already-covered family tripped nothing.

**Falsified individually, each at a distinct assertion:** a simulated 145th fixture reddens the total
(this is the `err-002` regression, reproduced); a mis-graded entry reddens the grade check; a
non-existent id reddens the existence check; listing a replayed fixture reddens
`no_unexercised_fixture_is_actually_replayed`; a wrong replayable count and a wrong required count each
redden their own assertion.

**One of those falsifications found a defect in this unit's own work.** The grade check initially
PASSED while mis-graded, because `profiles.json`'s `profiles` is an **array**, not a map: the lookup
returned `None`, the test took its "spec unavailable" branch, and it reported green while asserting
nothing. Fixed, and the skip branch now asserts `!require_conformance()` first, so under CI's
`ACDP_REQUIRE_CONFORMANCE` an unresolvable profile is a hard failure rather than a green skip.

**Also corrected: the module doc called the `conformance` job "non-required",** contradicting its own
required-checks note further down. Measured: `contexts` is
`["rustfmt","clippy","tests","conformance (spec fixtures)"]` and `ci.yml`'s `conformance` job publishes
that fourth name. This matters here because all three new tests are spec-gated, so a non-required
conformance job would have made this whole ratchet advisory.

**Status:** applied. Covering the 12 required fixtures is deliberately NOT in this unit — it is real
conformance work, scoped from #291.

---

## Decision: the wire code decides the HTTP status, not the extractor (U-523)

**The defect.** Every wrong-shaped body on `/auth/*` answered **422 with `error.code =
"schema_violation"`**. RFC-ACDP-0007 §5 is a table of `(code, status)` pairs and it pins
`schema_violation` to **400** (`RFC-ACDP-0007-capabilities.md:228`). **422 appears nowhere in that
RFC.** This was not a debatable status choice — it was a status the protocol does not define, reaching
the wire because `AcdpJson` set `status: rej.status()` and axum's `JsonRejection::JsonDataError`
carries 422.

**The fix is structural, not a patch.** `status_for_code(code, fallback)` derives the status from the
wire code, so a response whose code and status disagree is **unrepresentable** rather than merely
tested against. It mirrors `http_status_for_acdp`'s own arms, so the extractor and the error type
cannot drift apart about the same code.

**The fallback is the part that was already right and had to be kept.** `extract.rs`'s original
comment warned in its own words: *"The rejection's OWN status, never a hard-coded 400 … hard-coding
400 would silently downgrade an oversized body."* Both rejection enums are `#[non_exhaustive]`, so an
unrecognised future variant still keeps axum's status. **413 and 415 survive**, and each is
individually falsified rather than assumed.

**`/admin/*` folded in (was U-522).** `admin_retract` and `admin_republish` were the last two routed
body-bearing handlers with no media-type gate; both now use `AcdpBytes` and take the **same
absent-header choice** as `POST /contexts`. One extractor, one accept-set — three behaviours across
three modules would have been worse than the single inconsistency this started from.

**A count worth correcting:** the unit was assigned as "three ungated body handlers" in `admin.rs`.
There are **two routed** ones. The third `body: Bytes` is the private `admin_lifecycle_transition`
helper, which is not a route and takes its bytes as an ordinary parameter — a grep counting parameter
types, not handlers.

**Falsified individually, and one falsification exposed a real gap in this work.** Reverting
`status_for_code` reddens the wrong-shape row; breaking the 413 arm reddens the oversize test;
un-gating `admin_retract` reddens the admin agreement test. Breaking the **415** arm initially reddened
**nothing**, because `AcdpBytes` hard-coded its own 415 instead of deriving it — so the two paths could
have drifted exactly as this decision claims to prevent. `AcdpBytes` now derives it too, and the same
falsification reddens both the publish matrix and `/auth/*`.

**Status:** applied. Recorded in `docs/UPGRADING.md`, not `CHANGELOG.md` — the root changelog forbids
version sections and `root_changelog_stays_a_pointer` enforces it.

## U-524 — the last two ungated routed body handlers, and the accept predicate nobody had compared

**`POST /contexts/{ctx_id}/retract` and `/republish` now use `AcdpBytes`.** They were the only
routed body-bearing handlers still parsing whatever arrived. This was never a decision: #290's grant
named `POST /contexts` and #293's named `/admin/*`, and the data-plane lifecycle writes fell in
neither. The result was backwards — the **admin** copies of retract/republish enforced a media type
while the **producer-facing** ones did not, for the same operation on the same resource.

The non-comment diff per handler is an import and a parameter type. That is the point: the body
still reaches `lifecycle_transition` as raw `Bytes`, so hashing, signature verification and
deserialization are byte-identical to before. Only the accept check is new.

**The test name was the finding.** The new matrix test was written as
`every_body_bearing_route_shares_one_media_type_gate` — 5 routes x 9 content types, asserting the
absolute 415 verdict rather than mere agreement between routes. Checking the name before shipping it
showed it was false twice over: there are **eight** routed body-bearing handlers, not five, and the
other three (`/auth/*`) do not use `media_type_accepted` at all. `AcdpJson` delegates to
`axum::extract::Json` and maps `JsonRejection::MissingJsonContentType` to the §5 code
(`extract.rs:294`). Renamed to `every_acdp_bytes_route_shares_one_media_type_gate`.

**So there are two accept predicates, and they agree by coincidence.** `media_type_accepted`
(hand-written: `application/json`, any `application/*+json`, absent accepted) and `axum::Json`'s
mime-suffix rule are independent code. They currently return the same verdict for every *present*
media type and deliberately differ on an *absent* one — `AcdpBytes` infers, `AcdpJson` 415s. Nothing
made the first property true and nothing was holding it: an axum release that narrowed its suffix
rule would split the wire behaviour of `/auth/*` from `/contexts` with no local edit at all.

**Decision: pin the relationship rather than unify the predicates.** The new
`the_two_accept_predicates_agree_on_every_present_media_type` asserts agreement across all nine
present types and asserts the absent-header divergence **in both directions**. Unifying them was the
tempting alternative and is rejected: routing `AcdpJson` through `media_type_accepted` would start
accepting untyped bodies on `/auth/*`, the most attacker-controllable surface in the service
(`lib.rs:150`) — a wire change, dressed as a consistency cleanup. The second assertion exists
specifically so that cleanup reddens.

**Falsified, four ways, each naming its own mechanism.** Dropping the `+json` suffix rule splits the
families on `application/acdp+json`; making `AcdpBytes` reject absent reddens the `/contexts`
direction; remapping `MissingJsonContentType` off 415 splits them on `text/plain`; making `AcdpJson`
infer an absent header reddens the `/auth/*` direction. That fourth one was necessary, not
redundant: axum returns `MissingJsonContentType` for a *wrong* content type as well as an absent
one, so falsification three tripped the present-type loop and **never reached** the final assertion.
An earlier assertion masking a later one is exactly the failure `falsify each assertion, not each
test` describes, and it was live here.

**Status:** applied. Wire change recorded in `docs/UPGRADING.md` under 0.1.4, not `CHANGELOG.md`.

## U-527 — the replayer understood the minority spelling

**`extract()` gained Shape E, for `input.endpoint` + `input.body`.** Measured at pin `16211e6`: **6**
of 144 fixtures use the `request.method`+`path` spelling Shape A reads; **65** use `input.endpoint`.
Shape C handled exactly one endpoint literal (`GET /contexts/{ctx_id}`); everything else fell to the
`"non-HTTP fixture (vectors / schema / informative)"` fallback.

**The defect was the reason string, not the count.** Those fixtures declare a method, a path, a body
and an expected status. Reporting them as "non-HTTP" told every reader the wrong thing to do about
them, which is how six of issue #291's twelve stayed unread long enough to become an issue. A count
says a fixture is unexercised; a reason says what would fix it.

**Scope: fixtures with a CONCRETE body only — 12 of the 65.** The other 53 describe the request in
prose (`body_summary: "Concrete payload omitted..."`). Supporting bodyless GETs would additionally
admit `cur-001`, whose endpoint embeds `<previously-issued-cursor>` — an **angle**-bracket
placeholder the template gate does not catch, since it looks for `{`/`}`. It would replay a literal
placeholder in the query string and still receive its expected 400, passing for entirely the wrong
reason. `cur-002` is fully concrete and would be a real win; it is deliberately left, because it and
the `<...>` gate extension must land together. **Recorded rather than silently skipped.**

**`pub-001` and `pub-011` are excluded by a named predicate, and this is not working the failure
around.** `config()` sets `playground.enabled = true`, which by its own comment bypasses DID
verification, so this harness cannot reach a signature-verification outcome at all — `pub-001`
measurably replays to **200, publish accepted**, against an expected 400 `invalid_signature`, with a
signature of 64 literal `A`s. Skipping them with the real reason is what every other arm of
`extract()` does. Both stay in `UNEXERCISED_FIXTURES` pointing at U-528.

**`pub-011` is the sharper half and the reason the exclusion is a predicate rather than a fix.** It
expects the same code and *would have replayed green*: its `content_hash` is the literal placeholder
`"sha256:<recomputes-correctly-against-this-body>"` and the publish arm pins no error code, so a
schema rejection would have been scored as signature coverage. **Admitting it would have added a fake
green, which is worse than the honest gap it replaced.** Per the assign it is U-528's; nothing here
tries to fix it.

**`MIN_REPLAYED_EXCHANGES` (a `>=` floor) became `REPLAYED_EXCHANGES_AT_PIN = 38` (an equality).** A
floor cannot catch the failure it exists for: a dispatch bug that stops matching fixtures leaves the
count *lower*, and any number above the floor satisfies it. **U-527 is its own proof** — `extract()`
was silently declining 12 parseable fixtures and the floor read healthy the whole time. Falsified:
dropping one fixture yields 37, which passes `>= 30` and fails the equality.

**Status:** applied. 30 → 38 exchanges, 11 → 19 replayable fixtures, required-but-unexercised 12 → 8.
No wire change, so nothing in `docs/UPGRADING.md`.

## U-528 — the harness could not check a signature, and the tests could not tell

**The replayer now pins the fixture producer's key, and the U-527 predicate is deleted, not narrowed.**
`config()` sets `playground.enabled = true`, which skips DID verification, so every replayed signature
reached the store unexamined — `pub-001` publishes a signature of 64 literal `A`s and was **accepted
with a 200** against its expected 400 `invalid_signature`.

**Turning the playground off was the obvious fix and is the wrong one.** It would require a live
`did:web` resolver — DNS and TLS — in-process for every replayed publish. `playground.pinned_keys`
already performs **real** `acdp::crypto::verify` Ed25519 verification of a `did:web` producer with no
resolver; it is the mechanism `sig001_*`/`rev001_*` in this file have used all along. U-528 points the
replayer at it.

**`pinned_only = false`, deliberately** — that is the blast-radius decision. Strict mode would reject
every other agent with `key_not_authorized`, rewriting the verdict of fixtures that have nothing to do
with signatures. And `replay_harness()` is kept separate from `harness()` (eight other callers) and
from `shape_d_config()`, because `idem_playground_branch_honors_supports_idempotency_key_gate` depends
on `pinned_keys` being **empty** as its precondition, and `replay_shape_d` panics if a seeded publish
fails to return 200.

**The second fix, which the harness change alone would have hidden.** The publish arm pinned no error
code, so a publish fixture asserted only *"some 400"*. `pub-011` is the proof: with the key un-pinned
it receives `schema_violation: content_hash digest must be 64 lowercase hex chars, got:
<recomputes-correctly-against-this-body>` and **passes anyway**, scored as `invalid_signature`
coverage. Fixing only the harness leaves that intact. Codes are now pinned for publishes too.

**Pinning the codes exposed five wrong-reason passes, four of them mine.** `did-ssrf-001..004` — which
U-527 lit up — return `schema_violation` (their bodies omit the required `version` member) and never
reach DID resolution at all. And `pub-002` moved *because of this unit*: unpinned it returned
`hash_mismatch` matching its fixture, but the pinned path verifies the signature before the hash gate,
and its body fails both.

**Recorded in `CODE_DIVERGENCES`, not skipped and not excused.** Each entry names the code this
registry actually returns and why; the replayer asserts that code, so any of them changing in either
direction fails the build. Skipping them would have removed the coverage; ignoring them would have
kept the wrong-reason pass. `code_divergences_are_real_live_and_still_divergent` additionally fails if
an entry's fixture stops replaying or if its recorded code becomes the expected one — **an unexercised
excuse reads exactly like a live one.**

**`FIXTURE_PRODUCER_PUBLIC_KEY_B64` is checked against the spec.** A rotated keypair would verify every
signature fixture against the wrong key and **still go green**, because those fixtures expect a
rejection — a wrong key produces green exactly where a right key does.
`fixture_producer_key_still_matches_the_spec` closes that.

**Status:** applied. 38 → 40 exchanges, 19 → 21 replayable, required-but-unexercised **8 → 6**
(`pub-001`, `pub-011`). No wire change.

## U-531 — the gate knew one notation, and the sweep found a fourth wrong-reason pass

**The template gate now covers both placeholder notations.** It tested `path.contains('{')` and
`'}'` only. The spec also writes placeholders with **angle** brackets — `cur-001`'s endpoint carries
`cursor=<previously-issued-cursor>` — and a brace-only gate waves those straight through. The
consequence is not a crash but a **pass**: the literal text is a perfectly good malformed cursor, so
the registry returns the 400 the fixture expects and the fixture is scored green having tested nothing
about expired cursors.

**Its test now varies the spelling** — five placeholder paths across both notations and both path and
query position, plus a placeholder-free complement so the gate cannot satisfy everything by rejecting
everything. A count-based assertion ("N fixtures are gated") would have passed throughout the entire
blind period, which is how the gate arrived here half-blind.

**`cur-002` admitted, `cur-001` excluded on its own merits.** U-527 required a concrete body for every
method, which excluded `cur-002` (a fully concrete search request) purely to avoid admitting
`cur-001`. With the gate fixed, that blanket exclusion is no longer load-bearing, so Shape E accepts a
bodyless `GET`/`HEAD`. `cur-002` replays **and is checked** — it returns its expected `invalid_cursor`.
`cur-001` is skipped by the gate with a written reason.

**The sweep found a fourth instance, older than any of them.** With `CODE_DIVERGENCES` in place the
question was one grep: exactly one `want_error_code: None` remained, in **Shape A's publish arm**, and
`pub-008` was passing behind it. That fixture exists to prove a non-`did:web` `agent_id` is rejected;
its `signature.value` is 96 base64 chars where ed25519 requires 88, so signature-shape validation
rejects it first and the `agent_id` rule is never reached. It had been replaying green since long
before U-527.

**A pre-existing assertion was deliberately inverted.**
`four_pre_existing_exchanges_still_use_original_shapes` asserted
`want_error_code.is_none()` — *"Shape A's publish branch never pins an error code, this must still
hold"*. That invariant is what let `pub-008` hide. The ordering argument behind it was never wrong; it
simply does not justify asserting **nothing**. Where this registry genuinely orders validation
differently, `CODE_DIVERGENCES` records the code it does return, which is a stronger statement than
silence.

**The negative result is now asserted, not re-derivable.**
`every_replayed_fixture_pins_a_code_when_it_names_one` makes the rule structural: if a fixture supplies
an `error_code`, the exchange built from it must pin a code — its own, or a recorded divergence. No
future shape can opt out by leaving it `None`. 15 replayed fixtures name a code at the pin, and the
bound is asserted so an empty scan cannot read as clean. **My first guess at that bound was 14 and the
known-positive check caught it** — the guard's first act was to correct its author.

**Status:** applied. 40 → 41 exchanges, 21 → 22 replayable. **`required-but-unexercised` does not
move: it stays 6** — `cur-001`/`cur-002` are not profile-required. No wire change.

## Unit U-507 — reconciling `ASSUMPTIONS.md`'s open entries (lane-3, 2026-09-13)

### 1. The scope figure was the first deliverable, because three incompatible ones existed — CONFIRMED (Opus)

`grep -c UNCONFIRMED` = **50** counts prose (a `**Correction:**` narrating a *past* status; a line
cross-referencing a flip). An `^`-anchored `**Status:**` pattern = **28** misses five other live
shapes: mid-prose-line statuses, a parenthetical between key and colon, a token **wrapped onto the
next line**, and — 13 times — a **bullet or heading label with no `Status` word at all**. A careful
hand count = **35**, scoped to "deferred items" and predating eight later entries.

Measured: **37 items carrying 46 open declarations.** The hand count and this parser agree *exactly*
at 37 declarations before `:2537` — two methods built from opposite directions reaching the same
partition, which is the strongest available evidence for a census of this kind.

**The tool is shipped, at `docs/assumptions-status-census.py`**, because a number whose predicate
lives in a gitignored scratch file is not reproducible by a reviewer. It asserts its own partition is
total (`open + resolved + unrecognised == all`), so an unseen sixth shape fails the run rather than
silently lowering the count.

### 2. The grant's mechanism fit 6 of 41, and the fix preserved the auditable property — CONFIRMED (leader, on this lane's recommendation)

The unit was granted in-place rewrites of the status **token** only. Measured against the real file
that covers **6 of 41** remaining declarations: **22** carry their reason on the same line, where a
token-only flip yields a self-contradicting line, and **13** are label-form with no token to rewrite.
Batch 1 shipped one instance of the failure — `CONFIRMED (awaiting a repo admin to action the
branch-protection change)` — and it was **flagged in the file rather than fixed by self-widening the
grant**. Escalated with a recommendation; granted as recommended: rewrite a complete status *line*,
token plus reason clause, nothing else. Deletions still equal status lines rewritten, so the
reviewer's mechanical check did not weaken — only its denominator changed.

Root cause, as the leader recorded it: the grant was built around the one shape its undercounting
pattern could see, so the wrong count and the wrong spec were **one error surfacing twice**.

### 3. AC1's equality earned its keep — CONFIRMED (Opus)

    open items at unit start                 37
    fully resolved                           24
    deliberately still open (AC4/AC5)        13
    open declarations           46  ->  16   (30 flipped to a resolved token)
    status lines rewritten / deletions       51  (verified after the final merge)

The equality **caught two declarations this unit had walked past** — `:928`, a *second* declaration
inside an item whose first was already resolved, and the `CtxId` latent. A floor (`>= 20 resolved`)
would have passed with both still open. That is the whole argument for requiring an equality.

### 4. Three entries had already closed themselves, and three specified how to tell — CONFIRMED (Opus)

Closed by other work landing, with nobody going back to flip them: the shared playground validator's
placement (**#192/#193**), CI never exercising the shipped stack (**#270**), and `/metrics`'
cache posture (**#218**). The best-designed entries in the file **named their own closure signal** —
`/metrics`' said *"deleting that line is how the fix announces itself"*, and A2's said it must not be
called closed until a named test was deliberately deleted. Every one of those signals had already
fired. **Worth copying: an entry that specifies how its own resolution will be detectable.**

### 5. What this unit declined to do

- **Did not close anything by inertia.** `:928` and `:2372` stay open because "nobody objected for
  three days" is not evidence — and one of them forbids that reasoning in its own text.
- **Did not resolve what belongs to others.** Four entries are the human's or the leader's; each
  gained **Settled by** and **Owner** and kept its open token. Owners were *split* rather than
  lumped, because this file has used "escalated" for both.
- **Did not repair out-of-grant defects.** `crates/acdp-registry-core/src/handlers/context.rs:1262-1277`
  is stale — it cites a deleted test as machine-checking a residue and still calls landed work a
  requirement. Reported per U-505's precedent, not fixed.
---

## U-535 — the §4.5 parity seam anchors on a hand-transcribed spec table

**Decision:** add `EXPECTED_BY_SPEC` to `crates/acdp-registry-store/src/parity.rs` — a literal,
per-requester table of expected `ctx_id`s transcribed by hand from RFC-ACDP-0008 §4.5 — and compare
**both** the backend's SQL predicate and the Rust `retrieve_visible` rule against it. Correct the
harness's doc comments to claim only what is actually compared.

**What was wrong, and it is worse than a weak test.** The harness documented a "three-way seam"
(SQLite / Postgres / Rust default). Both of its differentials in fact bottomed out in one expression:
the trait default body *is* `retrieve_visible` (`acdp-registry-store/src/lib.rs`), and the N-call
reference `visible_by_n_calls` calls `retrieve_visible` too. Three comparisons, one anchor.

The consequence is not merely lost sensitivity. If `retrieve_visible` were wrong, the default would
agree with the reference perfectly, that leg would stay **green**, and the only implementation able to
disagree would be the SQL — which was derived independently. **The suite would have named the SQL as
the broken side.** A shared-centre differential *inverts the blame* onto the one implementation that
is still correct.

**Measured, not argued.** Breaking `retrieve_visible` (`None => false` → `None => true`) at baseline
produced, from the old legs: `over-disclosure: []` plus four raw `ctx_id`s listed as
"SEEN ONLY BY THE N-CALL REFERENCE (under-disclosure)" — i.e. the SQL accused of *under*-disclosing,
when the SQL was right and the reference was over-disclosing. The new anchor named
`the Rust retrieve_visible rule` 4 times and the SQL 0 times.

**Why a literal table and not a computation.** Deriving the expectation from anything in this
workspace reintroduces the defect. The table is written from the spec text, and failures report
fixture role names rather than nonce-bearing `ctx_id`s.

**Rejected: making the third leg independent by calling the upstream authority.** `can_retrieve`
(`acdp-server` `src/registry/server.rs`) is `pub(crate)`; no test here can call it. That premise is
**true** — it was verified against the resolved crate source, not assumed. So the upstream comparison
is recorded as a standing **manual** check: hand-diffed against `acdp-server` **0.13.1** on
**2026-09-13**, normalising only `caps.anonymous_public_reads` → `public_arm_open`, and the two match
arms were **textually identical**. Uncompared, not covered — re-run when the pin moves.

**Also closed:** `private_owner_only` and `restricted_with_audience` occurred exactly twice each
(published, pushed into the id list) and were never asserted on. Both now carry named absolute
outsider pins, and both are covered by the table across all six perspectives.

**Status:** applied. Test-only; no wire, schema or behaviour change. `retrieve_visible` was **not**
modified — changing what a visibility predicate decides is a security change and is out of scope for
test hardening.

## U-536 — the spec pin becomes one declarative source (lane-2, 2026-09-13)

Three decisions, all reversible, all settled by Opus inside the unit; none reached the human.
One supersedes a prior CONFIRMED decision in this file.

### 1. The pin moves out of `ci.yml` into `.spec-pin` — SUPERSEDES decision 9 above (line 2535)

- **Prior decision:** "`mutants.yml` derives the spec pin rather than duplicating it —
  CONFIRMED (Opus)" (line 2535). A `pin` step in `mutants.yml` grepped the 40-hex `ref:` out of
  `ci.yml` so the two jobs could not drift apart. That was the right call against the
  alternative it was compared to — a pasted second copy, which the bumper would never rewrite.
- **Why it is superseded, not reversed:** the derivation preserved the property but paid for it
  with a coupling that is invisible from either file. Nothing in `ci.yml` said another workflow
  parsed it, and a perfectly valid reindentation of its spec step broke the derivation
  *silently* — surfacing on the following Monday's cron, because `mutants.yml` has no
  `pull_request` trigger, in a job whose failure reads as "the ratchet is broken" rather than
  "someone moved a line in a different file".
- **Chosen implementation:** `.spec-pin` at the repo root as the single declarative source,
  read by both workflows through one composite action (`.github/actions/read-spec-pin`), by the
  bumper (`bump-spec.yml` now passes it `.spec-pin`), and by the conformance harness itself.
- **Rejected — the same ~15 lines of shell pasted into both workflows, plus a test that the two
  copies stay identical.** That test guards the *spelling*: it breaks on a reindentation and
  passes on a semantic change. `spec_pin_violations` instead asserts the property — nobody
  restates the pin, everybody uses the reader — which a human can check by inspection.
- **Status:** applied (`e21375b`, `67a4cac`). Decision 9's property is preserved and now has ten
  falsified invariants behind it instead of four.

### 2. The pin verdict is decided by CONTENT, never by git

- **Assumption:** a harness could identify "is this tree at the pin?" with `git rev-parse HEAD`.
- **Why that is wrong, and not a preference:** the way to materialise an exact revision locally
  is `git archive <sha> | tar x`, whose output carries no git metadata at all. The tree that
  must PASS is therefore precisely the one `git rev-parse` cannot identify, so a git-based check
  rejects the correct input. Git is used only to *name* a revision once found.
- **Evidence:** one digest covers an archive extract, an independent second extract, and a real
  `clone` + `checkout` of the pin (what `actions/checkout` produces) — all
  `rfc6962-sha256:03644a90…`. Zero `.gitattributes` in the 253-file tree at the pin rules out a
  filter making the two diverge; that grep was validated against a known positive first.
- **Rejected — `std::collections::hash_map::DefaultHasher`:** `std` does not promise its output
  is stable across releases, which is fatal for a value committed to a file. RFC 6962 via
  `acdp::crypto::merkle` was already reachable, so this added no dependency.
- **Status:** applied. Ten mutations, each RED at its own assertion with its own message.

### 3. Adopting a revision now costs three edits, and that cost is accepted

- **The cost:** `ref:`, `conformance-digest:` and `TOTAL_FIXTURES_AT_PIN` all change, and the
  bumper rewrites only the first. A `bump spec` PR therefore arrives RED on `conformance`.
- **Why accepted:** `TOTAL_FIXTURES_AT_PIN` already had that property, so such a PR was never
  green on arrival; the PR is held for review and never auto-merged; and the failure message
  itself carries the replacement digest, computed from the tree in front of it, so the fix is a
  copy rather than a second command to look up. The alternative — deriving the
  digest at test time from whatever tree is present — is the hole this unit exists to close.
- **Rejected — auto-updating the digest in the bump PR.** That would make the bot's PR
  self-certifying: it would rewrite the value that proves the tree is what the bot says it is.
- **Status:** applied, stated in `.spec-pin` beside the value rather than in a footnote, with
  the procedure in `CONTRIBUTING.md`.

**Two references this unit made stale, both outside its path grant and neither edited here:**
`ASSUMPTIONS.md:2872` (entry 9 describes the retired derivation) and `ASSUMPTIONS.md:315`
(records that the bumper needs `permission-workflows: write` *because* the pin lives under
`.github/workflows/` — that reasoning no longer holds; the pin is now a root-level data file, so
the scope is no longer load-bearing for the spec bump, and the change moved in the safe
direction). Reported to the leader rather than edited.
