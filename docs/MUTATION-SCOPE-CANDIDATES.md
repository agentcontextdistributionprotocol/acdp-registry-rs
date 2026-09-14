# Mutation oracle — scope candidates and their measured survivor inventory

Companion to `.cargo/mutants.toml` and `.github/workflows/mutants.yml`, for #216 item 1
("fault injection over `src/`"). The committed scope is **213 of 1427** mutants (14.9%).
This file records what the *next* tranches would cost, measured rather than estimated, so
a widening is a decision with a price attached instead of a glob edit.

**The rule this file exists to serve: pay first, widen last.** A tranche is admitted to
`examine_globs` only once its survivors are killed or individually argued. Widening into a
red budget would produce exactly what `.cargo/mutants.toml` warns about — "a red check
nobody can explain gets disabled, after which the real regression walks through."

---

## Candidate 1 — `crates/acdp-registry-sqlite/src/store.rs` (138 mutants)

**Status: measured, deferred. Tracked in #307.**

The strongest candidate on the merits, and the reason it is not yet in scope is its bill,
not its value.

| | |
|---|---|
| mutants | **138** (`cargo mutants --list --no-config --file <path>`) |
| verdicts obtained | **95 of 138 (69%)** |
| caught | 47 |
| **missed (survivors)** | **14** — **6 killed**, **7 equivalent**, **1 needs a seam**, **0 open** |
| unviable | 34 |
| **survivor rate among viable** | **14 / 61 = 23%** |
| extrapolated survivors at 138 | **~20**, against a budget of **5** |

Measured at `67da995` under the committed config's settings (`test_workspace`, `copy_vcs`,
`ACDP_SPEC_DIR` at the `.spec-pin` revision in `ACDP_REQUIRE_CONFORMANCE=1` mode).

**Why 95 and not 138** — the runs were stopped on disk headroom; see *Cost* below. The 95
are the union of three overlapping runs, which **agreed on every mutant they shared (0
conflicting verdicts)**. The remaining 43 have no verdict and are not guessed at.

### The 14 survivors

Each is a change to `src/` that the whole workspace suite did not notice. None has been
triaged into "real gap" vs "equivalent mutant" — that triage is the work of the units that
pay them down, and is deliberately not pre-judged here.

| site | mutation | status |
|---|---|---|
| `store.rs:49:16` | `delete !` in `SqliteStore::connect` | **KILLED (U-547)** |
| `store.rs:154:9` | `count_idempotency_records` → `Ok(Some(0))` | **KILLED (U-543)** |
| `store.rs:342:26` | `+` → `*` in `list_contexts` | **KILLED (U-543)** |
| `store.rs:342:26` | `+` → `-` in `list_contexts` | **KILLED (U-543)** |
| `store.rs:380:9` | `lifecycle_events_of_ctx` → `Ok(vec![])` | **KILLED (U-543)** |
| `store.rs:568:9` | `put` → `Ok(())` | **EQUIVALENT (U-546)** |
| `store.rs:821:9` | `mark_superseded` → `Ok(())` | **EQUIVALENT (U-546)** |
| `store.rs:832:9` | `first_version_ctx_id` → `Ok(None)` | **EQUIVALENT (U-546)** |
| `store.rs:923:9` | `idempotency_evict_expired` → `Ok(())` | **EQUIVALENT (U-546)** |
| `store.rs:994:35` | `>` → `<` in `commit_publish` | **EQUIVALENT (U-544)** |
| `store.rs:994:35` | `>` → `==` in `commit_publish` | **EQUIVALENT (U-544)** |
| `store.rs:994:35` | `>` → `>=` in `commit_publish` | **EQUIVALENT (U-544)** |
| `store.rs:1090:59` | `==` → `!=` in `commit_publish` | **KILLED (U-544)** |
| `store.rs:1306:35` | `!=` → `==` in `commit_publish` | **NEEDS A SEAM (U-544)** |

### The one survivor that was run to ground

`store.rs:342:26` is `q = q.bind(limit + 1)` — the `LIMIT limit+1` sentinel that tells
`try_paginate_rows` whether more rows exist. `limit * 1` removes the sentinel, so
`next_cursor` can never be set.

It was verified **by hand**, not accepted from the verdict: applying the mutation and
running the suite leaves all 14 `store_contract` tests green, including
`visibility_sql::pages_are_full_and_total_is_honest_under_mixed_visibility` **by name**.
The oracle's `MISSED` is honest.

And the reason is sharper than "untested", which is what the verdict alone would have
suggested. `list_contexts` has **10 call sites across `crates/`**:

- **1 production caller** — `acdp-registry-core/src/handlers/admin.rs:117`
- **2 delegating wrapper impls** that forward whatever they are given —
  `acdp-registry-store/src/parity.rs:864`, `acdp-registry-server/tests/http_integration.rs:8758`
- **7 test callers**, every one passing a limit of **50 or 100**
- **0 that exercise the `limit + 1` boundary**

The only small-limit pagination test in the repo (`parity.rs`, `const LIMIT: u32 = 2`)
exercises **`search`**, a different method. So the gap is not a method nobody calls — it is
a *branch* nobody reaches, inside a method with ten callers, on a path reachable from the
admin HTTP surface.

**That is the general lesson and the argument for widening eventually:** a method with ten
callers reads as covered. The oracle is what distinguishes "this method is called" from
"this path through it is asserted on". `lifecycle_events_of_ctx → Ok(vec![])` is the same
shape one level up — the whole method can return an empty event list unnoticed.

### Cost — and the finding that the tranche does not fit locally in one pass

| | |
|---|---|
| throughput | 6.6 s/mutant (9.1 mutants/min), warm cache |
| peak live temp tree | **2566 MiB**, reached ~50s in and then flat |
| sustained consumption | **−544 MiB/min for 290s with that tree flat** ⇒ **~60 MiB/mutant** |
| headroom needed, 138 in one pass | **~8.1 GiB** |
| headroom needed, 351 in one pass | **~20.5 GiB** |

**`cargo mutants` on this tranche cannot complete in one pass at the headroom this machine
has had (4–10 GiB).** Three runs were stopped on the disk floor. Run it **sharded**
(`--shard k/8`) — consumption is fully released when the process exits (free returned
4116 → 7208 MiB on termination), so shards fit where one pass does not.

Two traps found the hard way, both worth inheriting:

- **Killing `cargo mutants` does not stop the run.** It spawns
  `cargo test --verbose --workspace`, which spawns rustc and clang, and those children
  **outlive the parent**. After `pkill -f 'cargo mutants'` reported nothing alive, a live
  `cargo test --no-run`, two rustc and a clang were still building into a *new* temp tree.
  Kill by temp-tree path *and* by cwd — the `cargo test` driver does not name the tree in
  its argv. This is also why two full 2566 MiB trees can be resident during a strictly
  sequential shard loop.
- **Never clear `target/` mid-run.** Doing so to buy headroom coincided with a healthy run
  dying at 50/138. It also forces a cold rebuild, which pushed consumption to ~1566 MiB/min
  — so the 60 MiB/mutant figure above is a **warm-cache** number and not a ceiling.

**The ~60 MiB/mutant mechanism is now SUBSTANTIALLY EXPLAINED — corrected here, because this
file previously recorded it as open.** #309 (lane-3) found a SQLite sidecar leak: harnesses
owned a `tempfile::NamedTempFile`, which deletes exactly the path it owns, while SQLite writes
`-wal` and `-shm` beside it. Those outlive the test. Lane-3 reports that in the same 290s
window measured above, the leak alone produced 5,286 files / 1,689 MiB = **349 MiB/min** —
about **two-thirds** of the 544 MiB/min observed.

Stated as two-thirds rather than "solved" on purpose: **~195 MiB/min remains unattributed**,
and this file should not trade one confident wrong answer for another. The leak is fixed at
source; the files already leaked are still resident.

Why neither of the two sessions watching found it: both instrumented the *cargo-mutants temp
tree* and `target/`, and the leak was in neither — it was loose in `/private/var/folders`,
a root nobody had enumerated.

One hypothesis was separately tested and **eliminated**: deleted-but-still-open files. Sampled
*during* a live run, `lsof +L1` held flat at **192 MiB across 362–368 fds** while 1 GiB
disappeared — the same figure measured with nothing running.

**U-543 found and fixed a second instance of the same leak** that #309 did not cover, because
`tests/tmpdir_hygiene.rs` guards the *server* harness only. Measured like-for-like on one
command, `cargo test -p acdp-registry-sqlite --test store_contract`:

| source | files leaked per run, before → after |
|---|---|
| `acdp-registry-sqlite/tests/{store_contract,parity}.rs` | **28 → 0** |
| `acdp-registry-sqlite/src/store.rs` `#[cfg(test)]` (13 sites) | **26 → 0** |

The "before" figure was nearly missed: a first measurement scoped to `$TMPDIR` at depth 1
returned a confident **0**. An empty result is only as wide as its root.

---

## Candidate 2 — `crates/acdp-registry-pg/src/store.rs` (136 mutants)

**Status: disqualified until a database is wired into the runner.**

Its tests skip to **success** when `ACDP_REGISTRY_TEST_PG_URL` is unset (`pg_url_or_skip`,
`crates/acdp-registry-pg/tests/parity.rs`), and the mutation runner provides no database.
Every *viable* mutant would therefore report `MISSED` — stated that way deliberately, since
unviable mutants never compile and so have no verdict to be wrong about.

A survivor list that is entirely false positives is what `.cargo/mutants.toml` already calls
worse than no baseline at all. **This is a prediction from the gating code, not a measured
run** — confirming it would cost a 136-mutant run to observe a skip that `pg_url_or_skip`
states in four lines.

---

## Candidate 3 — `crates/acdp-registry-store/src/parity.rs` (89 mutants)

**Status: rejected on mechanism, not cost.**

`test-support` harness code, off by default, that never ships. Mutating a test's own
assertion weakens the test; the weakened test still passes; so the mutant survives **by
construction** and no survivor has a remedy. Mutation testing asks whether tests catch
defects in production code — pointing it at the tests inverts the question.

---

## Candidate 4 — `crates/acdp-registry-store/src/lib.rs` (38 mutants)

**Status: deferred, needs a real answer first.**

Holds `retrieve_visible`, the authoritative RFC-ACDP-0008 §4.5 predicate whose shared-centre
blind spot U-534 documented — genuinely worth covering. But roughly 30 of its 38 mutants
target **default trait method bodies that both real backends override**
(`tenants_of_ctxs`, `visible_ctx_ids`, `tenant_of_ctx`, `log_leaf_hashes` all reappear as
`<impl … for SqliteStore>`). Those are unreachable and would arrive as budgeted survivors
sharing one repetitive excuse.

The open question is whether unreachable default bodies are worth mutating at all, or
should be excluded by a mechanism narrower than a glob. That deserves its own unit.

---

## Remaining scope, for whoever sizes the next tranche

Workspace total is **1427** (`cargo mutants --list --no-config`). Largest files not yet in
scope and not covered above:

| file | mutants |
|---|---|
| `acdp-registry-server/src/main.rs` | 99 |
| `acdp-registry-types/src/config.rs` | 92 |
| `acdp-registry-core/src/rate_limit.rs` | 65 |
| `acdp-registry-auth/src/revocation_store.rs` | 61 |
| `acdp-registry-types/src/error.rs` | 52 |
| `acdp-registry-auth/src/service.rs` | 51 |
| `acdp-registry-core/src/handlers/admin.rs` | 49 |
| `acdp-registry-auth/src/jwt.rs` | 47 |

`main.rs` carries the same hazard as `pg`: it is heavily feature-gated
(`storage-pg`, `storage-memory`, `playground`), so survivors there would include code the
default-feature workspace suite never compiles.

### U-544: four of the five `commit_publish` comparison survivors are not coverage gaps

Slice 2 set out to kill five comparison mutants and killed **one**. The other four were run to
ground, and the result matters more than the kill: **a survivor is not automatically a missing
test.**

**`store.rs:994:35` (`if expires_at > now`, ×3) — OUTCOME-EQUIVALENT, probed not argued.** With
`<` applied, an `eprintln!` at the step-7 conflict gate fires exactly once and the suite stays
green. Skipping the TTL branch lets the publish proceed to
`INSERT … ON CONFLICT(agent_id, key) DO NOTHING`, which collides with the live record, reports
zero rows affected, rolls the new context back, and replays the stored response — the **same
`IdempotentReplay`, the same `ctx_id`**, by a second route. The idempotency contract is enforced
twice, so breaking the first enforcement is invisible at this API. `>=` carries a second,
independent argument: `now` is `Utc::now()` while `expires_at` is rebuilt from stored
milliseconds, so the two differ only on an exact millisecond boundary.

**`store.rs:1306:35` (`prior_hash != content_hash`) — NEEDS A SEAM, not a test.** An `eprintln!`
at `inserted == 0` fires **zero** times across the entire suite, including both racing tests.
SQLite's `BEGIN IMMEDIATE` serialises the racers, so every loser finds the committed record at
the step-1 read and is refused at `store.rs:1003` instead. The obvious deterministic route —
pre-expire the record so the claim collides — does not work either: step 1 DELETEs an expired
record for that key (`store.rs:964`) precisely so the claim cannot collide with a stale row.
Tried, and it published cleanly. Reaching :1306 needs interleaving this harness cannot produce.

**`store.rs:1090:59` — a genuine gap, now KILLED.** Every existing supersession test supersedes
as the *original producer*, where the first arm of
`prev_agent == req.agent_id || prev_contributors.iter().any(…)` short-circuits and the
contributor arm is never consulted. Inverted, the arm reads "any contributor who is NOT you",
which both refuses genuine contributors and admits any signer whenever v1 lists a contributor
other than them — the lineage takeover the comment at that site says the check prevents.

**Consequence for sizing this issue:** the 14 known survivors are not 14 units of work. At least
3 are equivalent and 1 needs infrastructure. Expect that ratio to hold for the 43 unjudged
mutants too.

### U-546: the whole `-> Ok(())` write-path family is dead trait surface

Slice 3 killed **nothing**, and that is the correct outcome. All four are `RegistryStore` trait
methods that `SqliteStore` must implement because the trait requires them, and that **nothing in
this workspace ever calls**.

**Resolved by TYPE, because the name is ambiguous.** A bare `.put(` count is meaningless here:
`ChallengeStore::put(ChallengeRecord) -> Result<(), AuthError>` owns 12 of the call sites, all in
`acdp-registry-auth`, and is a different trait entirely.
`RegistryStore::put(Body) -> Result<(), AcdpError>` has exactly **three**, and all three are
*delegating wrapper impls* that forward to another implementation:

| method | call sites | all delegating? |
|---|---|---|
| `put` | `parity.rs:772`, `http_integration.rs:8665`, `memory_ext.rs:38` | yes |
| `mark_superseded` | `parity.rs:793`, `http_integration.rs:8692`, `memory_ext.rs:50` | yes |
| `first_version_ctx_id` | `parity.rs:799`, `http_integration.rs:8698`, `memory_ext.rs:53` | yes |
| `idempotency_evict_expired` | `parity.rs:828`, `http_integration.rs:8730`, `memory_ext.rs:74` | yes |

No originating caller exists. The UFCS form (`RegistryStore::put(&x, …)`) that a dot-grep would
miss returns nothing either.

**One near-miss worth keeping.** The upstream `acdp` crate *does* call
`self.idempotency_evict_expired(...)` in non-test code — `acdp-0.1.0/src/registry/store.rs:405`.
That is inside `impl RegistryStore for **InMemoryStore**` (line 308), a different type, so it says
nothing about `SqliteStore`'s implementation. `SqliteStore::idempotency_lookup` deliberately does
**not** evict at lookup — its comment says the background task `evict_idempotency` keeps the table
bounded instead. A workspace-only grep would have missed that call entirely, and a
type-blind reading of it would have wrongly promoted this one to "reachable".

**Confirmed empirically, with a control.** `panic!` armed in all four methods, full
`cargo test --workspace` with `ACDP_SPEC_DIR` set and `ACDP_REQUIRE_CONFORMANCE=1`:
**0 runtime panics, 0 failed binaries.** The same probe placed in `get()` — a method that *is*
called — produced 9 panic lines, so the detector demonstrably works and the zero is a real zero
rather than a broken check.

**Revisit trigger:** this equivalence expires the moment any of the four gains an originating
caller. It is a property of the current call graph, not of the methods.

**Running classification: 5 killed, 7 equivalent, 1 needs a seam, 1 open** — out of 14 known
survivors, from 95 of 138 mutants judged. The one still open is `49:16` `delete !` in
`SqliteStore::connect`.

### U-547: the last known survivor, and what the whole exercise showed

`store.rs:49:16` is `if !parent.as_os_str().is_empty()`, guarding `create_dir_all(parent)`.
Deleting the `!` inverts it to "create the parent only when there isn't one".

It survived because **every existing test hands `connect` a path whose parent already exists** —
`tempfile::tempdir()` creates it — so `create_dir_all` is a no-op and skipping it changes nothing.
A real deployment pointed at `/var/lib/acdp/registry.sqlite` before that directory exists would
fail to start. Killed by `connect_creates_a_missing_parent_directory_chain`, which asserts the
parent is **absent** as an explicit precondition, so the test cannot silently stop exercising the
branch if someone changes the fixture.

### The known list is now fully resolved

| | |
|---|---|
| killed | **6** |
| equivalent | **7** |
| needs a seam | **1** (`1306:35`, race-only) |
| open | **0** *(true only of the KNOWN list; U-549 measured all 138 and found 11 more — see below)* |

**Half the known survivors were not coverage gaps.** Seven of fourteen were equivalent — code whose
mutation cannot change observable behaviour — and an eighth needs a test seam rather than a test.
That ratio is the single most useful number here for anyone sizing this work: **a survivor list is
not a work list**, and #307 should never have been sized by its survivor count.

### U-548: the disk block is lifted, and the judged/unjudged split was lost

**Both halves of the sentence that used to close this section were wrong, and they are corrected
here rather than quietly deleted.** It read: *"the only remaining work on this file is the 43
mutants that have never been judged — 95 of 138 have verdicts. That is blocked on disk headroom
for a sharded run."*

**"43 unjudged" can no longer be resolved to a list.** The per-mutant verdict files for the 95
judged mutants lived only in a session scratchpad and were deleted during an ENOSPC cleanup that
freed 59 MiB. Every surviving `outcomes.json` on the machine was parsed to look for them — with a
control confirming the parser finds the pattern when it is present — and none contains a
`sqlite/src/store.rs` verdict. **The remaining work on this file is therefore all 138, not 43.**
The per-run ledger is committed under `docs/mutation-runs/` from now on precisely so this cannot
recur; a 40-minute measurement must not live somewhere a cleanup can reach.

**"Blocked on disk headroom" is disproven.** `--copy-target=false` removes the block outright:

| measurement | value |
|---|---|
| command | `cargo mutants --no-config --file crates/acdp-registry-sqlite/src/store.rs --shard 1/8 --copy-target=false --test-workspace=true --minimum-test-timeout=300 -j1` |
| wall clock, 18 mutants | **97 s** |
| peak consumption below the post-clean baseline | **2547 MiB** (free 4770 → 2223) |
| peak temp tree observed | **2649 MiB** |
| **residual after exit** | **0** — free returned 4770 → 4770, temp trees 0 |
| sidecar `.sqlite` files created by the run | **0** |

Two caveats that keep those numbers honest. The temp tree was sampled at 30 s and the last in-run
sample landed 26 s before exit, so the true peak is **≥ 2649 MiB**, not exactly it. And the residual
figure is a **net-zero equality**, not a rate: 97 s spans too few steps to support a MiB/min claim,
and the tree grew monotonically throughout so there was never a flat phase to measure a rate
against. What is established is that a complete shard leaves nothing behind.

The `0 sidecar files` line is a direct count under `$TMPDIR`, date-bounded to the run window with
`TZ=UTC`, with two controls: a probe file created in the same root was visible to the same `find`,
and an unbounded `-newermt` returned the full 312 900 pre-existing files. Those 312 900 are the
known pre-existing backlog, untouched by this run — **the leak fix holds.**

**Projected cost for the full 138 in one pass**, from this shard's measured per-class means
(unviable 2.2 s, caught 9.0 s, baseline 16.3 s once) against the ≆33 % viable rate seen here:
**≈ 10.5 minutes.** Treat that as a **floor, not an estimate** — a *caught* mutant fails fast, while
a *missed* one runs the entire workspace suite to green, so any shard containing survivors costs
more per mutant than this one did.

**A note on a non-finding, kept because the error is instructive.** The ~30 s cold build initially
looked anomalous and was nearly written up as unexplained. It is not. An independent
`cargo test -p acdp-registry-server --test conformance_gate` from a cleaned `target/` compiled the
workspace in **25.77 s** — this machine simply builds this workspace that fast. The recorded budget
I was measuring against, `cargo test --locked --workspace --no-run` ≈ **2.0 GiB cold**, is a *disk*
figure; the "several minutes" I was implicitly comparing against was never recorded anywhere and was
my own addition. **A fabricated premise had produced a real-looking anomaly**, and it would have
shipped inside a paragraph whose every other sentence was measured.

### RETRACTED — U-548's re-run proved nothing (see U-549)

> **The section that stood here claimed U-543's kills were confirmed by cargo-mutants.
> That run was void.** It was invoked with `--no-config`, which discards the whole of
> `.cargo/mutants.toml`; the settings restored by hand omitted `copy_vcs = true`, so `.git`
> was absent from the mutant tree and three git-dependent conformance tests panicked in
> **every** mutant tree, marking every mutant caught for a reason unrelated to the mutation.
> The pre-registered prediction matched **for the wrong reason**. Full mechanism in
> `docs/mutation-runs/README.md`; the ledger is kept as `VOID-u548-...json`.
>
> **What was never retracted:** U-543's three mutants *were* killed by hand-application with
> a red observed each time. That evidence never depended on this harness. And U-549's valid
> run has since confirmed all three as `CaughtMutant` — so the claim is now true on evidence
> that actually supports it.

## U-549 — the full 138 under a valid harness, and D-W5-182 re-tested

Eight shards, one uniform method: `--config docs/mutation-runs/u549-sqlite-tranche.toml`,
`--copy-target=false`, `-j1`. Every ledger is committed beside this file.

| shard | caught | missed | unviable | n |
|---|---|---|---|---|
| 0/8 | 11 | 0 | 7 | 18 |
| 1/8 | 6 | 0 | 12 | 18 |
| 2/8 | 12 | 1 | 5 | 18 |
| 3/8 | 11 | 2 | 5 | 18 |
| 4/8 | 13 | 4 | 1 | 18 |
| 5/8 | 6 | 7 | 5 | 18 |
| 6/8 | 4 | 4 | 10 | 18 |
| 7/8 | 5 | 1 | 6 | 12 |
| **total** | **68** | **19** | **51** | **138** |

**Validity checks, all passed:** `end_time` present on all eight (a null `end_time` is the
in-flight tell, and a grep for the *key* rather than its *value* is a false positive);
mutant rows match every header; **zero** logs containing `not a git repository`; sizes sum
to 138 **and** the ledgers carry 138 distinct mutants with 0 duplicates — sizes alone miss a
duplicated shard, the union alone misses a dropped one.

**`--shard k/N` is ZERO-INDEXED.** Valid shards are `0/8`–`7/8`; `8/8` is empty. An ordinal
range of "2/8 through 8/8" covers 120 of 138 and silently drops shard 0/8 while producing a
confident eight ledgers.

### The answer: D-W5-182's 50%-of-viable does NOT survive

Both rates, each with its denominator on the same line as the number, because a rate whose
denominator lives in a footnote gets quoted without it:

- **RAW: 19 survivors / 87 viable = 21.8% of viable.**
- **UNJUDGED: 11 previously-unjudged survivors / 87 viable = 12.6% of viable.**

**12.6% is the figure comparable to D-W5-182's 50%**, which was `3 missed / 6 viable` from
shard 1/8 and was a pure-gap rate — its numerator contained no known-equivalents, because
nothing in that shard had been judged yet. So the comparable measured figure is **about one
quarter of the ruling's premise**, and the ruling was extrapolated from a single 18-mutant
shard whose three survivors were all subsequently killed.

All **8** already-judged non-gaps (7 equivalent + 1 needs-a-seam) came back `MissedMutant`,
exactly as recorded — so the exclusion set was computed from observed verdicts, not assumed
from the prior count. That distinction is load-bearing: a judged-equivalent that had been
*caught* would never appear in `missed` at all, and subtracting a fixed 8 would have removed
a survivor that was never counted.

### The 11 previously-unjudged survivors

| site | mutation | area |
|---|---|---|
| `1585:37` | `delete !` | tag filter in `list_contexts` |
| `1588:24` | `delete !` | tag filter in `list_contexts` |
| `1588:74` | `!=` | tag filter in `list_contexts` |
| `1596:86` | `!=` | `derived_from` filter |
| `1632:9` | `Ok(())` | `idempotency_evict_inner` |
| `1644:9` | `Ok(())` | `evict_idempotency` |
| `1869:5` | `String::new()` | `context_type_str` |
| `1869:5` | `"xyzzy".into()` | `context_type_str` |
| `1897:26` | `true` | status/expiry computation |
| `1897:26` | `false` | status/expiry computation |
| `1897:30` | `>` | status/expiry computation |

All are production code — `#[cfg(test)]` begins at line 1987. **Not paid down in this unit
by instruction:** measure, report, stop. Whether to widen scope or pay these down is a
scoping decision, not a lane call.

### What the correction did NOT void: the disk findings

U-548's verdicts were void; its **disk** measurements were not, and they have now been
re-confirmed under the corrected configuration. Across all eight valid shards free space
stayed flat — **4617 → 4587 MiB**, a −30 MiB drift that includes the committed ledgers
themselves — with **0 temp trees** and **0 sidecar `.sqlite` files** left behind. Residual
is still zero, now with `.git` actually present in the mutant tree.

The one figure that needs a caveat rather than a retraction: U-548's **peak** of 2547 MiB
was measured on a run that did *not* copy `.git`, so it **understates** the corrected
configuration by roughly the size of the repository history. The conclusion it supported —
that the tranche fits comfortably and disk is not the constraint — is unaffected, and the
whole 138 completed here in about 24 minutes of wall clock.

**`rc=2` from `cargo mutants` means survivors were found, not that the run failed.** Shards
0/8 and 1/8 exited 0 (no survivors); the other six exited 2. Treating a non-zero exit as a
failed run would have discarded six valid shards.

## U-550 — the 11 unjudged survivors, paid down

Five tests in `crates/acdp-registry-sqlite/tests/store_contract.rs` kill all **11**
previously-unjudged survivors. Confirmed by **two independent instruments**, because
U-548 is the standing argument against trusting one.

**Instrument 1 — hand-application.** Each of the 11 mutants applied to `src/store.rs` by
hand, the named test run, and the mutant reverted. **All 11 reddened.** Each anchor was
verified to match exactly once on clean source first, so a silently-unapplied mutation
could not masquerade as a kill.

**Instrument 2 — the oracle**, re-run under `docs/mutation-runs/u549-sqlite-tranche.toml`
against a committed baseline. Ledgers committed beside this file.

| shard | missed before (U-549) | missed after | outcome |
|---|---|---|---|
| 5/8 | 7 | **1** | six killed; `1306:35` survives, as predicted |
| 6/8 | 4 | **0** | all four killed |
| 7/8 | 1 | **0** | killed |

All 11 came back `CaughtMutant`. **`1306:35` remains `MissedMutant`** — U-544 proved that
branch unreachable at this API, and this unit deliberately did **not** manufacture a test
to force a kill on it. Had it flipped to caught, that would have *refuted* U-544's seam
verdict and been reported as such; it did not.

**Tranche status: 19 survivors → 8, and all 8 are already judged** (7 equivalent, 1 needs a
seam). There are now **zero unjudged survivors** in `sqlite/src/store.rs`.

### Two seams worth recording, because the obvious test would have passed vacuously

**`context_type_str` is observable only through the FILTER, never a read-back.**
`SearchResult.context_type` is rebuilt from the stored body JSON (`store.rs:1615`), so
asserting on a returned context's type passes against both constant mutants and proves
nothing. The mutated value is read in exactly one place: `store.rs:1416`,
`AND context_type = ?`. The test therefore publishes two types and queries each, since no
single constant can satisfy both queries.

**Several of these mutants WIDEN the result rather than emptying it.** `1585:37` makes every
context pass the tag filter; `1588:74` admits any context carrying some *other* tag. A
`contains`-style assertion passes against both. Every U-550 assertion is on an **exact set**
for that reason — and `1596:86` is the mirror case, emptying the result, so an
assertion that merely required the child to be *absent* would also have passed. The two
directions need opposite assertion shapes, and a set equality covers both.
