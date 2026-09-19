# Mutation-run ledgers

One committed `outcomes.json` per cargo-mutants run over a scoped tranche. These exist
because a 40-minute measurement must not live anywhere a disk cleanup can reach: the
per-mutant verdicts for 95 of this file's 138 mutants were lost exactly that way.

## Index — which ledgers are CURRENT

**One ledger is CURRENT for the whole CI scope: `u552-union-scope-351-outcomes.json`.**
Everything below it is history, kept for the reasons each section gives. A reader summing
the `u549-*` set gets 19 survivors; a reader summing `u549` + `u550` gets 8; the tranche has
been at **2** since U-552 paid six of them off with tests. Sum nothing — read the current
ledger.

| ledger | scope | result | status |
|---|---|---|---|
| `u552-union-scope-351-outcomes.json` | **351** — core 213 + `sqlite/src/store.rs` 138 | 219 caught / **7 missed** / 124 unviable / 1 timeout | **CURRENT** |
| `u551-core-scope-213-outcomes.json` | 213 | 134 caught / 5 missed / 73 unviable / 1 timeout | SUPERSEDED by `u552` |
| `u549`/`u550` store.rs shards (11 files) | 138, in 8 shards | 8 missed across the set | SUPERSEDED by `u552` |

**The `u549`/`u550`/`u551` ledgers are SUPERSEDED, not VOID** — the same distinction the
shard table below draws, now applied one level up. They were correct measurements of the
tree as it stood. U-552 added tests that killed six of the eight store.rs survivors and then
widened `examine_globs` so all 351 are measured in ONE run, which is what makes a single
current ledger possible at all. Nothing about how they were taken is in question.

**Why one run replaced eleven shard files:** a sharded report's `total_mutants` is that
shard's share, so `len(records) == total_mutants` holds per shard and a truncated run is
indistinguishable from a complete one. `.github/scripts/classify_removed_survivors.py`
refuses a sharded report for exactly this reason (`--expected-scope`). Shards were a disk
workaround, never the preferred shape.

### Historical — the shard-by-shard state before U-552

**A reader summing the `u549-*` set gets 19 survivors, and that has not been the tranche's
state since U-550.** Three of those shards were re-run; the table below is which file to
believe for each shard.

### `acdp-registry-sqlite/src/store.rs` — 138 mutants, shards 0/8–7/8

| shard | current ledger | missed | superseded |
|---|---|---|---|
| 0/8 | `u549-sqlite-store-shard-0of8` | 0 | — |
| 1/8 | `u549-sqlite-store-shard-1of8` | 0 | — |
| 2/8 | `u549-sqlite-store-shard-2of8` | 1 | — |
| 3/8 | `u549-sqlite-store-shard-3of8` | 2 | — |
| 4/8 | `u549-sqlite-store-shard-4of8` | 4 | — |
| 5/8 | `u550-sqlite-store-shard-5of8` | 1 | `u549-…-5of8` (was 7) |
| 6/8 | `u550-sqlite-store-shard-6of8` | 0 | `u549-…-6of8` (was 4) |
| 7/8 | `u550-sqlite-store-shard-7of8` | 0 | `u549-…-7of8` (was 1) |
| **total** | | **8** | all eight SUPERSEDED by `u552` |

**Six of those eight were killed by U-552, not re-labelled.** `568:9` `put`, `821:9`
`mark_superseded`, `832:9` `first_version_ctx_id`, `923:9` `idempotency_evict_expired`, and
both killable `994:35` comparisons (`<` and `==`). The two that remain are `994:35 >=`
(equivalent) and `1306:35 !=` (unreachable by design); both carry their reason in
`MUTANTS_SURVIVORS`.

**The `u549` 5/6/7 ledgers are SUPERSEDED, not VOID, and the distinction is load-bearing.**
They were correct measurements of the tree as it stood; U-550 then added tests that killed
11 of the survivors, so a later run of the same shards returns different verdicts. Nothing
about how they were taken is in question. Contrast `VOID-u548-*`, whose verdicts were wrong
when they were written — see below. **Conflating the two discards a good measurement along
with a bad one.**

### `acdp-registry-core` — the 213-mutant CI scope

| ledger | scope | result |
|---|---|---|
| `u551-core-scope-213-outcomes.json` | 213 (the three `examine_globs` files) | 134 caught / **5 missed** / 73 unviable / 1 timeout |

**SUPERSEDED by `u552-union-scope-351-outcomes.json`,** which re-measured all 213 of these
alongside store.rs and returned the same five core survivors, byte for byte. That agreement
across two runs at different scopes is the reason this file is worth keeping.

This was the measured basis for `MUTANTS_SURVIVORS` in `.github/workflows/mutants.yml` — the
committed survivor set the ratchet compares against, rather than a count. Taken with
the CI-equivalent environment (`ACDP_SPEC_DIR` at the pinned spec revision,
`ACDP_REQUIRE_CONFORMANCE=1`); without those, **63 of 87** fixture-replay tests skip and the
survivor set comes out wider than CI's. (Re-measured by U-552: the "42 of 70" this and
`mutants.yml` both carried was stale — the suite grew and the figure was never revisited.)

## Read this before trusting any file here

**A file whose name begins with `VOID-` is the record of a run whose verdicts are wrong.**
It is kept, not deleted, because the failure is more instructive than the data would have
been. Do not aggregate it, quote it, or treat its counts as measurements.

### `VOID-u548-sqlite-store-shard-1of8-outcomes.json`

Reported 6 caught / 0 missed / 12 unviable. **Every one of those "caught" verdicts is an
artifact.** This run was invoked with `--no-config`, which discards the whole of
`.cargo/mutants.toml`; the settings restored by hand on the command line omitted
`copy_vcs = true`. Without it `.git` is absent from the mutant tree, and three
git-dependent conformance tests panic with `fatal: not a git repository` in **every**
mutant tree:

    no_tracked_file_contains_a_conflict_marker
    every_docs_page_is_listed_in_the_docs_index
    every_directly_read_env_var_is_documented

cargo-mutants marks a mutant caught when any test fails, so every mutant was "caught" for
a reason unrelated to the mutation.

**The baseline cannot detect this.** cargo-mutants runs the baseline **package-scoped**
(`--package=acdp-registry-sqlite`) while every mutant runs `--workspace`. The baseline
therefore never executes the failing tests and reports a clean `Success`. A green baseline
is not evidence that the workspace suite is green in the mutant tree.

**What this voids:** the claim, published in `MUTATION-SCOPE-CANDIDATES.md` and in PR #315,
that U-543's kills were confirmed by cargo-mutants. They were not; that run confirmed
nothing. What remains true and is *not* retracted is that those three mutants were killed
by hand-application with a red observed each time — evidence that never depended on this
harness.

**How it was caught:** not by suspecting the flag. By noticing that **8 of 8 known
survivors came back caught**, including `store.rs:1306:35`, which U-544 had *proved*
unreachable. A result that agrees with nothing in the record is the tell. Before that, a
coherent and completely wrong explanation — that U-543/544/547's new tests had given a
"dead trait surface" real call sites — had survived four consecutive shards, because each
new shard agreed with it. **Agreeing evidence from the same broken harness is not
corroboration.**

**The fix is structural, not a restored flag.** Runs use a purpose-built config passed with
`--config`, which *replaces* rather than merges, so every setting sits in one reviewable
file. Enumerating what a bypass discarded is the step that failed — and it failed at 3 of 4
settings correct, which is a stronger argument against enumeration than a total failure
would have been. See `u549-sqlite-tranche.toml`.
