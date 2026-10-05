# Maintaining acdp-registry-rs

This page is for the people who hold admin on this repository: how CI decides what
blocks a merge, how branch and tag protection are kept honest, how a release happens,
and the few scheduled jobs whose failures land on nobody's PR. Contributors want
[CONTRIBUTING.md](../CONTRIBUTING.md); operators deploying the registry want
[OPERATIONS.md](OPERATIONS.md).

One rule runs through all of it: **the committed files and the live settings are the
sources of truth, never a copy in prose.** This page names files and shows how to read
live state with `gh api`; it deliberately does not list required-check names, because
every transcribed copy of that list in this repository has gone stale.

## Required and advisory checks

The committed baseline for `main`'s protection is
[`.github/required-checks.json`](../.github/required-checks.json):

- `required` — the status checks that block a merge. Each entry pins `app_id` to
  15368 (GitHub Actions). An unpinned required context is satisfied by *any* app with
  `statuses: write` (the deps bot, Vercel), so an unpinned entry is a hole, not a
  shorthand.
- `strict` — a PR branch must be up to date with `main` before it merges.
- `advisory_pending` — checks that run on every PR but do not block. A red advisory
  check leaves the merge button green, which is why some gates are run twice (see the
  upgrade-notes gate under [Release flow](#release-flow)). Most of these are waiting to be
  promoted by the [runbook](#runbook-enforce_admins-the-tag-ruleset-and-promoting-advisory-checks).
  `cargo-vet` is the exception: it stays advisory until a maintainer deliberately promotes
  it in a PR of its own, and the runbook excludes it (see
  [Supply-chain audits](#supply-chain-audits-cargo-vet)).
- `enforce_admins`, `tag_ruleset`, `pending_settings` — the settings a maintainer
  applies by hand; see [Current protection settings](#current-protection-settings).

Read the live setting rather than trusting the file or this page:

```sh
gh api repos/agentcontextdistributionprotocol/acdp-registry-rs/branches/main/protection \
  --jq '{checks: .required_status_checks.checks, strict: .required_status_checks.strict, enforce_admins: .enforce_admins.enabled}'
```

### The PR-time guard

`.github/scripts/required_checks_guard.py` runs in the `lint` job on every PR. It reads
the **committed** baseline (not the live setting) and, for every name in `required` and
`advisory_pending`, requires that the name resolves to exactly one workflow job (its
`name:`, else its id; `caller / callee` for a local reusable workflow). It rejects:

- a name that resolves to no job, or to more than one (fails loudly; never skipped);
- a job whose workflow is not triggered by `pull_request`, or whose `pull_request`
  trigger has a `paths`, `paths-ignore` or `branches-ignore` filter, a `branches`
  filter that does not match `main`, or a `types` filter missing `opened`,
  `synchronize` or `reopened` — some PRs would never get the check;
- a `branches` pattern using `?`, `+`, `[` or `\` (GitHub's filter syntax and the
  guard's matcher disagree on those, so the guard fails closed);
- a job-level `if:` or `needs:` (a skipped job reports success), `continue-on-error`,
  a matrix, or `${{ }}` in `name:` (the reported name would not be the literal one);
- a `required` entry without `app_id`, or pinned to anything but 15368;
- shape errors in the file itself: unknown or missing top-level keys, duplicate names,
  a malformed `tag_ruleset`.

The practical consequence: **renaming a required job fails `lint` in the PR that renames
it.** The fix is to change the job name, the baseline and the live setting in the same
window, or every later PR waits forever on a context that never reports.

Run it locally (PyYAML is preinstalled on GitHub runners but not on a stock macOS
`python3`):

```sh
pip3 install --user pyyaml
python3 .github/scripts/required_checks_guard.py --check
python3 .github/scripts/required_checks_guard.py --self-test   # negative controls
```

### The daily drift check

[`branch-protection-drift.yml`](../.github/workflows/branch-protection-drift.yml) runs
daily (05:23 UTC) and on `workflow_dispatch`. It mints a read-only token from the
`acdp-deps-bot` App (`administration: read`, `metadata: read`) and runs
`.github/scripts/protection-drift.sh --fetch`, which compares the live settings with the
baseline:

- **Required checks and `strict`** — always a hard failure on drift: a missing check, a
  check pinned to a different app, an unpinned or duplicated live context, an extra live
  check the baseline does not list, or `strict` changed.
- **`enforce_admins` and the tag ruleset** — while the baseline says
  `"pending_settings": true`, a mismatch is a `::warning::` and the job stays green; set
  to `false`, the same mismatch fails. When everything already matches while
  `pending_settings` is still `true`, the job emits a notice telling you to flip it.
- **Ruleset `bypass_actors`** — the API may not return the bypass list to a read-only
  token. When the baseline expects a non-empty list and the live one is absent, null or
  empty, the job reports it as unverifiable (`::notice::`) rather than failing; check the
  bypass list by eye after any change.
- A failed GET is reported as "could not read", never as "nothing configured".

Most failure messages carry the command that restores the baseline (a few, such as a duplicate tag ruleset or a failed GET, only say what to check). To prove the
hard-fail path against the live settings without merging a red `main`, push a branch
whose baseline sets `pending_settings` to `false` and run
`gh workflow run branch-protection-drift.yml --ref` with that branch name. The script's
own `--self-test` runs in `lint` on every PR.

## Current protection settings

Read with the GETs below when this page was written (October 2026). **Re-read them
before acting on this section** — it is a snapshot, the GETs are the truth.

```sh
R=repos/agentcontextdistributionprotocol/acdp-registry-rs
gh api $R/branches/main/protection \
  --jq '{checks: .required_status_checks.checks, strict: .required_status_checks.strict, enforce_admins: .enforce_admins.enabled}'
gh api "$R/rulesets?includes_parents=false"
```

At that time:

- the live required checks matched `required` in the baseline exactly, every one pinned
  to 15368, with `strict` on — the daily drift runs were green;
- `enforce_admins` was **false** and the repository had **no rulesets**, while the
  baseline records `enforce_admins: true` and the `protect-release-tags` tag ruleset with
  `pending_settings: true`.

So `enforce_admins` and the tag ruleset are **not yet applied**, and the
`advisory_pending` checks are not yet required. The drift job warns about both every
day by design. The next section is how to apply them.

## Runbook: enforce_admins, the tag ruleset, and promoting advisory checks

Nothing automated applies these settings; a maintainer runs the commands below with an
admin `gh` login. The read-only commands were run when this page was written; the
mutating ones (every `--method POST`, `PATCH`, `PUT` or `DELETE`) have not yet been run
against this repository.

**Order matters: settings first, then the baseline PR.** Prepare a PR that moves the
`advisory_pending` names **except `cargo-vet`** into `required` (each with
`"app_id": 15368`), leaves `cargo-vet` in `advisory_pending`, and sets
`pending_settings` to `false`, but merge it only *after* the settings are applied, so its
own head shows the promoted checks reporting. Until it merges, the drift job reports the
promoted checks as "already required live but still advisory_pending" — a warning, not a
failure.

What the settings are:

- **`enforce_admins`** — protection binds admins too. Set through its dedicated endpoint,
  never through a full `PUT` of the protection object (which resets every field the body
  omits).
- **Tag ruleset `protect-release-tags`** — the body is the baseline's `tag_ruleset`:
  every tag (`~ALL`), rules `creation`, `update`, `deletion`, `non_fast_forward`, active,
  with exactly one bypass actor: the `acdp-deps-bot` App (`Integration`, `always`), which
  release-plz uses to create release tags. There is **no admin bypass**; break-glass is
  disabling the ruleset. Because it covers every tag, it also blocks anyone (admins
  included) from creating, moving or deleting tags by hand, including the old `archive/*`
  style tags.
- **Promoting checks** — a read-modify-write of `required_status_checks` with the object
  form `{strict, checks: [{context, app_id}]}` via `PATCH`. **Never** use
  `POST .../required_status_checks/contexts`: it appends *unpinned* contexts that any app
  with status-write access could satisfy.

### Pre-flight (all must hold)

```sh
R=repos/agentcontextdistributionprotocol/acdp-registry-rs
gh pr list --repo agentcontextdistributionprotocol/acdp-registry-rs          # no open PRs you care about mid-flight
gh run list --repo agentcontextdistributionprotocol/acdp-registry-rs --workflow release-plz.yml --limit 3   # nothing in progress
gh api $R/branches/main/protection \
  --jq '{checks: .required_status_checks.checks, strict: .required_status_checks.strict, ea: .enforce_admins.enabled}'
gh api "$R/rulesets?includes_parents=false"        # expect [] before the first apply
# The bypass actor id in the baseline must be the deps bot's App id:
gh api orgs/agentcontextdistributionprotocol/installations \
  --jq '.installations[] | select(.app_slug=="acdp-deps-bot") | .app_id'
jq '.tag_ruleset.bypass_actors' .github/required-checks.json
# Vercel holds administration:write on the repos it is installed on; it must NOT be
# installed on this one. Listing an installation's repos needs the read:user scope.
VERCEL=$(gh api orgs/agentcontextdistributionprotocol/installations \
  --jq '.installations[] | select(.app_slug=="vercel") | .id')
gh auth refresh -h github.com -s read:user
gh api "user/installations/$VERCEL/repositories" --jq '.repositories[].full_name'
```

Also confirm the acdp-ci script described [below](#the-acdp-ci-standardizesh-hazard)
will not be applied to this repository during or after the window.

### Snapshot

```sh
SNAP=$(mktemp -d)          # outside the worktree
gh api $R/branches/main/protection > "$SNAP/protection-before.json"
gh api "$R/rulesets?includes_parents=false" > "$SNAP/rulesets-before.json"
echo "$SNAP"               # keep this path; rollback reads from it
```

### Apply

```sh
# 1. Tag ruleset, built from the baseline (rules become objects), id captured.
RS=$(jq '.tag_ruleset | .rules |= map({type: .})' .github/required-checks.json \
  | gh api --method POST $R/rulesets --input - --jq .id)
gh api $R/rulesets/$RS --jq '{enforcement, bypass_actors, rules: [.rules[].type]}'

# 2. Probe: an admin creating a tag must now be REJECTED (HTTP 422).
#    If it succeeds, delete the ref at once and stop:
#    gh api --method DELETE $R/git/refs/tags/zz-ruleset-probe
gh api --method POST $R/git/refs \
  -f ref=refs/tags/zz-ruleset-probe -f sha="$(git rev-parse origin/main)"

# 3. enforce_admins, through its dedicated endpoint.
gh api --method POST $R/branches/main/protection/enforce_admins --jq .enabled

# 4. Promote the advisory checks: live checks + the baseline's advisory_pending
#    MINUS the names in KEEP_ADVISORY, every one pinned (unique_by makes a re-run
#    harmless). cargo-vet stays advisory: promoting it would block every Dependabot
#    and bump-acdp PR until someone regenerates the exemptions. Inspect the body
#    first: it must not contain cargo-vet.
KEEP_ADVISORY='["cargo-vet"]'
gh api $R/branches/main/protection/required_status_checks \
  | jq --slurpfile b .github/required-checks.json --argjson keep "$KEEP_ADVISORY" \
      '{strict, checks: (((.checks | map({context, app_id}))
                         + ($b[0].advisory_pending - $keep | map({context: ., app_id: 15368})))
                         | unique_by(.context))}' \
  > "$SNAP/rsc-new.json"
jq . "$SNAP/rsc-new.json"
gh api --method PATCH $R/branches/main/protection/required_status_checks \
  --input "$SNAP/rsc-new.json"
```

### Verify

1. Merge the prepared baseline PR (it must pass the newly required checks).
2. Dispatch the drift job and watch it go green with no warnings:

   ```sh
   gh workflow run branch-protection-drift.yml --repo agentcontextdistributionprotocol/acdp-registry-rs
   sleep 5   # let the dispatched run register before listing
   gh run list --repo agentcontextdistributionprotocol/acdp-registry-rs --workflow branch-protection-drift.yml --limit 1
   gh run watch --repo agentcontextdistributionprotocol/acdp-registry-rs RUN_ID   # RUN_ID from the line above
   ```

3. Confirm the ruleset's bypass list by eye (`gh api $R/rulesets/$RS`), since the drift
   token may not see it.
4. Watch the next release-plz run: its tags must be created (positive proof that the bot
   bypass works), and the next dependabot or release PR must show the promoted checks
   reporting and still merge.

### Rollback and break-glass

```sh
# Admins no longer bound by protection:
gh api --method DELETE $R/branches/main/protection/enforce_admins
# Tag ruleset off (break-glass; keeps it for re-enabling) or gone:
echo '{"enforcement":"disabled"}' | gh api --method PUT $R/rulesets/$RS --input -
gh api --method DELETE $R/rulesets/$RS
# Required checks back to the snapshot (pinned object form again):
jq '{strict: .required_status_checks.strict,
     checks: (.required_status_checks.checks | map({context, app_id}))}' \
  "$SNAP/protection-before.json" \
  | gh api --method PATCH $R/branches/main/protection/required_status_checks --input -
```

Then revert the baseline JSON in a PR to match, or the drift job fails on the next run
(context drift is never a warning). If `RS` is lost, find the id with
`gh api "$R/rulesets?includes_parents=false"`.

## The acdp-ci standardize.sh hazard

The org's shared CI repository, `acdp-ci`, has a script, `scripts/standardize.sh`,
that rewrites branch protection across the org's repositories. Its **apply** path is
dangerous for this repository and must not be run against it until it is changed:

- for repositories that have required checks, the protection body it sends sets
  `enforce_admins` to **false**, silently undoing the runbook above;
- it sends checks as bare context names, which **unpins** every required check
  (`app_id` becomes null — "any app may satisfy this");
- its list of this repository's checks is hand-maintained in that script, so once
  advisory checks are promoted here, its read-only `--check` mode reports them as drift,
  and an apply with `--allow-check-removal` would **drop** them.

Its `--check` (alias `--dry-run`) mode is read-only and safe to run. The durable fix
belongs in `acdp-ci`: declare this repository's settings with `enforce_admins` on and
pinned checks, or read them from this repository's `.github/required-checks.json`. The
same repository's reusable `auto-merge.yml` reads classic branch protection, which is one
reason this repository keeps classic protection for `main` rather than moving it to a
ruleset.

## Release flow

1. Every push to `main` runs [`release-plz.yml`](../.github/workflows/release-plz.yml).
   `release-plz.toml` sets `git_only = true` (previous versions come from git tags, not
   crates.io) and `publish = false` (nothing is published to crates.io; do not flip it —
   that is a one-way door). release-plz opens or updates a release PR that bumps versions
   and per-crate changelogs from Conventional Commit subjects.
2. Merging the release PR makes release-plz create one tag per crate, named
   `{{ package }}/v{{ version }}`, plus GitHub Releases. It authenticates with a token
   minted from the `acdp-deps-bot` GitHub App (`ACDP_BOT_APP_ID` /
   `ACDP_BOT_PRIVATE_KEY`), because tags and PRs created with the default
   `GITHUB_TOKEN` do not trigger workflows. The same App is the tag ruleset's only
   bypass actor.
3. The server crate's tag (`acdp-registry-server/v*`) triggers
   [`docker.yml`](../.github/workflows/docker.yml), which builds the `storage-pg` image
   and pushes it to GHCR with the semver tags (full version and major.minor). Pushes to
   `main` publish `main`, `latest` and `sha-` tags; PRs build and smoke-test but never
   push.

Gates a release PR must pass:

- **Upgrade notes.** `docker/assert-upgrade-notes.sh --check` requires a section in
  [UPGRADING.md](UPGRADING.md) for the version being built, even if it says nothing
  changed. It runs in two places: inside the `rustfmt` job, which is required and
  therefore **blocks**, and in `docker.yml`, which **reports only** while
  `docker (build + smoke)` is advisory (that copy also covers the tag push, which
  `ci.yml` never sees).
- **Railway image tags.** `conformance_gate` requires `docker/RAILWAY.md` to name the
  major.minor of the newest `## X.Y.Z` section of `docs/UPGRADING.md` (or the workspace
  version, if newer). For a breaking release, merge the UPGRADING section and the
  RAILWAY.md tags on `main` *before* the release PR: release-plz closes and reopens a
  release PR that carries a human commit, so the page cannot be fixed on its branch.

## CI behaviour worth knowing

- **Coverage** (`coverage` job, advisory). One `cargo llvm-cov` number merged across
  four legs — the workspace, the server with `playground`, the server's Postgres
  integration suite, and the server on the memory backend — with
  `--fail-under-lines 88`. The floor is `floor(min) - 1` over at least three CI runs of
  the merged job; re-derive it the same way, recording the runs in the commit, whenever
  the code or the set of legs changes materially. Because the job is advisory, the floor
  is a signal, not a merge gate.
- **`ACDP_REQUIRE_PG`.** Postgres-backed tests skip (printing a line) when
  `ACDP_REGISTRY_TEST_PG_URL` is unset. With `ACDP_REQUIRE_PG` set a missing URL is a hard
  failure instead (most tests treat any value as set; the auth crate's `pg_revocation`
  test only fails on exactly `1`, and otherwise skips without printing). CI sets it in
  the `tests` and `coverage` jobs, but the required `tests` job does not pass the
  Postgres URL to the workspace run, so the auth Postgres test runs only in the
  advisory `coverage` job.
- **Docker smoke.** `docker (build + smoke)` builds the image, boots it against
  Postgres and checks health, then boots the documented compose quickstart through
  `docker/assert-quickstart-boots.sh` (as shipped, and again with auth enabled), and runs
  that script's self-test.
- **Yanked crates.** `deny.toml` sets `yanked = "deny"` and `cargo-deny` is required, so
  an upstream yank can block every merge with no commit here. Fix it with
  `cargo update -p` on the named crate, one package at a time; do not add an `ignore`
  entry for a yank.
- **Conflict markers.** `no_tracked_file_contains_a_conflict_marker` fails the build on
  any merge-conflict marker (including the diff3 base marker) in any tracked file.
- **Documentation guards.** Most doc-truth guards live in
  `crates/acdp-registry-server/tests/conformance_gate.rs` (documented wire codes and
  routes, the docs index, relative links resolving, sibling-repository links pinned
  (`sibling_repo_links_are_pinned`), no line-number citations in operator docs, Railway
  tags, and more); `metrics_integration.rs` checks the documented
  rate-limit scopes. Read the test names there rather than a list here.

## Supply-chain audits (cargo vet)

`cargo vet` (job `cargo-vet` in `ci.yml`, **advisory**) requires every third-party crate
in `Cargo.lock` to be covered by one of:

- an audit of our own in `supply-chain/audits.toml` (empty today);
- an imported audit. `supply-chain/config.toml` imports acdp-rs's `audits.toml`, where
  the protocol library's maintainers record full and delta audits of the crypto-critical
  crates it shares with this registry (ed25519-dalek, curve25519-dalek, signature, sha2,
  p256, ecdsa, elliptic-curve, rustls, ring, and their RustCrypto support crates), plus
  the Mozilla, Google and Bytecode Alliance sets acdp-rs also imports. The fetched sets
  are frozen in `supply-chain/imports.lock`, and CI runs `cargo vet --locked`, so the
  verdict comes from the tree alone;
- an exemption in `supply-chain/config.toml`: everything else, as generated by
  `cargo vet regenerate exemptions`. That includes the registry-only crates (axum,
  sqlx, tokio and the rest of the server stack), the older RustCrypto line that
  `jsonwebtoken` pulls in (for example digest 0.10, hmac 0.12, ed25519 2.x, pkcs8 0.10,
  sec1 0.7, spki 0.7), zeroize 1.9.0 (acdp-rs exempts it too), and the `acdp-*` crates
  from crates.io. An exemption means "trusted for now, not reviewed". It is a backlog,
  not a verdict.

The workspace's own crates carry `audit-as-crates-io = false` policies. release-plz has
`publish = false`, so they are not on crates.io; the policies keep `cargo vet` from
asking about them if that ever changes.

**cargo-deny and cargo-vet answer different questions** and neither replaces the other.
`cargo-deny` (required, configured by `deny.toml`) rejects crates with a known RUSTSEC
advisory, a yank, a disallowed license, a banned crate or an unknown source. It says
nothing about whether anyone has read the code. `cargo vet` records who reviewed which
version against which criteria (`safe-to-deploy`, `safe-to-run`), and it says nothing
about advisories: an audited version with a fresh advisory still passes vet, and
cargo-deny catches it. They share no configuration. An `ignore` in `deny.toml` does not
exempt anything from vet, and a vet exemption does not silence deny.

When `cargo-vet` goes red (a Dependabot bump, a `bump-acdp` PR, or a new dependency),
the job does not block the merge. Expect this on any change to `Cargo.lock`. A PR that
adds or bumps dependencies must refresh `supply-chain/` in the same PR, even though the
job is advisory, so that `main` stays green and the next PR's vet result means
something. To make it green again, on the PR's branch:

```sh
cargo vet                         # lists what is unvetted and suggests audits
cargo vet regenerate exemptions   # exempt the new versions (no review claimed)
cargo vet --locked                # what CI runs
```

`cargo vet regenerate exemptions` also drops exemptions that an audit now covers. Run
`cargo vet regenerate imports` occasionally to pick up new imported audits, acdp-rs's
included. The import tracks acdp-rs's `main` on purpose, so `imports.lock` is where new
trust arrives. **Review its diff like an audit before committing it**, whether it came
from `regenerate imports` or from a plain `cargo vet` without `--locked`. Check which
audits are new or changed and who certified them (`who`, `criteria`, version or delta,
and whether the notes say what was read). Check every new wildcard audit and
`[[publisher.*]]` or trusted-publisher entry: it vouches for future versions too. Check
that no import silently replaced an exemption you meant to keep reviewing. To record a real review instead, use
`cargo vet certify <crate> <version>` (or a delta from an audited version). The notes
should say what was read and what is not claimed, as acdp-rs's worksheets do. Each
`acdp` bump changes the `acdp-*` versions, so a `bump-acdp` PR always needs the
regenerate step until those crates are trusted some other way (for example a
`cargo vet trust` entry for their publisher, which is a maintainer decision).

The cargo-vet version is pinned in `ci.yml` (installed with `cargo install --locked`
because the pinned `taiki-e/install-action` has no manifest for it). Keep it in step with
acdp-rs's pin, so both repositories read the same `imports.lock` format.

`cargo-vet` stays **advisory** until a maintainer decides otherwise. The runbook's
promotion step leaves it out on purpose. While the `acdp-*` crates and most of the graph
are exempted, every Dependabot and `bump-acdp` PR would be blocked until someone ran
`cargo vet regenerate exemptions`. Promoting it is a separate, deliberate change. Remove
`cargo-vet` from `KEEP_ADVISORY` in step 4. Move it from `advisory_pending` to `required`
in the baseline in the same PR. Have a plan for the `acdp-*` bumps first, such as a
`cargo vet trust` entry or a regenerate step in `bump-acdp.yml`.

## Mutation oracle

[`mutants.yml`](../.github/workflows/mutants.yml) runs `cargo mutants` over the scope
defined in `.cargo/mutants.toml` on a **Monday cron** (05:17 UTC) and on
`workflow_dispatch`. The mutation run itself **never runs on PRs** (about two hours); the
mechanical half of its pins is checked at PR time instead — see
[PR-time pin check](#pr-time-pin-check-mutants-pins) below.

The gate is exact, not a budget:

- `MUTANTS_EXPECTED_SCOPE` must **equal** the measured number of mutants — a shrinking
  scope would otherwise read as an improvement;
- the set of surviving mutants must **equal** `MUTANTS_SURVIVORS`, line for line. Each
  entry is a verbatim `cargo mutants --list` line, which includes the line and column of
  the mutation, so an ordinary edit that moves code in a scoped file changes the expected
  lines even when nothing new survives.

When it fails, re-measure and update both values in `mutants.yml` (and the per-file
counts in `.cargo/mutants.toml`) in one commit, with a reason for any survivor added:

```sh
cargo mutants --list | wc -l                                     # total scope
cargo mutants --list | sed 's/:[0-9]*:[0-9]*:.*//' | sort | uniq -c   # per-file split
```

The ledgers of past runs, and which one is current, are indexed in
[docs/mutation-runs/README.md](mutation-runs/README.md).

Any edit to `MUTANTS_SURVIVORS` must also commit the `outcomes.json` of the run that
produced it and point `MUTANTS_PRIOR_LEDGER` at that file. The classifier pairs a drifted
survivor against that ledger, and it refuses a ledger that is missing any committed line.
`every_committed_survivor_is_in_the_prior_ledger` in
`crates/acdp-registry-server/tests/conformance_gate.rs` runs the same check on every PR.

### PR-time pin check (`mutants pins`)

Three of the ratchet's pins describe the mutant *listing*, not verdicts, and
`cargo mutants --list` parses source without building (well under a second).
[`mutants-pins.yml`](../.github/workflows/mutants-pins.yml) therefore checks them on every
PR that can move them, so the Monday run is not the first to find out:

1. `MUTANTS_EXPECTED_SCOPE` equals `cargo mutants --list | wc -l`;
2. the per-file table in `.cargo/mutants.toml` equals the listing's per-file split (and
   its `total` row equals both the rows' sum and `MUTANTS_EXPECTED_SCOPE`);
3. every `MUTANTS_SURVIVORS` line, and every `Timeout` mutant recorded in
   `MUTANTS_PRIOR_LEDGER` (the budgeted one), appears in the listing verbatim. A survivor
   that only moved is reported with the line it moved to.

The logic is [`.github/scripts/check_mutants_pins.py`](../.github/scripts/check_mutants_pins.py);
its unit tests (`test_check_mutants_pins.py`) run in the required `tests` job on every PR.
The cargo-mutants version is written once, on `mutants.yml`'s install line
(`tool: cargo-mutants@27.1.0`); the PR job reads and installs that same version, and the
check requires it to equal the prior ledger's `cargo_mutants_version`, so the cron, the PR
check and the ledger cannot list with different tools. Bump it only with a re-pin. After a
green check the job inserts one blank line at the top of a scoped file that holds a cited
survivor or timeout line (chosen from the pins, not hard-coded) and requires the same
check to fail — a falsification against the real tree and tool. If no scoped file holds a
cited line, it skips the falsification with a notice. Run it locally with:

```sh
cargo mutants --list --colors never > /tmp/listing.txt
python3 .github/scripts/check_mutants_pins.py check --listing /tmp/listing.txt
```

It cannot see verdicts: a new survivor, or a killed one, still only shows up on the cron.
When it fails, either make the edit line-neutral in the scoped files or re-pin in the same
PR: run `gh workflow run mutants.yml --ref <branch>`, commit that run's `outcomes.json` as
the new `MUTANTS_PRIOR_LEDGER`, and update the scope, the table and `MUTANTS_SURVIVORS`
from it. Do not hand-edit a survivor line to the position the failure hints at without
that run: survivor lines are verdicts and must match the prior ledger, which
`every_committed_survivor_is_in_the_prior_ledger` checks.

**When it runs.** The path filter is *inside* the job, not on the trigger: the job always
starts, and its first step diffs the PR's merge commit against `main` (`HEAD^1..HEAD`).
Unless a scoped file (`examine_globs`), `.cargo/mutants.toml`, `mutants.yml`,
`mutants-pins.yml`, the script or its tests, or anything under `docs/mutation-runs/`
changed, the remaining steps are skipped and the job is green in seconds. It fails
toward running: a push to `main`, a diff that cannot be computed, a relevance step that
crashes, or any relevance answer other than an explicit `false` runs the check.

**Advisory, not required — and why it can be promoted.** It is listed under
`advisory_pending` in `.github/required-checks.json`. A trigger-level `paths:` filter or a
job-level `if:` would make the check never report on unrelated PRs, which
`required_checks_guard.py` rejects for both `required` and `advisory_pending` (a required
context that never reports blocks every PR forever). Gating at step level keeps the context
reporting on every PR, so the guard resolves it and it is *safe* to require. It stays
advisory for now because requiring it changes live branch protection (a maintainer
action, compared daily by the drift check), and because a red result is never wrong to
merge past in an emergency: the cost of ignoring it is one red Monday run, not a broken
`main`. The "promote the advisory checks" step of the runbook above promotes it together
with the others.

**Status:** pinned at scope 354 from run 37248684286 (a `workflow_dispatch` on main
09615e9, 101.2 min of the 180-minute cap). #401 removed the lifecycle charge pre-flight and
with it four caught mutants; the seven survivors are the same mutants on the same lines, and
the run's report is committed unchanged as the prior ledger. (History: #395 pinned scope 358
from run 37222567772 after #373-#376 added twelve; the 2026-09-28 run 36417581090 failed
after #341 shrank the scope from 351 to 346, and #371 re-pinned it the same way.) Edits to the four scoped files after a re-pin
must be line-neutral until the next one (`mutants pins` checks this on the PR). Check
`gh run list --workflow mutants.yml --limit 3` for the current state.

## Spec bumps

[`bump-spec.yml`](../.github/workflows/bump-spec.yml) opens a PR that rewrites only the
`ref:` in `.spec-pin`; the PR arrives red on `conformance` until the digest and fixture
count are updated by hand — the steps are in
[CONTRIBUTING.md](../CONTRIBUTING.md#adopting-a-new-acdp-spec-revision).

A bump that changes `schemas/json/` can also turn `conformance` red without touching a
fixture: `served_capabilities_documents_satisfy_the_pinned_schema` and
`served_registry_documents_satisfy_the_pinned_schemas` (in `tests/conformance.rs`) spawn
the built binary and validate what it serves (`/.well-known/acdp.json` with auth off, auth
on and every advertisable profile on, plus a publish response, a context, a search
response, a log checkpoint and an error body) against the new revision's schemas (#385).
The digest covers only fixtures, so that failure is the signal that a served document no
longer matches the spec; fix the document, not the test.

Docs that link into the spec repository at the pinned revision must be re-pointed to the
new `ref:` in the same bump PR. `sibling_repo_links_are_pinned` (in `conformance_gate.rs`)
requires every spec link's ref to equal the `ref:` in `.spec-pin` (the one exception is
`SPEC_DOCS_REF` in that test, for links under the spec's `docs/` directory, which is
updated by hand), so a bump PR also
fails the `tests` and `conformance (spec fixtures)` jobs — listing each stale link —
until they are re-pointed. From the
bump branch, with `origin/main` still at the old pin:

```sh
old=$(git show origin/main:.spec-pin | sed -n 's/^ref: //p')
new=$(sed -n 's/^ref: //p' .spec-pin)
[ -n "$old" ] && [ -n "$new" ] || { echo "could not read the old or new ref"; exit 1; }
git grep -lE "agentcontextdistributionprotocol/agentcontextdistributionprotocol/(blob|tree|raw)/$old" \
  -- '*.md' 'config/*.toml' 'docker/*' ':!plans' ':!crates/*/CHANGELOG.md' \
     ':!DECISIONS.md' ':!ASSUMPTIONS.md' ':!docs/ENGINEERING-LOG.md' \
  | xargs perl -pi -e "s#(agentcontextdistributionprotocol/agentcontextdistributionprotocol/(?:blob|tree|raw)/)$old#\${1}$new#g"
cargo test --locked -p acdp-registry-server --test conformance_gate sibling_repo_links_are_pinned
```

The rewrite touches link URLs only: dated records keep the SHA they were written
against, and the "today" value in the Link-convention block of
[docs/README.md](README.md#link-convention) is prose, so update it by hand. Then
re-check each `#anchor` against the new revision's headings — the guard reads no
network and cannot see a renumbered section.
