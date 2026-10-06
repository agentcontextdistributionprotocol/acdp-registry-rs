# Contributing

Thanks for your interest in `acdp-registry-rs`.

## Setup

1. Install Rust 1.88 or newer (`rustup install stable`).
2. (Optional) Install `cargo-deny` and `cargo-vet` if you plan to touch
   dependencies.

The protocol library [`acdp`](https://crates.io/crates/acdp) is consumed from
crates.io — no sibling checkout is required.

## Pre-PR check set

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Run the feature-flag variants if you touched the server binary or its callers:

```bash
cargo clippy -p acdp-registry-server --no-default-features --features storage-pg --all-targets -- -D warnings
cargo clippy -p acdp-registry-server --features storage-sqlite,playground   --all-targets -- -D warnings
cargo test   -p acdp-registry-server --features storage-sqlite,playground
cargo clippy -p acdp-registry-server --no-default-features --features storage-memory --all-targets -- -D warnings
cargo test   -p acdp-registry-server --no-default-features --features storage-memory

# The binary's feature space is eight: 4 backend states (sqlite | pg | memory |
# none) x playground on/off. CI builds all eight (#200); these four are the ones
# the commands above omit. Skipping them passes locally and reddens `clippy` on
# the PR, which is a required check.
cargo clippy -p acdp-registry-server --no-default-features --features storage-pg,playground --all-targets -- -D warnings
cargo clippy -p acdp-registry-server --no-default-features --features storage-memory,playground --all-targets -- -D warnings
cargo clippy -p acdp-registry-server --no-default-features --all-targets -- -D warnings
cargo clippy -p acdp-registry-server --no-default-features --features playground --all-targets -- -D warnings
```

The authoritative list is the comment above those steps in
`.github/workflows/ci.yml` — it carries the arithmetic that generates the count,
so you can check the list rather than trust it.

CI additionally runs checks you can reproduce locally (which of them block a
merge is recorded in `.github/required-checks.json`, not here):

```bash
# Postgres-backed tests. Point ACDP_REGISTRY_TEST_PG_URL at a disposable
# database. With it unset they skip (printing a line); with ACDP_REQUIRE_PG
# set (use `1`) a missing URL is a hard failure instead. CI sets it, but
# not every Postgres test runs in every job; see docs/MAINTAINING.md.
ACDP_REGISTRY_TEST_PG_URL=postgres://acdp:acdp@localhost:5432/acdp_registry \
    cargo test -p acdp-registry-pg
ACDP_REGISTRY_TEST_PG_URL=postgres://acdp:acdp@localhost:5432/acdp_registry \
    cargo test -p acdp-registry-server --no-default-features --features storage-pg --test pg_integration

# MSRV — the workspace must compile on Rust 1.88.
cargo +1.88 check --workspace --all-targets

# rustdoc must build warning-free.
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps

# Dependency audit (runs unconditionally in CI, not optional).
cargo deny check

# Supply-chain review coverage (advisory in CI). A new or bumped dependency
# needs an audit or an exemption; see docs/MAINTAINING.md
# "Supply-chain audits (cargo vet)". Install the version CI pins:
#   cargo install --locked cargo-vet --version 0.10.2
cargo vet --locked
# A crypto-critical crate (supply-chain/crypto-critical.txt) must be audited,
# not exempted, unless its exact version is on
# supply-chain/crypto-exemptions-allowed.txt. Same advisory job:
python3 .github/scripts/check_crypto_exemptions.py

# Spec conformance — replays HTTP-shaped fixtures from the ACDP spec
# against a live server built from this crate. ACDP_SPEC_DIR must point
# at a checkout of the *whole* spec repo (agentcontextdistributionprotocol/
# agentcontextdistributionprotocol), not just its schemas/conformance
# subdirectory — the harness also reads registries/profiles.json from
# the spec root for the KNOWN_FAMILIES/EXCUSED coverage ratchet; pointed
# at schemas/conformance alone, that ratchet silently skips instead of
# running (same replay count and exit code either way — check the log
# for "skipping" lines, not just the exit code). ACDP_REQUIRE_CONFORMANCE=1
# turns a missing spec into a hard failure instead of a silent skip.
ACDP_REQUIRE_CONFORMANCE=1 ACDP_SPEC_DIR=/path/to/spec/checkout \
    cargo test -p acdp-registry-server --features storage-sqlite,playground \
    --test conformance --test conformance_gate -- --nocapture
```

CI's `conformance` job checks out the spec at the revision pinned in
`.spec-pin` (the only place the SHA is written; `ci.yml` reads it from there),
so a push to the spec repo cannot silently change this repo's CI result. When
adopting new fixtures, bump the pin deliberately in its own PR — see
[Adopting a new ACDP spec revision](#adopting-a-new-acdp-spec-revision). The
bump can be driven by running the `bump spec` workflow from the Actions tab
(optionally with an explicit SHA), which opens a PR for review rather than
committing directly.

The same `--test conformance` run also spawns the built `acdp-registry` binary
and validates the documents it serves against the spec's `schemas/json/*.schema.json`
with the `jsonschema` dev-dependency (#385), so the spec checkout must include
`schemas/json/`. Validation is offline: every spec schema is registered by its
`$id`, and a `$ref` outside that set fails the test rather than being fetched.

CI also measures coverage with `cargo llvm-cov`: one number merged across the
workspace, playground, Postgres-integration and memory-backend legs, with a floor
of 88% lines (summary on the run page, lcov artifact attached). The `coverage`
check is advisory, so the floor is a signal rather than a merge gate. The Docker
check builds the `storage-pg` image, boots it against Postgres, and then boots
the documented compose quickstart through `docker/assert-quickstart-boots.sh`.

A few gates fire on things that do not look like your change:

- `cargo-deny` runs with `yanked = "deny"`, so a crate yanked upstream blocks
  merges until `cargo update -p` on that crate moves the lockfile off it.
- `no_tracked_file_contains_a_conflict_marker` fails on any merge-conflict
  marker in any tracked file.
- Most documentation guards live in
  `crates/acdp-registry-server/tests/conformance_gate.rs`; a docs edit can fail
  `tests` there (for example, a relative link to a missing file, or a source
  file cited by line number in an operator doc).
- The mutation oracle (`.github/workflows/mutants.yml`) runs on a Monday cron
  and on manual dispatch, never on PRs. It gates on the exact mutant count and
  the exact survivor set, so an edit to a scoped file can turn the next Monday
  run red; re-measure with `cargo mutants --list`. See
  [docs/MAINTAINING.md](docs/MAINTAINING.md#mutation-oracle).

### Declaring coverage for a new fixture family

If the pinned spec adds a new fixture-id family (`KNOWN_FAMILIES` in
`crates/acdp-registry-server/tests/conformance.rs` grows to 30), every plain
`cargo test --workspace` run — no spec checkout required — fails at
`known_families_partition_into_covered_excused_or_deferred` until the new
family is classified as exactly one of:

- **`COVERED`** — real coverage exists, via `CoverageMechanism::Replayed`
  (the family produces >= 1 exchange through the generic HTTP replayer),
  `CoverageMechanism::Direct(&[...])` (named in-process test functions), or
  both.
- **`EXCUSED`** — a spec-grounded, structural reason this repo doesn't owe
  HTTP-replay coverage for it (mechanically checked against the spec's
  `required_fixtures`/`conditional_fixtures`; see `EXCUSED`'s doc comment).
- **`DEFERRED`** — not yet covered, with a non-empty written reason and an
  open GitHub issue number tracking it.

"Uncovered with no entry anywhere" is not an option the ratchet allows.

## Commit messages

Conventional Commits are required. The release pipeline derives the changelog
and version bumps from these prefixes:

- `feat:` — new user-visible capability
- `fix:` — bug fix
- `docs:` — documentation only
- `refactor:` — internal refactor with no behavior change
- `test:` — tests only
- `chore:` — tooling / metadata
- `BREAKING CHANGE:` (or `!` suffix) — semver-breaking change

## Adding a new endpoint

Four steps; steps 2–4 are enforced by a test that fails the build if skipped:

1. Write the handler in `crates/acdp-registry-core/src/handlers/`, returning
   `RegistryError` so failures get the wire envelope.
2. Mount it in the core router (`crates/acdp-registry-core/src/lib.rs`) in the
   group whose layers it needs (data plane, `/auth/*`, admin, or auxiliary).
3. Classify its cache posture in `DATA_PLANE_ROUTES` or `NON_DATA_ROUTES`
   (`crates/acdp-registry-server/tests/http_integration.rs`);
   `every_route_in_the_core_router_is_classified` fails on an unclassified route.
4. Document the route, and any new wire code, in
   [`docs/HTTP-API.md`](docs/HTTP-API.md); `every_mounted_route_is_documented`
   and `every_wire_code_the_code_emits_is_documented`
   (`crates/acdp-registry-server/tests/conformance_gate.rs`) fail otherwise.

## Documentation

Reference docs live in [`docs/`](docs/README.md) (HTTP API, authentication,
configuration, multi-tenancy, webhooks, operations). When a change alters the
HTTP surface, config, auth, or operational behavior, update the relevant page in
the same PR. Document protocol-level concepts by linking to the
[`acdp` library docs](https://github.com/agentcontextdistributionprotocol/acdp-rs/tree/8a888edaa15c4475bbaeccff45567921e3153730/docs)
rather than restating them.

## Migrations

- Number new migrations sequentially within `crates/acdp-registry-<backend>/migrations/`.
- Never edit an applied migration; add a new one.
- Each migration must be idempotent (`CREATE TABLE IF NOT EXISTS`, `ON CONFLICT
  DO NOTHING`, etc.).
- Postgres migrations must stay additive (new tables/columns, no drops that an
  older binary's queries depend on) — `PgStore::migrate` runs with
  `ignore_missing(true)` so a binary one release behind the database keeps
  serving during a rolling upgrade or rollback. That tolerance exists from 0.2.0
  on; older binaries refuse a database that is ahead of them. Removing a table
  or column takes two releases: stop using it in one, drop it in a later one. A
  migration that breaks the binary one
  version behind it must say so in that version's section of
  [`docs/UPGRADING.md`](docs/UPGRADING.md) (release notes are generated from
  commit subjects and cannot carry it).

## Adopting a new ACDP spec revision

The pinned spec revision lives in **`.spec-pin`** at the repo root — one declarative
source, read by `ci.yml`, `mutants.yml`, the `bump spec` bot, and the conformance
harness. **Never restate the sha anywhere else**; `spec_pin_violations`
(`crates/acdp-registry-server/tests/conformance_gate.rs`) fails the build if you do.

Adopting a revision changes **three** values, and the bot rewrites only the first, so a
`bump spec` PR **arrives red on `conformance`** and needs two edits by hand:

1. `ref:` in `.spec-pin` — the bot does this.
2. `conformance-digest:` in `.spec-pin` — by hand, but you do not have to compute it:
   the failing test prints the digest it measured from the tree in front of it, so this is
   a copy. (It is an RFC 6962 Merkle root over the revision's `*.json` fixtures.)
3. `TOTAL_FIXTURES_AT_PIN` in `crates/acdp-registry-server/tests/conformance.rs` — by
   hand, if the fixture count changed.

That is a real cost and it is deliberate: the alternative is computing the digest from
whatever tree happens to be present, which is exactly the hole it closes. Three spec
checkouts on one machine disagreed by up to two fixtures before this existed, and the
directory named `-pinned` was the one two revisions behind.

To run the conformance suite against a spec tree, materialise the pinned revision rather
than pointing at a checkout you already have:

```bash
mkdir -p /tmp/spec-at-pin
git -C <your acdp spec checkout> archive $(grep '^ref: ' .spec-pin | cut -d' ' -f2) \
  | tar x -C /tmp/spec-at-pin
ACDP_REQUIRE_CONFORMANCE=1 ACDP_SPEC_DIR=/tmp/spec-at-pin \
  cargo test -p acdp-registry-server --features storage-sqlite,playground --test conformance
```

The harness **refuses** a tree that is not at the pin, and names the revision it found
instead of failing on a fixture count. It decides from content, not from git: a
`git archive` extract has no `.git`, so the tree that must pass is precisely the one
`git rev-parse` cannot identify.

### Editing `.spec-pin`

Its format is an external contract with acdp-ci's `bump-spec-ref` bot, which finds the pin
by substring-matching every line, **comments included**:

- **Do not spell `repository: <the spec repo>` or `acdp-ci/actions/checkout-spec@` in a
  comment.** Either makes the file look like it has two pin anchors, and the bot then
  declines to bump it at all — the pin freezes with nothing going red. Describe the forms
  in prose, as the existing comments do.
- **Keep `conformance-digest:` below `ref:`.** A 64-hex digest contains a 40-hex run, so
  above `ref:` it would be read as the pin.

Both rules are asserted by `spec_pin_violations`, so you will find out on your PR rather
than the next time the bot tries to bump.

## Composite actions and the lint gate

`actionlint` cannot parse an `action.yml` — it enumerates `.github/workflows/*` only, and
pointed at an action file it reports `"jobs" section is missing`. So the `run:` blocks in
`.github/actions/*/action.yml` are covered by a **separate** `lint.yml` step that extracts
each block and runs `shellcheck` over it. Two consequences worth knowing:

- A `${{ … }}` expression is replaced by a placeholder before linting (the same
  substitution actionlint makes internally), so a quoting defect *inside* an expression is
  not caught.
- The step fails if it extracts zero blocks from a non-zero number of action files. That is
  deliberate: an extractor that matches nothing reports success identically to one that
  finds nothing wrong.

## When a change is warranted

See [STATUS.md](STATUS.md): the repository is considered done while `main` is green and no issue is open, and it lists the four
things that reopen it. Deferred assumptions live in [DEFERRED.md](DEFERRED.md); they do not become issues on their own.

## Maintainers

Required vs advisory checks, branch and tag protection (and how to apply it),
the release flow, and the scheduled jobs are documented for maintainers in
[docs/MAINTAINING.md](docs/MAINTAINING.md).

## Security disclosures

See [SECURITY.md](SECURITY.md).
