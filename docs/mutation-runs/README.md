# Mutation-run ledgers

One committed `outcomes.json` per cargo-mutants run over a scoped tranche. These exist
because a 40-minute measurement must not live anywhere a disk cleanup can reach: the
per-mutant verdicts for 95 of this file's 138 mutants were lost exactly that way.

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
