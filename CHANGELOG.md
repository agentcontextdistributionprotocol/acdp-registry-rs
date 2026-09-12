# Changelog

All notable changes to this project will be documented in this file. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- **Security (availability): `GET /contexts/search?limit=` could abort the registry
  process from an unauthenticated request.** The handler sized its accumulator with
  `Vec::with_capacity` directly from the caller-supplied `limit`, using `.max(1)` — a
  floor, with no upper bound. `limit` is a `u32` off the query string and the route is
  reachable without an `Authorization` header, so `?limit=4294967295` requested 893 GB in
  one allocation and the process aborted with SIGABRT. The store-side `.min(100)` did not
  help: it runs after the accumulator is allocated. `limit` is now clamped in the handler
  to the same cap the stores enforce. Regression test landed deliberately red first, so
  the PR's own CI history shows it catching the live defect.


### Changed

- **Wire behaviour: the registry now emits a cache posture on requester-relative
  responses** (#205, the wire half of #190). Additive response headers — no
  client parses their absence and nothing about request handling changes — but
  it is a wire change, which is why #205 was split out of #190 rather than
  shipped with the docs fix.

  Requester-relative routes (`GET /contexts/{ctx_id}`, `/contexts/{ctx_id}/body`,
  `GET /contexts/search`, `GET /lineages/{lineage_id}`,
  `/lineages/{lineage_id}/current`, `GET /log/checkpoint`, `GET /log/proof`,
  `GET /log/entries`, and the publish/retract/republish `POST`s) now answer
  `Cache-Control: private` and `Vary: authorization, x-tenant-id`. `/auth/*`,
  `/admin/*` and `GET /healthz` answer `Cache-Control: no-store`. The three
  `/.well-known/*` documents are unchanged at `public, max-age=300`, and a test
  pins all three. `GET /metrics` is knowingly outside the posture and is tracked
  as #218.

  **No live cache-poisoning bug existed** — every header the registry emitted
  was already on a requester-invariant document. This closes a hardening gap:
  requester-relative routes carried no directive at all, which is safe only for
  as long as nobody puts a CDN with a default TTL in front of the registry.

  `private` rather than `no-store` on the data plane: the threat is shared
  caches, which `private` excludes precisely, whereas `no-store` only adds
  protection against caches that ignore directives anyway while forbidding
  legitimate same-requester client caching. `Vary` names both axes because
  `x-tenant-id` is the only tenant signal when `auth.enabled = false`.

<!-- #130 conformance reclassification -->

- **`rcpt`, `lhr` and `log` move from `DEFERRED` to `EXCUSED`, closing #130; the
  coverage tally is now 21 `COVERED` / 8 `EXCUSED` / 0 `DEFERRED`** (29 families).
  Tests only — no runtime, API or wire-format change.

  The three families were never uncovered in the sense `DEFERRED` means. Each
  splits in two: a producer/emission half this registry implements, whose spec
  goldens are recomputed here (`rcpt-001`, `lhr-001`, `log-001`, `log-003`), and a
  consumer-role **verification** half. Only the verification residue was deferred,
  and it is not coverable by this harness at all: those fixtures name profiles
  `HARNESS_PROFILES` does not advertise and carry no `endpoint` and no `vectors`
  array, so a "direct pass" over them would exercise `acdp-types`/`acdp-crypto`
  rather than this registry. Verification is the consumer's obligation under
  RFC-ACDP-0010 §8 (receipts), RFC-ACDP-0011 (lineage-head receipts) and
  RFC-ACDP-0012 (transparency logs).

  `DEFERRED`'s contract is that every entry cites an **open** tracking issue, so
  closing #130 forced the choice: reclassify, or leave the ratchet citing a closed
  issue. They are excused on the **obligation-ownership** ground — the strong kind,
  the same one `rot` rests on, where no harness configuration change could make
  this registry responsible — and explicitly *not* on the weaker current-profile
  ground `lc` rests on. `EXCUSED`'s doc comment now names those two grades and
  requires every entry to say which it claims.

  **This extends the REG-11 Phase 14 `lc` ruling (a user decision) to three further
  families, with the maintainer's explicit approval.** The extension is stated in
  the code, at each moved entry and in the block comment above them, rather than
  swept in silently.

  Rule 1 (`no_excused_family_is_required_by_our_profile`) still holds: none of the
  three appears in `acdp-registry-core`'s `required_fixtures` or
  `conditional_fixtures` at spec pin `d1f06d0`, and none is in
  `CORE_INEXCUSABLE_FAMILIES`.

- **`DEFERRED_PARTIAL_DIRECT` is now `PARTIAL_DIRECT`, and its invariant widened
  from `DEFERRED` to `DEFERRED` ∪ `EXCUSED`.** The old name and invariant coupled
  four good golden-test pins to one bucket, so a reclassification that changes
  nothing about the pinned coverage would have reddened all four. The half that
  carries the weight is unchanged and still enforced: a `COVERED` family must not
  appear there, because its tests belong in `COVERED`'s own `Direct(...)` list.
  Both enforcement tests were renamed and generalized to scan `EXCUSED` reasons
  alongside `DEFERRED` ones:
  `deferred_partial_direct_test_functions_are_present` →
  `partial_direct_test_functions_are_present`, and
  `deferred_reasons_naming_golden_tests_are_pinned_by_partial_direct` →
  `classification_reasons_naming_golden_tests_are_pinned_by_partial_direct`.

- **Anti-vacuity work forced by `DEFERRED` becoming empty.** The prose/pin tie
  iterated `DEFERRED` alone and would have silently become a no-op. It now scans
  both buckets *and* checks the forward direction — every test `PARTIAL_DIRECT`
  pins must be named by its own family's reason prose — so a pin and the sentence
  justifying it can no longer be deleted independently. `PARTIAL_DIRECT` is also
  asserted non-empty. The partition test itself is unaffected: its real property
  (every `KNOWN_FAMILIES` family classified exactly once) is proven by its loop
  over `KNOWN_FAMILIES`, not over `DEFERRED`.

  Both mutations were verified red before being relied on: renaming
  `rcpt001_registry_receipt_golden_recomputed_and_remintable` fails
  `partial_direct_test_functions_are_present`, and deleting its name from `rcpt`'s
  reason prose fails
  `classification_reasons_naming_golden_tests_are_pinned_by_partial_direct`.

- **The mutation-oracle thread moved from #130 to #216.** The file's own
  anti-vacuity guards are substring oracles over `include_str!` that catch
  wholesale deletion and gutting but not `assert!(true)`-grade hollowing; the real
  fix is `cargo-mutants` or a fault-injection harness. #130 had been the de-facto
  anchor for that separate concern, so it was split out before #130 closed.

- **Corrected two false security claims in the Railway recipe, and disclosed its
  read posture** (#208). [`docker/RAILWAY.md`](docker/RAILWAY.md) claimed the
  `JWT_SECRET` "is never validated" with auth off, and that the `changeme`
  rejection "is gated on auth being enabled". Both have been false since W3-U5:
  the `changeme` and base64/≥32-byte checks run at startup whenever the secret is
  non-empty on HS256, regardless of `auth.enabled` — only the *empty-secret*
  check is auth-gated (`validate_config`,
  `crates/acdp-registry-server/src/main.rs`). No boot outcome changes; the doc
  now matches the binary.

  The recipe also now states its **read posture** plainly, which it never did:
  with auth off and `auth.anonymous_public_reads` at its default `false`, no
  context is readable by anyone — `public` included — because the retrieval
  predicate is `anonymous_public_reads || requester.is_some()` and auth-off makes
  every caller anonymous. Publishing still works. The recipe documents both
  opt-in flags and what each one opens, including that enabling auth lets **any**
  valid token holder read every `public` context: token issuance gates on the
  challenge binding, expiry, algorithm and DID method (default `["did:web"]`),
  with no agent allowlist, so the bar is control of any domain serving a
  `did.json`.

  **The recipe keeps auth OFF.** The held W2-U3 patch that would have enabled it
  is superseded and must not land: its prose predates W3-U5 and misstates the
  secret-validation gating, and enabling auth would have widened `public` reads
  through a flag named for something else.

### Added

<!-- W3-U10 -->

- **CI now builds all eight valid feature configurations, not four** (#200). The
  `clippy` job gained four steps: `storage-pg,playground`,
  `storage-memory,playground`, no-backend (`--no-default-features`), and
  `playground` with no backend. The space is 4 backend states (sqlite | pg |
  memory | none) x playground on/off; the four new ones were valid, buildable
  and never exercised, so a change could break one and land green.

  No wire, API or runtime behaviour changes. The two backend-less
  configurations were warning-dirty and would have failed under CI's
  `RUSTFLAGS: "-D warnings"`, so `crates/acdp-registry-server/src/main.rs` gained
  a `cfg_attr`-scoped `allow(dead_code)` that applies **only**
  when no storage backend is enabled — the configuration in which `run()` is
  deliberately inert and the entire serve path is unreachable by construction.
  Every configuration that can actually serve still treats `dead_code` as a hard
  error.

  Two things #200 did not know, recorded because they cost time to find. The
  issue lists three missing configurations; there are four — `playground` with
  no backend was missed, and it was broken in exactly the same way. And the
  issue's suggested fix, applied literally, would have turned CI red on the
  first run: the backend-less builds emit 7 dead-code warnings in the bin target
  and 2 in the test target, which `-D warnings` promotes to errors.

  `--all-features` remains impossible here by design and is not the fix — it
  enables all three backends at once and trips the `compile_error!` at
  `main.rs:45-54`, which rejects any pair. That is why coverage is enumerated,
  and the `ci.yml` comment above the new steps is now the single index of the
  space, with the arithmetic that generates the count.

  Scope: eight is **this binary's** feature space, not the workspace's.
  `acdp-registry-types` builds without its default `axum` feature — a supported
  consumer scenario per its own manifest comment — and no CI job builds it.
  #200 assumed that one was covered incidentally; it is not. Filed as #221
  rather than widened into this change.

<!-- REG-11 Phase 7 -->

- **`lin` and `caps` move from `DEFERRED` to `COVERED`** (`REG-11` Phase 7,
  `#115`): two new direct-vector tests in
  `crates/acdp-registry-server/tests/conformance.rs`.
  - `lin_vectors_reproduce_lineage_derivation` reuses the existing
    `assert_lineage_vector` helper (previously exercised only via
    can-001) against `lin-001-lineage-derivation-golden`'s 3 vectors.
    `lin-001` carries `applies_to_profiles: ["acdp-registry-core",
    "acdp-consumer"]`, but this is a pure-function check of
    `derive_lineage_id` with no HTTP leg, so it deliberately bypasses the
    runtime profile gate rather than adding one.
  - `caps_vectors_validate_capabilities_document` runs all 7 caps-*
    fixtures' `input.response_body` through
    `acdp::validation::validate_capabilities`, plus caps-007's 3
    `reject_variants` (hand-applied against the single dotted path they
    override, `limits.max_publish_per_minute`). Measured against the
    fixtures directly: caps-001/006/007(base) accept and
    caps-002/003/004/005 plus caps-007's 3 variants reject — 4 rejecting
    base fixtures, not 6. caps-006 in particular is an *accept* case:
    the `CapabilitiesDocument` schema tolerates unknown top-level fields
    (only its `limits` sub-object is closed). Rejection is accepted from
    either serde deserialization or `validate_capabilities` itself, since
    both are `schema_violation` and indistinguishable to a real consumer.
    No HTTP leg here either: `acdp-registry-server` is bin-only, so this
    crate cannot import its own `build_capabilities` for an HTTP-level
    comparison.

  `lc` (the third family originally filed under `#115`) remains
  `DEFERRED` — it is profile-gated, not closeable by a direct-vector pass.
  `MIN_REPLAYED_EXCHANGES` (30) and the exchange replay count are
  unaffected: both new tests are non-HTTP vector passes.

<!-- end REG-11 Phase 7 -->

<!-- REG-11 Phase 9 (Lane B) -->

- **`GET /healthz` now reports the running build** (`REG-11` Phase 9,
  `#117`): the body carries a top-level `version` alongside `status` and
  `storage`, on both the `200` and the `503`/degraded response — build
  identity matters most when the service is unhealthy, which is also the
  precedent `acdp-control-plane` sets in its own health tests.

  The frozen contract is one field, not a body shape:

  > `GET /healthz` MUST include a top-level `"version"`: a non-empty,
  > human-readable identifier of the running build. It SHOULD begin with
  > the package's SemVer and MAY carry SemVer build metadata
  > (`+g<shortsha>`). **Consumers MUST treat it as opaque** — display or
  > equality at most, never parsing.

  The value is composed as `CARGO_PKG_VERSION` plus an optional
  `+g<shortsha>`, because every workspace crate still carries a
  placeholder `0.1.0` and the package version alone would not distinguish
  two builds. CI injects the commit through a new `ACDP_BUILD_SHA` build
  ARG in `docker/Dockerfile`, fed from `.github/workflows/docker.yml`.
  **Outside `docker.yml` — a local `cargo run`, or any other image build —
  `ACDP_BUILD_SHA` is unset and `version` degrades to the bare `0.1.0`,
  which does not uniquely identify a build.** The unique-identification
  property holds for images built by `docker.yml`, not universally. No new
  dependency and no `build.rs`: `.dockerignore` excludes `.git/`, so a
  `git describe` at build time could not work.

  The ARG is declared *after* the `cargo chef cook` layer. Its value
  changes every commit, so declaring it earlier would invalidate the
  dependency cache the two-stage build exists for; the final layer
  rebuilds each commit regardless, so it costs nothing there.
  `docker.yml`'s smoke step now asserts the running image's `version`
  carries the commit it was built from, so the injection path is covered
  by CI rather than assumed.

- **`GET /admin/status` gains a `build` group** (`REG-11` Phase 9,
  `#117`): `version` (the same string `/healthz` serves), `commit`, and
  `storage_impl`. Coarse identity stays public; the finer detail is
  disclosed only behind the existing admin bearer. The endpoint's other
  groups are unchanged and it remains bearer-gated.

  `commit` is **omitted entirely** when `ACDP_BUILD_SHA` was unset. That
  absence is meaningful rather than an error: it is how an operator tells
  a `docker.yml`-built image from any other build without reading docs.

  `storage_impl` reports the compiled-in store *type* (from
  `std::any::type_name`), not a `storage-*` Cargo feature name. It is an
  **opaque diagnostic identifier**: `type_name` output carries no
  stability guarantee and may change across compiler versions, so it must
  be displayed rather than parsed or branched on. This deviates from the
  REG-11 plan, which specified a `storage_feature` string — those
  features are declared on `acdp-registry-server`, not on
  `acdp-registry-core` where the handler lives, so `cfg!(feature =
  "storage-sqlite")` there does not merely evaluate false, it fails to
  compile under the `-D unexpected-cfgs` implied by CI's `-D warnings`.
  `acdp-registry-core` is generic over `S: ExtendedRegistryStore`
  precisely so it need not know about storage backends.

  Closing `#117` unblocks `acdp-ui-console#64`; it does not by itself
  make that console display a live registry version, which needs its own
  change there.

<!-- end REG-11 Phase 9 -->

<!-- REG-11 Phase 10 -->

- **`meta` and `data-ref` move from `DEFERRED` to `COVERED`** (`REG-11`
  Phase 10, `#130`): two new direct-splice tests in
  `crates/acdp-registry-server/tests/conformance.rs`, following the
  fragment-splice-into-a-self-signed-publish technique
  `anc001_well_formed_anchor_is_accepted_and_round_trips`/
  `anc002_malformed_anchor_content_hash_is_rejected` established — neither
  family is reachable through the generic replayer (both carry only an
  `input.*_under_test` fragment, no top-level `request`, and meta-003
  expects a positive publish outcome the replayer's Shape A refuses by
  design).
  - `meta001_003_metadata_depth_and_size_caps_enforced` covers all 3
    `meta-*` fixtures (RFC-ACDP-0002 §3.3/§5.2): meta-001's concrete
    depth-9 metadata and meta-003's depth-8 boundary are used verbatim;
    meta-002 carries no concrete payload at spec pin `d1f06d0` (only a
    described construction), so this test builds the described "100 keys,
    each ~700 bytes" shape itself and proves — via
    `acdp::crypto::canonicalize_value` — that it clears the fixture's own
    65536-byte JCS boundary before sending it. meta-001/002 splice their
    metadata onto a metadata-free base request's struct literal (the
    anc-002 technique: `RequestBuilder::build()` would otherwise reject
    them client-side); meta-003 builds normally and round-trips (served
    body + recomputed `content_hash`), mirroring anc-001.
  - `data_ref001_007_publish_path_rejections_enforced` covers
    data-ref-001..007 (RFC-ACDP-0002 §6) — the 7 DataRef publish-path
    rejections in `acdp-registry-core`'s `required_fixtures`, verified
    directly against `registries/profiles.json` at this pin.
    `data-ref-008-external-data-ref-hash-mismatch` is
    `applies_to_profiles: [acdp-consumer]` only (a consumer fetch-time
    check) and is correctly excluded, not merely omitted. Each fixture's
    `data_ref_under_test` fragment deserializes directly into the real
    `DataRef` type and is spliced into a data_refs-empty base request the
    same way. Two fixtures needed adaptation, not straight splicing:
    - data-ref-005's own JSON carries a placeholder string ("<a base64
      string whose decoded byte length is 65537>", not valid base64) by
      design; this test builds a real 65537-byte payload and proves via a
      decode round-trip (same `STANDARD` base64 engine
      `acdp_validation` itself decodes with) that it clears the cap.
    - data-ref-007 exposed a genuine spec/dependency divergence: spec pin
      `d1f06d0`'s `acdp-data-ref.schema.json` nests `content_hash` inside
      `embedded` (citing this exact fixture in its own description), but
      the `acdp` `0.9.1` dependency this registry runs has not caught up
      — its `EmbeddedContent` type has no `content_hash` field at all
      (`#[serde(deny_unknown_fields)]` on `encoding`/`content` only), and
      `acdp_validation::verify_embedded_hash` reads the DataRef-level
      `content_hash`, never a nested one. Splicing the fixture's JSON
      verbatim would fail at deserialization with an "unknown field"
      `schema_violation` — the right HTTP status by accident, for the
      wrong reason, not the `data_ref_hash_mismatch` the fixture pins. The
      test instead moves the fixture's own wrong-hash and content values
      to the DataRef-level `content_hash` field the dependency's validator
      actually reads, proving the intended RFC-ACDP-0002 §6.6 check 8
      semantic holds here rather than silently masking the divergence.

  `MIN_REPLAYED_EXCHANGES` (30) and the exchange replay count are
  unaffected — measured: `conformance: replayed 30 exchange(s);
  failures=0`, identical before and after this change; both new tests use
  a dedicated harness outside the generic replay loop, same as `anc-*`.
  Both self-skip cleanly (measured) when `ACDP_SPEC_DIR` is unset.
  Mutation-tested (measured) against a scratch copy of the spec tree
  outside this repo and outside the pinned spec worktree: perturbing
  meta-003's metadata to depth 9 and data-ref-001's `expected.error_code`
  each independently turned their respective test red, then were
  reverted (the scratch copy was discarded, not the pinned worktree).

<!-- end REG-11 Phase 10 -->

<!-- REG-11 Phase 11 -->

- **`body` and `status` move from `DEFERRED` to `COVERED`** (`REG-11`
  Phase 11, `#130`): two new direct tests in
  `crates/acdp-registry-server/tests/conformance.rs`. Neither family
  carries a `request`/publish shape at all — both are pure response-body
  assertions (`body-*`'s `input.body_fields_under_test`, `status-*`'s
  `input.response_body`/`response_body_excerpt`) — and both fields are
  registry-ASSIGNED (`origin_registry`) or registry-DERIVED
  (`registry_state.status`), so no `PublishRequest` field lets a caller
  drive either through this registry's own HTTP surface. Both tests prove
  the "registry never emits" contrapositive instead: publish a context,
  `GET` it back, and assert the SERVED shape.
  - `body001_002_origin_registry_hostname_never_did_form` covers
    body-001/002 (RFC-ACDP-0002 §3.1): the served `origin_registry`
    equals this harness's configured authority, matches ctx_id's own
    authority component, contains no `:`, and passes
    `acdp::validation::validate_body` (body-001's positive shape); and
    body-002's own `did:web:registry.example.com` literal, spliced onto a
    clone of the served body, is confirmed rejected by that same
    validator as `schema_violation` — the fixture's negative value gets
    real teeth even though this registry can never produce it itself.
  - `status001_004_served_status_matches_open_enum_pattern` covers
    status-001..004 (RFC-ACDP-0004 §4.1): the served
    `registry_state.status` round-trips through
    `acdp::types::primitives::Status::parse` (status-001's forward-compat
    positive shape), and each of status-002/003/004's own rejected
    literals (uppercase, embedded space, empty) is confirmed to fail that
    same open-enum parser and never equal the live served value.

  `MIN_REPLAYED_EXCHANGES` (30) and the exchange replay count are
  unaffected — measured: `conformance: replayed 30 exchange(s);
  failures=0`, identical before and after this change; both new tests use
  the shared `harness()` outside the generic replay loop, same as
  `meta`/`data-ref`. Both self-skip cleanly (measured) when
  `ACDP_SPEC_DIR` is unset. Mutation-tested (measured) against a scratch
  copy of the spec tree outside this repo and outside the pinned spec
  worktree: perturbing body-002's `origin_registry` to a bare hostname
  (no longer a DID) and status-002's `status` to `"active"` (no longer
  malformed) each independently turned their respective test red, then
  the scratch copy was discarded (not the pinned worktree) and both
  tests were reconfirmed green.

<!-- end REG-11 Phase 11 -->
<!-- REG-11 Phase 12 -->

- **`schema` moves from `DEFERRED` to `COVERED`** (`REG-11` Phase 12,
  `#130`): one new test,
  `schema_vectors_openness_and_absent_vs_null_enforced`, in
  `crates/acdp-registry-server/tests/conformance.rs`, covering the 8
  `schema-*` ids in `acdp-registry-core`'s `required_fixtures` at spec pin
  `d1f06d0` (verified directly against `registries/profiles.json`; the
  backlog plan's claim of 8 was correct, but only as the required-for-core
  subset — 14 `schema-*.json` fixtures exist on disk, 001..014; the other
  6 target `acdp-consumer` only and are correctly out of scope):
  schema-002/003/008/009/010/011/012/014.
  - schema-002 is the "registry never emits" half of RFC-ACDP-0007
    §3.3.1's openness map applied to `PublishResponse` — a real publish
    through the harness, asserting the served response both parses as
    `PublishResponse` and carries no `content_hash` key, not merely that
    the fixture's own malformed body fails to deserialize.
  - schema-003/008/009/011/012 are publish-path rejections, each isolating
    one closed sub-object (`EmbeddedContent`, `Signature`, `DataPeriod`) or
    non-nullable optional (`DataRef.format`/`.location`) by splicing the
    fixture's own malformed JSON fragment into an otherwise-valid signed
    body's raw `Value` and POSTing it directly — a new helper,
    `anc_publish_raw`, since the violation in each case is a shape the
    typed `PublishRequest`/`DataRef` builders cannot produce at all
    (`deny_unknown_fields` has no Rust field to carry an extra key; `Option`
    fields with `skip_serializing_if` never serialize a literal `null`).
  - schema-010 surfaced a genuine wrong-reason trap, the same shape as
    Phase 10's data-ref-007: its own `input.response_body_excerpt` is a
    bare `{"limits": {...}}`, not a full `CapabilitiesDocument`, so
    deserializing it verbatim fails on missing top-level fields before
    `Limits`'s `deny_unknown_fields` (confirmed live at
    `acdp-types-0.9.1/src/capabilities.rs:96-98`, not the stale `0.8.4`
    line the plan cited) ever rejects the excerpt's `limits.extra` key.
    Worse, replacing this registry's own real `caps()` document's `limits`
    wholesale (rather than merging) strips
    `limits.idempotency_key_ttl_seconds`, which
    `acdp_validation::validate_capabilities` requires whenever
    `supports_idempotency_key` is `true` (as `caps()`'s is) — a second,
    unintended rejection reason caught by mutation-testing this test
    itself before landing it. The final version merges the fixture's
    `limits` keys onto a clone of `caps()`'s own (self-checked "accept")
    `limits` object, isolating exactly the field the fixture exists to
    exercise.
  - schema-014's `input.response_body` is already a full, otherwise-valid
    document (only `limits.idempotency_key_ttl_seconds` is `null`), so it
    is used verbatim — no splicing, no trap.

  `MIN_REPLAYED_EXCHANGES` (30) and the exchange replay count are
  unaffected — measured: `conformance: replayed 30 exchange(s);
  failures=0`, identical before and after this change (48 -> 49 passed in
  `conformance.rs`; `conformance_gate.rs` unchanged at 1 passed). The new
  test self-skips cleanly (measured) when `ACDP_SPEC_DIR` is unset.
  Mutation-tested (measured) against a scratch copy of the spec tree
  outside this repo and outside the pinned spec worktree: independently
  perturbing each of schema-002/003/008/009/010/011/012/014's fixture data
  or expectations, and deleting a fixture outright, turned the test red
  every time (the schema-010 case surfaced the wrong-reason bug above,
  fixed before the mutation suite passed clean); the scratch copy was
  discarded afterward, not the pinned worktree, which diffs identical to
  upstream both before and after.

  Repeated-fact updates alongside the `COVERED`/`DEFERRED` move: the
  `CORE_INEXCUSABLE_FAMILIES` doc comment's "already `COVERED`" / "still
  `DEFERRED`" family lists, and both "the remaining N cite #130" counts
  (module header and the `DEFERRED` doc comment). The `CORE_INEXCUSABLE_
  FAMILIES` comment also carried a pre-existing staleness predating this
  phase — `caps` and `lin` were still listed as "currently `DEFERRED`"
  there despite closing to `COVERED` in Phase 7, the same shape of gap
  Phase 10 found and fixed for a different repeated fact — corrected here
  while touching the same sentence to remove `schema`.

<!-- end REG-11 Phase 12 -->
<!-- REG-11 Phase 13 -->

- **`sig`, `rev`, and `dk` move from `DEFERRED` to `COVERED`** (`REG-11`
  Phase 13, `#130`): five new direct tests plus one existing test newly
  registered, in `crates/acdp-registry-server/tests/conformance.rs`. All
  three families are golden-signature/did:key vectors the generic replayer
  cannot reach (no top-level `request`, and the positive vectors carry real
  cryptographic material a synthetic replay can't verify), so — like
  `anc`/`can` — they get DIRECT, in-process coverage beside the replayer,
  which still (correctly) reports them non-HTTP-replayable.
  - **Real fixture counts, corrected against the plan**: `sig` carries 3
    fixtures at spec pin `d1f06d0`, not the 1 the plan named — `sig-001`
    (Ed25519, unconditionally required), `sig-002` (ECDSA-P256, 2 vectors,
    conditional on `supported_signature_algorithms` including
    `ecdsa-p256`, which this file's shared `caps()` already advertises —
    a LIVE obligation, not a hypothetical one), and `sig-003` (did:key,
    conditional on advertising `did:key`). `rev` carries 2 fixtures
    (`rev-001`, `rev-002`), but only `rev-001` applies to
    `acdp-registry-core` — `rev-002` is `acdp-consumer`-only and stays out
    of scope (`HARNESS_PROFILES` advertises only `acdp-registry-core`).
    `dk` carries 4 fixtures (`dk-001`..`dk-004`), all bundled into the same
    `conditional_fixtures` entry as `sig-003` (`supported_did_methods`
    includes `did:key`) except `dk-003`, gated the opposite way
    (`did:key` NOT advertised at `acdp_version >= 0.2.0`).
  - **New mechanism: `playground.pinned_keys`.** This file's shared
    `caps()`/`harness()` cannot cryptographically verify a `did:web`
    signature (no live DID resolver in-process), so `sig-001`/`sig-002`/
    `rev-001` (all `did:web` producers) use
    `acdp-registry-core`'s existing `playground.pinned_keys` +
    `pinned_only` mechanism (`playground.rs`, FEAT-Phase5) instead: a
    dedicated per-test harness pins the fixture's own public key for its
    producer DID, so the publish path runs REAL Ed25519/ECDSA-P256
    verification (`acdp::crypto::verify`) against a golden vector, not the
    *unpinned* playground branch that — per its own doc comment — accepts
    "any agent_id+signature pair".
  - `sig001_ed25519_golden_verified_offline_and_accepted_via_pinned_publish`
    recomputes `canonical_form`/`content_hash` from `sig-001`'s
    `producer_content` via `acdp::crypto::canonical_preimage`, verifies the
    golden signature offline via `acdp::crypto::verify::verify_ed25519`,
    then POSTs the fixture's own `publish_request_body` verbatim to a
    pinned-key harness and asserts HTTP 200.
    `sig001_signature_byte_perturbation_is_rejected` is the accompanying
    mutation proof the task explicitly asked for: flips one base64
    character of the golden signature and asserts the SAME pinned harness
    now rejects it `invalid_signature`/400 — proving the pinned path is
    real verification, not a rubber stamp.
  - `sig002_ecdsa_p256_golden_accepted_and_der_signature_rejected` covers
    both of `sig-002`'s vectors: the RFC-6979-deterministic P-256 accept
    vector, and — same `producer_content`, same mathematical signature,
    re-encoded as 70-byte ASN.1 DER instead of the required 64-byte IEEE
    1363 r‖s — the registry MUST reject the DER encoding as
    `invalid_signature` per `registries/signature-algorithms.md`'s
    NORMATIVE prohibition. `acdp::crypto::verify::verify_ecdsa_p256`
    rejects on decoded length before any cryptographic operation, so an
    implementation that DER-decodes first and verifies second would wrongly
    accept it; this test proves this repo's dependency doesn't.
  - `rev001_key_revocation_context_golden_accepted_and_self_signed_rejected`
    covers `rev-001` (RFC-ACDP-0014 §4/§5) two ways: the fixture's own
    accept vector (a producer-signed key-revocation context, signed by K2,
    revoking K1 — `KeyRevocation::check_not_self_signed` passes since
    `fingerprint(K2) != revoked_key_fingerprint`), plus a hand-built
    negative (same `producer_content`, since `content_hash` excludes
    `signature`/`key_id`, re-signed and re-pinned as K1 — the very key
    being revoked). The negative is `rev-001`'s own
    `verification_steps[4]` made concrete via `Producer`, not a separate
    fixture id (none exists in the pinned spec): the registry MUST reject
    it `key_not_authorized`/403 (RFC-ACDP-0014 §5 step 2). **The plan's own
    text flagged `rev`'s prior `DEFERRED` reason as wrong** — confirmed:
    that reason described "before/after compromise-boundary semantics",
    which is `rev-002`'s content (out of scope), not `rev-001`'s. `rev-001`
    is a plain accept golden, structurally identical to `sig-001`/`003`.
  - `dk001_002_004_did_key_resolution_negatives_hit_schema_layer_not_resolver`
    covers `dk-001` (wrong multicodec prefix), `dk-002` (3 malformed-multibase
    cases), and `dk-004` (fragment mismatch) — and documents a genuine
    **wrong-reason trap found while writing it, of the same shape as
    Phase 10's `data-ref-007`, but more severe**: empirically verified
    (against this exact `acdp = "0.9.1"` dependency), this repo's own
    upstream schema-layer validation (`acdp_validation::validate_agent_did`
    / `validate_did_key_key_id_form`) calls the SAME did:key resolver
    functions the RFC-ACDP-0001 §5.11.1 resolver path would, but wraps
    every failure as `AcdpError::SchemaViolation` — so `verify_publish_
    request_signature_offline` (the code path that would emit
    `key_resolution_failed`) is never reached for any of these three
    fixtures. Unlike `data-ref-007`, there is no alternative request shape
    that reaches the intended path — this is architectural, not fixable by
    restructuring the test's input. `dk-002`'s own fixture text explicitly
    sanctions `schema_violation` as an alternative outcome ("Registries MAY
    reject case-by-case at schema validation... if their did pattern
    catches it first") — a genuine pass. `dk-001` and `dk-004` carry no
    such carve-out and DO diverge from their pinned `key_resolution_failed`
    — a real conformance gap in the `acdp` v0.9.1 dependency this phase
    cannot fix (out of scope: only `conformance.rs`/`CHANGELOG.md` may be
    touched). This test asserts the OBSERVED `schema_violation`/400 rather
    than the fixture's literal code for those two, with the divergence
    stated in both the test's doc comment and here — an always-red test
    pinned to a code this dependency will never produce is not a usable CI
    ratchet. **Flagged for follow-up**: a cross-repo issue against the
    `acdp` crate (mirroring how `data-ref-007` was filed as
    `agentcontextdistributionprotocol#60`) has not been opened by this
    phase — recorded here for a human to action, not filed unilaterally.
  - `did_key_golden_vector_accepted_and_gated` (pre-existing, REG-11 Phase
    prior to this one) already exercised `sig-003`'s accept path and
    `dk-003`'s capability-gate rejection end-to-end; it simply had never
    been registered in `COVERED`. This phase registers it under both `sig`
    and `dk`.

  `MIN_REPLAYED_EXCHANGES` (30) and the exchange replay count are
  unaffected — measured: `conformance: replayed 30 exchange(s);
  failures=0`, identical before and after this change; every new test uses
  a dedicated pinned-key harness outside the generic replay loop, same as
  `anc-*`/`meta`/`data-ref`. All new tests self-skip cleanly (measured)
  when `ACDP_SPEC_DIR` is unset. Mutation-tested (measured) against a
  scratch copy of the spec tree outside this repo and outside the pinned
  spec worktree: perturbing `sig-001`'s `content_hash`, perturbing one
  base64 character of `sig-001`'s golden signature, perturbing `rev-001`'s
  `content_hash`, and shrinking `dk-002`'s case count each independently
  turned their respective test red, then were reverted (the scratch copies
  were discarded, not the pinned worktree).

  This phase also corrected one pre-existing, unrelated staleness bug found
  while editing the same doc comment `sig`/`rev`/`dk` live in: the
  `CORE_INEXCUSABLE_FAMILIES` doc comment still listed `caps` and `lin` as
  `DEFERRED` (Phase 7 moved both to `COVERED` and missed updating this
  prose). Corrected here alongside the three-family move this phase makes;
  the two "remaining N cite #130" counters elsewhere (accurate before this
  phase) are updated from 13 to 10 to match.

<!-- end REG-11 Phase 13 -->

<!-- REG-11 Phase 14 -->

- **`did-ssrf`, `err`, and `rate` move from `DEFERRED` to `COVERED` — the
  last three `CORE_INEXCUSABLE_FAMILIES` stragglers — and `lc` moves from
  `DEFERRED` to `EXCUSED`** (`REG-11` Phase 14, `#115`/`#130`): three new
  direct tests in `crates/acdp-registry-server/tests/conformance.rs`, plus
  one ratchet declaration requiring no new test.
  - **Real fixture counts, verified against `registries/profiles.json` at
    spec pin `d1f06d0`**: `did-ssrf` carries exactly 5 fixtures
    (`did-ssrf-001`..`005`), all unconditionally in
    `acdp-registry-core.required_fixtures`. `err` and `rate` carry exactly
    1 fixture each (`err-001`, `rate-001`), also both unconditionally
    required — not derived from the plan's prose, read directly off the
    pinned spec file and cross-checked against the fixture count on disk.
  - **The prior `did-ssrf` `DEFERRED` reason's claim was verified, not
    trusted, before building on it** (a previous agent overturned a wrong
    ruling on this exact family once already): re-reading `extract_shapes`
    directly confirms all 5 `did-ssrf-*` fixtures really do fall out at
    fixture-shape extraction (`targets_unadvertised_profile` passes them —
    `applies_to_profiles` includes `acdp-registry-core` — but their
    `input.endpoint`/`input.body` shape satisfies none of Shapes A/B/C/D),
    landing in the same `"non-HTTP fixture (vectors / schema /
    informative)"` bucket as `can`/`sig`. Measured directly in this run's
    own skip tally: `did-ssrf: 5 (non-HTTP fixture (vectors / schema /
    informative))`, `err: 1 (...)`, `rate: 1 (...)`. The seam claim held up
    too: `acdp::did::WebResolver` (re-exported by the `acdp` facade this
    crate already depends on) applies `SsrfPolicy::default()`
    unconditionally, and `acdp::safe_http` exposes `reject_if_any_forbidden`
    and `SsrfPolicy::classify_redirect` for the mixed-answer and
    same-authority-redirect rules — all reachable from this dev-dependency
    test binary with **zero new Cargo dependencies** (the `client`/
    `test-transport` features were already unified on transitively via
    `acdp-client`'s own unconditional `acdp-did/client` requirement and
    this crate's existing `acdp = { features = ["test-transport"] }`
    dev-dependency).
  - `did_ssrf001_005_producer_did_resolution_refuses_forbidden_targets`:
    `did-ssrf-001`/`002`/`003` (IP-literal cases only — hostname-based
    `additional_test_cases` needing real DNS, e.g. `metadata.example.com`,
    are explicitly excluded and documented, not silently dropped) drive the
    real, async `WebResolver::resolve` end-to-end — offline and
    deterministic, since the SSRF policy refuses before any socket
    activity — and cross-check the resulting `AcdpError::KeyResolution`
    against this repo's own `RegistryError::wire_code`/`http_status`
    (`key_resolution_failed`/400, matching the fixtures exactly).
    `did-ssrf-004`/`005` drive `reject_if_any_forbidden`/`classify_redirect`
    directly with the fixtures' own DNS-answer and redirect literals — one
    layer beneath the full `WebResolver`/reqwest pipeline (no live TLS/DNS
    mock stood up), so the test does NOT claim a final wire status for
    those two, only that the pure enforcement point itself rejects (plus,
    by reading — not running — `classify_reqwest_error`, documents why that
    should still resolve to `key_resolution_failed`/400 end-to-end: the
    `"SSRF policy"` substring match). This distinction (measured vs.
    derived) is spelled out in the test's own doc comment so it is never
    mistaken for a full-pipeline proof.
  - `err001_internal_error_envelope_matches_pinned_shape_and_leaks_nothing`:
    `err-001`'s own text calls its trigger "implementation-defined...
    fixtures cannot reproduce" — no black-box request can deterministically
    force a persistence-layer throw. This test instead drives the REAL
    production `RegistryError -> HTTP response` projection (the
    `IntoResponse` impl every handler's `Result<_, RegistryError>` return
    type goes through) for all five variants that legitimately project to
    `internal_error` in this codebase, each seeded with a deliberately
    sensitive-looking detail string, asserting none of it reaches the wire
    — mirroring `acdp-registry-types::error`'s own
    `internal_errors_do_not_leak_detail` unit test, now also proven from
    this repo's conformance file against the fixture's pinned
    status/code/content-type. The fixture's illustrative message text ("An
    unexpected error occurred.") is deliberately NOT asserted verbatim —
    this repo's real implementation emits a different string ("internal
    error"), and nothing in `err-001`'s own text pins the wording, only the
    non-leak property.
  - `rate001_publish_rate_limit_trips_429_with_retry_after`: not a missing
    seam — `limits.publish_rate_per_minute` is a live config knob enforced
    by the in-process `AgentRateLimiter`, already proven end-to-end for the
    sibling `/auth/challenge` limiter by `http_integration.rs`'s
    `challenge_endpoint_is_rate_limited`. This test exercises the SAME
    limiter on the publish path for real: a harness configured with
    `publish_rate_per_minute = 1`, one publish that succeeds, a second
    (different content, same producer) that trips the limiter, asserting
    the REAL HTTP response (429, `application/acdp+json`,
    `error.code == "rate_limited"`, a present `Retry-After` header) against
    the fixture's own pinned status/code.
  - **`lc` moves `DEFERRED` -> `EXCUSED`** (a user decision, not
    re-derived here): `lc`'s 3 fixtures each carry `applies_to_profiles:
    ["acdp-registry-lifecycle"]`, disjoint from this harness's advertised
    `HARNESS_PROFILES = ["acdp-registry-core"]` — verified directly against
    `registries/profiles.json` at this pin, both for each fixture's
    `applies_to_profiles` and for `lc`'s absence from
    `acdp-registry-core`'s `required_fixtures`/`conditional_fixtures`.
    `no_excused_family_is_required_by_our_profile` still passes (spec-gated
    guard confirming no excused family is actually owed). This needed no
    new test — only a written excuse in `EXCUSED` — since the runtime
    profile gate already skips all 3 fixtures; what was missing was the
    *declaration*, not a capability. #115 now has zero `DEFERRED` members.
  - **Corrected a stale claim found while editing the same module doc
    comment this phase's changes live in** (same discipline as Phase 13's
    unrelated `CORE_INEXCUSABLE_FAMILIES` fix): the "Required-checks
    decision" paragraph said `conformance (spec fixtures)` was NOT among
    this repo's required branch-protection contexts and that adding it was
    "deliberately left undone." Re-verified directly against branch
    protection: `required_status_checks.contexts` is now
    `["rustfmt", "clippy", "tests", "conformance (spec fixtures)"]` — a
    repo admin actioned the Phase 11 recommendation on 2026-09-01 (see
    `ASSUMPTIONS.md`'s "Executed" entry for that decision). The paragraph
    is rewritten to state the current, verified status rather than the
    stale one; the underlying `Replayed`-vs-`Direct` split it was
    explaining is left in place since it is still accurate. (The
    equivalent stale sentence in this Phase 13 changelog entry above is
    left as a historical record of what was true when it was written, not
    rewritten.)

  `MIN_REPLAYED_EXCHANGES` (30) and the exchange replay count are
  unaffected — measured: `conformance: replayed 30 exchange(s);
  failures=0`, identical before and after this change; all three new tests
  sit outside the generic replay loop, same as `anc-*`/`sig`/`rev`/`dk`.
  All three self-skip cleanly (measured) when `ACDP_SPEC_DIR` is unset — no
  panics. Test counts: 56 -> 59 passed in `conformance.rs` (+3), 1 passed
  in `conformance_gate.rs` (unchanged) — 60 total. `cargo fmt --check` and
  `cargo clippy --all-targets --features storage-sqlite,playground -- -D
  warnings` both clean.

  **Mutation-tested (measured)** against a scratch copy of the spec tree
  under `/tmp`, outside this repo and outside the pinned spec worktree —
  each mutation applied, run in isolation, confirmed red, then reverted
  before the next; the scratch copy was deleted afterward and a final run
  against the real pinned worktree reconfirmed green (60/60, replayed 30,
  failures=0):
  - `err-001`: `expected.error_code` -> a wrong string -- RED (wire_code
    self-check).
  - `err-001`: `expected.http_status` -> `599` -- RED (http_status
    self-check).
  - `rate-001`: `expected.response_body.error.code` -> a wrong string --
    RED (wire error.code mismatch).
  - `rate-001`: `expected.http_status` -> `599` -- RED (real HTTP status
    mismatch after tripping the real limiter).
  - `did-ssrf-001`: `expected.error_code` -> a wrong string -- RED (fixture
    self-check).
  - `did-ssrf-004`: `dns_mock.answers` changed to two PUBLIC addresses
    (removing the forbidden one) -- RED (`reject_if_any_forbidden`
    genuinely stopped rejecting; the mixed-answer enforcement is really
    exercised, not assumed).
  - `did-ssrf-005`: `redirect_to` changed to keep the same port (`:443`,
    matching `redirect_from`) -- RED (`classify_redirect` genuinely started
    accepting the redirect).
  - `did-ssrf-005`: the fixture's own positive-control
    `additional_test_cases[0].authority_match` flipped `true` -> `false` --
    RED (the test's independently-computed `classify_redirect` result,
    `true`, mismatched the now-wrong fixture expectation, `false`) --
    proves this assertion is a real comparison, not an echo of whatever the
    fixture says.

<!-- end REG-11 Phase 14 -->

<!-- REG-11 Phase 15 -->

- **`cur` moves from `DEFERRED` to `COVERED`, and `rcpt`/`lhr`/`log` are
  narrowed to their consumer-role residue** (`REG-11` Phase 15, `#130`):
  five new direct tests in
  `crates/acdp-registry-server/tests/conformance.rs`, plus a new
  `DEFERRED_PARTIAL_DIRECT` registry and three new ratchet self-checks.
  - `cur001_002_expired_and_malformed_cursors_are_distinguished` drives
    both `cur-*` fixtures over real HTTP. It publishes three contexts,
    takes a cursor the registry actually minted, and **first proves the
    un-aged cursor both returns 200 and advances past page 1** — without
    that control, a registry that silently ignored the cursor entirely
    would satisfy the test. It then ages only the plaintext mint stamp
    (`CURSOR_TTL_SECS = 3600`, so a 3,605,000 ms offset) and asserts
    status, `error.code` and content-type, every expected value read from
    the fixture rather than hardcoded, plus `cursor_expired` and
    `invalid_cursor` remaining distinct.
  - `rcpt001_registry_receipt_golden_recomputed_and_remintable`,
    `lhr001_lineage_head_receipt_golden_recomputed_and_remintable`,
    `log001_leaf_root_and_inclusion_golden_recomputed` and
    `log003_consistency_proof_golden_recomputed` **recompute rather than
    parse**: JCS canonicalization, SHA-256 preimage/leaf hashing, RFC 6962
    root and consistency verification, offline Ed25519 verification, and
    an Ed25519 **re-mint through this repo's own
    `acdp_registry_core::receipt::build_signer`** reproducing the pinned
    signature byte-for-byte. Each carries a mutation negative. The
    goldens publish `private_seed_hex` while `ReceiptConfig` wants
    base64, so the seed is hex-decoded and re-encoded; getting that
    backwards yields a valid-looking signer with the wrong key.
  - **The signer identity is pinned as literals, not split out of the
    golden's own `key_id`.** Deriving authority and fragment from the
    `key_id` then asserting the re-minted `key_id` matches is a
    round-trip tautology — `build_signer` reassembles
    `did:web:{authority}#{fragment}` from the very string it was handed,
    so it passes for *any* fragment. Confirmed by mutation: with the
    derived form, rewriting every golden's fragment to `BOGUS-FRAGMENT`
    left all four tests green.
  - **`rcpt`/`lhr`/`log` stay `DEFERRED`, and their reasons now name only
    what is genuinely uncovered.** Each previously claimed two causes, the
    first of which stopped being true once the goldens landed. Two
    independent grounds remain, both verified against spec pin `d1f06d0`:
    `rcpt-002/003/004`, `lhr-002/003/004` and `log-002/004` declare
    profiles this harness does not advertise, and advertising one the
    registry does not implement would make the ratchet lie; and
    `acdp-registry-core` implements only the **producer** side of receipts
    (`load_signing_key`, `build_signer`, `build_did_document` — there is no
    verify fn), while those fixtures carry no `endpoint` and no `vectors`
    array, so a "direct pass" over them would assert something about
    `acdp-types`/`acdp-crypto` rather than about this registry.
  - **`DEFERRED_PARTIAL_DIRECT`** closes the hole that creates: the
    partition buckets by *family*, not fixture, so a family legitimately
    deferred for its residue had no way to pin the golden-half tests —
    deleting one left every existing check green. Two tests rather than
    one, because the first was itself falsifiable:
    `deferred_partial_direct_test_functions_are_present` checks each named
    test exists, still wears its attribute and still carries its
    assertion-count ratchet, and
    `deferred_reasons_naming_golden_tests_are_pinned_by_partial_direct`
    ties the const's own membership to the `DEFERRED` prose that claims
    those tests cannot be deleted.
  - **Seventeen stale `conformance.rs`-relative line pins replaced with
    symbol references**, which cannot rot, and `no_numeric_self_citations`
    added to keep them out. Four stale *cross-file* pins repaired — three
    invalidated by the `acdp` 0.10.0 bump, one by spec-pin drift. That
    test deliberately does **not** match cross-file pins, since a
    reference into another file cannot be a symbol this file resolves, so
    that class stays rot-prone; the limitation is stated in the test's own
    doc rather than glossed.
  - **The anti-vacuity guards' claims are deliberately conservative.**
    Three verification rounds established that a substring test over
    `include_str!` cannot distinguish an assertion from the letters
    `a-s-s-e-r-t` in a comment: `{ /* assert */ }` defeats the generic
    check and `// EXPECTED_ asserted` defeats the stricter-looking one,
    and `assert!(true)` survives any tightening. The guards therefore
    document that they catch **wholesale gutting and deletion, and that is
    all** — proving a test asserts something real needs a mutation oracle
    (`cargo-mutants` or fault injection over `src/`), not a text oracle.
    Recorded on `#130` rather than overclaimed in the file.
  - `HARNESS_PROFILES`, `caps()`, `config()`, `KNOWN_FAMILIES`,
    `CORE_INEXCUSABLE_FAMILIES`, `EXCUSED` and `MIN_REPLAYED_EXCHANGES`
    are byte-identical to their prior contents — the ratchet was closed on
    real behaviour, never widened to manufacture coverage. `#130` stays
    **open**: the residue above is real and is not owed.

<!-- end REG-11 Phase 15 -->

- **A coverage-completeness ratchet closes the gap Phases 7-10 left open**
  (`REG-10` Phase 11): the existing four `KNOWN_FAMILIES`/`EXCUSED` ratchet
  tests fail only on an *unclassified* family or an *illegitimate excuse* —
  never on zero coverage, which is exactly how `vis`/`idem` sat uncovered
  before Phases 8-10, and how `caps`/`lin`/`lc` (#115) and 15 more families
  (#130) still do today. Two new consts and two new tests in
  `crates/acdp-registry-server/tests/conformance.rs` close it:
  - **`COVERED: &[(&str, &[CoverageMechanism])]`** models the two legitimate
    coverage mechanisms a family can claim — `Replayed` (produced >= 1
    exchange in `replays_spec_fixtures_when_present`'s own per-family
    tally) and `Direct(&[fn_names])` (named in-process test functions).
    Modelling both, rather than deriving `COVERED` purely from replay
    results as originally preferred, is load-bearing: `anc`, `can`,
    `idem`, and `wit` are genuinely covered by direct tests and produce
    **zero** replayed exchanges, so a replay-only derivation would have
    branded all four uncovered — including `can` (Phase 7) and `idem`
    (Phase 10), the two families this very effort added. `vis` claims
    both mechanisms (it clears `MIN_REPLAYED_EXCHANGES`, currently
    confirmed still 30, AND carries 10 dedicated per-fixture test
    functions).
  - **`DEFERRED: &[(&str, &str, u32)]`** lists the 18 families with no
    coverage yet, each with a non-empty reason and an open tracking-issue
    number: `caps`/`lin`/`lc` cite #115; the remaining 15
    (`body`/`schema`/`sig`/`dk`/`did-ssrf`/`data-ref`/`cur`/`err`/`meta`/
    `rate`/`status`/`rcpt`/`lhr`/`log`/`rev`) cite #130.
  - **`known_families_partition_into_covered_excused_or_deferred`** is the
    new fifth ratchet test, and the one point of this phase: it asserts
    `KNOWN_FAMILIES == COVERED ∪ EXCUSED ∪ DEFERRED` as a set. Unlike the
    four existing ratchet tests, it is deliberately **unconditional** — it
    touches no spec data, so it does not skip when `ACDP_SPEC_DIR` is
    unset. The required `tests` CI job runs `cargo test --workspace` with
    no spec configured, so this is what actually blocks a merge; verified
    directly by temporarily adding an unclassified 30th family and
    confirming only this test goes red under that exact job configuration.
  - **`covered_direct_families_have_present_test_functions`** is the
    mutation-proof half for `Direct`-mechanism families: it scans this
    file's own compiled-in source (`include_str!`) for each named test
    function, confirming it still exists with a test attribute directly
    above it. This is deliberately an EXISTENCE check, not a correctness
    check — the honest limit of what a spec-independent, self-inspecting
    const can verify; documented as such rather than overclaimed, including
    the two evasions it cannot catch (`#[ignore]` written above `#[test]`,
    and the whole function wrapped in a `/* ... */` block comment).
    Verified by temporarily stripping a `#[test]` attribute and observing
    the expected failure, then reverting.
  - The `Replayed` half of the same mutation proof lives inside
    `replays_spec_fixtures_when_present` itself: every family claiming
    `CoverageMechanism::Replayed` must have produced >= 1 exchange in that
    very run's tally, checked against the spec at the pinned SHA.
  - **Required-checks decision, recorded but not executed** (a repo-admin
    action, out of scope for this diff): `conformance (spec fixtures)`
    should join `rustfmt`/`clippy`/`tests` as a required branch-protection
    context. Confirmed directly against this repo's branch protection that
    it is not currently required. Leaving it advisory means only the
    spec-independent half of this ratchet (the set-equality test and the
    direct-mechanism scan) can ever block a merge; a regression that
    silently drops a family's replayed exchanges while its `COVERED` entry
    and direct tests stay intact would go unnoticed by required checks
    alone. Flagged as a follow-up for a human with repo-admin access.

- **`idem-001` through `idem-005` (RFC-ACDP-0003 §6 idempotency-key
  lifecycle) now have DIRECT, fixture-driven coverage** (`REG-10` Phase 10).
  These five fixtures don't fit Shape D — their top-level key is
  `preconditions` (an existing idempotency record, never a literal
  `ctx_id`), not `setup`, and `idem-005`'s `input` is a bare array of
  publish descriptors, not `scenarios[]` — and none of Shape D's seeding
  machinery has anything to seed here: the object under test IS the
  publish response itself. So, same precedent as `anc`/`can`/`vis-003`/
  `vis-007`: two direct tests, run beside the generic replayer (which
  still, correctly, shows all five as unreached — "requires pre-seeded
  state"). `idem001_004_publish_idempotency_key_lifecycle_and_restart_durability`
  runs the full, mutually-dependent `idem-001`→`idem-002`→`idem-003`→
  `idem-004` sequence against one shared, file-backed harness: `idem-001`
  (fresh publish), a genuine registry-restart proof (reconnect to the same
  on-disk SQLite file as a NEW `SqliteStore`/`RegistryServer`/`Router`,
  proving the idempotency record — not just the context row — survives),
  `idem-002` (same key + hash → byte-identical stored response returned,
  not re-executed), `idem-003` (same key, different hash → 409
  `duplicate_publish`, with a mutation proof that `idem-001`'s record is
  unmodified AND that the rejected body was never persisted), and
  `idem-004` (new key, same content → a fresh `ctx_id` AND `lineage_id`
  despite byte-identical content). `idem005_no_support_ignores_idempotency_key_header`
  runs against a SEPARATE, non-playground harness (a did:key producer
  through the SDK's verified publish path) that genuinely does not
  advertise `supports_idempotency_key`, proven by reading it back off
  `GET /.well-known/acdp.json`, and asserts two independent publishes with
  the same key both succeed with DIFFERENT `ctx_id`s. This repo's
  `POST /contexts` returns HTTP **200** on success, not the fixtures' own
  literal `201`, and never sets a `Location` header at all — both
  deviations are recorded in a doc comment following the `anc-001`
  precedent, not "fixed". `idem-002`'s `registry_must_not` clause (no
  re-DID-resolution, no re-signature-verification) is stated honestly as
  un-observable to a black-box HTTP assertion — the full-response-body
  equality is the closest indirect evidence available, not a claim of
  having observed the internals. `idem-006` (a concurrency-race fixture)
  and `idem-007` (a capabilities-document validation check gated on
  `acdp_version >= 0.3.0`) are recorded not-owed with their real reasons:
  `idem-006` sits in the pinned spec's `tolerated_outcomes` — a THIRD
  obligation category this repo's model didn't previously name, alongside
  `required_fixtures`/`conditional_fixtures` — not a strict requirement;
  `idem-007`'s version gate never fires against this harness's advertised
  `0.1.0`. `MIN_REPLAYED_EXCHANGES` is unchanged at 30 — neither test
  replays through the generic harness.
  **Finding recorded, not fixed (out of this phase's scope — test file and
  CHANGELOG only):** `acdp-registry-core`'s playground publish branch
  (`crates/acdp-registry-core/src/handlers/context.rs`, the manual
  idempotency lookup/record dance around `publish_unverified_for_tests`)
  honors ANY `Idempotency-Key` header whenever one is present, with no
  check of `supports_idempotency_key` anywhere in that branch — unlike
  every other publish path (verified did:web, did:key, pinned-verified),
  which routes through the upstream `acdp-server` SDK's own
  `RegistryServer::commit_via_store` and gates correctly. It is
  unreachable in a deployed registry, though not for the reason one might
  assume: the playground publish branch carries no
  `#[cfg(feature = "playground")]` (only the admin router does), so it
  compiles into a stock build and activates on the runtime toggle alone.
  What actually makes the divergence unrealizable is that
  `crates/acdp-registry-server/src/main.rs:1026` hardcodes
  `supports_idempotency_key: true` with no config knob, so the shipped
  binary can never advertise `false` — the state in which honoring the
  header would be wrong. It becomes live the moment that field is made
  config-driven, as every other capability already is. Filed as an issue
  rather than fixed here; it meant `idem-005` had to be
  built against a non-playground, did:key harness instead of the shared
  playground harness `idem-001`..`004` use — see the doc comment on
  `idem005_no_support_ignores_idempotency_key_header` for the full
  write-up.

- **`vis-008` (5 scenarios) — the last parked `vis` seed shape,
  `setup.lineages` — now replays end-to-end through Shape D** (`REG-10`
  Phase 9c). `MIN_REPLAYED_EXCHANGES` rises from 25 to 30. This is the last
  fixture needing a THIRD substitution table, `fixture_lineage_id ->
  minted_lineage_id`, built alongside the existing ctx_id and DID tables
  (`SeedLineage`/`SeedLineageVersion`, `parse_seed_lineages`) rather than
  special-cased outside the substitution layer. `vis-008` seeds two
  two-version lineages (`a1 -> a2`, both `restricted`, same audience/owner;
  `b1` `public` -> `b2` **`private`**, same owner — the head is private
  while v1 stays public) through REAL `Producer::supersede_body()`-chained
  publishes, in ascending `version`-field order (the fixture carries no
  explicit `supersedes` key), never a direct store write. `status`
  (`active`/`superseded`) is never a seed input — `PublishRequest` has no
  such field — it is asserted as the registry COMPUTES it from the
  supersession, cross-checked against the fixture's own `status` literal
  per lineage version; at pin `417211f` the registry's computed status
  matches every one of the fixture's four literals exactly, so no
  `anc-001`-style deviation note was needed. Two response shapes no earlier
  phase needed: `GET /lineages/{lineage_id}` returns a bare JSON array
  (scenario 0's `stranger on a fully-restricted lineage gets 200 + []`, not
  404 — asserted via an EXPLICIT `body == []` equality check, not inferred
  from `matches_ctx_ids` being an empty set, because an unsubstituted or
  unknown `lineage_id` also 200s with an empty array); `GET
  /lineages/{lineage_id}/current` returns a single `FullContext` object
  with singular `ctx_id` and a nested `registry_state.status` (scenario 4).
  A new `assert_substitution_sound` helper generalizes the existing
  raw-and-percent-encoded substitution-occurred proof (previously inline
  and ctx_id-specific) so it covers the lineage_id table too — closing the
  one place a wrong-but-200 answer could otherwise read as correct.
  Mutation-proven: `vis008_mutated_lineage_version_order_fails_replay`
  swaps the `version` field between lineage b's two entries (nothing
  else), which reverses which version publishes first and flips the
  lineage's real head from private to public — scenario 3 then gets 200
  instead of its expected 404, failing the replay. `ret-002` (also
  `setup.lineages`) was checked deliberately and does NOT become
  replayable as a side effect: its lineage versions carry no `visibility`
  key and one carries `expires_at`, both outside
  `parse_seed_lineage_version`'s recognized set, and its first lineage
  requires an "abnormal state: every version is superseded" that a real
  publish sequence cannot produce (publishing v2 always makes v2, not v1,
  the active head) — it remains classified `requires pre-seeded registry
  state`, unchanged from before this phase.

- **`vis-002` (4 scenarios), `vis-005` (4 scenarios), and `vis-009` (3
  scenarios) — multi-context, capability-toggling visibility fixtures — now
  replay end-to-end through Shape D, and `vis-007` gets direct in-process
  coverage** (`REG-10` Phase 9b). `MIN_REPLAYED_EXCHANGES` rises from 14 to
  25 (14 + `vis-002`'s 4 + `vis-005`'s 4 + `vis-009`'s 3). Two Shape D
  capabilities Phase 8 built and proved only synthetically are exercised
  against real fixtures for the first time: a per-scenario router rebuild
  driven by `registry_capabilities_subset.anonymous_public_reads`
  (`vis-002` scenarios 2/3 toggle `true`→`false` back-to-back against the
  identical anonymous requester; `vis-009` toggles `false`→`true`→`false`
  across all three scenarios), and ctx_id substitution reaching QUERY
  STRINGS in both raw and percent-encoded form (`vis-005` scenario 2's
  `search?derived_from=<percent-encoded private ctx_id>`). The
  substitution-occurred check inside `replay_shape_d` was strengthened
  alongside this: previously it only asserted no *raw* literal ctx_id
  leaked into the built request path, which would have silently missed a
  failed *query-string* substitution (the percent-encoded literal would
  sit unnoticed in the path); it now also asserts, positively, that
  whenever a scenario's original path referenced a fixture ctx_id at all,
  the built path carries the MINTED replacement — catching exactly the
  "substitution silently failed, empty result reads as a legitimate
  negative" failure mode the Phase 8/9b plans both flag. `expected`
  parsing gains two new assertable (not merely recognized) keys,
  `total_estimate` and `matches_ctx_ids` — the latter translated through
  the fixture's ctx_id substitution map at replay time, so a search that
  returns the right *count* but the wrong *identity* (exactly what
  `vis-005`'s two same-`did:agent:owner` seeds could produce if the Phase
  8 `did_map` two-pass fix ever regressed) is caught, not just an
  off-by-one. `vis-005`'s two seeds sharing one literal `agent_id` is the
  exact shape Phase 8's GAP 1 (`did_map` overwrite on a shared literal
  agent) was fixed for but never exercised by a real fixture until now;
  `vis005_private_audience_search_excluded_via_derived_from` asserts
  `did_map.len() == 1` on it directly. Across `vis-002` (3), `vis-005` (4),
  `vis-007` (1, direct coverage), and `vis-009` (2), the pinned spec
  fixtures carry exactly 10 `expected.total_estimate` occurrences; 9 of the
  10 are asserted on their exact value, alongside `matches_count`. The
  tenth — `vis-005` scenario 2, `search?derived_from=<private ctx_id>` — is
  **not** a conformance divergence: the spec explicitly licenses an
  approximate `total_estimate` ("May be approximate; not guaranteed to be
  exact", `schemas/json/acdp-search-response.schema.json`; "SHOULD NOT be
  relied upon for exact counts", `rfcs/RFC-ACDP-0005-discovery.md:219`; the
  spec's own `examples/search/empty-page-post-filter-response.json` ships
  the identical shape — an empty post-filtered page with a non-zero
  estimate). One genuine, pre-existing registry characteristic surfaced
  while building this: `total_estimate` (both `acdp-registry-sqlite` and
  `acdp-registry-pg`, `DESIGN-01`) is computed from the same SQL scan that
  applies RFC-ACDP-0008 §4.5 visibility, but `derived_from` (like
  `status`/`tags`) is a documented *post*-SQL refinement applied afterward
  in Rust — so it is a pre-refinement upper bound for a `derived_from`-
  filtered search, not the post-filter count `vis-005` scenario 2's fixture
  happens to pin at `0`; that `0` is one of several conformant values, and
  this registry emits another. Verified live: `matches` correctly scopes to
  empty (proving both the `derived_from` filter and the ctx_id substitution
  work), while `total_estimate` returns the harmless pre-refinement scan
  count instead. Exact-value assertion is therefore skipped for that one
  `derived_from`-filtered scenario (see the carve-out in
  `parse_scenarios_array`, and the corpus-wide tripwire
  `derived_from_carve_out_matches_exactly_one_corpus_scenario`, which fails
  loudly if a second such fixture ever appears); every other scenario
  across all four fixtures keeps the full exact-value assertion. What the
  carve-out does *not* skip: leak-invariance (RFC-ACDP-0005 §2.5.5 Q2's
  MUST that a registry "avoid leaking their existence via per-requester
  variance in the estimate") is asserted directly against a live registry
  response in `vis005_private_audience_search_excluded_via_derived_from` —
  the audience member and an outsider get the identical `total_estimate` on
  the same `derived_from` query, both strictly below the producer's.
  `anonymous_public_reads: false`
  is NOT an unconditional "403" rule: `vis-009` scenario 2 sets the flag
  `false` but expects a *successful* search because its requester is
  authenticated — the flag gates anonymous reads only, and both the
  `vis-002`/`vis-009` dedicated tests assert this directly rather than
  the naive stricter reading. `vis-007` cannot reach Shape D at all: its
  scenario 2 (`expected: {outcome:
  "registry_must_not_emit_this_response", rationale}`) carries no `status`
  whatsoever, so `parse_expected` fails on it and, by Shape D's
  parse-all-or-nothing rule, the whole fixture stays unparseable there —
  `vis007_search_match_restricted_visibility_disposition` seeds the one
  restricted context directly and replays scenarios 0 and 1 for real
  (`status`/`matches_count`/`total_estimate` all asserted), same
  direct-coverage precedent as `vis-003`; only the MAY-shaped
  `match_visibility_field_disposition`/`consumer_invariant` keys and
  scenario 2 wholesale are recorded not-assertable. Mutation proofs (an
  in-memory-only fixture clone, never written to the spec checkout) on
  both `vis-002` (restricted context flipped to `public`) and `vis-005`
  (the private seed flipped to `public`) fail replay specifically on a
  `matches_count`/`matches_ctx_ids` mismatch. Shapes A, B, and C remain
  textually unchanged.

- **`vis-001` (5 scenarios) and `vis-004` (4 scenarios) — single-context
  restricted/private visibility fixtures — now replay end-to-end through
  Shape D, and `vis-003` (search response field-naming) gets direct
  in-process coverage** (`REG-10` Phase 9a). `MIN_REPLAYED_EXCHANGES` rises
  from 5 to 14 (4 pre-existing + `vis-006`'s 1 + `vis-001`'s 5 + `vis-004`'s
  4). `vis-001` (RFC-ACDP-0008 §4.5 existence-leak prevention) seeds one
  `restricted` context and exercises producer / audience-member / outsider
  / genuinely-nonexistent-ctx_id / non-audience-contributor across five
  requester identities against the same ctx_id — the first fixture in this
  file to require the bearer path to actually distinguish requesters (Phase
  8's proof fixture, `vis-006`, is requester-identity-agnostic by
  construction: `did:agent:any-authenticated-or-anonymous` against a public
  context behaves the same with auth on or off). `vis-004` (RFC-ACDP-0008
  §4.5 / RFC-ACDP-0002 §7 private/audience retrieval asymmetry) seeds one
  `private` context with an `audience` and covers the same four-way split.
  Both fixtures carry a scenario with
  `request.context_subset_for_test.contributors` — a per-scenario mutation
  of the seeded row's `contributors` list, not a requester swap, and
  exactly the key Phase 8's allowlist excluded them on. Shape D is widened
  to fold it onto the (single) seed at seed time rather than left
  unsupported: the registry's only write path (`POST /contexts`) mints a
  new `ctx_id` per call, so there is no in-place "update contributors on
  this existing row" endpoint to genuinely mutate mid-replay, and applying
  it at seed time is observably identical to that framing here since
  `contributors` never affects any other scenario's status/error_code —
  `can_retrieve` and `can_surface_in_search` branch only on visibility,
  `agent_id`, `audience` and `anonymous_public_reads`, so contributors
  carries attribution rather than retrieval authorization — and both
  fixtures are single-seed and retrieval-only. That scoping is deliberate:
  `contributors` *does* gate authorization on the supersession
  producer-continuity path, so the same seed-time fold applied to a
  publish/supersede fixture would change authorization rather than preserve
  it. `parse_shape_d` fails closed (returns `None`, routing to
  the existing skip path) rather than guess which seed a *multi*-seed
  fixture's `context_subset_for_test` would target. `vis-001` scenario 4's
  genuinely nonexistent ctx_id (`…-000000000000`, distinct from the seeded
  `…-000000000001`) needed no special-casing — it was never seeded, gains
  no substitution-table entry, and a dedicated test
  (`vis001_restricted_denied_as_404_replays_via_shape_d`) asserts the
  ctx_id map contains exactly the one context that actually was seeded.
  `vis-003` has no `setup` (only `background`) and its scenarios use
  `input.endpoint`/`input.received_response`, never
  `request.method`/`request.path`, so it matches neither Shape D nor Shape
  B; `vis003_search_response_emits_matches_not_results` drives a real `GET
  /contexts/search` and asserts the fixture's own
  `response_body_constraints` (`matches` present, `results` and its listed
  alternates absent) directly against the real response body — same
  precedent as the existing `anc`/`wit`/`can` direct-coverage tests. Its
  other two scenarios (`expected.consumer_behavior` /
  `expected.minimum_diagnostic_content`) are consumer-side obligations a
  registry cannot satisfy or violate by construction; recorded
  not-applicable, with reasoning, in that test's own doc comment rather
  than silently dropped. `vis-004`'s own mutation proof (seeded visibility
  flipped `private` → `public` on an in-memory-only fixture clone, never
  written to the spec checkout) fails replay specifically on the
  outsider/contributor scenarios' now-wrong 404 expectation, alongside the
  pre-existing `vis-006` mutation proof — together demonstrating Shape D
  exercises the registry's real visibility-scoping logic rather than
  trivially passing. Shapes A, B, and C remain textually unchanged.

- **The conformance replayer gains a fourth shape ("Shape D") that seeds
  registry state before replaying, and `vis-006` (RFC-ACDP-0005 §2.2
  public-visibility search disclosure) is now the fifth exchange it
  proves live** (`REG-10` Phase 8). Previously the replayer's three shapes
  (`conformance.rs`'s `extract_shapes`) only handled self-contained
  exchanges; every fixture carrying `setup` — all of `vis-*`, `idem-*` and
  friends — was a blanket skip ("requires pre-seeded registry state"),
  because the registry mints its own `ctx_id` and the fixtures' literal
  ones (`pub-013` proves a producer-supplied `ctx_id` is rejected) can't
  be replayed against directly. Shape D closes that gap for the shapes it
  understands: it seeds `setup.context_published` / `.contexts_published`
  through the real publish API (never a direct store write), building a
  `fixture_ctx_id -> minted_ctx_id` substitution table and, for any seeded
  `agent_id` that isn't already `did:web` (this registry only advertises
  `did:web`, and `pub-008` proves it rejects anything else), a
  `did:agent:* -> did:web:*` substitution table for a producer identity
  the harness holds the key for — `audience` entries and requester DIDs
  route through the same table so an audience check stays consistent with
  whichever bearer `sub` a scenario presents. It mints a per-scenario
  bearer from `effective_requester_did` (no `Authorization` header at all
  when it's `null`). Shape D runs under its own `shape_d_config()` with
  `auth.enabled = true`: the shared `config()` Shapes A/B/C use leaves auth
  off, which is right for them since they need no caller identity, but with
  auth off `caller_from_headers` returns `None` unconditionally, so every
  bearer Shape D minted was being discarded. Without that one line the
  per-scenario bearer is inert and any identity-sensitive assertion built on
  it would pass for the wrong reason. When a scenario's
  `registry_capabilities_subset` overrides `anonymous_public_reads`, the
  harness reconstructs the `RegistryServer` with the new capabilities
  document rather than only rebuilding the router around it: `search` and
  `retrieve` gate that flag on the server's own baked-in `caps`, not on
  `RegistryConfig`, so rebuilding the router alone left the override
  silently inert. Seeded state survives because `SqliteStore` is
  `SqlitePool`-backed and the pool is an `Arc`, so the clone shares the same
  in-memory database. Every Shape D fixture gets its own
  fresh in-memory store, isolated from the shared store Shapes A/B/C
  replay against. Dispatched deliberately **ahead of** Shape B (not after
  the fallback, as an earlier draft of this phase's plan had it): a
  `setup`-carrying fixture's `scenarios[]` also satisfies Shape B's own
  predicate, and Shape B has no seeding step — letting it capture such a
  fixture first would silently replay it against an empty store and read
  the resulting 404s as legitimate negative results. Shapes A, B, and C
  are textually unchanged. A fixture whose seed shape or scenario
  assertions Shape D doesn't recognize yet (`setup.lineages`,
  `matches_ctx_ids`, `total_estimate`, `context_subset_for_test`, …) still
  falls through to the narrowed — not deleted — `unseeded_precondition_reason`
  skip path rather than being partially replayed; this is what keeps this
  phase scoped to exactly one fixture (`vis-006`, the only single-exchange
  `vis` fixture) even though the rest of `vis-*` structurally satisfies
  Shape D's dispatch predicate. A dedicated regression test,
  `four_pre_existing_exchanges_still_use_original_shapes`, asserts the
  four exchanges replayed before this phase (`pub-004`, `pub-005`,
  `pub-008`, `ret-001`) still extract via their original shapes with
  identical fields — the gravest failure mode this phase could introduce
  is Shape D silently over-matching one of them. A second dedicated test
  proves Shape D end-to-end on `vis-006` and then, against an in-memory-only
  mutated copy of the fixture (never written to the spec checkout) whose
  seeded context's visibility is flipped to `restricted`, proves the
  replay now fails — demonstrating the harness exercises the registry's
  real visibility-scoping logic rather than trivially passing. A failed
  seed publish panics rather than skips, so a broken substitution can't
  quietly read as "fixture not applicable". Two further tests cover paths no
  fixture reaches yet, using synthetic in-test fixtures rather than spec
  reads: `shape_d_seeding_maps_one_shared_literal_agent_to_one_minted_did`
  pins the multi-seed `contexts_published` path, where two seeds sharing one
  literal `agent_id` must resolve to a single minted DID — seeding is
  two-pass for this reason, minting every distinct agent before any publish,
  so a repeat cannot overwrite an earlier mint and an `audience` naming a
  later-seeded agent still resolves; and
  `seeded_harness_rebuild_changes_router_behavior_and_preserves_seeded_state`
  pins both halves of the rebuild — that the anonymous-read posture actually
  changes, and that the seeded rows survive it. Note `vis-006` itself does
  not exercise the bearer path: its requester is
  `did:agent:any-authenticated-or-anonymous` against a public context, so it
  behaves identically with auth on or off. The bearer and rebuild mechanisms
  are proven by the synthetic tests, not by the replayed fixture. `MIN_REPLAYED_EXCHANGES`
  rises from 4 to 5.

- **`can-*` (RFC-ACDP-0001 canonicalization & hashing) moves from zero
  coverage to direct, fixture-driven coverage of all 35 vectors across all
  12 fixtures** (`REG-10` Phase 7). None of `can-*` is HTTP-replayable —
  the family carries no request/response shape at all — yet all 12 ids sit
  in the pinned spec's `acdp-registry-core.required_fixtures`, which makes
  `can` mechanically inexcusable under this file's `EXCUSED` ratchet. Two
  new tests in `conformance.rs` consume every fixture's own data directly,
  same precedent as `anc`/`wit`:
  `can_vectors_reproduce_canonical_form_and_hash` covers 30 of the 35
  vectors (can-001 through can-006, can-008 through can-012) by driving
  `acdp::crypto`'s public JCS surface directly —
  `canonical_preimage` for the Body/`content_hash`-shaped vectors,
  `canonicalize_value` for can-011's bare numeric-formatting objects (not
  ACDP bodies, so the Body-specific exclusion-set path is the wrong tool)
  and can-001's three `canonical_form`-only vectors, and
  `derive_lineage_id` for can-001's three `lineage_id`-only vectors. `can-001`
  alone packs three distinct `expected` shapes into its 7 vectors; a naive
  hash-equality loop would have silently covered only one of them.
  `can-006`'s two divergent-precision vectors are additionally asserted to
  produce different `canonical_form`/hash from each other, not just to
  each independently match their own pinned value. The second new test,
  `can007_registry_created_at_millisecond_truncation`, covers the
  remaining 5 — can-007 alone carries no `input`/hash at all, just a
  `registry_compliance` table keyed off example timestamps — by driving
  `acdp::time::trunc_ms` directly, the actual function
  `acdp-registry-sqlite`/`acdp-registry-pg` call when minting
  `created_at`, proving both that it reproduces the canonical millisecond
  form and that it floors rather than rounds. An explicit
  `EXPECTED_CAN_HASH_VECTOR_COUNT`/`EXPECTED_CAN_VECTOR_COUNT` pair (30 and
  35) guards against the vacuous-pass failure mode where a loop silently
  iterates zero vectors and passes green; proven by mutation on three
  fixtures (can-001, can-002, can-011), each of which fails when its
  `sha256_hex` is corrupted and passes again once restored. Both new tests'
  doc comments record the tension with this file's own anc-004 precedent
  (`conformance.rs`'s module doc-comment already argues against re-testing
  an upstream crate's golden vectors): most of `can`'s vectors do exactly
  that, but the coverage ratchet makes `can` inexcusable regardless, and
  the conformance claim is about this binary, not about which crate owns
  the tested code. No new dependency: `acdp::crypto` already re-exports
  `acdp_crypto`'s `canonicalize_value`/`canonical_preimage`/
  `derive_lineage_id`, and `acdp::time::trunc_ms` was already reachable.
  `KNOWN_FAMILIES`'s doc comment and the module doc-comment both gain a
  `can` paragraph mirroring the existing `anc` one — classification
  unchanged (still "non-HTTP fixture"), only coverage changed.

- **The witness aggregator's reject-then-no-write path is now directly
  tested** (`REG-10` Phase 4, GitHub issue #112). `witness.rs`'s
  `verify_and_store` was split at the point right after DID resolution:
  it now resolves the witness DID document and tail-calls a new private
  `verify_and_store_resolved(store, log, witness_did, doc_value,
  cosig_value) -> bool`, a verbatim lift of the verify-then-store half
  (the `verify_cosignature_against_own_log` match through the
  `upsert_witness_cosignature` call and the three metric-labeled early
  returns). This is a no-behavior-change refactor — `verify_and_store`'s
  signature, visibility, and callers are untouched — that makes the
  store-writing half callable directly against a real `SqliteStore`
  without a live witness endpoint. Two new tests exercise it:
  `rejected_cosignature_is_not_persisted_by_the_store_path` (a forged-root
  cosignature is reported unstored, and leaves no row at either the
  forged or the honest checkpoint tuple) and
  `verified_cosignature_is_persisted_by_the_store_path` (the positive
  control — a genuine cosignature is reported stored and reads back).
  Previously only the pre-store verification helper had forward-guard
  assertions; nothing exercised the actual persistence path.

- **`.github/workflows/bump-spec.yml`, a manual and dispatch-driven
  replacement for hand-written spec-pin bumps** (`REG-10` Phase 3, GitHub
  issue #110). It delegates to acdp-ci's reusable `bump-spec-ref.yml@v1`
  workflow, mirroring the existing `bump-acdp.yml` pattern: `with: file:
  .github/workflows/ci.yml` names the file whose single spec-pin anchor gets
  rewritten, `sha` picks the target commit (on `workflow_dispatch`, the
  input — blank meaning spec HEAD; on `repository_dispatch`, the event
  payload's SHA), and `secrets: inherit` supplies `ACDP_BOT_APP_ID` and
  `ACDP_BOT_PRIVATE_KEY`, already proven available in this repo via
  `notify-website.yml`. The reusable workflow hard-fails on zero or more
  than one spec-pin anchor in the named file, rewrites only the `ref:`
  following that anchor, asserts the rewrite landed, and opens a PR on
  branch `deps/spec-<sha:0:12>` for review — it never auto-merges, and it
  opens no PR when the target SHA already matches the current pin. Two
  triggers are wired: `workflow_dispatch`, runnable from the Actions tab
  today with an optional explicit `sha` input; and `repository_dispatch` on
  `spec-released`, which stays inert until the spec repo's
  `notify-spec-consumers.yml` adds this repo to its consumer matrix (as of
  this writing that matrix lists only `acdp-rs` and `acdp-verifier-py`).
  `CONTRIBUTING.md`'s conformance-pin paragraph now points at this workflow
  as an alternative to bumping the pin by hand.

- **`anc-001`/`anc-002`/`anc-003` move from "skipped as non-HTTP by the
  generic replayer" to direct, fixture-driven coverage, and require-mode CI
  is confirmed green at spec pin `417211f`** (`REG-3` Phase 7 — the closing
  phase of `plans/reg3-anchors.md`). None of `anc-001/002/003` is replayable
  through `crates/acdp-registry-server/tests/conformance.rs`'s generic
  `extract_shapes` at any pin: `anc-001` expects a *positive* (2xx) publish
  outcome carrying a `content_hash`/`signature` its own `input.notes` calls
  placeholders that don't recompute over the fixture's own body (Shape A
  refuses any non-400 publish outcome by design), and `anc-002`/`anc-003`
  carry only an `input.anchor_under_test` fragment, not a full body. So,
  following the same precedent already established for `wit-001`/`wit-004`
  and the did:key golden vector, three new in-process tests —
  `anc001_well_formed_anchor_is_accepted_and_round_trips`,
  `anc002_malformed_anchor_content_hash_is_rejected`,
  `anc003_empty_anchors_array_is_rejected_with_established_ordering` — read
  each fixture's own data via the existing `spec_fixtures()`/`read_json`
  helpers (resolved by the fixture's own `id` field through a directory
  scan, not a hardcoded filename), splice it into a freshly-signed body
  built with the same producer/`RequestBuilder` technique REG-3 Phase 5
  uses, and publish it against a **locally-built** capabilities document
  advertising `acdp_version: "0.5.0"` (`anc_caps_050`/`anc_harness_050` —
  the shared `caps()`, which stays `"0.1.0"` for
  `replays_spec_fixtures_when_present`, is never mutated). `anc-001`
  asserts HTTP 200 (this repo's actual publish success code, not the
  fixture's own literal `201`) plus both of the fixture's stated
  post-publish invariants (anchors served byte-identical; recomputed
  `content_hash` matches). `anc-002` asserts 400 `schema_violation` and its
  doc-comment states plainly that this exercises the *upstream*
  `acdp_validation::validate_anchors` shape check inherited from the `acdp`
  0.8.2 bump, not this repo's own Phase 3 version gate. `anc-003` asserts
  400 `schema_violation` on both a sub-`0.5.0` and a `0.5.0`-advertising
  registry, and additionally pins the ordering Phase 3 already established:
  on the sub-`0.5.0` registry this repo's own §10 version gate fires first
  (message names §10, not the SDK's "MUST be omitted entirely" wording); on
  the `0.5.0` registry the gate passes and the SDK's own empty-array rule
  fires instead. `anc`'s classification is unchanged by this phase — it was
  never `EXCUSED` and still isn't (`KNOWN_FAMILIES`'s doc-comment now
  records the added direct coverage) — and the skip manifest in
  `replays_spec_fixtures_when_present` still correctly shows
  `anc: 5 (non-HTTP fixture ...)`, since the replayer itself still doesn't
  replay any `anc-*` fixture; the three new tests run beside it, not in
  place of it. `MIN_REPLAYED_EXCHANGES` stays at exactly 4.

  `anc-004` and `anc-005` are deliberately OUT OF SCOPE: `anc-004` is a pure
  hash-computation golden vector (no endpoint, no request) over
  `acdp-crypto`'s JCS/hash pipeline, which this repo delegates to via the
  `acdp` dependency and does not own — Phase 5's
  `anchors_round_trip_byte_exact_sqlite` / `pg_anchors_round_trip_byte_exact`
  already prove that pipeline handles anchors correctly *through this
  repo's own storage*, which is what this repo is accountable for.
  `anc-005` is consumer-side behavioral (a scheme-unaware verifier
  tolerating an unknown scheme) — a registry has no verifier role, and the
  pinned spec places all five `anc-*` fixtures in `acdp-consumer`'s
  `required_fixtures`, never in any `acdp-registry-*` profile's.

  Confirmed green: `ACDP_REQUIRE_CONFORMANCE=1 ACDP_SPEC_DIR=<pinned
  417211f checkout> cargo test -p acdp-registry-server --features
  storage-sqlite,playground --test conformance --test conformance_gate`
  exits 0, with `replayed 4 exchange(s); failures=0` and zero
  `ACDP_SPEC_DIR unset` lines.

- **Behavioral and structural proof that `anchors[].uri` is never
  dereferenced** (`REG-3` Phase 6). Proves RFC-ACDP-0016 §6's NORMATIVE
  rule — stricter than the DataRef SSRF posture — that "there is no code
  path in core verification that ever reads `anchors[].uri`". Two tests,
  because neither alone is sufficient:
  `anchors_uri_never_dereferenced_publish_and_retrieve`
  (`crates/acdp-registry-server/tests/http_integration.rs`) binds a
  loopback `TcpListener`, publishes a context whose `anchors[0].uri`
  targets it, retrieves the context back, and asserts (after a bounded
  drain window, not an immediate check) that the listener observed **zero**
  connections at every point — while webhook delivery (the one subsystem
  near the publish path that *does* make a real outbound call) is
  deliberately wired live against a *second*, independent listener, so
  "zero" is a discriminating claim rather than an artifact of a harness
  that makes no outbound calls at all. The SSRF guard is configured with
  `SsrfPolicy::allow_test_loopback()` throughout so the guard is provably
  not what keeps the anchor listener silent — the claim under test is
  "nothing attempts the connection," not "a guard blocked it."
  `crates/acdp-registry-server/tests/anchors_uri_never_dereferenced.rs`
  adds the structural half: it enumerates every outbound-HTTP call site in
  the whole `crates/` tree (scoped to the zero-argument HTTP-client dispatch
  idiom, which cannot collide with channel `.send(msg)` calls that always
  take an argument) and asserts the set is *exactly* the three audited,
  legitimate ones (`acdp-registry-webhook/src/lib.rs`,
  `acdp-registry-auth/src/revocation_poller.rs`,
  `acdp-registry-core/src/witness.rs`) — failing loudly if a fourth ever
  appears — and that none of those three files mentions "anchor" in any
  form. Both live mutation checks were performed and confirmed: temporarily
  adding a throwaway fetch of `anchors[0].uri` to the publish path turned
  the behavioral test red (`left: 1, right: 0` on the zero-connections
  assertion); temporarily corrupting the structural test's expected-file
  set turned both structural assertions red with a clear drift diff. Both
  mutations were reverted before landing.

- **Byte-exact `anchors` round-trip proof, both storage backends** (`REG-3`
  Phase 5). Proves RFC-ACDP-0016 §5's normative requirement and anc-001's
  stated post-publish invariant: `anchors` survives publish → store →
  retrieve byte-exactly, such that `acdp::crypto::compute_content_hash`
  over the retrieved body reproduces the published `content_hash`. No test
  in this repo recomputed `content_hash` from a retrieved body at all
  before this (`grep -rn compute_content_hash crates/` was zero hits) —
  this is a first for the repo, not just for anchors.
  `anchors_round_trip_byte_exact_sqlite` /
  `anchors_two_entries_preserve_order_sqlite`
  (`crates/acdp-registry-server/tests/http_integration.rs`) and
  `pg_anchors_round_trip_byte_exact` / `pg_anchors_two_entries_preserve_order`
  (`crates/acdp-registry-server/tests/pg_integration.rs`) publish a
  **freshly-signed, self-consistent** request through the real router
  (`RequestBuilder::build()` computes its own `content_hash` — anc-001's
  own placeholder `content_hash`/`signature` are never replayed; anc-001 is
  used only as the shape reference for the first anchor's
  `scheme`/`content_hash`), fetch both `GET /contexts/{ctx_id}` and
  `GET /contexts/{ctx_id}/body`, and assert against both: the served
  `anchors` array is order-sensitive deep-equal (raw `serde_json::Value`,
  since `AnchorEntry` has no `Eq`/`Hash`) to what was sent, AND the
  recomputed hash matches. The two-anchor test body carries a `uri`, a
  flattened extension key holding a plain integer
  (`AnchorEntry.extensions`, `#[serde(flatten)]`) and a second flattened
  key holding `1e-7` — a value Postgres's `jsonb` type is known to
  re-render differently (in text form) from `serde_json`'s own output,
  unlike the plain integer, which round-trips through JSONB unchanged
  either way — on the first entry, and a structurally different second
  entry whose `scheme` sorts alphabetically *before* the first entry's (so
  a "helpful" ascending sort would visibly reorder the pair rather than
  being a no-op on the fixture). Array ORDER is therefore genuinely
  exercised (reordering changes the JCS preimage, and an accidental sort
  would now be caught), and Postgres's JSONB storage
  (`serde_json::to_value`, a normalizing representation — number
  re-rendering, key dedup/reorder) has real surface to diverge from
  SQLite's `TEXT` storage (`serde_json::to_string`) if it were going to. It
  doesn't: both backends reproduce the published `content_hash`
  byte-exactly — including across the `1e-7` re-rendering — run against a
  real `postgres:16-alpine` container — **no cross-backend JSONB
  normalization divergence found**. The PRIMARY proof of byte-exactness is
  the hash-recomputation assertion itself: because JCS canonicalization is
  sensitive to field drop, field mutation, and array reorder alike, a
  passing recompute on its own already rules out all three. Each
  round-trip test additionally runs a narrower, supplementary regression
  guard (`assert_ne!`) that simulates `anchors` being dropped from the
  served value and confirms the hash-recompute assertion would go red in
  that specific case — this guard only exercises the drop case, not
  reorder or mutation, so it does not by itself establish byte-exactness;
  it exists to catch a regression where the recompute assertion above
  stops actually depending on `anchors` (e.g. a future refactor that reads
  `content_hash` from a cached field instead of recomputing it).
- **RFC-ACDP-0016 §10/§14 version gate** (`REG-3` Phase 3). RFC-ACDP-0016
  (typed external `anchors`) is still **Draft**, not Final — this repo
  implements the two MUST-reject rules the spec defines for that field
  while the rest of the RFC remains a plain-library-type pass-through (see
  the Phase 2 `acdp` 0.8.1 → 0.8.2 bump below). A publish request carrying
  `anchors` is now rejected with `schema_violation` / HTTP 400 unless
  **both**: (§10) the registry's own **advertised** `acdp_version` — the
  exact string served at `GET /.well-known/acdp.json` — is `>= 0.5.0`, and
  (§14) the request's own **declared** `body.acdp_version` is `>= 0.5.0`
  (absent ⇒ `0.1.0` per `VERSIONING.md`'s layers table, so an omitted
  field is rejected exactly like an explicit `"0.1.0"`). The check runs in
  `publish_inner` (`crates/acdp-registry-core/src/handlers/context.rs`)
  immediately after the request body deserializes and before the per-agent
  rate limiter, so a version-rejected publish never consumes a producer's
  publish budget, and it sits above the `did:key` / playground-pinned /
  test-only / default `did:web` branch, covering all four publish paths
  with one gate. No new error variant or wire code was minted — both
  predicates reuse the existing `RegistryError::Acdp(AcdpError::SchemaViolation(..))`
  → `schema_violation` / 400 idiom, with the rejection message naming which
  of the two predicates failed. A private `version_at_least(v, major, minor)`
  helper (with its own unit tests, including the numeric-vs-lexical
  `"0.10.0" >= 0.5.0` case) is reimplemented in `context.rs` rather than
  reusing `acdp-validation`'s own version of the same name, which is
  private and not re-exported by the `acdp` facade crate; it fails closed
  on any version string that isn't plain `MAJOR.MINOR.PATCH`. `anchors: []`
  on a sub-0.5.0 registry is caught by this gate before the SDK's own
  empty-vec rejection ever runs (both produce the same wire outcome, but
  this gate fires first); `anchors: null` continues to be rejected at
  deserialize time, unrelated to this gate. The read path
  (`GET /contexts/{ctx_id}`, `/body`) is untouched — §10 gates publish
  only, and a body stored while the registry advertised `0.5.0` is served
  byte-exactly even if the registry's advertised version later changes.
  This phase does **not** make `acdp_version: "0.5.0"` reachable from any
  shipped configuration — the gate is exercised only via
  `crates/acdp-registry-server/tests/http_integration.rs`'s explicit
  `CapabilitiesDocument` override harness (`caps_050()`); making 0.5.0
  actually reachable in production config is a separate, later, one-way-door
  change.

- **JWT revocation** (`SEC-01`, `FEAT-02`): new `RevocationStore` trait with
  in-memory, SQLite, and Postgres backends; `issued_tokens` migrations
  (Sqlite 006, Postgres 005); `AuthService::issue_token` records every
  minted `jti` so `POST /auth/token/revoke` can authorize ownership;
  `JwtSigner::with_revocations` rejects revoked tokens at validate time.
- **Cross-registry resolution** (`FEAT-01`): `retrieve` forwards `ctx_id`s
  whose authority differs from the local registry through
  `acdp::client::CrossRegistryResolver`. Gated by
  `registry.cross_registry_resolution = true` (default).
- **Visibility search filter** (`FEAT-07`):
  `GET /contexts/search?visibility=public|restricted|private`.
- **Webhook event correlation** (`FEAT-04`, `FEAT-05`):
  `ContextPublished` carries `X-Run-Id` and the publish request's
  `derived_from` list.
- **Configurable CORS** (`SEC-02`): `[registry.cors] allowed_origins`.
  Empty (default) sends no CORS headers — replaces the prior
  `CorsLayer::permissive()`.
- **Body-size limit layer** (`SEC-06`): `tower_http`
  `RequestBodyLimitLayer` applies `limits.max_payload_bytes` to every
  route, not just publish.
- **SSRF guard on webhook URL** (`SEC-03`) and **non-empty webhook
  secret** (`SEC-04`): both enforced at startup by
  `WebhookEmitter::try_spawn` and `main::validate_config`.
- **DID-method fast-fail** (`SEC-05`):
  `AuthService::issue_challenge` rejects `agent_id`s that don't begin
  with `did:web:` before writing to the challenge store.
- **Pre-bind config validation** (`FEAT-09`): `main::validate_config`
  decodes `jwt_secret`, validates the webhook URL via the SSRF policy,
  checks TLS materials exist, and refuses the literal `changeme`
  placeholder.
- **Conformance harness** (`BUG-07`): `tests/conformance.rs` replays
  `pub-*` and `ret-*` fixtures from `ACDP_SPEC_DIR` when present;
  status + `json_contains` assertions with a null-as-wildcard sentinel.
- **Playground matrix test** (`DESIGN-03`): asserts `/admin/contexts` is
  mounted when the `playground` feature is compiled in but the runtime
  flag is off.
- **`POST /auth/token/revoke` endpoint** (`FEAT-02`).
- **Graceful shutdown** (`OPS-03`): `axum_server::Handle` with a 30s
  drain on `SIGTERM` / `Ctrl-C`.
- **Pretty-log toggle** (`OPS-04`): `ACDP_LOG_FORMAT=pretty` switches
  `tracing-subscriber` away from JSON for local development.
- Initial 8-crate workspace scaffold.
- `acdp-registry-types`: configuration (TOML + env), `RegistryError` with HTTP
  projection, webhook event envelopes, JWT bearer claims.
- `acdp-registry-store`: `ExtendedRegistryStore` trait extending
  `acdp::registry::RegistryStore` with `list_contexts`, `health`, `migrate`.
- `acdp-registry-sqlite`: SQLite backend with FTS5 virtual table, migrations,
  atomic `commit_publish`, idempotency cache, and visibility-filtered search.
- `acdp-registry-pg`: Postgres backend with `TIMESTAMPTZ` / `TEXT[]` / `JSONB`,
  `tsvector` FTS, `FOR UPDATE` row locking on the supersession check.
- `acdp-registry-auth`: DID challenge-response via `acdp::did::WebResolver` +
  `verify_ed25519`, HS256 JWT issuance/validation, pluggable
  `ChallengeStore` (in-memory + SQLite + Postgres).
- `acdp-registry-webhook`: HMAC-SHA256-signed POSTs with retry/backoff.
- `acdp-registry-core`: axum router + handlers generic over the storage trait.
- `acdp-registry-server`: binary wiring via Cargo features
  (`storage-sqlite` default, `storage-pg`, `storage-memory`, `playground`).
- Docker image (multi-stage with `cargo-chef`) + docker-compose with Postgres.
- GitHub Actions: `ci.yml` (fmt + clippy across feature matrix + test +
  cargo-deny), `release-plz.yml`, `docker.yml`.

### Changed

<!-- W2-U1 #185 (lane-1) -->

- **BEHAVIOUR CHANGE — a registry that boots today may refuse to start after
  this.** A config with `playground.enabled = true`, `playground.pinned_only =
  true` and **no** `[[playground.pinned_keys]]` entries is now refused at
  startup, **whether or not `[receipt]` is configured** (`#185`). Previously the
  check was nested inside the receipts block, so a registry with no `[receipt]`
  got no check at all.

  **If this stops your registry booting, it was not enforcing pinning.** That
  combination reads as a lockdown and does the opposite: `enforce_pinned_signature`
  (`playground.rs`) returns `PinOutcome::Skipped` when `pinned_keys` is empty,
  *before* `pinned_only` is consulted, and `Skipped` falls through to the fully
  unverified publish path. So every non-`did:key` agent was publishing without a
  signature check. (`did:key` identities take their own verified route before the
  playground gate and were never affected.) No deployment loses a working security
  property here; some discover they never had one.

  **Remedies**, in the message and in `docs/CONFIGURATION.md`: add at least one
  `[[playground.pinned_keys]]` entry, or set `playground.enabled = false`. If you
  intended to populate keys at runtime via `POST /admin/pinned-keys/reload`, boot
  with `pinned_only = false`, write both settings to disk, then reload — that
  endpoint swaps the whole `[playground]` section.

  **No shipped default is affected** — neither the compose stack nor the Railway
  recipe configures pinning at all.

  The startup message is rewritten, which was `#185`'s original point: it claimed
  the config *"would reject every publish outright"*, false in the dangerous
  direction — it reads as deny-all when the truth is the opposite. It now names
  the fall-through, explicitly negates the deny-all reading, and gives the
  remedies. Note the state it misnamed is real, just attached to the wrong config:
  a `pinned_keys` list whose entries have all expired *does* deny every publish —
  filed as `#193`.

  Because the guard now runs earlier in `validate_config`, a config that is both
  this *and* separately invalid (bad receipt seed, unknown profile, missing TLS
  path) reports the playground diagnosis first.

  Proven by mutation rather than coverage: the new test was run against the
  unhoisted guard and observed to **fail** — `expect_err` panicking on `()`, i.e.
  the validator returning `Ok` — then to pass after the hoist; re-scoping the
  guard back to receipts-only makes it fail again while the pre-existing receipts
  test stays green, which is what proves the test pins the hoist rather than the
  guard's existence.

  Runtime semantics are deliberately unchanged: `Skipped` = "no policy active"
  remains the contract `config.rs` documents and callers rely on. Two residual
  gaps found and filed rather than fixed, both outside this change's scope:
  `POST /admin/pinned-keys/reload` applies config with **no** validation, so this
  guard and every other config guard can be bypassed at runtime (`#192`); and
  pinned-key *entries* are never validated at startup (`#193`).

  *Line-pin sweep (CHARTER rule 10), recomputed from the final tree.* The hoist
  moves code, so pins into `main.rs` in the append-only records drift: bail A
  `:259` → `:282`, its rationale comment `:249` → `:272`, the public-bind guard
  `:475` → `:488`. The pins at `:267`/`:271-273` do not merely drift — **their
  target text no longer exists**, having been replaced. The `changeme` guard
  (`:92-104`, `:102-103`) sits above the edit and is unmoved. **Reported, not
  re-pointed:** re-pointing means editing existing lines in files this change
  treats as additive-only, and rewrites records that were true when written.
  `config.rs`'s `"Has no effect when pinned_keys is empty"` stayed at `:739`
  because the new doc text was appended *after* it rather than inserted above,
  deliberately, so `DECISIONS.md`'s citation of `config.rs:739-740` is still
  exactly valid.

<!-- REG-11 #144 + #139 (Lane C) -->

- **Both bump workflows pass the bot secrets explicitly instead of
  `secrets: inherit`** (`REG-11`, `#144`): `.github/workflows/bump-acdp.yml`
  and `.github/workflows/bump-spec.yml` now forward only `ACDP_BOT_APP_ID`
  and `ACDP_BOT_PRIVATE_KEY`. This is least-privilege hardening, **not** a
  fix for a breakage — `inherit` forwarded these two under the same names,
  so behaviour is unchanged. What changes is everything *else* that was in
  scope: this repo holds `CARGO_REGISTRY_TOKEN`, and `inherit` handed it to
  a job that runs `npm install` and two `curl | sh` installers. Both callees
  declare exactly these two secrets as `required: true` at the `v1` ref the
  callers pin, run with `permissions: {}`, and mint their own short-lived
  App token, so `inherit` always granted strictly more than they consume.
  No caller `permissions:` block was added — neither callee reads the
  caller's `GITHUB_TOKEN`. (`auto-merge.yml`'s callee does, so this does not
  transfer there.)

- **`bump-spec.yml`'s `repository_dispatch` comment corrected** (`REG-11`,
  `#139`): it claimed that trigger was "inert until the spec repo adds this
  repo to `notify-spec-consumers.yml`'s matrix". That is false, and
  falsifiable from this repo alone — `bump-spec.yml` has repeated successful
  `event=repository_dispatch` / `spec-released` runs (2026-09-05 onward),
  and PR `#147`, the spec bump merged as `e58eaf4`, was opened by
  `app/acdp-deps-bot` off that path. The replacement deliberately does not
  restate which consumers the spec repo notifies: mirroring another repo's
  routing table in a comment this file cannot keep in sync is what made it
  rot. The dispatch matrix itself is the spec repo's (`spec#40`) and is not
  edited here.

<!-- end REG-11 #144 + #139 -->

<!-- REG-11 #156 (Lane B) -->

- **BREAKING** (`#156`): the memory-backend tenancy refusal now covers
  `auth.require_tenant = true` as well as a non-empty
  `[[auth.tenant_agents]]`. Either signal alone, combined with
  `storage.backend = "memory"`, is refused at startup.

  This **narrows** the acceptance criterion the original refusal shipped
  under, which said every other backend/tenancy combination still starts.
  That is deliberate: `require_tenant = true` with an empty `tenant_agents`
  is a real configuration. With no agent bindings, no registry-issued token
  ever carries a `tenant` claim, so on the read path a caller asserts its
  tenant with the `X-Tenant-Id` header — which is what the registry's own
  default-deny message instructs. (Publishes differ: strict mode
  deliberately ignores that spoofable header when the producer has no
  binding, so a publish is denied outright.) On the memory backend those
  reads then fail identically to the case already refused.
  Strict mode denies every request resolving to no tenant, and any tenant a
  caller does assert cannot match the reserved `default` that every row
  reports, so each tenant-scoped read returns zero rows while the registry
  starts cleanly. Keying the refusal on `[[auth.tenant_agents]]` alone left
  that arm silently under-serving.

  An untenanted memory registry — neither signal set — still starts, which
  is the ephemeral demo case the backend exists for.

  Nothing in the repo relied on the newly-refused combination. A sweep for
  the memory backend combined with either tenancy signal found no test,
  fixture, example config, compose file, CI job, or doc snippet that hits
  it: `config/registry.example.toml` ships `sqlite` with
  `require_tenant = false`, `docker/config.docker.toml` ships `postgres`,
  and every `StorageBackend::Memory` site in the tree is in
  `validate_config` or its own tests. (The three integration tests that set
  `require_tenant = true` also set `tenant_agents`, so they were already
  inside the Phase 8 guard; they run on SQLite and never call
  `validate_config` regardless.)

<!-- end REG-11 #156 -->

<!-- REG-11 Phase 8 (Lane B) -->

- **BREAKING** (`REG-11` Phase 8, `#137`): `storage.backend = "memory"`
  combined with a non-empty `[[auth.tenant_agents]]` is now refused at
  startup. `validate_config` runs before migrations and before the socket
  is bound, so the process exits with a message naming both the backend and
  tenancy instead of starting a registry that serves nothing. A deployment
  that set both previously started cleanly and answered every tenant-scoped
  read with zero rows; it must now move to the `sqlite` or `postgres`
  backend, which are tenancy-aware.

  A warning would have nothing working to preserve on the read path.
  `MemoryStore` (`crates/acdp-registry-server/src/memory_ext.rs`) overrides
  none of the three tenancy methods on `ExtendedRegistryStore`, so it
  inherits their untenanted defaults: `set_tenant_of_ctx` is a no-op, and
  `tenant_of_ctx` / `tenants_of_ctxs` report `default` for every row.
  `default` is `RESERVED_TENANT`, which `reject_reserved_tenant`
  (`crates/acdp-registry-core/src/handlers/context.rs`) refuses from both
  `X-Tenant-Id` and the token claim — so the one tenant every row reports is
  the one tenant no caller may assert, and the filter matches nothing.

  The new check sits immediately after the existing `#17` guard that
  refuses `tenant_agents` with `require_tenant = false`, keeping the two
  tenancy refusals together; because that guard forces strict mode whenever
  `tenant_agents` is set, the rejected configuration is always the strict
  one. No other backend/tenancy combination was affected by Phase 8
  itself (see the `#156` entry above, which later narrowed this).
  Documented in `docs/MULTI-TENANCY.md` (new "Backend support" section)
  and `docs/CONFIGURATION.md`.

  *(This entry originally noted that `require_tenant = true` with an empty
  `tenant_agents` on the memory backend fails the same way and was not
  refused, tracking it as `#156`. That gap is closed by the `#156` entry
  above, in the same release, so the caveat has been removed rather than
  left to read as a standing limitation.)*

<!-- end REG-11 Phase 8 -->

<!-- REG-11 Phase 6 (Lane A) -->

- **The `EXCUSED`/`DEFERRED` ratchet now actually ratchets, in every CI job**
  (`REG-11` Phase 6, `#115`, `#130`): previously nothing in
  `crates/acdp-registry-server/tests/conformance.rs` forced a spec-required
  family *out* of `DEFERRED` and away from `EXCUSED` — zero coverage plus a
  written reason plus an open issue number was permanently green. A new
  `CORE_INEXCUSABLE_FAMILIES: &[&str]` const mirrors, by family, the pinned
  spec's `acdp-registry-core` `required_fixtures` ∪ `conditional_fixtures`
  (18 families: the 5 already `COVERED` — `can`, `idem`, `pub`, `ret`,
  `vis` — plus the 13 still `DEFERRED` — `body`, `caps`, `data-ref`,
  `did-ssrf`, `dk`, `err`, `lin`, `meta`, `rate`, `rev`, `schema`, `sig`,
  `status`). Two assertions give it teeth: **`core_inexcusable_families_
  are_never_excused_or_unclassified`** is unconditional (no `ACDP_SPEC_DIR`
  needed, so it runs in the required `tests` job) and fails if any mirror
  family is ever moved into `EXCUSED` or drops out of `COVERED ∪ DEFERRED`;
  the existing spec-gated `no_excused_family_is_required_by_our_profile`
  (required `conformance (spec fixtures)` job) now also `assert_eq!`s the
  mirror against the same set recomputed live from the pinned spec, so the
  literal cannot silently rot against a future spec-pin bump. Moving a
  family `DEFERRED` → `COVERED` requires **zero** edits to either assertion
  or the const, by design — the mirror is keyed to the spec's family-level
  obligations, not to which bucket currently covers them. The unconditional
  half is deliberate belt-and-suspenders: `conformance (spec fixtures)` sets
  `ACDP_REQUIRE_CONFORMANCE=1` and is a required status check today, but
  that required-check set is maintained by a sibling repo's hand-written
  mirror (independently confirmed stale as of 2026-09-05), so the
  unconditional `tests`-job half is what keeps the guarantee even if that
  context is ever dropped from branch protection. Measured, not derived:
  `ACDP_SPEC_DIR=<pinned-spec-checkout> ACDP_REQUIRE_CONFORMANCE=1 cargo
  test -p acdp-registry-server --features storage-sqlite,playground --test
  conformance --test conformance_gate -- --nocapture` → 44 passed + 1
  passed, 0 failed, `replayed 30 exchange(s); failures=0` (`pub: 3`, `ret: 1`,
  `vis: 26` — `MIN_REPLAYED_EXCHANGES` unchanged); `cargo test --workspace`
  and `cargo clippy --workspace --all-targets -- -D warnings` both clean.
- **Corrected seven `DEFERRED` reasons that misdescribed this repo's own
  implementation** (`REG-11` Phase 6, `#130`): four were factually false —
  `rate` claimed a missing "clock/limiter seam", but
  `limits.publish_rate_per_minute` (`config.rs:560-561`) is already enforced
  by the in-process `AgentRateLimiter` (`rate_limit.rs`, wired at
  `state.rs:86-89`), proven end-to-end by the sibling challenge-limiter test
  (`http_integration.rs:843-873`); `data-ref` called the family
  "consumer-leaning", but the 7 `data-ref-*` fixtures in
  `acdp-registry-core`'s `required_fixtures` (`data-ref-001..007`) are
  registry-side publish-path rejections (RFC-ACDP-0002 §6) — the 8th,
  `data-ref-008-external-data-ref-hash-mismatch`, is a consumer fetch-time
  check and is not required of this profile; `status` called it "lifecycle
  status transitions", but the fixtures test the `status` *string's*
  grammar (RFC-ACDP-0004 §4.1), not lifecycle state; `rev` called it a
  "verification-side family", but RFC-ACDP-0014 §4/§5 assigns this to the
  registry, which already implements `ContextType::KeyRevocation`. Three
  more overstated the gap as "needing a new seam" when the seam already
  exists: `log`'s `/log/checkpoint`, `/log/proof`, and `/log/entries`
  routes are always mounted (`crates/acdp-registry-core/src/lib.rs:86-88`).
  `log`, `rcpt`, and `lhr` each have two real causes, not one: one fixture
  per family (`log-001`/`log-003`, `rcpt-001`, `lhr-001`) carries no
  `applies_to_profiles` and is a non-HTTP golden vector needing a
  direct-vector pass; the rest (`log-002`/`log-004`, `rcpt-002..004`,
  `lhr-002..004`) are restricted to a profile this harness doesn't
  advertise (`HARNESS_PROFILES`, `conformance.rs:425`) — advertising it
  would not make the non-HTTP vectors run.
  No family moved bucket in this phase — this is a record correction plus
  the new ratchet, not new coverage.

- **`did-ssrf`'s deferral reason corrected (same review pass).** It claimed the family
  "needs a controlled resolver seam this harness doesn't have yet". That named the wrong
  cause and was falsifiable from this suite's own output: all five `did-ssrf-*` fixtures
  are reported under `non-HTTP fixture (vectors / schema / informative)` in the skip
  tally, so they never reach a resolver. The seam also already exists —
  `acdp_did::WebResolver` applies `SsrfPolicy::default()` unconditionally, and exposes
  `with_ssrf_policy` and `with_test_endpoint` under the `test-transport` feature already
  enabled on `acdp` in this crate's dev-dependencies. The real gap is a direct-vector
  pass like `can`'s.

<!-- end REG-11 Phase 6 -->

<!-- REG-11 #143 (Lane C) -->

- **`ci.yml`'s conformance job checks the spec out through
  `acdp-ci/actions/checkout-spec` rather than a hand-rolled `actions/checkout`**
  (`REG-11`, #143). The pinned spec ref is unchanged —
  `d1f06d0d49b73d411a3983d3877321ccaccd38e7`, exactly as merged in #147; only
  the mechanism that checks it out changed. This was an adoption ask, not a
  compliance defect: #143 states outright that the previous form already
  satisfied the substance of the spec-pinning rule.
  The action resolves at `015910153b61c32abbe018afe85d44868897bf3b # v1` — the
  commit `acdp-ci`'s **annotated** `v1` tag dereferences to (`refs/tags/v1` →
  tag object `82b2a25…` → commit `0159101…`, confirmed against the remote via
  `gh api`; the `action.yml` blob at that commit is `8c5dacd…` on GitHub and in
  the local clone alike). That is `acdp-verifier-py`'s call-site pin verbatim,
  and it follows `REG-8`/`REG-10`'s rule that a non-`actions/*` action resolves
  at an immutable 40-hex SHA with a `# <version>` comment. The
  `acdp-ci/.github/workflows/*@v1` reusable-workflow refs are untouched and stay
  tag-tracked — a composite action is a `uses:` step inside this job, not the
  family propagation that exemption protects.
  Three guarantees the hand-rolled form did not have: the action rejects a `ref`
  that is not 40-hex lowercase before any network call; it re-reads the
  checked-out `HEAD` and fails when it differs from the pin; and it refuses the
  `set-env: false` + `require-conformance: true` combination, which would export
  `ACDP_REQUIRE_CONFORMANCE` without `ACDP_SPEC_DIR`.
  The `Conformance (spec fixtures required)` step still sets both variables
  itself rather than relying only on the action's `$GITHUB_ENV` exports, so
  require-mode stays a property of *this* file and cannot be switched off by an
  action input. `ACDP_SPEC_DIR` now reads the action's `path` output instead of
  restating `${{ github.workspace }}/acdp-spec`, so the checkout location and
  the variable cannot drift apart.
  No `repository:` or `path:` override is passed, even though both would merely
  restate the action's own defaults: `bump-spec-ref.yml` counts a
  `repository: <spec>` line as a **second** pin anchor and then fails the bump
  outright rather than rewrite only the first, so adding one would silently
  disable `bump-spec.yml`. Checked by running that workflow's own anchor-count
  `awk`, its `perl` rewriter and its post-rewrite assertion — copied verbatim
  from `acdp-ci` at `v1` — against the new file: one anchor, `d1f06d0…`
  resolved as the current pin, and a simulated bump rewrote exactly one line,
  the spec `ref:`, leaving the action's own `@0159101…` pin alone.
  For this job, a green result is not by itself evidence — so acceptance was two
  negative controls plus the replay tally, not colour. Measured with
  `cargo test -p acdp-registry-server --features storage-sqlite,playground
  --test conformance --test conformance_gate` against a spec worktree detached
  at the pinned `d1f06d0…`: `replayed 30 exchange(s); failures=0`, with a
  `ran by family` tally of `pub: 3` / `ret: 1` / `vis: 26`. No raw pass count
  is recorded here on purpose — that number moves with every merge that adds
  a test (it was 43 against this branch's original merge-base, 44 once `#158`
  landed, 46 after Phase 7) and says nothing about this change. The replay
  tally is the invariant #143 actually has to preserve, and it did not move.
  Re-pointed at a nonexistent `ACDP_SPEC_DIR` with
  `ACDP_REQUIRE_CONFORMANCE=1` the same command panics at
  `conformance.rs:2549` and exits 101; with neither variable set it exits **0**
  while replaying nothing, logging `ACDP_SPEC_DIR unset or no fixtures
  resolvable; skipping`. That second control is why the pin-and-require design
  exists, and why the CI job log — not the check mark — is the acceptance
  artifact. CI reproduced the replay tally and the per-family breakdown
  exactly.
  Note that `MIN_REPLAYED_EXCHANGES` (derived from source as 30) is met
  *exactly*, with no headroom: losing one replayed exchange turns the required
  job red.

<!-- end REG-11 #143 -->

- **BREAKING** (`REG-11` Phase 3, `#133`): `GET /admin/contexts` is now
  gated behind `require_admin_bearer`, like every other `/admin/*` route.
  A registry with an empty `auth.admin_tokens` (the shipped default in both
  `config/registry.example.toml` and `docker/config.docker.toml`) now
  answers 403 `admin-only` where it previously served rows to anyone —
  operators who rely on this route MUST configure at least one entry in
  `auth.admin_tokens` and send it as `Authorization: Bearer <token>`.
  `caller_from_headers` was deliberately removed from this handler: the
  admin gate has already resolved "who is calling", and re-parsing the same
  header a second time under `caller_from_headers`'s rules would hand the
  admin token to `validate_bearer`, which fails on a non-JWT and returns 403
  whenever `auth.enabled = true` — precisely the registries that configure
  admin tokens. `tenant_for_request` is still called; it is a separate
  resolution step, deliberately tolerant of a non-JWT bearer. The frozen
  disclosure rule: an admin bearer counts as an **authenticated but
  unnamed** caller for the RFC-ACDP-0008 §4.5 public arm. Restricted and
  private bodies are never disclosed to the admin listing — the SQL
  predicate's restricted/private arms both require a non-NULL requester DID
  (`LIST_VISIBILITY_SQLITE`, `crates/acdp-registry-sqlite/src/store.rs:423-436`,
  and its Postgres twin, `crates/acdp-registry-pg/src/store.rs:171-175`),
  which a `None` requester can never reach. Both shipped configs gained a
  commented `# admin_tokens = ["<generate out of band>"]` under `[auth]` so
  the now-mandatory setting is discoverable.
- **Restored the RFC-ACDP-0008 §4.5 `anonymous_public_reads` disjunct to
  `list_contexts`** (`REG-11` Phase 2, `#133`): the pg/sqlite list-visibility
  predicate was a bit-for-bit copy of `search`'s disclosure predicate MINUS
  the `anonymous_public_reads ||` term on the `public` arm — `retrieve` and
  `search` both already honor the flag; `list_contexts` was the sole
  outlier. `ExtendedRegistryStore::list_contexts` gains a fifth parameter,
  `anonymous_public_reads: bool` (`crates/acdp-registry-store/src/lib.rs`),
  mirroring `RegistryStore::search`'s parameter of the same name; both SQL
  backends' visibility predicate now reads
  `Public => anonymous_public_reads || requester.is_some()`, arm-for-arm
  with `search` (`acdp-registry-pg/src/store.rs:171-175`,
  `acdp-registry-sqlite/src/store.rs:423-436`; the SQLite `LIST_VISIBILITY_SQLITE`
  const grows from three `?` placeholders, all bound to the requester, to
  five: `?req`, `?anon`, `?req`×3).
  **Observable behavior is unchanged in this release**: the sole production
  call site, `admin_list` behind `GET /admin/contexts`
  (`acdp-registry-core/src/handlers/admin.rs`), passes a literal `true`
  rather than `auth.anonymous_public_reads`, so today's "public rows always
  listed" outcome is reproduced byte-for-byte. Wiring the real config value
  there, together with gating the route behind the admin bearer, is a
  later, deliberately atomic change — not this one. Both backends'
  `sql_disclosure_matches_rfc_4_5_across_the_matrix` oracle test now
  exercises `list_contexts` under both `anonymous_public_reads` values
  inside the same `for anon_reads in [true, false]` loop that already
  covered `search`, closing the one place this repo's own §4.5 restatement
  didn't reach.
- **Twelve dependency majors** (`REG-11` Phase 1, `#136`): `rand` 0.8→0.10
  (`rand::thread_rng()` → `rand::rng()`, `RngCore` no longer at the crate
  root — `crates/acdp-registry-auth/src/service.rs`,
  `crates/acdp-registry-server/src/main.rs`), `hmac` 0.12→0.13 (`KeyInit`
  now imported separately from `Mac` —
  `crates/acdp-registry-webhook/src/lib.rs:250`), `jsonwebtoken` 9→11
  (switches the JWT crypto provider to the `rust_crypto` feature — no
  other provider was enabled by default before), `thiserror` 1→2,
  `ed25519-dalek` 2→3, `toml` 0.8→1.1, plus `tower-http`, `sha2`, `base64`,
  `config`, `metrics-exporter-prometheus`, and `serial_test`. `serial_test`
  is held at `3.x` rather than the dependabot-proposed `4.x`, which
  requires rustc 1.93.1 and would break the `msrv (1.88)` CI job. Added a
  `deny.toml` `advisories.ignore` entry for `RUSTSEC-2023-0071` ("Marvin",
  an RSA private-key timing side channel): `rsa` enters the dependency
  graph only because jsonwebtoken 11's `rust_crypto` feature enables it
  unconditionally, every JWT verification pins a single non-RSA algorithm
  before any crypto verifier is constructed, and this registry holds no
  RSA private key material to time. **This is the repository's second
  advisories-ignore entry, not its first** — `RUSTSEC-2025-0134` was
  ignored and then removed by PR #97 once `axum-server` 0.8 dropped
  `rustls-pemfile` from the graph entirely; this one is expected to
  outlive that pattern, since no upstream `rsa` patch exists for Marvin.
- **Extracted a shared `tests/common/` harness for `conformance.rs` and
  `http_integration.rs`** (`REG-10` Phase 6). Rust integration tests
  compile to separate binaries, so `conformance.rs` couldn't reach
  `http_integration.rs`'s router-building code and had grown three
  near-identical ad-hoc routers plus its own copies of
  `pct_encode_path_segment`, `body_to_json`, and `producer`. All of that
  now lives in `crates/acdp-registry-server/tests/common/mod.rs`
  (`mod common;` in both files), parameterized over store backend
  (`StoreMode::Memory` | `StoreMode::File` — `conformance.rs` used an
  in-memory `SqliteStore`, `http_integration.rs` a `NamedTempFile`; both
  are preserved) and capabilities document, so `conformance.rs`'s three
  routers and `http_integration.rs`'s harness ladder now both build on
  `common::build_harness_with_webhook`. `body_to_json` is deliberately kept
  as two functions rather than one: the two files' copies differed, with
  `http_integration.rs`'s panicking on an empty or non-JSON body and
  `conformance.rs`'s degrading to `Value::Null`. The strict form is the
  shared default at all 53 call sites, and `body_to_json_lenient` is used
  at exactly one — the fixture replayer, which drains arbitrary spec
  fixtures where an empty response is legitimate. Collapsing them onto the
  lenient form would have silently removed a guard from ~50 assertions.
  Pure refactor: the only line changed inside a `#[test]` body is that one
  replayer call, which is behavior-preserving, and the full suite passes
  with the exact same test count as before (175, across all seven binaries
  under `--features storage-sqlite,playground`).

- **Reworded the wit-001/wit-004 quorum assertion's message and its
  preceding comment** (`REG-10` Phase 5, GitHub issue #113) in
  `wit004_key_mismatch_cosignature_is_rejected_and_wit001_golden_is_accepted`.
  The old message claimed the assertion proved `report_both.witnesses`
  names *wit-001's* witness and not wit-004's — impossible by
  construction, since the test pins both fixtures to the same witness
  assertionMethod key and only ever registers one witness DID. The
  assertion is unchanged; it actually proves the one verifying
  cosignature is attributed exactly once in `witnesses`, consistent with
  `witnessed_count`. No executable change.

- **`storage-memory` gets its first required CI coverage** (`REG-10`,
  issue #109). `.github/workflows/ci.yml`'s `clippy` job gains a fourth
  step, `clippy (memory)`, running
  `cargo clippy -p acdp-registry-server --no-default-features --features
  storage-memory --all-targets -- -D warnings`; the `test` job gains a
  matching `cargo test (memory)` step running `cargo test -p
  acdp-registry-server --no-default-features --features storage-memory`.
  Both are appended as steps to the existing jobs, matching how the
  pg/sqlite/playground legs are already structured — no new job, no
  matrix, and `--no-default-features` is load-bearing: `storage-sqlite`
  is the crate's `default` feature, and a bare `--features storage-memory`
  would trip the `storage-sqlite`+`storage-memory` mutual-exclusion
  `compile_error!` in `crates/acdp-registry-server/src/main.rs`. Both legs
  land in the *required* `clippy`/`test` jobs, not the advisory `msrv`
  job, so a future break in `memory_ext.rs` now blocks merge instead of
  going uncaught entirely — before this change no CI job built
  `storage-memory` at all. Deliberately scoped as compile + lint coverage,
  not behavioral coverage: `--all-targets` compiles the bin and test
  targets under this feature set (closing a compile gap `memory_ext.rs`
  had never been checked against before), and `cargo test`'s memory leg
  runs the binary's own unit tests plus three tests from the two
  always-compiled integration binaries, but `tests/conformance.rs`,
  `tests/http_integration.rs`, and `tests/metrics_integration.rs` are all
  `#![cfg(feature = "storage-sqlite")]` and compile to zero tests under
  `storage-memory` — this leg proves the memory cfg gates and the memory
  `run()` arm typecheck and pass lint, not that the memory-backed store
  behaves correctly against the HTTP surface. Verified locally: both
  commands pass as-is (clippy clean; 40 tests pass — 37 unit plus 3 from
  the two always-compiled integration test binaries, with `conformance.rs`
  / `http_integration.rs` / `metrics_integration.rs` / `pg_integration.rs`
  each reporting `0 tests` under this feature set); a deliberate type
  error introduced into `memory_ext.rs`'s `put` impl made the clippy leg
  fail with `E0061`, confirming the leg is non-vacuous, then was reverted.
  `CONTRIBUTING.md`'s feature-flag variant block gains the matching
  `storage-memory` invocations alongside the existing pg/sqlite ones.
- **`acdp_version` capability advertisement now reaches `"0.5.0"`** (`REG-3`
  Phase 4 — **one-way-door**). Phase 3's RFC-ACDP-0016 §10/§14 version gate
  shipped with no reachable configuration of the shipped binary that could
  ever clear it — the ladder topped out at `"0.4.0"`, so every anchored
  publish was rejected forever. `crates/acdp-registry-server/src/main.rs`'s
  `build_capabilities` is refactored from the four-rung ordered if/else
  ladder into an order-independent `max()` over independent per-feature
  version claims (`ladder_claims` / `acdp_version_claim`): witnesses
  configured still contributes `"0.4.0"`, lifecycle/log/head-receipts still
  contributes `"0.3.0"`, a configured receipt key still contributes
  `"0.2.0"`, the base floor is still `"0.1.0"` — and a fifth,
  **unconditional** claim of `"0.5.0"` is added for `anchors` support,
  since RFC-ACDP-0016 §10 is explicit that anchors is "a body field, not a
  registry surface" with no new profile or admin-config gate to check (the
  accept/reject/store/serve handling runs on every publish regardless of
  config). Because that claim is both unconditional and the largest value
  in the `max()`, it wins for every configuration: **every reachable
  deployment of the shipped binary now advertises `acdp_version >=
  "0.5.0"`**, including a completely bare one with no receipt key, log, or
  witnesses configured. This is a deliberate, acknowledged trade-off, not
  an oversight — the previous four rungs no longer distinguish themselves
  in what `build_capabilities` actually serves (a consumer can no longer
  infer "does this registry aggregate witness signatures" from
  `acdp_version` alone), which is exactly the cost the plan names for this
  option. `RegistryServer::try_new`'s `validate_capabilities` startup check
  still passes for every existing config permutation (its only
  version-conditioned guard is `>= 0.3.0` requiring
  `supports_idempotency_key: true`, which is unconditionally `true` here
  regardless of version). This phase **executes the follow-up OQ2's own
  `DECISIONS.md` entry (2026-08-29) already recorded** — "if a 5th
  `acdp_version` rung is ever added, consider replacing the ordered if/else
  ladder with an order-independent `max()` over per-feature version
  claims" — rather than superseding OQ2's decision; OQ2's conditional
  0.4.0-ahead-of-0.3.0 ordering is unchanged, just re-expressed as one
  candidate among several. Logged `UNCONFIRMED` in `ASSUMPTIONS.md` pending
  `/reconcile` sign-off, per this repo's standing practice for one-way-door
  decisions.
- **Bumped `acdp` 0.8.1 → 0.8.2** (`REG-3` Phase 2), pulling all twelve
  `acdp-*` workspace crates in lockstep. This inherits `AnchorEntry`,
  `PublishRequest::anchors`, `Body::anchors`, `validate_anchors`, and
  anchors-in-preimage hashing (RFC-ACDP-0016) as plain library types — this
  repo adds no local struct or validation code for them in this commit. The
  crypto dependency graph moved with it, not just `acdp` itself: major
  version bumps across `base16ct` (0.2→1.0), `crypto-bigint` (0.5→0.7),
  `ecdsa` (0.16→0.17), `elliptic-curve` (0.13→0.14), `ff`, `group`, `p256`,
  `primeorder`, `rfc6979`, `sec1`, plus new dependencies
  `curve25519-dalek` 5.0, `ed25519-dalek` 3.0, `der`, `digest` 0.11,
  `sha2` 0.11, and `signature` 3.0 (`hashbrown` 0.16 dropped). This repo's
  own direct `ed25519-dalek = "2.2"` dev/runtime dependency
  (`acdp-registry-auth`, `acdp-registry-server`) now coexists with `acdp`'s
  transitive `ed25519-dalek` 3.0 as two separate major-version instances in
  the graph; it previously relied on feature unification with `acdp`
  0.8.1's own (then-matching) `ed25519-dalek 2.x` dependency to enable the
  `rand_core` feature (`SigningKey::generate`), which the 0.8.2 bump broke
  by moving `acdp`'s own dependency to `ed25519-dalek` 3.0. Fixed by
  splitting `acdp-registry-auth`'s `ed25519-dalek = "2.2"` dependency: kept
  featureless under `[dependencies]` (its actual production use, `jwt.rs`'s
  PEM decoding, never calls `generate`) and added a new
  `[dev-dependencies] ed25519-dalek = { version = "2.2", features =
  ["rand_core"] }` for tests that do call `generate` — rather than widening
  the production feature set for a test-only need. `acdp-registry-server`'s
  own `[dev-dependencies]` `ed25519-dalek` entry gains the same explicit
  `rand_core` feature for the same reason (it did not carry the feature
  before this commit; feature unification with `acdp`'s own then-2.x
  dependency supplied it implicitly). Neither fix bumps this repo's own
  dependency to 3.0 or pins `acdp` back — both are manifest-only, no
  production source changed. `cargo deny check` stays green (advisories,
  licenses, bans, sources all `ok`) with `deny.toml`'s `ignore = []`
  unchanged; the expanded graph's ~30 new/duplicated transitive crates
  (`base64`, `block-buffer`, `cmov`, `const-oid`, `cpubits`, `crypto-common`,
  `ctutils`, `curve25519-dalek`, `der`, `digest`, `ed25519`, `ed25519-dalek`,
  `fiat-crypto`, `hmac`, `hybrid-array`, `pkcs8`, `primefield`, `serdect`,
  `sha2`, `signature`, `spki`, `wnaf`, plus a `lru` 0.16→0.18 bump not
  previously called out) all license-clear under the existing allow-list and
  raise no new advisory; `[bans] multiple-versions = "warn"` accepts the
  resulting duplicate-major-version crates (including the `ed25519-dalek`
  2.x/3.x split) as warnings, not failures. **This commit alone would let a
  publish carry `anchors` with no version gate whatsoever** — it ships only
  combined with the RFC-ACDP-0016 §10/§14 version gate (`REG-3` Phase 3) in
  the same PR, and must not reach `main` on its own.
- **Pinned conformance spec SHA bumped to `417211f6a13aeceef4db00eb67f98ed0ed13761b`**
  (`REG-3`) in `.github/workflows/ci.yml`. The only substantive delta since the
  prior pin is RFC-ACDP-0016's draft and its conformance pack (the new `anc-*`
  fixture family, SPEC-9); the third commit in the range is an unrelated
  SPEC-7 rev-002 change with no anchors content. This repo does **not** yet
  implement RFC-ACDP-0016 — `anc` is classified in
  `crates/acdp-registry-server/tests/conformance.rs`'s `KNOWN_FAMILIES` (not
  `EXCUSED`) and picked up by the replay harness's non-HTTP fallthrough, same
  as `wit`; real coverage is a separate, later change.
- **Pinned conformance spec SHA** (`REG-2`) adopted in
  `.github/workflows/ci.yml`: now `31cf8743b62debe2c7c8572ce3a3a0b7ca5ad099`.
  Annotation-only at this pin: RFC-ACDP-0015 promoted Draft → **Final**
  (0.4.0), the `invalid_witness_cosignature` error code promoted
  Proposed → **Stable**, and the `acdp-log-witness` profile promoted
  Draft → **Final**. No fixture family, fixture shape, `id`, `request`,
  or `expected` field changed; the conformance harness runs unchanged
  against the new pin (`16 passed; 0 failed`, 4 exchanges replayed).
- **SHA-pinned credential-bearing workflow actions** (`REG-8`): every
  third-party action in `docker.yml` and `release-plz.yml` (plus
  `peter-evans/repository-dispatch` in `notify-website.yml`) now
  resolves at an immutable 40-hex commit SHA with a `# vX.Y.Z` (or
  `# stable` for `dtolnay/rust-toolchain`, a selector rather than a
  version) comment, matching `acdp-rs`'s two-tier pinning posture:
  `docker/setup-buildx-action@8d2750c68a42422c14e847fe6c8ac0403b4cbd6f
  # v3.12.0`, `docker/login-action@c94ce9fb468520275223c153574b00df6fe4bcc9
  # v3.7.0`, `docker/metadata-action@c299e40c65443455700f0fdfc63efafe5b349051
  # v5.10.0`, `docker/build-push-action@10e90e3645eae34f1e60eeb005ba3a3d33f178e8
  # v6.19.2` (both call sites), `dtolnay/rust-toolchain@4be7066ada62dd38de10e7b70166bc74ed198c30
  # stable` (matches `acdp-rs`'s own current pin verbatim),
  `MarcoIeni/release-plz-action@2eb1d8bcb770b4c48ccfaad919734b38b51958c9
  # v0.5.131`, and `peter-evans/repository-dispatch@28959ce8df70de7be546dd1250a005dd32156697
  # v4.0.1`. First-party `actions/checkout@v4` stays tag-pinned
  (deliberate, matching the sibling's first-party tier), and the
  `acdp-ci/.github/workflows/*@v1` reusable-workflow refs are
  untouched (pinning those would break family propagation).
  `docker/login-action`, the pushing `docker/build-push-action` call,
  `MarcoIeni/release-plz-action`, and `dtolnay/rust-toolchain` are not
  exercised by this PR's own CI (gated off `pull_request` events or
  behind `on: push: branches: [main]`). `docker/login-action`,
  `docker/build-push-action`, and `MarcoIeni/release-plz-action` were
  each independently re-verified against their tag via `gh api`.
  `dtolnay/rust-toolchain` is the one deliberate exception: it is
  pinned to match `acdp-rs`'s own current SHA verbatim rather than to
  whatever `@stable` resolves to today (the two diverge — see the
  Approach section of `plans/reg2-reg5-reg6-reg8-reg9-wave4.md`
  Phase 9), so verifying it against `@stable` would show a mismatch
  by design; parity with the sibling's pin is the actual check.
- **Retired stale supply-chain narration** (`REG-9`): the
  `Cargo.toml` comment above the `acdp` dependency wrongly described a
  0.5.3-era, per-sub-crate version mix; it now states that `acdp`
  0.8.1 is a facade crate over eleven sub-crates published to
  crates.io and kept in lockstep at the same version, without naming
  individual sub-crate versions. `deny.toml`'s dead `allow-git` entry
  for the `acdp-rs` repo (a leftover from when `acdp` was git-sourced)
  is removed, which silences a persistent `cargo deny`
  `unmatched-source` warning. The one behavioral consequence: any
  future git dependency on `acdp-rs` is now a `cargo deny` finding
  instead of a silent allowance.
- **Wire-code mapping for `invalid_witness_cosignature`** (`REG-2`):
  `AcdpError::InvalidWitnessCosignature` now maps to wire code
  `invalid_witness_cosignature` / HTTP 502 in
  `acdp-registry-types::error::{acdp_wire_code, http_status_for_acdp}`,
  instead of falling through to the `internal_error`/500 catch-all.
  Deliberately kept distinct from `invalid_log_proof` even though both
  are 502, so a client can tell which upstream artifact failed
  verification. No handler emits this error yet (`grep -rn
  InvalidWitnessCosignature crates/acdp-registry-core/src/handlers/`
  returns zero hits) — this only closes the wire-mapping gap ahead of
  emission, which is a separate later change.
- **`acdp_version` claims 0.4.0 when witnesses are configured** (`REG-2`):
  `main::build_capabilities`'s version ladder gains a new rung, checked
  first, `!cfg.witnesses.is_empty() => "0.4.0"`. RFC-ACDP-0015 §6.1 witness
  cosignature aggregation — already implemented in full by
  `acdp-registry-core::witness` — is the sole registry-side 0.4.0
  obligation, so a deployment that aggregates witnesses was serving a
  0.4.0 wire member (`witness_signatures`) under a 0.3.0 `acdp_version`
  banner; this closes that under-claim and makes the prior `REG-2`
  wire-code entry's below-0.4.0 gate satisfiable rather than vacuously
  true (no deployment could ever have claimed 0.4.0 before this). Gated
  on `!cfg.witnesses.is_empty()` rather than `cfg.log.enabled`, since
  `validate_config` already refuses startup with witnesses configured and
  `log.enabled = false` — the new rung stays monotone with the existing
  0.3.0 rung on the real startup path without over-claiming 0.4.0 for
  every transparency-log registry that aggregates nothing. Does **not**
  advertise the `acdp-log-witness` profile — the spec forbids that for a
  registry (a witness is not a registry); only the version string
  changes.
- **Fixture-driven `wit-004`/`wit-001` coverage** (`REG-2`):
  `acdp-registry-server/tests/conformance.rs` gains
  `wit004_key_mismatch_cosignature_is_rejected_and_wit001_golden_is_accepted`,
  the first genuine coverage of the `wit-*` family against real pinned
  fixture data. Drives `acdp::client::verify_witness_cosignature_value`
  and `evaluate_witness_quorum` directly (not HTTP — RFC-ACDP-0015 §8
  witness-cosignature verification is a pure library check): the pinned
  wrong-key `wit-004` cosignature is rejected with
  `InvalidWitnessCosignature` naming the actual signature-verification
  failure, the paired `wit-001` golden verifies under the same witness
  key as a positive control, and the rejected cosignature does not count
  toward the N-witnessed quorum while the golden one does. `wit-*`
  remains classified "non-HTTP fixture" by the HTTP replay harness — this
  adds coverage beside it, not a reclassification.
- **Strengthened registry-side fork-refusal tests** (`REG-2`):
  `acdp-registry-core::witness`'s existing
  `cosignature_over_wrong_root_is_rejected` and
  `cosignature_beyond_current_head_is_rejected` previously asserted
  only `matches!(err, AcdpError::InvalidWitnessCosignature(_))` — a
  variant shared by the log-id mismatch, the beyond-head case, and every
  §8 verification failure, so `cosignature_over_wrong_root_is_rejected`
  could not tell a real root-mismatch rejection from an accidental
  beyond-head one. Both tests now additionally assert on their own
  distinct error MESSAGE wording (root_hash/checkpoint-mismatch vs.
  "beyond this registry's current head"), and both now assert the store
  holds zero cosignatures for the checkpoint tuple after the rejection —
  the property that actually matters operationally, since a rejected
  forged cosignature must never be stored or the aggregator could later
  serve a bogus one. `cosignature_over_wrong_root_is_rejected`'s forged
  root is now hardcoded to wit-002's own pinned root-rewrite vector
  (`sha256:deadbeef00000000000000000000000000000000000000000000000000000000`
  from `wit-002-consistency-refusal.json`) instead of an arbitrary byte
  pattern. **Scope note:** `wit-002` describes a WITNESS's obligation (a
  witness refuses to cosign a root-rewrite BEFORE signing and persists
  evidence of the refusal). This repo is a REGISTRY, not a witness — it
  never cosigns anything and structurally cannot exhibit that half of
  wit-002's behavior. This change covers only the mirror-image defense
  the registry DOES own: refusing to STORE/AGGREGATE a cosignature that
  doesn't match its own recomputed root, pinned to wit-002's forged root
  value. It is **not** a claim of wit-002 coverage.
- **BREAKING: `registry.profiles` allowlist** (`REG-5`):
  `main::validate_config` now refuses to boot if `registry.profiles`
  contains anything other than the seven *registry* profiles the pinned
  ACDP spec defines (`REGISTRY_ADVERTISABLE_PROFILES`, new in
  `acdp-registry-types::config`: `acdp-registry-core`,
  `acdp-registry-discovery`, `acdp-registry-federated`,
  `acdp-registry-receipts`, `acdp-registry-head-receipts`,
  `acdp-registry-transparency-log`, `acdp-registry-lifecycle`) — derived
  by rule (every `profiles[].id` in the spec's `registries/profiles.json`
  prefixed `acdp-registry-`), not hand-maintained, and checked by a new
  conformance test against the pinned spec so a future spec change turns
  CI red rather than drifting silently. The allowlist check runs BEFORE
  the existing per-profile backing-config guards (receipts key,
  head-receipts, lifecycle, transparency-log), so a typo is reported
  before an unrelated missing-config complaint. `acdp-log-witness`
  specifically is rejected with a dedicated message: a witness is not a
  registry (RFC-ACDP-0015 §6.1) — a registry MAY aggregate cosignatures
  under `acdp-registry-transparency-log` without ever advertising
  `acdp-log-witness` itself. This is a breaking change for any deployment
  that had an unknown or `acdp-log-witness` string in `registry.profiles`
  — this repo's own shipped examples and tests (`config/
  registry.example.toml`, `docker/config.docker.toml`, and every
  in-repo test config) were audited and only ever set allowlisted
  values, so none of them are affected.
- **BREAKING** (`SEC-07`): `auth.anonymous_public_reads` now defaults
  to `false`, matching `CLAUDE.md`. Operators upgrading who rely on
  world-readable public contexts MUST set the field explicitly:
  `[auth] anonymous_public_reads = true`.
- **Pagination, search** (`BUG-01`, `BUG-02`, `BUG-03`): cursor-based
  pagination is now driven by the SQL `LIMIT limit+1` sentinel — no
  more phantom next pages when the in-Rust visibility filter drops
  rows. Postgres `list_contexts` binds `LIMIT` via `$N` instead of
  string concatenation; search applies the cursor predicate and limit
  in SQL on both backends.
- **Health endpoint** (`BUG-05`): `GET /healthz` returns HTTP 503
  (with `status: "degraded"`) when storage health fails, so load
  balancers and Kubernetes readiness probes take the pod out of
  rotation. The body shape is unchanged.
- **DB-backed challenge store** (`BUG-06`, `DESIGN-02`): SQLite and
  Postgres binaries now wire `SqliteChallengeStore` /
  `PgChallengeStore` instead of the in-memory store. Multi-replica
  Postgres deployments no longer break the handshake when an agent
  hits a different replica for the token step.
- **Challenge duplicate-nonce error mapping** (`BUG-04`): SQLite and
  Postgres `ChallengeStore::put` map unique-constraint violations to
  `AuthError::ChallengeReplay`, matching `InMemoryChallengeStore`.
- **Cross-registry resolution failures** map to HTTP 502 (bad
  gateway), matching `KeyResolutionUnreachable`.
- **`total_estimate` in search responses** (`DESIGN-05`) is now `None`
  rather than the page-local match count (which was misleading; it was
  always ≤ `limit`).
- **`ContextType` storage** (`DESIGN-04`): typed accessor replaces the
  `serde_json::to_value(...).as_str()` round-trip in both backends and
  the publish event, so future multi-field variants don't silently
  serialize to the empty string.
- **Webhook emitter constructor** is now `WebhookEmitter::try_spawn`
  (URL + secret validation); `spawn` is kept for tests but no longer
  invoked by the server binary.
- **Default tracing format** stays JSON; opt into pretty via
  `ACDP_LOG_FORMAT=pretty`.
- **Docker Compose**: secrets sourced from `${VAR:-default}` env
  substitution; `auth.jwt_secret = "changeme"` aborts startup.
- **`axum` bumped 0.7 → 0.8, `tower` 0.4 → 0.5, `tower-http` 0.5 → 0.6**
  (`REG-6`): completes the HTTP-stack line-up the prior `axum-server`
  bump left half-done, collapsing the duplicate `tower` (0.4.13/0.5.3)
  and `tower-http` (0.5.2/0.6.11) trees the lockfile carried from the
  mismatched pairing — `Cargo.lock` now resolves a single `tower` 0.5.x
  and `tower-http` 0.6.x. All nine route path params in
  `acdp-registry-core::build_router` move from axum 0.7's `:ctx_id` /
  `:lineage_id` syntax to 0.8's `{ctx_id}` / `{lineage_id}` syntax (no
  behavior change — `matchit`'s static-over-dynamic route priority,
  e.g. `/contexts/search` over `/contexts/{ctx_id}`, is preserved).
  `TimeoutLayer::new` is deprecated in tower-http 0.6; the 30s response
  timeout now uses
  `TimeoutLayer::with_status_code(StatusCode::REQUEST_TIMEOUT, Duration::from_secs(30))`,
  same behavior. **Operator-visible change:** the Prometheus `route=`
  label on every parameterized endpoint (e.g. `GET /contexts/{ctx_id}`)
  changes from the old `/contexts/:ctx_id` form to the new
  `/contexts/{ctx_id}` form, since the label is sourced from axum's
  `MatchedPath`. Dashboards or alerts keyed on the old label form will
  go silently blank until updated to match. A new assertion in
  `tests/metrics_integration.rs` pins the new label form on a
  parameterized request so this doesn't regress unnoticed again.
- **SHA-pinned `ci.yml`'s third-party actions, and corrected an unreachable pin**
  (`REG-10`, #111): the fifteen `dtolnay/rust-toolchain`, `Swatinem/rust-cache`,
  `taiki-e/install-action`, and `EmbarkStudios/cargo-deny-action` refs across `ci.yml`'s
  eight jobs now resolve at an immutable 40-hex commit SHA with a trailing
  `# <version-or-branch>` comment, extending `REG-8`'s pinning posture from
  `docker.yml`/`release-plz.yml` to the last unpinned workflow. First-party `actions/*`
  refs stay tag-pinned, unchanged.
  All seven `dtolnay/rust-toolchain` call sites now pin
  `6c977a6ca4077a0ceb28ffbe03f59d46e9ac8772 # master` and pass an explicit `toolchain:`
  (`stable` at six sites, `"1.88"` in `msrv`). dtolnay requires a pinned SHA to sit within
  `master`'s history — anything else is eventually garbage-collected — and the
  previously-used `4be7066…`, inherited from `release-plz.yml`, is today reachable from no
  ref at all. That pin is corrected in `release-plz.yml` as part of this change rather than
  copied into six more jobs.
  `Swatinem/rust-cache@6323deb… # v2.9.2` (six sites) and
  `EmbarkStudios/cargo-deny-action@3c63498… # v2.1.1` are ordinary tag-to-SHA pins.
  `taiki-e/install-action@1ed6d7be… # v2.87.2` pins a release SHA on `main` and passes
  `tool: cargo-llvm-cov` explicitly. The `@cargo-llvm-cov` tool tag defaults that input, but
  upstream strongly discourages pinning tool tags by hash: those commits are regenerated per
  release and are never in `main`'s history, so a hash pin starts referencing a commit that
  is not present on the repository.
  Because `# master` is a ref selector rather than a semver tag, Dependabot's
  `github-actions` ecosystem (`.github/dependabot.yml`, monthly) has nothing to track for the
  `dtolnay/rust-toolchain` pins; the `# v2.9.2`, `# v2.1.1`, and `# v2.87.2` pins will be
  kept current.

<!-- W2-U2 / #187 -->

- **Cursor parse failures no longer name the parse step** (`W2-U2`, `#187`): the
  keyset-pagination cursor codec is lifted out of both store backends into
  `acdp_registry_store::cursor` and every `InvalidCursor` arm now carries one
  payload, so the wire message is exactly
  `{"error":{"code":"invalid_cursor","message":"invalid cursor: malformed"}}`.
  It previously appended the failing parse step —
  which field was missing, which failed to parse as an integer — describing the
  cursor's internal layout. `cur-002`'s fixture rationale asks a registry not to
  "leak why a cursor failed to parse beyond the registered code"; that is now
  satisfied rather than documented as unsatisfied.

  **This is an observable change to the `message` string.** The *Rust* API is
  untouched: `error.code` still discriminates `invalid_cursor` from
  `cursor_expired` (both HTTP 400), which is the distinction clients branch on,
  and `message` was never a stable contract. But the *wire* string did change,
  and a client string-matching the old per-arm wording will stop matching — it
  should read `error.code`. Those two axes are why the commit carries a
  conventional-commits `!` marker while this entry says the API is unbroken:
  the marker exists to force the version bump that the wire change warrants,
  not to claim a Rust-level signature changed. Nothing in this repo asserted
  the old message text except the conformance tripwire retired below.

  Behind the wire change, `encode_cursor`/`decode_cursor` were byte-identical
  duplicates in `acdp-registry-sqlite` and `acdp-registry-pg`; there is now one
  copy and both stores call it. The count in `#187` was low: the tree held **8
  arms per store, 16 literals**, not 7/14 — neither the issue nor
  `DECISIONS.md` entry 10 counted `"cursor is not utf-8"`. One of the eight was
  unreachable (`splitn` always yields a first element) and is deleted rather
  than collapsed, so the accounting is 16 literals to 1 constant plus one dead
  branch removed.

  The conformance tripwire that existed solely to detect this change is retired
  with its explanatory note and its `ASSUMPTIONS.md` entry, and replaced by the
  inverse assertion — the message must equal `invalid cursor: malformed`
  exactly. A structural self-inspection test in the store crate additionally
  requires every `AcdpError::InvalidCursor` construction to use the shared
  constant, so a future arm cannot reintroduce a leak past a by-example test.

<!-- W3-U4 / #195 -->

- **`base64` removed from the `acdp-registry-sqlite` and `acdp-registry-pg`
  manifests** (`W3-U4`, `#195`): both declared it and neither used it. The
  dependency became unused when `#187` lifted the cursor codec — the only
  `base64` caller in either crate — into `acdp-registry-store`, which declares
  it correctly.

  **No resolved dependency graph changes at all — not the workspace's, and not
  a downstream consumer's.** The `base64` package is not dropped: ten other
  crates still use it, `acdp-registry-store` among them, and `Cargo.lock` loses
  exactly the two dependency edges. It also stays reachable *from these two
  crates* through mandatory, non-optional paths — `acdp-registry-sqlite` and
  `acdp-registry-pg` each depend unconditionally on `acdp-registry-store` and on
  `acdp`, both of which pull `base64`. `cargo tree -p acdp-registry-sqlite`
  still reports the same 13 `base64` entries it did before.

  So the gain is **manifest hygiene, not dependency reduction**: the two
  manifests no longer declare something their code does not use, they stop
  misrepresenting what the crate needs, and they stay clean under a future
  `cargo-udeps` or `cargo-machete` gate — of which this repo currently has
  none, which is why nothing flagged it.

  Safe under every feature combination, and provably so rather than by
  sampling: neither crate defines a `[features]` block or a single optional
  dependency, so `cargo metadata` reports `features={}` for both and there is no
  gated path that could reference the crate. `grep` across each crate's entire
  directory — `src/`, `tests/`, `migrations/` — returns zero occurrences.

<!-- W3-U6 / #196 -->

- **Cursor wire format documented with the wrong separator** (`W3-U6`, `#196`):
  `crates/acdp-registry-server/tests/http_integration.rs` described cursors as
  `base64("mint_ms|anchor_ms|ctx_id")`. The separator is a **colon**, and that
  is load-bearing rather than incidental — `ctx_id` values are URIs containing
  colons (`acdp://reg/ctx-1`), which is exactly why the decoder splits with a
  limit of 3 and lets the final field keep its own. A reader who trusted the
  `|` and wrote a greedy split would corrupt every `ctx_id`.

  The comment now also names `acdp_registry_store::cursor` as the single
  authority for the format. That is the half that matters: this defect existed
  because a wire format was described in a place other than where it is
  defined, which is the same failure `#187` removed by collapsing two
  byte-identical copies of the codec into one crate.

- **Nine line-pins repaired in `docs/AUTHENTICATION.md`, plus a prose
  correction in `docs/OPERATIONS.md`** (`W3-U6`). The breakdown matters more
  than the total, because only the first group is drift:
  - **six drifted** when `#201` inserted into `handlers/admin.rs`;
  - **three were misaligned independently of that merge** — one pinned
    `caller_from_headers` starting inside its doc comment, and two (the same
    pin cited twice) stopped one line before the end of the statement they
    describe.

  `docs/OPERATIONS.md` repaired **prose, not a pin**: the line it cites was
  correct and unchanged.

  A tenth pin, in `crates/acdp-registry-store/src/lib.rs`, carried the **same
  claim** as `docs/OPERATIONS.md` — that `admin_list` passes
  `anonymous_public_reads = true` unconditionally — but cited the *requester*
  line rather than the one that sets it. Two places asserted one fact and
  named different lines; only one could be right. Repaired together with its
  sibling, because a half-fixed truth-claim is worse than an unfixed one: the
  corrected doc would otherwise lend the stale one credibility by contrast.

  The `admin.rs` drift resolved at **three** distinct offsets (+27, +34, +42),
  not the two that were expected, so no uniform shift could have landed all of
  them — every pin was re-derived against the merged tree by locating its
  construct, rather than by applying an offset. Two of the six were
  additionally short by one line *before* the drift, ending just before the
  closing brace of the test they cite; their end lines were therefore wrong by
  four rather than three, the drift and the pre-existing error compounding.

  `OPERATIONS.md` said `admin_list` "passes `anonymous_public_reads = true`"
  and cited a line where the local is spelled `admin_sees_public_arm`. Both
  names are real — the local binds positionally to the store parameter — so the
  text now names both and cites each.

### Fixed

<!-- W3-U1 #192 #193 (lane-1) -->

- **Pinned-key entries are validated for usability, not just presence, and the
  reload endpoint can no longer bypass config validation** (`W3-U1`, `#193`,
  `#192`). `#185` made startup refuse `playground.enabled` +
  `pinned_only = true` + an *empty* `pinned_keys` list. These are the two
  remaining routes to the state that guard exists to refuse:
  - **Under it** (`#193`): the guard checked the list was non-empty, never that
    an entry was *usable*. An entry with a typo'd `algorithm`, key material that
    is not base64, an `ed25519` key that is not 32 bytes, an `ecdsa-p256` key
    that is not 65 bytes or does not begin `0x04`, or `valid_from >=
    valid_until`, booted clean and then failed at request time — as a redacted
    `internal_error` (`crates/acdp-registry-types/src/error.rs:125`), so the
    operator saw a bare 500 with nothing pointing at the config. All five are
    now startup refusals naming the entry by index and DID.
  - **Around it** (`#192`): `reload_pinned_keys` ran
    `RegistryConfig::load(None)` and then `*guard = fresh.playground` with
    nothing in between, so *every* config guard — including `#185`'s — was
    bypassable at runtime on a registry that booted clean. The handler now
    validates before taking the write lock and returns **`400`** with the live
    config untouched. `400`, not `500`, and deliberately distinct from the
    existing `ConfigReload` `500`: that one means the registry could not read
    its own config (a server fault), this one means the operator wrote
    something invalid. Conflating them makes a config typo page someone as an
    outage.

  Both doors now call one function, `validate_playground_config` in
  `crates/acdp-registry-core/src/playground.rs` — placed there because
  `validate_config` lives in the server *binary*, which
  `acdp-registry-core` cannot call into, and because the algorithm list it
  checks against is `PinnedAlgorithm::parse`, private to that module.
  `acdp-registry-types` is unchanged, so no new public API surface is
  committed ahead of the `release-plz` `publish` flip.

  **Not a refusal:** a pinned-key list where no entry is *currently* within its
  validity window warns loudly instead of refusing. Expiry is time-dependent,
  so refusing would make bootability a function of the wall clock, and under
  `pinned_only = false` the state is behaviourally identical to having no pins
  at all, which is supported. The warning branches on `pinned_only`, because the
  two modes do opposite things — strict rejects every `did:web` publish, lax
  **accepts them with no signature check**.

  **Upgrade note:** a config that boots today can be refused after this — any
  entry matching one of the five is now fatal, however harmless it looked
  before. Three shapes cover essentially all of it: a **rotated-out entry left
  in place with broken key material** (expired *and* malformed — wholly inert
  before, since `pinned_for_at` filters on the validity window and nothing ever
  decoded it); **any** broken entry while `playground.enabled = false`, because
  these rules do not consult `enabled`; and a **currently-live** entry with a
  typo'd `algorithm` or bad key material, which was already failing but only
  for that one agent's publishes and only as an opaque 500, which is how it
  goes unnoticed. Delete rotated-out entries rather than leaving them broken.
  Runtime pin-evaluation semantics are unchanged: this refuses bad config at
  the two doors, it does not change what a good config means. Docs:
  `docs/CONFIGURATION.md` (startup validation) and `docs/HTTP-API.md`
  (the reload endpoint's three responses).

- **The playground publish branch now honors `supports_idempotency_key`**
  (`REG-11` Phase 5, `#128`): `crates/acdp-registry-core/src/handlers/
  context.rs`'s playground unpinned publish branch — the manual
  idempotency lookup/record dance it runs around `publish_unverified_for_tests`
  — reaches the SDK's `RegistryServer::commit_via_store` just like the other
  three publish paths (verified did:web, did:key, pinned-verified), but only
  via an unconditional `commit_via_store(req, None, None, None)` tail call
  (`server.rs:557`) that hardcodes the idempotency key to `None`, so
  `commit_via_store`'s own `supports_idempotency_key` gate (`server.rs:666`)
  is a no-op for this path and the branch cannot delegate this decision to
  it — previously honored ANY `Idempotency-Key` header unconditionally, with
  no check of
  `state.server.capabilities().supports_idempotency_key` anywhere in that
  branch. Fixed with one shared `idem_key` binding, computed once before the
  lookup and reused at both the lookup and record call sites, rather than two
  independent `&&` conditions: gating only the lookup would still write a
  record that a later `supports_idempotency_key = true` flip would start
  replaying — resurrecting replays from records that should never have
  existed — and gating only the record would still replay today. A single
  binding makes that divergence unrepresentable. Also corrected the
  handler's doc comment, which claimed `Idempotency-Key` "is honored when
  the registry advertises support" — true of the other three branches, false
  of this one before the fix — to instead name the actual mechanism and why
  this branch cannot delegate this decision.
  Two new direct tests in `crates/acdp-registry-server/tests/conformance.rs`,
  placed immediately after `idem005_no_support_ignores_idempotency_key_header`:
  `idem_playground_branch_honors_supports_idempotency_key_gate` (two
  publishes of the same body with the same key now get different `ctx_id`s
  when the capability is `false`) and
  `idem_playground_branch_writes_no_idempotency_record_when_gated_off`
  (asserts `GET /admin/status`'s `idempotency.records == 0`, which a
  lookup-only fix would not catch). Both use a **did:web** producer, not
  did:key — `context.rs`'s did:key branch is checked, and returns, before
  the playground branch is ever reached, so a did:key producer would
  silently re-exercise the already-gated SDK path and prove nothing about
  the branch under test. `idem005_no_support_ignores_idempotency_key_header`'s
  own doc comment is amended (not its assertions) to reflect that the gap it
  recorded is now fixed and covered from both sides.
  `#128`'s second bullet (`docs/CONFIGURATION.md:242`) was already
  discharged by PR #132 (`REG-11` context finding 5); no further doc change
  was needed for it here.
  No test result is asserted as measured in this entry: `cargo test` cannot
  run in this environment (EACCES writing `.d` files even with
  `CARGO_TARGET_DIR` on a fresh scratch dir), so verification is via CI on
  the PR.

- **Documentation sweep for the `#133` fix** (`REG-11` Phase 4): removed every
  doc statement and source comment describing `GET /admin/contexts` as
  ungated or as "the one exception" among `/admin/*` routes — `docs/HTTP-API.md`
  (the "Most `/admin/*` routes" paragraph, the "one exception" paragraph,
  and the route's own "Not admin-bearer gated" paragraph — the route-table
  footnote had already been removed in an earlier phase), `docs/OPERATIONS.md`
  (two "is an exception" / "is not" gated statements),
  `docs/CONFIGURATION.md` (the `admin_tokens` row's "does not
  disable `GET /admin/contexts`" clause), `crates/acdp-registry-core/src/lib.rs`'s
  route comment ("but NOT every `/admin/*` route"), and
  `crates/acdp-registry-types/src/config.rs`'s `admin_tokens` doc (which named
  only `POST /admin/pinned-keys/reload`, stale even before this plan — the
  field already gated five routes before Phase 3, and now gates all six).
  Added the disclosure rule to `ExtendedRegistryStore::list_contexts`'s trait
  doc (`crates/acdp-registry-store/src/lib.rs`), the one file this correction
  belongs in that no phase's file list had named.
  **This explicitly supersedes the "Documentation-accuracy pass (`REG-10`
  docs follow-up)" entry under the Documentation section below** (the one
  claiming `HTTP-API.md`'s "requires the admin bearer... it does not" and
  describing the gap as "a pre-existing gap... not a behavior change") —
  that entry was correct when written (PR
  #132, before Phases 2-3 shipped the fix) and is now the opposite of current
  behavior. Read it as history, not as the current contract.
  This is the sentence [SECURITY.md](SECURITY.md)'s "Keep
  `auth.anonymous_public_reads = false` unless the registry is meant to serve
  world-readable public contexts" was really about: it needed no edit here,
  but it only became fully true with this fix — before Phase 3,
  `GET /admin/contexts` disclosed public contexts to anonymous callers
  regardless of that setting.
  Docs-only; no behavior change. Not compiled locally — `rustdoc` (`docs`
  CI job) is the verification signal for this phase.

<!-- U-002 #179 (Lane 2) -->

- **Webhook deliveries no longer emit a duplicate `event_id` JSON key**
  (`U-002`, `#179`): `WireEnvelope`
  (`crates/acdp-registry-webhook/src/lib.rs`) serialises a top-level
  `event_id` and then `#[serde(flatten)]`s the event.
  `WebhookEvent::ContextRetracted` and `::ContextRepublished`
  (`crates/acdp-registry-types/src/event.rs`) each carry their own
  actor-minted `event_id`, so those two deliveries — and only those two —
  put the key on the wire **twice**. The envelope serialises first and the
  flattened variant second, so every last-wins parser (`serde_json`,
  JavaScript `JSON.parse`, Python `json`) resolved `event_id` to the
  *lifecycle* UUID and silently shadowed the envelope's per-delivery dedupe
  id; first-wins implementations saw the opposite. One of the two meanings
  was always lost, and which one depended on the receiver's parser.
  - **Wire change:** on `context.retracted` and `context.republished` the
    actor-minted lifecycle id now serialises as **`lifecycle_event_id`**.
    The envelope's `event_id` keeps its name and its meaning — the
    per-delivery dedupe id, still echoed in `X-ACDP-Event-Id`. The other
    three event types are byte-identical to before.
  - **For receivers:** anything deduping on the `X-ACDP-Event-Id` header is
    unaffected. A receiver that read the body's `event_id` on these two
    types and relied on last-wins was reading the lifecycle id and now reads
    the delivery id — read `lifecycle_event_id` to keep the old value.
  - Implemented with `#[serde(rename)]` rather than a Rust field rename:
    every construction site lives in `crates/acdp-registry-core`, and the
    emitted JSON is byte-identical either way.
  - **`schema_version` deliberately stays `"1.0"`.** It describes the
    envelope, which did not change, and it is stamped per *delivery* — so a
    `search.executed` body carrying a bumped version would assert that
    something about that delivery changed when nothing did. That holds however
    many variants a future change touches. The constant's doc comment, which
    previously promised a bump on "any" backwards-incompatible shape change,
    has been narrowed to match what it actually tracks.
  - Enforced by a test that drives all five variants through the real
    emitter and asserts on the serialized body. Duplicate detection walks
    the raw JSON with a `MapAccess` visitor rather than parsing to
    `serde_json::Value`, whose map silently keeps only the last of a
    repeated key and therefore cannot observe this defect at all.
    `context.retracted` and `context.republished` are also documented in
    [WEBHOOKS.md](docs/WEBHOOKS.md) for the first time — the two event types
    carrying the bug were the two that had never been written down.

<!-- end U-002 #179 -->

<!-- W2-U3 release & CI plumbing -->

- **Releases work again, and the container pipeline can finally publish a version
  tag** (`W2-U3`): `release-plz` had produced no release since 2026-06-13 despite
  83 squash-merged pull requests on `main` since then — 23 of them `feat`/`fix`,
  the prefixes that should bump a version — reporting success in under a minute
  each time. It resolves "what was last released" from crates.io even with
  `publish = false`; nothing here is published, so all eight crates read as
  never-released, it proposed the current `0.1.0`, and the release step then
  refused because that tag already existed. `git_only = true` moves the baseline
  onto git tags, and `git_tag_name` changes to `{{ package }}/v{{ version }}` so
  the un-packageable June tags stop matching without being deleted.

  **Reading old tags:** the eight `acdp-registry-<crate>-v0.1.0` tags and their
  GitHub Releases are untouched and still valid; new ones use a `/` instead of the
  final `-`. The eight slash-namespace `v0.1.0` Releases were backfilled by hand
  on 2026-09-11 (#210): the bootstrap run that minted those tags deliberately
  suppressed Releases (#204), and release-plz only acts on version bumps, so it
  would never have created them. Until the backfill, "Latest release" pointed at
  June's `9bd4fb3` while the GHCR image `:0.1.0` was built from `6ae49bf` — 124
  commits apart. `acdp-registry-server/v0.1.0` is now Latest, and the two agree. **The first release run after this change re-bases the tag namespace
  and produces no release PR — that is expected; the run after it produces one.**

  `docker.yml` triggered on `tags: ["v*"]`, which has never matched any tag this
  repo creates, so that trigger had never fired and its `type=semver` rule was
  dead — the image has only ever existed as `:latest`, `:main` and `:sha-<sha>`.
  It now
  triggers on `acdp-registry-server/v*` (one tag per release, not eight) and
  extracts the version with an explicit `match=`, guarded by a step that fails the
  job if a tag push ever stops yielding a semver version. `release-plz` also now
  authenticates as a GitHub App, because tags pushed with the default
  `GITHUB_TOKEN` do not start workflow runs — without that, the corrected trigger
  would still never fire. That App token is scoped to `contents` and
  `pull-requests` only, and `CARGO_REGISTRY_TOKEN` is no longer passed to the
  release job at all: nothing in `git_only` mode publishes to crates.io, so
  withholding it makes an accidental publish impossible rather than merely
  unintended.

  [`docker/RAILWAY.md`](docker/RAILWAY.md) advertised
  `ghcr.io/…/acdp-registry:0.1.0` built on "every `v*` tag"; that image has never
  existed, and the image is `linux/amd64`, not multi-arch. Both corrected.
  `PROGRESS.md` and `.drive.lock` are now git-ignored, and the duplicate `## 6.`
  heading in `DECISIONS.md` is disambiguated without renumbering entries 7-10.

<!-- end W2-U3 -->

<!-- W3-U5 (lane-1) -->

- **The `docker compose up` quickstart boots** (`W3-U5`). It did not. The
  stack shipped `ACDP_REGISTRY_JWT_SECRET:-changeme`, and on HS256 every
  non-empty `jwt_secret` is decoded and length-checked on every boot —
  `changeme` is valid base64 of six bytes, so the container exited `rc=1`
  before serving a request. `docker/docker-compose.yml` now ships **no**
  secret (`${ACDP_REGISTRY_JWT_SECRET:-}`); an empty secret with
  `auth.enabled = false`, which `docker/config.docker.toml` sets, is a
  supported configuration, so the demo needs no secret material committed to
  the repo. Chosen over shipping a real 32-byte default, which would have
  worked at the cost of a fixed secret in a tracked file that gets copied into
  non-disposable use.

  Observed against the real image and a real Postgres, not reasoned from the
  code: no secret set → boots, `GET /healthz` `200`,
  `GET /.well-known/jwks.json` `200 {"keys":[]}`, `POST /auth/challenge`
  `404`. With `ACDP_REGISTRY_JWT_SECRET=changeme` → exit `1`, and the error is
  the container's entire output.

- **`jwt_secret` is validated regardless of `auth.enabled`, before migrations
  run** (`W3-U5`). `validate_config` only checked a non-empty `jwt_secret`
  with auth ON, so an auth-off registry with a bad secret still failed — just
  late, from `serve_with_store`, after `store.migrate()` had connected and run
  DDL. That contradicted what `validate_config` documents about itself
  ("before running migrations or binding the socket"). The check is hoisted
  out of the gate.

  **No boot outcome changes.** Every casing of `changeme` is valid base64 of
  six bytes, already below the floor the serve path always applied, so
  ungating the literal guard swaps a generic length error for an actionable
  hint. The seven-row boot matrix reproduces with identical `BOOTED`/`EXITED`
  and identical `rc`. One genuine difference, scoped rather than smoothed
  over: for a config with **two** fatal faults the first error reported can
  differ, because validation now precedes the backend checks and
  `PgStore::connect`.

  The EMPTY-secret check deliberately keeps its `auth.enabled` gate — an
  auth-off registry with no secret is supported — and that asymmetry is
  commented rather than left to read as an oversight. Not closed here, and
  stated rather than swept: the algorithm check and the EdDSA
  `jwt_private_key_pem` check remain gated on `auth.enabled`. With auth off an
  unrecognised algorithm boots and an empty secret boots, but an empty EdDSA
  PEM still refuses — from the serve path, *after* migrations. A bad
  `jwt_secret` is now refused before migrations; a missing EdDSA PEM is still
  refused after them.

- **Four documents that described the old gating now describe the binary**
  (`W3-U5`) — `docker/docker-compose.yml`, `config/registry.example.toml`,
  `SECURITY.md`, `docs/CONFIGURATION.md` — in one commit with the compose
  change, so no window exists where `SECURITY.md` warns about a hazard the
  repo has just removed. `SECURITY.md` had the error inverted: it warned that
  a placeholder "can survive there unnoticed" with auth off, when a
  placeholder in fact stops the stack from booting.
  `config/registry.example.toml`'s "Two checks, gated the same way: neither
  runs while auth is disabled" was wrong on both counts.

  Each claim is scoped to HS256 where HS256 is load-bearing. **Under EdDSA
  `jwt_secret` is never examined** — not for the literal, not for length, auth
  on or off — so a stale placeholder is silently ignored there rather than
  rejected. `SECURITY.md` gains that as its own bullet, because the bullet
  after it recommends EdDSA for federation and an unqualified "a placeholder
  cannot survive unnoticed" would have been false for exactly the
  configuration being recommended.

### Security

<!-- U-001 #174 (lane-1) -->

- **Both stores now enforce the RFC-ACDP-0014 §4 `supersedes` rule** (`#174`,
  supersedes PR `#175`): `acdp` 0.10.0 adds `predecessor_admission` to
  `PublishCommit`, and `PgStore::commit_publish` /
  `SqliteStore::commit_publish` invoke it on the predecessor's stored `Body`,
  propagating its `Err`. Before this, the rule was the last unenforced row of
  that table.
  - **This is a behavior change affecting every deployment, not an opt-in
    one.** The hook is live whenever the registry advertises `acdp_version >=
    0.3.0`, and `ANCHORS_VERSION_CLAIM` (`crates/acdp-registry-server/src/main.rs`)
    is unconditional — every configuration advertises `>= 0.5.0`, asserted for a
    bare config by `capabilities_acdp_version_ladder`. So **every** registry now
    rejects, with `schema_violation` (non-transient — do not retry), a publish
    that supersedes a key-revocation context with anything but a
    same-trust-class key-revocation. Publishes previously accepted will now be
    refused. That is the intended effect: the registry was advertising
    compliance it did not enforce.
  - **The ordering is itself the security property.** The call sits after tenant
    scoping, producer continuity, lineage/version coherence and
    `AlreadySuperseded`, and before every write. Running it earlier would turn
    publish into a cross-tenant, non-owner existence-and-`context_type` oracle on
    the predecessor, leaking past the uniform `superseded_target{NotFound}` both
    stores already return for absent / wrong-tenant / non-owner targets. Upstream
    mandates this position ("never earlier"); the contract floor would violate it.
  - Refusal leaves nothing behind — no successor row, no supersession of the
    predecessor, no transparency-log leaf, no idempotency record — because the
    call precedes every write and `Transaction::Drop` rolls back.
  - The predecessor `Body` is read by adding `body_json` to each store's existing
    predecessor `SELECT`: no extra round-trip, no new lock, and no change to pg's
    `FOR UPDATE` lock mode. It is deserialized lazily, only when the hook is
    present, so a gate-off registry pays nothing and cannot newly fail. A body
    that will not decode fails the publish rather than silently skipping the
    check.
  - **17 regression tests** (8 sqlite, 9 pg) exist for one reason: the field is a
    plain struct field, so a store that binds it and never calls it compiles
    cleanly, warns about nothing, and silently enforces nothing — and the
    conformance fixtures do not cover the reject path (spec issue `#57`). Each
    test was verified by mutation: the call was deleted, hoisted above each gate,
    defanged at the parse, made to swallow the one error variant the real closure
    emits, pointed at the lineage-head row instead of the predecessor, and keyed
    on the predecessor's type, the successor's type, the tenant, the receipt
    minter and the visibility. Each was observed failing its guard, and most left
    all-but-one test green — which is the point: several plausible wrong
    implementations pass almost the whole suite, so each needed its own test. The
    list is not a proof of exhaustiveness; three of these mutations were found by
    review after the suite was already written and passing.

<!-- REG-11 #168 (Lane B) -->

- **The `/metrics` bearer gate now compares in constant time** (`#168`). It used
  `presented != Some(token)` — `&str` equality, which is free to stop at the
  first differing byte and so leaks the matching-prefix length of a configured
  credential. `/admin/*` has compared with a constant-time `ct_eq` since `#23`,
  deliberately: a dedicated helper, a doc comment, a unit test pinning the
  property, and an allowlist fold with no early return so it does not leak
  *which* entry matched. Two gates comparing the same kind of secret disagreed.

  `ct_eq` was private to `handlers::admin`; it moves to a shared
  `acdp_registry_core::secure_compare` and both gates now call it. One
  implementation, not a copy each — a duplicated fold is one refactor away from
  drifting, which is the outcome the issue explicitly warned against.

  **Severity: hardening, not a live vulnerability.** Remote timing attacks on a
  `memcmp`-style comparison over HTTP are hard, and no shipped config exposes
  the gate. The argument is consistency with a decision this codebase already
  made. Two limits are documented rather than glossed: the length guard returns
  early, so token *length* remains observable at both gates; and no unit test
  here can demonstrate constant-timeness — the tests pin behaviour, and the
  timing property rests on a single reviewed implementation.

- **Fixed a coverage hole found while implementing the above**: nothing asserted
  that `/metrics` refuses a **wrong** bearer token. Replacing the comparison
  with a literal `true` left the entire workspace suite green (measured). Every
  refused case in the existing tests fails at the `"Bearer "` prefix, so the
  authorization decision itself — the reason the gate exists — was untested, and
  `/metrics` could have been made to accept any token with CI staying green.
  `metrics_wrong_bearer_token_is_refused` covers it, including the near-miss
  shapes (`Bearer scrape-secre`, `Bearer scrape-secreT`) that a timing oracle
  would target. The equivalent mutation on `/admin/*` turns four tests red, so
  the gap was specific to `/metrics`.

<!-- REG-11 #162 (Lane B) -->

- **A whitespace-only `metrics.bearer_token` is refused at startup** (`#162`):
  `metrics_endpoint` applies its bearer gate only when the *trimmed* token is
  non-empty, so `" "` or `"\t"` was read as "no gate configured" and `/metrics`
  was served unauthenticated to anyone who could reach the port — leaving no
  failed-auth signal in the logs, because no authentication was attempted. The
  realistic source is a value templated from an unset environment variable, and
  it fails **open**. `validate_config` now refuses a blank-but-present value
  while `metrics.enabled = true`.

  An **empty** value keeps its documented meaning — `/metrics` is open, the
  shipped default for a trusted scrape network — so only a value that was set
  and then blanked is refused. The guard is scoped to `metrics.enabled` because
  the route is not mounted otherwise; it still fails closed, since enabling
  metrics later trips the check before anything binds.

  Deliberately **narrower** than the `auth.admin_tokens` guard added in `#161`:
  padded-but-non-blank values are accepted here. That guard refuses padding
  because only one of its two sides trims, which makes a padded admin token
  authenticate over HTTP/2 and fail over HTTP/1.1. The `/metrics` path trims the
  configured value *and* the presented one, so `" tok "` and `"tok"` behave
  identically on both protocols and there is nothing protocol-dependent to
  refuse.

<!-- REG-11 #161 (Lane B) -->

- **Startup now refuses a blank or whitespace-padded entry in
  `auth.admin_tokens`** (`#161`).

  The failure this closes is not an operator blanking their admin allowlist —
  it is a deployment with **several working admin tokens where one templated
  from an unset variable**. The allowlist compare folds over every entry
  without early return (deliberately, for constant time), so
  `["tok-a", "tok-b", ""]` admits the empty token *alongside* the real ones.
  Every genuine token keeps working, the list is non-empty, and nothing looks
  anomalous — the deployment reads as correctly configured while
  `/admin/*` is open, including the live pinned-keys reload and the
  registry-attested retract/republish routes.

  The mechanism: `require_admin_bearer` strips `"Bearer "` and does not trim,
  so `Authorization: Bearer ` yields `""`, which matches an empty entry.
  **This is reachable over HTTP/2**, which preserves trailing whitespace in
  header values. Over HTTP/1.1 `httparse` strips trailing SP/HTAB/CR/LF before
  the value reaches the handler, so the same request arrives as `"Bearer"` and
  is refused. The registry serves both protocols.

  Padded entries such as `"tok "` are refused for the same reason from the
  other direction: HTTP/1.1 trims the request header but not the configured
  value, so such a token authenticates over HTTP/2 and 403s over HTTP/1.1.
  That fails closed rather than open, but it is the same templating accident
  and it is indistinguishable from a typo.

  **This is a hardening gap, not a live vulnerability.** It requires operator
  misconfiguration, and no shipped configuration is affected — both
  `config/registry.example.toml` and `docker/config.docker.toml` leave
  `admin_tokens` commented out, i.e. an empty *list*, which correctly means
  "admin routes disabled" and remains valid. What makes it worth closing is
  the direction of the failure: templating from an unset environment variable
  **fails open rather than closed**, which is the wrong direction for a
  security gate.

  The guard is in `validate_config`, beside the existing `auth.jwt_secret`
  checks — the same class of shared secret, already guarded three ways (empty,
  the `changeme` placeholder, and a decoded-length floor) while `admin_tokens`
  entries were not inspected at all. Failing at startup rather than per request
  keeps a misconfiguration from presenting as a client-side 403.

  The constant-time comparison itself (`ct_eq`, `#23`) is correct and
  unchanged; the gap was strictly upstream of it, in what reached the
  allowlist.

<!-- end REG-11 #161 -->

- **`chacha20` bumped 0.10.1 → 0.10.2** (`REG-11` Phase 2 ride-along;
  lockfile-only version bump, not a `deny.toml` ignore): `0.10.1` is
  yanked from crates.io. It sits behind `rand::rng()` on the
  auth-challenge-nonce path (`crates/acdp-registry-auth/src/service.rs`),
  so this closes the CSPRNG core to a yanked dependency without any code
  change; `cargo update -p chacha20` only re-resolves the lockfile.
- `SEC-01` through `SEC-07` — full sweep landed; see Added/Changed
  above for individual items. Notable: empty-string webhook secrets
  no longer silently produce valid HMACs (`SEC-04`), and the
  `RequestBodyLimitLayer` (`SEC-06`) protects every route from
  arbitrarily-large request bodies.
- **`h2` bumped 0.4.15 → 0.4.19** (lockfile-only version bump, not a
  `deny.toml` ignore): resolves `RUSTSEC-2026-0258`, in which `h2`
  queued empty `DATA` frames without limit, risking unbounded memory
  growth or a length-overflow panic. Reached transitively via
  `hyper` ← `axum` / `axum-server` / `hyper-rustls` / `reqwest`.
- **`[graph] all-features = true`** (`REG-7`) in `deny.toml`: the
  cargo-deny advisory/license/bans gate now resolves every
  feature-gated subgraph, including the `storage-pg` path the Docker
  image ships (`STORAGE_FEATURE=storage-pg` by default in
  `docker/Dockerfile`), instead of only the default-features graph.
  `cargo deny --workspace check` remains green with no new findings.
- **`axum-server` bumped 0.7 → 0.8** (`REG-6`): removes `rustls-pemfile`
  from the dependency graph entirely (0.8.0 replaced it with
  `rustls-pki-types`'s `PemObject` trait), so the `RUSTSEC-2025-0134`
  ignore entry is deleted from `deny.toml` rather than merely
  satisfied. `axum_server::Handle` is now generic over the bind
  address (`Handle<A: Address>`); `main.rs` annotates its one
  `Handle::<SocketAddr>::new()` construction site (shared by both the
  non-TLS and TLS-capable serve paths) and the `spawn_shutdown_watcher`
  signature accordingly. No router or crypto-provider changes —
  `tls-rustls` still resolves to `rustls/aws-lc-rs`.

### Documentation

<!-- run-close sweep (leader) #190 #191 -->

- **Five documented behaviours the code does not implement, plus four pins and
  two anchors** (`#190`, `#191`). All re-derived from the artefact before
  editing; two were worse than their issues recorded and two were in nobody's
  issue at all.

  - **`invalid_log_proof` is reachable from this registry's own handler.**
    Three sites said otherwise — `docs/HTTP-API.md`'s error table, the wire-code
    arm in `crates/acdp-registry-types/src/error.rs`, and the doc comment on
    `invalid_log_proof_is_502_with_registered_code`. `/log/proof` echoes the
    leaf for retrieval-authorized requesters via `record.leaf()`
    (`handlers/log.rs:359`), and a stored leaf that no longer parses under the
    closed schema raises it locally. Recorded but deliberately **not** changed:
    that path answers `502`, which blames an upstream for a local data fault —
    a wire change, not a docs fix.
  - **`lifecycle.enabled = false` does not stop emission.** `CONFIGURATION.md`
    stated the absolute; there is no `lifecycle.enabled` check in either store.
    Both attach `registry_state.lifecycle_events` and derive the `retracted`
    status from stored columns unconditionally, so a registry that enabled
    lifecycle, accumulated events, then disabled it keeps serving both while
    answering `501` on the endpoints.
  - **`docs/RECEIPTS.md` promised a `Cache-Control: private` posture that does
    not exist.** The only `Cache-Control` this registry emits is
    `public, max-age=300` on three `/.well-known/*` routes, none of them
    requester-relative. Rewritten as an operator obligation with the exposure
    scoped to deployments that actually front a shared cache. Whether the
    registry should emit `private`/`no-store` itself is `#205`, split out so a
    wire change is not shipped inside a docs correction.
  - **Ten stale spec-SHA citations, removed rather than re-pointed.**
    `conformance.rs` cited `417211f` ten times against a CI pin of `d1f06d0`.
    Every assertion was still true — only the coordinate was wrong, so nothing
    could turn red. Re-pointing would re-stale on the next bump; the counts now
    read "at the CI-pinned spec", which resolves to the single `ref:` in
    `ci.yml` that `bump-spec.yml` rewrites.
  - **Four pins and two anchors.** `OPERATIONS.md` pinned `store/src/lib.rs:74`
    for `anonymous_public_reads`; `:74` is `tenant`. `CONFIGURATION.md` pinned
    `context.rs:414` for the `did:key` branch (that line is a comment about the
    playground snapshot; the branch is `:421`) and misquoted its rationale
    comment's range. `AUTHENTICATION.md` stated one `/metrics` fact twice, each
    with its own pin at the same lines — one claim, two pins drifting in
    lockstep, reading to a checker as two independent confirmations. Two broken
    anchors in `HTTP-API.md` and `CONFIGURATION.md`; every same-file and
    cross-file anchor across `docs/` now resolves.

  Pins touched here name their construct next to the line, so a pin that rots
  degrades to *searchable* rather than to *wrong*.

<!-- U-005 #180 (lane-1) -->

- **Corrected two false claims replicated across eleven operator-facing sites**
  (`#180`). Both would have led an operator to configure the registry
  incorrectly, and they failed in opposite directions.

  **1. `[receipt]` and the playground were documented as flatly incompatible.
  They are not.** `main.rs:259` refuses only `playground.enabled &&
  !pinned_only` — the fully unverified sub-mode. Pinned-only playground
  alongside receipts is deliberately supported (rationale at
  `main.rs:249-258`; `receipt_with_pinned_only_playground_is_accepted`). The
  harm was **foreclosure**: an operator who wanted receipts was told to disable
  the playground outright when pinned-only would have served them.
  `docs/RECEIPTS.md` was doubly wrong — its rationale ("the playground path
  never resolves the producer key") is false in pinned mode, which is the point
  of pinned mode. Corrected at `docs/CONFIGURATION.md`, `docs/RECEIPTS.md`,
  `config/registry.example.toml`, each now also carrying the **second**
  precondition (`main.rs:267` refuses `pinned_only = true` with an empty
  `pinned_keys`) so the corrected docs cannot strand an operator at startup.
  `config/registry.example.toml` gains a commented, copyable `pinned_only` +
  `[[playground.pinned_keys]]` stanza; it previously contained no mention of
  pinning at all.

  **2. "`changeme` is always rejected" never held for either stack that ships
  it.** The only such check (`main.rs:92-104`) is nested inside `auth.enabled
  && jwt_signing_alg != "EdDSA" && !jwt_secret.is_empty()`, and
  `docker/config.docker.toml:24` sets `auth.enabled = false`. **`docker compose
  up` with the shipped `changeme` default boots cleanly** — the guard never
  fires in the one setup where a placeholder secret is likeliest to survive
  into production. **Operators auditing whether they were affected should note
  this: if you relied on that documented check with auth disabled, it never
  ran.** Corrected at `SECURITY.md`, `docker/docker-compose.yml` (×2),
  `docker/RAILWAY.md`, `config/registry.example.toml`,
  `docs/CONFIGURATION.md`, `docs/OPERATIONS.md`, `docs/AUTHENTICATION.md`. The
  check is also case-insensitive after trimming (`main.rs:102-103`), not a
  literal match, and does not apply under EdDSA — both now stated.

  **`docker/RAILWAY.md` was the worst instance.** Its required-env-var table
  never sets `ACDP_REGISTRY_AUTH__ENABLED`, `AuthConfig::default()` is
  `enabled: false`, and the GHCR image mounts no config file there — so the
  documented *production* recipe runs unauthenticated while its only mention of
  auth was a false guarantee. It also instructs `ALLOW_PUBLIC_BIND = true`,
  waiving the guard at `main.rs:465-478` whose own bail text names its
  precondition as a proxy that terminates TLS **and authenticates**; Railway's
  edge does only the first. A note under the table now states all of this.
  Scoped deliberately: publishes and lifecycle events remain bound to
  DID-signature verification, so this is **not** "anyone can publish anything."

  **Deliberately not changed:** `auth.enabled` anywhere (including the compose
  stack), the `${ACDP_REGISTRY_JWT_SECRET:-changeme}` default, and the addition
  of `ACDP_REGISTRY_AUTH__ENABLED = true` to `RAILWAY.md`'s required env vars.
  The last is a change to what a deployment recipe *instructs*, which is an
  operator-visible posture change rather than a docs fix; `#180` says posture
  must not change silently in a docs pass, so it is raised for a ruling
  instead. Documentation only — no logic, signature, or behaviour changed.

  Two further corrections found by the pre-merge verifier, both in text this
  change itself introduced: `pinned_only = true` with an empty `pinned_keys`
  does **not** "reject every publish" — `playground.rs:109-111` short-circuits
  to `PinOutcome::Skipped` before `pinned_only` is read, so publishes fall
  through to the *unverified* path. That rationale had been copied from the
  startup guard's own bail message (`main.rs:271-273`), which is itself wrong;
  the message is filed separately. And `playground.pinned_keys.algorithm`
  accepts `ecdsa-p256` as well as `ed25519` (`playground.rs:58-64`), which the
  reference table had listed as ed25519-only.

  *Line-pin sweep (CHARTER rule 10):* two live pins point into
  `docs/CONFIGURATION.md` — `DECISIONS.md:267` → `:112` and
  `CHANGELOG.md:2054` → `:242` — both below both edit points, both drifting
  `+5`. **Reported, not re-pointed:** re-pointing means editing existing lines
  in files this change treats as additive-only. The three `changeme` mentions
  in CHANGELOG history (`:1274`, `:1966`, `:2287`) are left stale by the same
  historical-record principle. No test or workflow asserts on any changed
  string (verified across `crates/` and `.github/`).

<!-- REG-11 #164 (Lane C) -->

- **Corrected a class of stale `401` doc comments in the ACDP request path**
  (`#164`): six comments across
  `crates/acdp-registry-core/src/handlers/{auth.rs,context.rs}` described
  auth failures as `401`. They return `RegistryError::AuthToken`/`Jwt`/
  `AuthChallenge`, which `http_status` maps to **403** (`not_authorized`),
  pinned by `auth_errors_are_403`. Comments only — no logic, signature, or
  behaviour changed.

  The most load-bearing was `caller_from_headers`' *"Returns `Err(401)`"*,
  which had already been cited as a source in a `#152` review and put a false
  `401` claim into `docs/HTTP-API.md`'s neighbourhood before being caught —
  which is what makes this a defect worth fixing rather than a typo: a stale
  doc comment is more credible than prose in an issue, because it sits
  directly above the function.

  Fixed as a class rather than the two reported instances. The sweep is
  bounded by a fact that makes it checkable: `StatusCode::UNAUTHORIZED` has
  exactly **one** non-test occurrence in `crates/` (`metrics.rs:130`), so
  `/metrics` is the only endpoint in this registry that answers `401`, and any
  comment claiming `401` on an ACDP path is wrong by construction. Comments
  that correctly describe `/metrics`, and those in `error.rs` explaining why
  there is no 401-bearing code, were deliberately left alone.

<!-- end REG-11 #164 -->

<!-- REG-11 #166 (Lane B) -->

- **`docs/AUTHENTICATION.md`: corrected two false claims** (`#166`) that reached
  `main` in `c8f9a40`.

  It stated that **two** bearer parsers coexist, as a complete enumeration.
  There are **three**: the third is inline in the `/metrics` gate
  (`crates/acdp-registry-core/src/metrics.rs:124-128`) and is a hybrid of the
  other two — case-sensitive on the scheme like `require_admin_bearer`, trimming
  like `extract_bearer`. The count was wrong in two places, not one; the second
  instance was in the per-parser acceptance table, which now carries a
  `/metrics` column.

  It also stated that auth failures "on this registry" are `403`, "never" `401`,
  with no `WWW-Authenticate` challenge. That is true of the ACDP routes and of
  `/admin/*`, and false of `/metrics`, which answers `401` **and** sends
  `WWW-Authenticate: Bearer realm="metrics"` (`metrics.rs:130-134`). The sentence
  is now scoped, and `/metrics` is named as the exception to both halves.

  Both claims were introduced *by the fix for an earlier false claim* — a
  correctly-scoped statement about the ACDP auth path was restated as a claim
  about the whole registry. A new `/metrics` section documents that endpoint's
  gate directly, including the fact that it sits outside the ACDP auth pipeline
  entirely (`crates/acdp-registry-core/src/lib.rs:153-155`), so no bearer it
  receives is ever validated as an ACDP token.

  **The parser differences are now pinned, not merely described.** The first
  draft of this section claimed all three parsers were "locked by tests" — a
  third false claim of the same class it was correcting, since only the admin
  parser's differences were covered. Measured on the parent commit: deleting
  `.map(str::trim)` from `metrics.rs`, deleting `extract_bearer`'s `"bearer "`
  arm, and deleting `extract_bearer`'s own `.map(str::trim)` each left the
  entire workspace suite green. Two tests close that gap —
  `extract_bearer_accepts_two_casings_and_trims` and
  `metrics_bearer_parser_shape_is_pinned` — and each of the three mutations now
  turns exactly one of them red.

  This also protects `#162`'s rationale: that guard is narrower than `#161`'s
  *because* the `/metrics` path trims both the configured and the presented
  token, a claim asserted in four places. Had that trim been refactored away,
  CI would have stayed green while every padded-token deployment began `401`-ing
  its Prometheus scrapes.

  Also corrected: `docs/HTTP-API.md`'s error-envelope note, which stated
  registry-wide that no `WWW-Authenticate` challenge is emitted — the very
  sentence `AUTHENTICATION.md` links to as its authority.

<!-- REG-11 #152 (Lane B) -->

- **Bearer-parsing behaviour is now documented** (`#152`), in a new
  "Presenting a bearer" section of `docs/AUTHENTICATION.md`, with
  cross-references from `README.md` and `SECURITY.md`.

  Two facts were true but written down nowhere. First, an `Authorization`
  header the registry does not recognise as a bearer — a non-`Bearer` scheme, a
  misspelled header, a non-UTF-8 value — is treated as **anonymous** on the
  ordinary read/publish routes rather than refused: a typo'd scheme never reaches
  token validation at all. Only a well-formed bearer that then fails validation
  is rejected, and that rejection is `403 not_authorized` — this registry emits
  no `401` on the auth path. What the caller sees after the anonymous
  classification depends on the route and on `auth.anonymous_public_reads`
  (default `false`), so it may be a refusal or a filtered result set.
  `/admin/*` refuses all the same inputs with `403`.

  Second, the parsers disagree. (This entry said "the two parsers"; there are
  three — the `/metrics` gate carries a third, corrected under `#166`.)
  `extract_bearer` accepts `Bearer ` or
  `bearer ` and trims the token; `require_admin_bearer` accepts `Bearer ` only
  and does not trim. So `bearer <jwt>` authenticates on `/contexts/*` and 403s
  on `/admin/*`. Both admin behaviours are pinned by tests
  (`bearer_scheme_is_case_sensitive`, `rejects_token_with_extra_whitespace`), so
  the strictness is deliberate rather than accidental.

  The section corrects a claim in the issue that prompted it: `#152` argued the
  lax parser is the RFC 7235-conformant one. It is not — both parsers hard-code
  their prefixes, so `BEARER` and `BeArEr` are rejected by **both**. Neither is
  case-insensitive; one simply accepts an extra spelling.

  Trailing whitespace is documented as protocol-dependent, which is the one
  behaviour here that cannot be stated flatly: `httparse` strips trailing
  whitespace from HTTP/1.1 header values before any handler sees them, while
  HTTP/2 preserves it. `Bearer <jwt> ` is therefore accepted everywhere over
  HTTP/1.1 and refused by `/admin/*` over HTTP/2.

<!-- end REG-11 #152 -->

- Reference guides under `docs/`: an index (`README.md`) plus
  `HTTP-API.md` (every endpoint, media types, and the RFC-ACDP-0007
  error envelope), `AUTHENTICATION.md` (DID challenge-response, JWT
  claims, HS256 vs EdDSA/JWKS, token revocation, cross-issuer
  revocation federation), `CONFIGURATION.md` (the full config tree and
  startup validation), `MULTI-TENANCY.md` (tenant resolution and strict
  mode), and `WEBHOOKS.md` (event payloads and the signature scheme).
- `ARCHITECTURE.md` and `OPERATIONS.md` refreshed to match the current
  code (crates.io `acdp` dependency, EdDSA/JWKS, revocation federation,
  multi-tenancy, admin endpoints, rate limiting). Protocol-level material
  links to the `acdp` library docs rather than being restated.
- `README.md`, `CONTRIBUTING.md`, and `SECURITY.md` corrected to reflect
  that `acdp` is consumed from crates.io (no sibling path dependency) and
  the current auth/hardening surface.
- **Documentation-accuracy pass (`REG-10` docs follow-up) — two inaccuracies
  corrected, both in the permissive direction (docs described the code as
  safer/more restricted than it actually is). Two source comments carrying
  the same false permissive framing were also corrected
  (`crates/acdp-registry-core/src/lib.rs`, `crates/acdp-registry-core/src/handlers/admin.rs`)
  — comments only, no logic or signature changes, no behavior change.**
  - `CONFIGURATION.md`'s `[playground]` section and `README.md`'s feature
    list said the DID-signature bypass itself was "compiled in only with
    the `playground` Cargo feature." It is not: only the two `/admin/*`
    routes (`admin_router` in `crates/acdp-registry-core/src/lib.rs`) are
    `#[cfg(feature = "playground")]`-gated. The publish handler's
    DID-verification skip
    (`crates/acdp-registry-core/src/handlers/context.rs`, the
    `playground_snapshot.enabled` branch) is a plain runtime `if` present in
    every build, including a stock release binary — verified directly: that
    file's only `cfg` attributes are three `#[cfg(test)]` blocks. Corrected
    to say so; the existing "never enable
    in production" warning is retained and strengthened — it now leads the
    section (previously buried behind a paragraph of compile-gating detail)
    and reads as the stronger, accurate claim (the risk exists regardless of
    how the binary was built, and is scoped to non-`did:key` publishes),
    not weakened. `OPERATIONS.md` and `HTTP-API.md` already scoped
    the feature-gate claim correctly, to the admin routes only.
  - `HTTP-API.md`'s endpoint table and Admin section said `GET
    /admin/contexts` requires the admin bearer (`auth.admin_tokens`), like
    the other `/admin/*` routes. It does not: `admin_list`
    (`crates/acdp-registry-core/src/handlers/admin.rs`) calls only
    `caller_from_headers`/`tenant_for_request` — the same resolution the
    regular tenant-scoped read routes use — and never
    `require_admin_bearer`, unlike `reload_pinned_keys`, `admin_status`,
    `lineage_audit`, and the lifecycle-transition handlers, which do.
    With `auth.enabled = false` the route is fully anonymous — and even with
    `auth.enabled = true`, no bearer is required at all when the
    `Authorization` header is simply absent (`caller_from_headers` returns
    `Ok(None)`), so an unauthenticated request still enumerates public
    contexts. This is a pre-existing gap between the route's `/admin/` path
    and its actual authorization, not a behavior change — `OPERATIONS.md`
    and `README.md`'s endpoint table corrected to match.

<!-- W3-U5 (lane-1) — correcting the U-005 entry above -->

- **This changelog said `docker compose up` boots cleanly. It did not**
  (`W3-U5`). Correcting a false claim introduced by the `U-005` entry above.
  Appended, never rewritten, per `D-008`.

  **The claim, quoted verbatim:**

  > **`docker compose up` with the shipped `changeme` default boots
  > cleanly** — the guard never fires in the one setup where a placeholder
  > secret is likeliest to survive into production.

  **What was actually true.** It did not boot. It exited `rc=1` before serving
  a request, and had done so for as long as the placeholder had been there.
  The first half of that sentence was false; the second half was true, and is
  *why* the first half was believed. The literal `changeme` guard is indeed
  gated on `auth.enabled` and indeed never fired in the compose stack — but it
  was never what stopped the boot. `serve_with_store` passes any non-empty
  `jwt_secret` to `JwtSecret::from_base64`, which imposes a 32-byte floor with
  no `auth.enabled` gate. `changeme` is valid base64 of six bytes, so the
  stack died on the length floor, in a code path `U-005` never examined.

  Two further sentences in that entry are also wrong and are corrected here
  rather than in place. It described the check as "nested inside `auth.enabled
  && jwt_signing_alg != "EdDSA" && !jwt_secret.is_empty()`" — the
  `auth.enabled` conjunct is gone as of this unit. And it instructed:
  "**Operators auditing whether they were affected should note this: if you
  relied on that documented check with auth disabled, it never ran.**" That
  instruction is wrong in a way worth being explicit about: the check that
  actually stopped the boot — the ≥32-byte floor in the serve path — *always*
  ran with auth disabled. An operator following that audit advice would have
  looked for a silent acceptance that never happened.

  **What operators should actually note.** No deployment was made less safe by
  the original error: the stack refused to start rather than starting
  insecurely. But anyone who tried the documented quickstart and concluded the
  repo was broken was right, and anyone who read this file to decide whether
  to bother was misled. If you are on EdDSA, a different fact applies — a
  `changeme` in `jwt_secret` is ignored entirely there, then and now.

  For the record, since this entry corrects a pin as well as a claim: `U-005`
  listed seven files as corrected. Four are corrected again here.
  `docker/RAILWAY.md`, `docs/OPERATIONS.md` and `docs/AUTHENTICATION.md` still
  carry the false gating claim and are **not** this unit's to change; they are
  reported to their owners with quotes rather than edited.
