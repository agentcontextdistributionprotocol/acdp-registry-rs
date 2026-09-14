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
| **missed (survivors)** | **14** |
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

| site | mutation |
|---|---|
| `store.rs:49:16` | `delete !` in `SqliteStore::connect` |
| `store.rs:154:9` | `count_idempotency_records` → `Ok(Some(0))` |
| `store.rs:342:26` | `+` → `*` in `list_contexts` |
| `store.rs:342:26` | `+` → `-` in `list_contexts` |
| `store.rs:380:9` | `lifecycle_events_of_ctx` → `Ok(vec![])` |
| `store.rs:568:9` | `put` → `Ok(())` |
| `store.rs:821:9` | `mark_superseded` → `Ok(())` |
| `store.rs:832:9` | `first_version_ctx_id` → `Ok(None)` |
| `store.rs:923:9` | `idempotency_evict_expired` → `Ok(())` |
| `store.rs:994:35` | `>` → `<` in `commit_publish` |
| `store.rs:994:35` | `>` → `==` in `commit_publish` |
| `store.rs:994:35` | `>` → `>=` in `commit_publish` |
| `store.rs:1090:59` | `==` → `!=` in `commit_publish` |
| `store.rs:1306:35` | `!=` → `==` in `commit_publish` |

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

**The mechanism behind the ~60 MiB/mutant is still unexplained.** It is not the temp tree
(flat while free fell) and not `target/` (grew ~400 MiB across the same window). One
hypothesis was tested and **eliminated**: deleted-but-still-open files. Sampled *during* a
live run, `lsof +L1` held flat at **192 MiB across 362–368 fds** while 1 GiB disappeared —
the same figure measured with nothing running. Cause is settled (it stops when the run
stops); mechanism is open. Do not let this file imply otherwise.

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
