# Engineering log

The narrative record of what changed in this repository and **why** — the
reasoning, the rejected alternatives, and the evidence behind each change.
Entries are newest-first.

This is deliberately *not* the changelog. Per-release notes live in the eight
per-crate `crates/*/CHANGELOG.md` files, which `release-plz` maintains, and in
the GitHub Releases that mirror them. This file was the root `CHANGELOG.md`
until it was retired: it had accumulated thousands of lines under a single
`## [Unreleased]` heading while `0.1.0`, `0.1.1` and `0.1.2` had all shipped, so
it asserted that nearly all of its own content was unreleased. See decision 15
in `DECISIONS.md`, which records the measurement and the command that reproduces
it.

**Finding the release that contains an entry.** No release attribution is
written into this file, because a hand-written one goes stale at the next
release — the exact defect that retired it. Derive it instead:

```sh
# which commit last touched the lines of an entry
git blame -L <start>,<end> -- docs/ENGINEERING-LOG.md

# which releases contain that commit (earliest one shipped it)
git tag --contains <commit> | grep '/v'
```

Position is not a reliable guide to age: entries have been inserted under
pre-existing category headings rather than strictly prepended, so a section can
hold entries from several releases. Use the commands.

## Entries

<!-- U-501 addendum — acdp-registry-rs#336, adopting acdp-rs's Proven/commit_proven split -->

### U-501 addendum — the did:web gap closes, and a plan's acceptance criterion doesn't survive contact with the shipped API

#242 closed two of four publish branches by hand in September: `publish_identity_proven_offline`
(a hand-rolled hash+signature duplicate) let did:key charge a late failure, and
`enforce_pinned_signature` did the same for the playground-pinned branch. The did:web production
branch was the one gap left — establishing identity from the registry side would have meant a
second DID-document resolution per publish, a second SSRF surface, and a cache that could disagree
with the SDK's. The design this repo wanted from the SDK instead is written down in
`plans/cross-repo/acdp-rs-publish-charge-seam.md` and was filed upstream.

`acdp-rs` shipped it in v0.14.0: `Proven<'a>` / `prove_publish_identity` /
`prove_publish_identity_did_key` / `prove_publish_identity_pinned` / `commit_proven` (acdp-rs#273).
Issue #336 tracked adopting it. The did:web branch now proves identity via the SDK's real DID
resolution, arms the charge, then commits — no new I/O, matching the design ask exactly. did:key
and pinned were rewired onto the same prove/commit shape, which deletes the hand-rolled did:key
duplicate: the SDK's own `publish_verified_did_key_in_tenant` is now *defined* as
`prove_publish_identity_did_key` + `commit_proven`, so there is nothing left for a separate
in-repo check to duplicate.

**The part worth recording is what happened to one line in the adoption plan, not the adoption
itself.** The plan — written and reviewed before implementation — specified, as its most heavily
flagged acceptance criterion, that a failed did:key/pinned proof must NOT be rejected: it had to
reproduce the deleted oracle's "false = don't charge, never reject" contract, staying an accepted,
uncharged publish. Implementing that literally turned out to require falling back to committing
genuinely unverified content on a failed proof — the only 0.14.0 path that could satisfy the
letter of the criterion admits exactly and only forged, signature-invalid did:key publishes. That
is not preserving behavior; it is a new hole.

The premise turned out to already be false one version earlier than the plan assumed. The "real"
SDK call the old oracle's `false` used to fall through to ran a strict superset of the oracle's own
checks even in acdp-server 0.13.1 — so nothing the oracle ever rejected was genuinely being
accepted downstream. Two tests already in this repo's suite had been pinning *rejection*, not
acceptance, for these exact cases all along
(`naming_a_victim_does_not_spend_their_budget`, `a_replayed_envelope_over_a_different_body_does_not_spend_the_budget`)
— the record contradicted the plan's premise before the plan was ever read against it.

A written, human-reviewed design document was wrong about a security-relevant behavior, and the
way that surfaced was a fresh agent re-deriving the claim from source rather than trusting the
document's framing — the same discipline `/reconcile` applies to this repo's own `UNCONFIRMED`
entries, run here against another repo's plan instead. The fix: reject on a failed proof for
did:key/pinned too, matching did:web. `DECISIONS.md`'s "U-501 addendum" entry carries the full
analysis; this file just marks that a plan being reviewed once does not make it correct forever,
and does not exempt an adoption from checking it against the code that actually shipped.

One smaller, real side effect rides along: because the SDK's prove functions bundle schema
validation into the identity proof (the deleted oracle didn't), the charge-arm point for did:key/
pinned moves slightly later. A validly-signed but schema-invalid publish — which the old oracle
deliberately charged, on the theory that a noisy-but-signed producer should still pay — is now an
uncharged rejection instead. Judged benign (every such rejection now fails before the one
expensive step, the signature verify) and recorded rather than left for a future reader to
rediscover as a mystery.

<!-- unit U-561 (lane-2) — the open entries that were not open, and the count that was three different numbers -->

### U-561 — `ASSUMPTIONS.md`'s open entries, and what counting them cost

U-507 reconciled this file's open entries in September. U-561 asked a narrower question: of the
declarations still open, **which name an event that has since happened?** An entry like that is
stale in the worst way — it reads as an open question that was in fact answered, so the record
asserts more doubt than the evidence supports.

**Answer: one of seventeen.** U-556's `NEEDS-CHANGE` ("child stdout is not drained during the
probe") specified a structural fix — file-backed stdio for the spawned child — and that fix landed
in U-560 (#327). The resolution was already appended to the file; the *status line* 110 lines above
it still read `NEEDS-CHANGE` with nothing pointing forward. Settled, with the evidence.

The other sixteen are correctly open. Their defects are **citation decay and missing triggers**,
not wrong status — which is a different unit's worth of work from what the backlog implied.

**The expensive part was the count, and it is the part worth recording.** Four numbers were
produced for "how many open declarations are there", and the first three were all wrong:

| count | method | verdict |
|---|---|---|
| 37 | `grep -c UNCONFIRMED` | **28 of the 37 are prose** |
| 9 | anchored `^- \*\*Status:\*\* UNCONFIRMED` | **2.9x undercount** |
| 26 | first corrected extractor | **overcount** — a 400-char window bled into the next line, and `**UNCONFIRMED → CONFIRMED.**` *resolution headings* read as open |
| **17** | transition-aware extractor, self-tested against 9 known-closed and 17 known-open fixtures before its number was quoted | holds; all 31 unflagged token-bearing lines then read individually |

The 9 failed for a reason **this repository had already written down**: U-507's own census records
that statuses here appear in six shapes, including thirteen times as a bullet or heading label with
no `Status` word at all. The pattern that produced 9 was derived from the entries its author had
written personally — which is precisely the shape that misses what somebody else wrote. *Search the
record before writing the sweep; it may already contain the taxonomy, better.*

**Two near-misses would each have closed an entry wrongly**, and both were caught only by refusing
to take a grep's word:

- *"there is no `ETag` anywhere"* — `git grep -il etag` **does** return a file. It is a **comment**
  reading "the 404 carries no ETag", which **corroborates** the entry it appears to refute.
- *"the conformance fixtures do not cover the RFC-ACDP-0014 §4 reject path at all"* — conformance
  now cites "RFC-ACDP-0014 §4/§5" in several places, but `rev-001` is a single-vector **ACCEPT**
  golden for §5 step 2. Same section label, different path.

**One entry named nothing that could ever close it** — no `Settled by:`, no owner, no condition. An
open status with no trigger is open by construction rather than by evidence, which is the same
defect as a fired trigger pointed the other way. It now carries a falsifiable one.

### Citation decay, including this file's own

Line numbers into a **moving file** decay; a line number citing **one named commit's diff** does
not, because that diff is immutable. Cite a symbol or a quoted string for live code.

The section above titled *"Two references this unit made stale, outside its path grant"* — a unit
recording references it had invalidated — now has that exact problem itself:

- `ASSUMPTIONS.md:2872` ("entry 9, `mutants.yml` derives the spec pin") → the heading is at
  **`:2881`**; `:2872` is unrelated prose about JSON whitespace.
- `ASSUMPTIONS.md:315` (`bump-spec-ref.yml` requesting `permission-workflows`) → the claim is at
  **`:313-314`**; `:315` lands in the right entry on the wrong sentence.

Both corrected by **appending**, not by editing another unit's lines. Two more were found inside
`ASSUMPTIONS.md` itself and are corrected there.

### What this unit does not fix

Not an exhaustive list. `W2-U3` asserts `Status: CONFIRMED` and, on the next line, *"PARTIALLY
narrowed, still UNCONFIRMED"* — both its own words. Left for that entry's owner to resolve rather
than settled from outside, and recorded so it is not mistaken for closed. Eleven entries remain
gated on a human ruling, an operator observation, a coordinator decision, or the scheduling of
another unit; one of those waits on a settings decision that is itself blocked on a credential
nobody has granted, so it cannot fire.

<!-- unit U-560 (lane-2) — a test that asserted a manifest spelling, and a diagnostic nobody could reach -->

### U-560 — asserting the resolved property, not the string that requests it

Two independent defects in `crates/acdp-registry-server/tests/tls_startup.rs`, shipped
together because they are the same mistake in two registers: a check that reads a
*request* and reports it as a *result*.

**1. `rustls_is_a_normal_dependency` scanned a manifest and claimed a feature set.**

It section-scanned this crate's `Cargo.toml`, asserted the `rustls` line contains
`"ring"` and does not contain `aws-lc-rs`, under the message "requesting both providers
is the original defect: rustls cannot choose and panics at startup". It passed — while
the crate resolved **both** providers:

```
rustls v0.23.45 [aws-lc-rs,aws_lc_rs,default,log,logging,
                 prefer-post-quantum,ring,std,tls12]
```

A manifest line cannot show a resolved feature set. `aws_lc_rs` has two independent
enablers and `ring` has five; exactly one of those seven edges appears on the line the
scan reads, and the edge that decides the question arrives as an **absence** — the
missing `default-features = false`. No amount of care with the string fixes that, because
the string is not where the answer lives.

The replacement asserts the consequence this process *can* observe: with rustls unable to
select a provider from crate features, `ClientConfig::builder()` panics, which is exactly
why `install_crypto_provider()` (`main.rs:99-108`) exists. It pins the panic **message**,
not merely `is_err()`, because `builder` has two documented panics and an `is_err()` check
would pass on the wrong one.

**What that panic proves is stated no wider than it is.** `from_crate_features()` returns
`None` for *three* feature states (`rustls-0.23.45/src/crypto/mod.rs:259-263`): two
providers, no providers, or `custom-provider`. So the asserted invariant is the one all
three share — rustls cannot pick a provider unaided, so `main` must install one. A
compile-time reference to `rustls::crypto::ring::default_provider` rules out the
"no providers" reading; `custom-provider` is left unruled-out and the comment says so
rather than implying coverage.

**The manifest scan was kept** — but not, as a first draft of this entry said, because
normal-vs-dev-only is otherwise unobservable. That is wrong: since U-530 `main.rs:100`
names `rustls::` in the **bin**, and Cargo does not expose dev-dependencies to bins, so
moving the line fails the binary's own compile with `E0433` and produces **zero** test
results. The dev-only branch of the scan can never print.

What earns those lines is the separate `ring` assertion, which **is** live and which no
compile gate reaches: drop `features = ["ring"]` and the bin still compiles, because `ring`
resolves anyway through `reqwest`/`hyper-rustls`/`tokio-rustls`/`sqlx-core` unification —
while this test goes red. The crate must keep making its own *direct* request for its
recorded provider choice rather than inheriting it by accident of the graph.

This assertion is an invariant rather than a snapshot because the two-provider state is
now permanent by a human decision of 2026-09-19 — recorded on the lane board as U-563,
WONTFIX — that both providers stay, so `install_default` stays load-bearing. **That id
resolves nowhere in this repository**, and by this unit's own rule (a self-referential
citation is not a citation) it cannot be the record. This paragraph is therefore the
record: the change that would leave one provider is switching `axum-server` to
`tls-rustls-no-provider` in the root `Cargo.toml` *and* adding `default-features = false`
to this crate's `rustls` line — both, since each enabler is independently sufficient — and
the decision was not to.

**Why the decision went that way**, since `tls_startup.rs:16` points here for the
reasoning rather than re-deriving it. This crate wants `ring` specifically
(`crates/acdp-registry-server/Cargo.toml:41-43` — `aws-lc-rs` drags `aws-lc-sys` and
`prebuilt-nasm`, a C/asm build dependency, into the shipped binary's TLS path). But rustls
declares `prefer-post-quantum = ["aws_lc_rs"]`, so a `ring`-only build gives up
post-quantum hybrid key exchange. Post-quantum won, and both providers stay. Note what is
*not* part of that trade: **TLS 1.2 is provider-independent** (`tls12 = []`), and an
`aws_lc_rs`-only build would be both single-provider and post-quantum — the constraint is
that this crate wants `ring`, not that no single-provider form keeps post-quantum. Both
were asserted the other way earlier in this unit and corrected against the manifests.

**2. The larger half: the diagnostic a reader actually meets carried nothing.**

The child's stdout/stderr were captured only on the early-exit branch. Every other
failure — the common case — reported a probe error with none of the registry's own
output.

Those two paths are not the alternatives they appear to be, and the control that proved
it also explains why nobody noticed. The retry loop breaks on a **non-retryable** probe
error, and a timeout is non-retryable (`ProbeError::io`:
`retryable: stage.io_failure_may_be_transient() && !timed_out`). So when another process
already holds the port *without accepting*, the first probe times out in the handshake,
the loop breaks before its next `try_wait`, and the early-exit branch never runs **even
though the child has already exited**. Measured with a listener that binds 18443 without
accepting: the child exits in 0.06 s with `Address already in use`, and the test lands on
the probe assertion, three runs of three.

The qualifier is load-bearing and a first draft of this entry omitted it. A holder that
*accepts* and closes produces `UnexpectedEof`, which **is** retryable, so the loop
survives to its next `try_wait` and the early-exit branch fires normally — verified with
the same harness in `accept-close` mode. So the branch that is skipped turns on whether
the holder accepts, not on "port in use" as a cause; the likelier real holder is a server,
which accepts. The narrower true statement is still what justifies the change: a probe
failure can arrive with the child already dead and its output unread.

The child now writes to files in the tempdir the test already owns, and the probe-failure
assertion reads them. Output entering a panic message is capped at the **first** 64 KiB
per stream with the cut and both byte counts disclosed — the head, because this is a
*startup* diagnostic and the signal is the first thing the binary said. There is no
"full log at `<path>`" pointer: the tempdir is deleted when the test returns, so it would
dangle by the time anyone followed it.

**A thing the control showed in passing, still unfixed.** The child logs
`{"message":"listening","addr":"127.0.0.1:18443"}` and *then* fails to bind with
`Address already in use`. The log line asserts something that is not true yet. The
install/`listening` ordering this module's docstring describes is still unasserted by any
test.

**3. The measurements, and which are ours.**

Inherited from U-556 and **not** re-verified here: the ~40 KB-at-default-filter figure
and the ~20 KB-per-probe figure — and the second of those is *superseded* for this test's
own client by the ~11.1 KB below, rather than merely carried forward.

Measured for this unit on 2026-09-19 (debug binary, TLS on, sqlite, `RUST_LOG=trace`):
successful TLS startup **~35.6 KB**; startup plus one probe by this test's own rustls
client **46,708 / 46,710 / 46,743 B**; per-request cost constant to within two bytes
across eight requests; a forced early exit **~200 B**, all of it stderr — indicative
only, because that line embeds the tempdir path and moves with its length (196 B and
237 B measured for a short and a long path), so it is not a constant to rely on.

The **64 KiB pipe ceiling** belongs in neither list as first written. U-556 measured it
(65,531 / 65,536 B) and this unit re-measured it independently, observing the unread pipe
stop at exactly **65,536 B**. An earlier version of this section listed it as inherited-
and-not-re-verified four lines above quoting this unit's own fresh measurement of the
same quantity — a contradiction in the paragraph whose entire job is attribution.

So the pipe arrangement this unit replaced had room, after startup, for **two** probes by
this test's client, and the **third** wedges — the child stops responding without exiting
and the caller hits its 5 s read timeout. The test issues one request, because only
`Connect`/`Handshake` probe failures retry and neither issues HTTP. The margin was one
spare request: a real bound, held in place by a `matches!` arm in a different function
that told nobody relaxing it what it cost. Files remove the dependence rather than
document it.

**Any request-count figure of this kind must name the client it describes.** The count is
a property of the client, not of the server. A heavier OpenSSL client costs ~16.1 KB per
request against the real probe's ~11.1 KB and wedges one request earlier. Three
instruments in this unit gave three different answers — three, two, and one — before that
was noticed; each had measured a different client, and none of the claims had named one.
An earlier draft of this entry's source material also asserted the child had written
96,573 B "by the moment a single probe returned", past the ceiling, and asked why a
pipe-bound child keeps serving past it. That figure was never a one-probe figure: one
probe leaves the child at ~46.7 KB, **under** the ceiling. The question had no referent
and is withdrawn; the retraction is recorded in `ASSUMPTIONS.md`.

The superseded "~16 KiB pipe buffer" premise belongs to `ASSUMPTIONS.md`, not to this
log — U-556's entry here never made that claim — and it had already been self-corrected
in place by U-556's own reconcile.

**A note for two other entries in this file.** This unit rewrote `tls_startup.rs` heavily,
moving `let dir = tempfile::tempdir()` from `:145` to `:316`. The U-542 entry below and
`DECISIONS.md:3628` both cite `tls_startup.rs:145` as the model for the owned-`TempDir`
pattern; both are still right in substance and now want `:316`. Recorded here because this
file is append-only and those lines cannot be corrected in place.

**4. What this unit does not fix.** Not an exhaustive list — in a unit about completeness
claims, a closed numbered list under this heading would be one. These are the ones known
at merge:

- The install/`listening` ordering remains unasserted, as above.
- Port `18443` is still hard-coded, and is now load-bearing for two falsifications rather
  than one. A parallel run against a busy 18443 fails in a way that now, at least,
  explains itself.
- Both crypto providers remain, by the human decision recorded above, so
  `install_crypto_provider()` stays load-bearing and the phase-1 assertion stays true.
- **The branches this unit added have no in-suite test.** `ChildStream::Truncated`,
  `::Unreadable` and the cap arithmetic were each falsified out-of-tree and the edits
  reverted, so nothing in CI executes them. That widens the standing limit U-556's own
  entry records ("several error branches have no test") rather than closing it.

<!-- unit U-556 (lane-2) — the TLS startup test that never spoke TLS -->

### U-556 — `tls_startup` asserted TLS and observed TCP

`tls_startup_installs_a_provider_and_serves` spawns the real binary with
`registry.tls.enabled = true`. It is the only executable guard on the crypto-provider
defect its own module docstring describes. Its readiness loop broke on
`TcpStream::connect(("127.0.0.1", port)).await.is_ok()` while its assertion read
*"the registry never accepted a TLS connection"*. The observation was TCP; the claim was
TLS. **Anything that bound the port satisfied it** — so a rustls handshake regression, of
exactly the class the `0.23.41 → 0.23.45` bump for RUSTSEC-2026-0285 had just made, would
have left it green.

This was a **wrong-assertion** bug as much as a missing-capability one. Adding a handshake
without correcting the message would have left the same false claim in place with a longer
test behind it, so an acceptance criterion was written for each half — and that mattered,
because the first draft of the criteria all passed with the false message untouched.

**The probe replaces the TCP connect *inside* the poll loop, and the placement is
load-bearing.** The first design put the handshake after the loop; that is unsatisfiable.
The handshake must run before `child.kill()`, while the assertion that reports a
never-ready server runs after it — no position after the loop satisfies both. Guarding it
with `if served { … }` would have introduced the runtime skip branch the docstring already
forbids. Inside the loop all three constraints hold at once, the early-exit / exit-101
diagnostics stay byte-identical *and* live during the TLS attempts, and the loop's success
condition finally means what the assertion's message says.

**The self-signed certificate is pinned, not waved through.** `rcgen`'s
`generate_simple_self_signed` emits `IsCa::NoCa` and therefore no basicConstraints
extension at all, so webpki accepts the leaf as its own trust anchor with full chain and
hostname verification on.

**Upstream documents this exact usage as unsupported, and that is a standing limit, not a
footnote.** `rustls-webpki-0.103.15/src/trust_anchor.rs:21-23` documents
`anchor_from_trusted_cert` — the function `RootCertStore::add` calls — as one that *"should
not be used to treat an end-entity certificate as a `TrustAnchor` in an effort to validate
the same end-entity certificate during path building. Webpki has no support for self-signed
certificates."* That is this case, named. It nevertheless works, because `IsCa::NoCa` emits
no basicConstraints and so never reaches the `CaUsedAsEndEntity` arm — settled by running it
rather than by believing the comment or its absence. But the pinning therefore rests on
behaviour upstream explicitly declines to promise: **a webpki change could break this test
without breaking the registry**, and the fix then is an rcgen CA+leaf chain with the CA
pinned, as `tests/didweb/mod.rs:62-100` already builds.

**A correction worth recording, because the first draft of this entry got it backwards.**
That draft claimed the pinning makes a stale registry child squatting on the hard-coded port
`18443` "fail closed because it serves a different certificate". Measured — `openssl s_server`
on 18443 with an unrelated cert, then the test — the run *does* fail closed, but at
`tls_startup.rs:224`: the **pre-existing, byte-identical** early-exit branch, reporting
`exit status: 1, which is NOT the 101 this test exists for`, with `Address already in use
(os error 48)` on the child's stderr. (That branch names config, storage
init and port-in-use as *candidate* causes; it does not diagnose which, and that
non-commitment is deliberate — it is the lesson recorded at `tls_startup.rs:209-214`.)
The certificate never indicts anything. What U-556 actually changes is that the
handshake fails *retryably*, so the loop no longer breaks early on a false positive and the
already-existing `try_wait` branch gets another iteration to catch the child's real exit —
where the old bare TCP connect set `served = true` on iteration 1 and went green. The
conclusion held; the mechanism was invented. It is recorded here because a plausible
mechanism attached to a true conclusion is the hardest kind of wrong claim to notice.

**`ClientConfig::builder()` cannot be used here, and the reason is the defect itself.**
This binary's graph enables both `ring` and `aws_lc_rs` — via two independent direct edges,
not one: `crates/acdp-registry-server/Cargo.toml` requests `features = ["ring"]` without
`default-features = false` while rustls' own `default` includes `aws_lc_rs`, *and*
`axum-server`'s `tls-rustls` enables `rustls/aws-lc-rs`. So the convenience constructor
panics with the very "could not automatically determine the process-level CryptoProvider"
message this test file exists because of. `builder_with_provider` is mandatory.

**Every timeout is on the std socket inside the blocking closure, and `tokio::time::timeout`
is not a substitute.** It abandons a blocking task rather than cancelling it, and dropping
the test's runtime waits for blocking tasks that already started. Without socket-level
timeouts a listener that accepts and never speaks TLS would hang the test **forever** —
the loop stalling on iteration 1, never reaching `child.kill()`, leaking the child, and
burning the `tests` job's 45-minute cap with no diagnostic. Note the direction of that: the
naive fix would have converted a silent false *pass* into a silent 45-minute *hang*. The
bound was verified by standing up a listener that accepts then sleeps 600s — 5.003s,
correctly classified non-retryable.

**Falsification.** A test that cannot be made to fail has been lengthened, not widened. Six
breaks were applied one at a time, each isolating a single assertion, because an earlier
assertion that fails first masks every later one. Each was applied as a temporary local edit,
run, and reverted at phase 1's verification gate and re-run independently by that gate — so
**nothing in the tree evidences them**, which is exactly why they are transcribed here:

| Break | Result | Assertion proved live |
|---|---|---|
| Pin an unrelated self-signed cert | `[handshake] invalid peer certificate: BadSignature (kind: InvalidData)` | certificate pinning |
| Client restricted to TLS 1.2 | handshake **succeeds**, `Some(TLSv1_2)` vs `Some(TLSv1_3)` | the TLS-1.3 assertion |
| Request a 404 path | `404` vs `200` | the status assertion |
| Request a 200 path returning other JSON | `Null` vs `"ok"` | the body-content assertion |
| Offer only `h2` in ALPN | `Some("h2")` vs `Some("http/1.1")` | the ALPN assertion |
| Force a `config`-stage failure | `[config] "not a valid name" is not a valid server name: invalid dns name` | the message blames the test, not the registry |

The TLS-1.2 break is the one that carries the weight: the handshake **succeeds**, so it is
the only break that exercises the version assertion at all. A falsification that merely
severed the connection would have gone red without ever touching it — a probe that reads
nothing you varied is decorative.

**The one shipped property with no in-tree probe, now measured.** The no-drain decision rests
entirely on `Exchange`-stage failures being non-retryable — retrying the whole exchange would
re-issue a real HTTP request every 100ms for up to 50 iterations. Nothing in the suite exercises
that. Probed by forcing the response parse to fail: the run reports `[exchange] no header/body
separator …` and finishes in **0.93s**, against **5.32s** for the retryable handshake break in
the table above, which burns all 50 iterations. The ~5x gap is the discriminating evidence; a
retryable `Exchange` would have matched the slow number. Still not an in-tree test — the
branches listed under the limits below remain unprobed.

**The failure message was, briefly, this unit's own defect.** The first cut read *"the
registry never completed a TLS handshake"* — while firing for `config`, `exchange` and
`task` failures too, i.e. blaming the registry for client-side bugs in the test. That is a
fixed explanation pre-diagnosing every other failure, which is exactly what the existing
exit-101 branch at the top of the same loop was written to avoid, and which the plan had
warned about in as many words. It was caught by the phase verifier, not by its author. The
shipped message names the failing stage and says which stages indict which party.

**What this does NOT now cover — read this before assuming the file is settled.**

- **Ordering is still unasserted.** Neither test asserts that the provider install happens
  *before* the `listening` log. That limit and its reasoning are unchanged by this unit;
  `main.rs:1049` still logs `listening` ahead of either bind branch. A reader seeing "TLS is
  really tested now" must not infer the ordering property came with it.
- **`rustls_is_a_normal_dependency` is hollow, and is not fixed here.** It asserts the
  manifest's `rustls` line does not contain `aws-lc-rs`, under the message *"requesting both
  providers is the original defect"* — and passes, **while the crate resolves both providers
  anyway** through default features, as measured above. It greps a literal string; the
  property it claims to guard is the resolved feature set, which that string cannot see. It
  is the same defect class as the one this entry is about, in the same file, and it needs its
  own unit and its own falsification rather than being folded in here.
- **Port `18443` remains hard-coded**, a latent hazard under any future CI parallelism — and
  per the correction above, the diagnostic a reader gets when it collides is the *port-in-use*
  branch, not anything about TLS.
- **The module docstring at `tls_startup.rs:6-9` still carries the superseded account** of why
  two providers are enabled (it names `axum-server` + `reqwest`; the measured pair is
  `crates/acdp-registry-server/Cargo.toml:47`'s inherited default features + `axum-server`).
  Both edges named there are real, but they are not the same pair, so a reader who opens the
  file gets the older story. Left unedited **not** for scope reasons — that docstring is in
  phase 1's only in-scope file — but because the unit pre-registered a criterion confining its
  changed hunks to three pre-image ranges, and the docstring is at lines 6-9. Editing it would
  have broken that criterion and shifted the line-absolute `sed` range guarding the early-exit
  block. Correcting it is a one-line follow-up for whoever takes the sibling-test unit. (This
  bullet first gave "path scope" as the reason, which was simply wrong — the same
  right-conclusion-invented-reason shape corrected five paragraphs above, committed twice in
  one entry.)
- **Several error branches in the new probe have no test and no falsification**: the 64 KiB
  overflow guard (unreachable in practice — `/livez` is ~50 bytes), the `JoinError`/panic arm,
  the missing-separator and unparseable-status-line arms, and the timeout-to-non-retryable
  classification (measured out of tree at 5.003s, not in the suite). Their failure mode is a
  confusing message, never a false pass, which is why they were accepted unprobed — but they
  are unprobed.
- **A handshake failure carries no output from the child process.** stdout/stderr are captured
  only on the early-exit branch (`tls_startup.rs:206-208`), which is disjoint from the
  handshake path. The new message's stage tag says *which* step failed; it cannot say what the
  registry was logging while it failed.
  *(Fixed by U-560 — see its entry above. The probe-failure message now carries both streams.
  U-560 also withdrew the reading that the stage tag identifies a culprit: under a foreign
  process holding the port the stage is `handshake` and the registry is innocent.)*

<!-- unit U-557 (lane-3) — clearing the yanked crates and making the ratchet enforce -->

### U-557 — the two yanked crates, cleared; `yanked` flipped to `deny`

`deny.toml` had carried `yanked = "warn"` with a comment explaining that this was
**not** laziness: the stricter setting had been measured, and it failed, because the
workspace depended on two yanked crates. That comment also pre-specified the fix and
its ordering — *"Flip this to `deny` in the same change that clears `spin` and `wnaf`,
not before — flipping first would just redden the build for everyone."* This entry
records executing exactly that, and the two things that turned out to be more
interesting than the version numbers.

**The clearing was the easy half.** Both crates had non-yanked in-range successors, so
`cargo update --package spin` (0.9.8 → 0.9.9) and `--package wnaf` (0.14.0 → 0.14.1)
sufficed. No manifest constraint widened; no transitive dependency pinned either crate
to the yanked version. Each was run separately so its lockfile hunk stays attributable.

**`spin`'s yank was not a security event, and saying so matters.** Every yanked version —
all 16 of them — falls inside the band **0.7.0 to 0.12.1**, which holds 21 versions. The
five live ones inside that band (`0.7.2`, `0.8.1`, `0.9.9`, `0.10.1`, `0.11.1`) are
replacements published out of it, and a sixth, `0.12.2`, was published just above it the
same day. All six landed on **2026-07-13, from the same owner** (`zesterer`); a seventh,
`0.12.3`, followed on 2026-08-17. Everything at or below 0.6.0 is untouched and still live.
Six release lines republished in a single day is a maintainer-wide re-release. No RUSTSEC advisory applies to 0.9.9
(`RUSTSEC-2023-0031` is `patched = [">= 0.9.8"]`, `RUSTSEC-2019-0031` is withdrawn,
`RUSTSEC-2019-0013` covers `< 0.5.2`). Recorded because "two yanked crates" reads as two
vulnerabilities, and one of them was a publishing decision.

**`wnaf 0.14.1` is not a cosmetic patch, which is the part worth remembering.**
It takes a **new non-optional dependency on `primefield`**. So the lockfile diff is not
the two `version =` lines plus checksums it looks like it should be — it also gains a
`"primefield",` line inside `wnaf`'s own `dependencies` list. No new `[[package]]` block
appears, because `primefield 0.14.0` was already locked, already pulled in by
`primeorder`; that is why `cargo` reported "Locking 1 package" rather than two. License
and advisory surface are unchanged, but real code moved, and it moved on the ECDSA P-256
scalar-multiplication path (`wnaf` ← `primeorder` ← `p256` ← `acdp-crypto`) — which is
why this change was gated on the workspace test run rather than on the lockfile diff
looking small.

This was caught by the plan's own review round, not by implementation. The acceptance
criterion as first drafted asserted the diff "touches exactly two `version =` lines plus
their `checksum =` lines" — a *correct* implementation would have failed it. A criterion
precise enough to be falsifiable is also precise enough to be falsely specified, and the
review round is what separates the two.

**The verification trap, and the general shape of it.** `cargo deny check` exits 0 and
prints `advisories ok, bans ok, licenses ok, sources ok` on the *un-bumped* tree under
`yanked = "warn"` — character for character what it prints on the fixed tree. Pasting
that summary as proof of the flip would have proved nothing: a reviewer holding only the
diff and that output could not tell a post-flip run from a pre-flip one. The
discriminating assertion is the **absence** of the diagnostic, not the presence of the
summary: `grep -c 'yanked'` over the captured run, which was **4** before (two
`warning[yanked]` diagnostics, each with a `yanked version` annotation line) and is
**0** after. *A summary line that is identical in the passing and failing cases is not
evidence, however green it looks.*

**What this now costs, and it is deliberately a cost.** On 2026-09-16 `cargo-deny`
became a **required** status check on `main`. Before that, this flip would have made a
job fail without letting it block — a true signal with no teeth, which is the gap the
previous entry on this subject was really describing. Now both halves are installed, and
the consequence is that an upstream maintainer yanking any crate in the graph will block
every merge in this repo, on a commit that changed nothing. That is the intended ratchet.
The rewritten comment block in `deny.toml` is written for whoever meets it in that state:
it names the one-line fix, and it argues against the shortcut of adding an `ignore` entry
to get unblocked — which is now the path of least resistance, and which would convert a
solvable lockfile problem into a permanent silent exemption.

The superseded entry below (`yanked` stays `warn` "because `deny` fails today") is left
standing. It was true when written, it named the condition for its own retirement, and
that condition has now been met — editing it would destroy the record of a decision that
was made correctly with the information available.

<!-- unit U-542 (lane-3) — the sqlite sidecar leak: owning the file is not owning the directory -->

### U-542 — 89.3 GiB of orphaned SQLite sidecars, and the shape that caused it

`tempfile::NamedTempFile` deletes exactly the path it owns. SQLite creates `-wal` and `-shm`
**beside** that path, so every file-backed test orphaned two files. One `$TMPDIR` held **312,898
`acdp-*` sqlite files / 89.3 GiB**: 143,466 `-wal` + 143,428 `-shm` against **14** surviving bare
`.sqlite`. Fourteen parents against 286,894 orphans is the whole diagnosis — the guard was working
on one of the three files it needed to.

**Fix:** own the directory. `Harness.db` is now a `tempfile::TempDir` containing `registry.sqlite`,
so the drop removes everything SQLite put in the tree — including whatever a future SQLite version
decides to add. `tls_startup.rs:145` already had this shape; it was the model.

**How it was found, which is the transferable part.** The disk decline that led here was chased for
hours across three sessions and every candidate was eliminated correctly — Colima's images (flat,
by allocated size and mtime), swap (no new swapfile), the lane worktrees. The consumer was in
`/private/var/folders`, and **every search had been rooted at `$HOME` or the workspace**, neither of
which contains it. Three empty results in a row read as "we have looked everywhere" when they meant
"we have looked in the same place three times". An empty search result is only as wide as its root;
publish the roots beside the negative.

**Two measurement notes worth keeping.**
- `du` reported 117,162 MB for that directory and was **distrusted** on the strength of a real prior
  incident where it over-reported a 208k-entry directory by 20x. An independent `stat -f '%b'` sum
  over all 347,827 loose files returned 114,355 MB against a children-sum of 3,211 MB — **`du` was
  right and the suspicion was wrong**. A prior that survives contact with a control is worth more
  than one quietly dropped.
- Prefix counting needs care: `find -name 'acdp-pin-*'` reports 8,294 because it substring-matches
  `acdp-pin-cell-`; the true figure is 7,924. Deriving the prefix list *from the filesystem* rather
  than from a hand-written table is also what surfaced `acdp-rev-e2e-`, which the hand list missed.

**The guard, and why it is not a `$TMPDIR` count.** `tests/tmpdir_hygiene.rs` builds a file-backed
harness, drops it, and asserts `== 0` of `{db}`, `{db}-wal`, `{db}-shm` remain — an equality, since
a `<=` bound would be satisfied by the very leak it exists to catch. It is scoped to one harness's
own paths because several test binaries share one `$TMPDIR` on a developer machine, and a
process-wide count would fail randomly, which is how a guard gets ignored.

**It was demonstrated in both directions on the same binary**, which is the acceptance evidence:
RED before the fix, naming `acdp-test-lI6bJ5.sqlite-{wal,shm}`; GREEN after. The whole-`$TMPDIR`
criterion was additionally satisfied as a measurement — 260 tests produced a delta of **0**, against
a pre-fix control of 1 test producing **+2**. A delta of zero means nothing without that control:
it is also what you would see if no file-backed test had run.

**Reclaiming the 89.3 GiB was deliberately left out of scope.** `/private/var/folders` is shared
machine state, a live test may hold an open WAL, and a 287k-file removal is an explicit human
decision. Fixing first is what makes reclaiming worth doing at all — space returned to a suite that
recreates it buys hours, not a solution.

**The latent class.** Any `NamedTempFile` whose path is handed to something that may write siblings
— SQLite, a lockfile, a `.journal` — has this defect waiting. Owning the directory is the general
answer.


<!-- unit U-507 (lane-3) — reconciling ASSUMPTIONS.md's open entries; the census, and what it caught -->

### U-507 — `ASSUMPTIONS.md`'s open entries, reconciled against the tree

`ASSUMPTIONS.md` is tracked, cumulative, and had grown past 3000 lines. Its open entries were being
counted three different ways, none of them right, and several had been silently resolved by work that
shipped weeks or hours earlier. This unit censused them, resolved what the tree could settle, and left
the rest open with an owner.

**The count was the first problem, and every available number was wrong.**

| figure | what it actually counted |
|---|---|
| 50 | `grep -c UNCONFIRMED` — includes prose narrating a *past* status |
| 28 | an `^`-anchored `**Status:**` pattern — misses five other live shapes |
| 35 | a careful hand count, scoped to "deferred items", predating 8 later entries |
| **37 items / 46 declarations** | every declaration, classified |

The 28 is the interesting failure: it undercounts by the same mechanism that makes 50 overcount, one
boundary over. This file writes statuses mid-prose-line, with a parenthetical between key and colon,
with the **token wrapped onto the following line**, and — 13 times — as a **bullet or heading label
with no `Status` word at all** (`- **UNCONFIRMED — awaiting human ruling:**`). A line-anchored pattern
cannot see any of those, and returns a confident number rather than an error.

**The census tool ships, at `docs/assumptions-status-census.py`.** A figure whose predicate lives in a
scratch file is not reproducible, and the leader could not verify 32/32 precisely because no grep
censuses this file — lowercase status words collide with ordinary English here ("cannot be confirmed
after the fact"). The tool asserts its own partition is total, so an unseen sixth shape fails the run
instead of quietly lowering the count. It independently reproduces the earlier hand count of 35,
decomposed identically — two methods from opposite directions, same partition.

**Result.**

    open items at unit start              37
    fully resolved                        24
    deliberately still open (AC4/AC5)     13
    open declarations        46  ->  16   (30 flipped to a resolved token)
    status lines rewritten / deletions    51    verified AFTER the final merge

**The equality caught two items the unit had walked past** — one a *second* declaration inside an
entry whose first was already resolved. A floor (`>= 20 resolved`) would have passed with both still
open, which is the argument for stating an equality rather than a threshold.

**Three entries had already closed themselves.** The shared playground validator's placement (#192,
#193), "CI never exercised the shipped stack" (#270), and `/metrics`' cache posture (#218) were all
settled by other work landing, with nobody going back to flip them. That is the failure mode this unit
exists to correct, and it will recur unless entries are re-read when related work merges.

**The pattern worth copying: entries that specify their own closure signal.** `/metrics`' said
*"deleting that line is how the fix announces itself"*; A2's said it must not be called closed until a
named test was deliberately deleted. Both signals had already fired — the test was deleted in
`f8a866d`, the same commit that landed the predicate, established with `git log -S` on the test name
rather than from a changelog. An entry that says how its own resolution will be detectable is worth
more than one that merely records a status.

**What the unit declined to do.** Nothing was closed by inertia: two entries stay open because "nobody
objected" is not evidence, and one of them forbids that reasoning in its own text. Four entries that
belong to the human or the leader kept their open token and gained **Settled by** and **Owner** — split
rather than lumped, because this file has used "escalated" for both. One out-of-grant defect is
reported rather than repaired: `crates/acdp-registry-core/src/handlers/context.rs:1262-1277` cites a
deleted test as machine-checking a residue and still calls landed work a requirement.

**A method note, recorded because it recurred six times in one session.** Six sweeps in this unit
returned a plausible number from the wrong input rather than an error: a body searched without its
heading, a `docker-compose.yml` grepped at the repo root when the file is under `docker/`, and
`INSERT INTO contexts` matching `contexts_fts` as a prefix. Two of those would have produced false
findings against another lane's work. The pattern was correct every time; the input or the boundary
was not, and nothing failed loudly.

<!-- unit U-510 (lane-3) — CI now builds the feature configurations it checks; closes #265 -->

### Fixed

- **CI's feature-configuration steps type-checked but never linked, and five configurations were
  never built at all.** `cargo clippy` and `cargo check` do not run codegen or the linker, so a
  failure that appears only at those stages passed every one of them. Proven directly rather than
  argued from documentation: with `target/debug/acdp-registry` removed, a **green**
  `cargo clippy --all-targets` for a configuration leaves **no binary at all**, while `cargo build`
  for the same configuration produces one (61M). Reproduced on a second configuration. A green
  clippy cannot surface a link error because it never links.

- **The count in #265 was four; the enumeration is nine, and two of the corrections matter.**
  `ci.yml`'s `clippy` job runs nine feature-configuration checks — lane-1's four are the subset
  W3-U10 added for #200. Cross-referencing against every step that actually links:
  `storage-pg,playground`, `storage-memory,playground`, no-backend and `playground`-alone were
  linked by **nothing**; the other four were linked only *incidentally*, by `cargo test` steps whose
  feature lists happen to match, which any edit to those steps could have removed in silence — the
  same fragility this file already documents for #221. And **`storage-pg`, the configuration that
  ships, never had its binary linked in `ci.yml` either**: its test step passes
  `--test pg_integration`, and `acdp-registry-server` is bin-only, so that links a test binary
  rather than the server. It was covered solely by `docker.yml`'s image build
  (`STORAGE_FEATURE=storage-pg`) — a different workflow, incidentally, which a path filter there
  would have silently removed.

- **Five `cargo build --locked … --all-targets` steps, each paired with its clippy step** and
  carrying a byte-identical feature list, asserted programmatically rather than by eye. Additions,
  not replacements: clippy is stronger on lints, build on codegen and linking, and neither subsumes
  the other. Each shipped command line was extracted from the YAML and executed verbatim, so what
  was tested is what runs.

### Changed

- **The build steps live inside the existing `clippy` job, deliberately.** That job is one of the
  four contexts in branch protection's `required_status_checks`, so this coverage is merge-blocking
  the day it lands. A new job would have produced a check that is *not* required and therefore could
  not prevent a merge — the limitation U-508's `lint` gate hit, which is still awaiting a decision.
  This is **not** the mislabelling U-508 refused when it declined to put shell linting inside
  `rustfmt`: there the concern differed from the job's name, whereas building a feature
  configuration is the same concern this job already served nine times. The distinction is concern
  identity, not convenience. The job must not be renamed — those four names are a contract with
  branch protection, and a required context that stops reporting leaves every PR waiting on a check
  that never arrives.

- **The `msrv` job's two `cargo check` steps stay `check`, by decision.** Their purpose is that
  1.88 accepts the language and API surface, and both of their configurations are now linked at
  stable. Codegen divergence between 1.88 and stable for identical source is a materially narrower
  risk than an unlinked configuration. Recorded rather than silently skipped.

### Added

- **Cost, measured in CI rather than extrapolated from a laptop — and the laptop was wrong by 5x.**
  Locally, warm, the five builds totalled 21s and each was usually *cheaper* than the clippy step
  beside it, because clippy runs extra lint passes over the same graph. **In CI the `clippy` job went
  from 39s to 2m27s: +108s, not +21s.** The local figure was optimistic because that machine had
  already built every feature combination during the measurement pass, whereas the runner's cargo
  cache holds no artifacts for combinations this repo had never built.
  **CORRECTED BY U-513 — the sentence that stood here quoted a margin, and a margin was never
  supportable.** It said *"`tests` is 2m45s, so PR latency is still set by `tests` — with 18 seconds
  of headroom"*, and paired that with a trigger: *"if the `clippy` job ever exceeds `tests`, the split
  should be reconsidered."* Both are withdrawn. The 18s figure was **one sample of each job**, and the
  trigger had **already fired before the sentence was written** — run 34766261172 had clippy at 98s
  against tests at 86s. The falsifying datum was in hand.
  Measured properly, from GitHub-hosted runners with `Swatinem/rust-cache` and no local timings:

  | | n | range | median | spread |
  |---|---|---|---|---|
  | `clippy`, after this change | 5 | 98-176s | 144s | **78s** |
  | `tests`, same runs | 5 | 86-165s | 149s | **79s** |
  | `clippy`, before | 7 | 28-54s | 33s | 26s |
  | `tests`, before | 7 | 142-155s | 150s | 13s |

  Per-run margins were `+12, -18, -27, +26, -4` seconds (positive = clippy leads). **Every one is far
  smaller than either spread, and a margin smaller than the run-to-run spread is not a margin** — so
  no headroom figure belongs here in either direction.
  What the data *does* support is categorical rather than numeric: before this change `clippy` was
  never near the critical path (its slowest run, 54s, was under tests' fastest, 142s); after it the
  two distributions overlap and **`clippy` led in 2 of 5 runs**, one of them on `main`. So the change
  moved `clippy` from *never* the critical path to *sometimes* it. Derived expected cost, as the mean
  of `max(0, clippy - tests)` across those 5 runs: **~8s**, against a ~150s critical path.
  **The trigger fired and was weighed; it did not go unnoticed.** The per-PR/scheduled split is still
  declined, but on re-derived grounds, since the original reason ("it relieves a job that is not the
  bottleneck") is false in 2 of 5 runs. The two objections that do survive: a scheduled job makes the
  feature lists two sources of truth — this file already records that a written-out list goes stale
  silently and that it *already did once* — and it delays breakage detection by up to a day.
  **A strictly better third option exists and is recorded rather than taken:** move the five build
  steps to a separate job running *in parallel* instead of on a schedule. `clippy` returns to ~33s,
  the builds occupy ~111s of their own job, both sit under tests' median, and added latency is
  **zero** rather than ~8s — and it *moves* the feature lists rather than copying them, so neither
  surviving objection applies. It is not taken because a new job is a new check name, and branch
  protection's required contexts are enumerated, so the builds would stop blocking merges — the exact
  trade `lint.yml` is stuck in. Trading the blocking property for ~8s is the wrong way round. **That
  makes one pending settings decision the unblocker for two improvements.**

- **A finding that narrows #265's own risk claim, worth recording because it is easy to overstate
  the fix.** The classic undefined-symbol link failure is **unreachable from this repo's source**:
  `Cargo.toml` sets `unsafe_code = "forbid"`, and `forbid` cannot be overridden by `#[allow]`, so no
  `extern "C"` declaration can exist here. Two falsification attempts died on this and on a related
  point, and both failures are more informative than a success would have been: a `RUSTFLAGS`
  link-argument probe is **not** a valid discriminator, because `RUSTFLAGS` also reaches build
  scripts and proc-macros, which *are* linked — so clippy fails too. What the new steps therefore
  close is real but bounded: dependency and native link failures under a particular feature
  combination, environment-level link failures — this repo has an observed instance, the
  `SDKROOT`/`ld-1267` failure, which passes clippy and fails only at link — and
  post-monomorphization codegen errors in safe code.

<!-- unit U-508 (lane-3) — shellcheck + actionlint, and what the gate cannot do -->

### Added

- **`shellcheck` and `actionlint` now run on every pull request**, in
  `.github/workflows/lint.yml`. Neither had ever run in this repo, which mattered from the moment
  U-503 added its first shell script — 222 lines in the container publish path. The cost of the
  gap is on the record: that script's first draft silently dropped the last token of its input
  (`while read` returns false on a line with no trailing newline) and therefore *passed the release
  run it was written to reject*. It was caught by falsifying against real incident data, which is
  not a mechanism that repeats itself reliably. `shellcheck` catches that class mechanically.

- **A separate workflow rather than jobs in `ci.yml`, for a reason that is not tidiness.** The
  linters need no Rust toolchain and no cargo cache, so they share nothing with that file's matrix;
  `ci.yml` is ~500 lines that several people edit concurrently; and a distinct check name means a
  shell finding is reported as `lint` rather than arriving disguised as `rustfmt`. That last point
  is the same mislabelling argument U-503 used to justify keeping the release rebuild — a check, like
  an image, should not carry a name that misdescribes its contents.

- **Both linters are pinned, and neither is fetched blindly.** `shellcheck` comes from the
  `taiki-e/install-action` SHA already trusted in `ci.yml`, at **0.11.0**, rather than from the
  runner image, which ships **0.9.0** — a linter whose version floats with the image can redden a
  commit that touched nothing, and can pass locally while failing in CI. `install-action` has no
  `actionlint` manifest (checked: 103 manifests, `actionlint` is not among them), so `actionlint`
  is fetched from its own release, pinned by version **and** verified against the checksum
  published with that release. A job whose entire purpose is raising the floor on shell quality
  does not get to pipe an unverified remote script into a shell.

- **The job asserts both binaries exist before linting anything.** `actionlint` shells out to
  `shellcheck` for `run:` blocks; with `shellcheck` missing it would lint workflow syntax, say
  nothing at all about the embedded shell, and still exit 0. That is a gate quietly covering less
  than it claims, and it is invisible from the outside — so presence is asserted and both versions
  are printed.

### Fixed

- **Four findings, three fixed properly and one suppressed narrowly.** `ci.yml:357` and
  `docker.yml:170` each had `for i in $(seq 1 30)` with `i` never used (SC2034); they are "repeat N
  times" loops, so `for _ in` makes the warning go away by the code being correct. `ci.yml`'s
  coverage summary made four separate `>>` appends to `$GITHUB_STEP_SUMMARY` (SC2129), now one
  grouped append. The one suppression is SC2020 on `tr ', \t' '\n\n\n\n'` in
  `docker/assert-image-tags.sh`: that maps three separator *characters* each to a newline, which is
  what `tr` is for, while SC2020 warns against expecting `tr` to replace *words*. Equalising the
  set lengths does not silence it, and the alternatives are worse — unquoted parameter expansion
  trades SC2020 for SC2086 plus glob exposure needing `set -f`, and `sed` diverges between BSD and
  GNU on `\n` in the replacement. Rewriting tested code in the publish path to quiet a *note* is
  the wrong trade. One directive, one code, with the reason written out, at function scope because
  a `#` between backslash-continued lines is part of the command rather than a comment.

- **`--self-test` in `docker.yml` stays, and is not redundant with `shellcheck`.** They prove
  different things: `shellcheck` proves the script is well-formed shell, the self-test proves its
  logic still rejects the real pre-fix tag sets from runs 34734871991 and 34725795501. Neither
  implies the other, and the proof is the incident itself — the last-token bug was *perfectly
  well-formed shell* that a linter would have passed while the guard silently accepted the release
  it existed to reject.

### Changed

- **What this gate does not do, recorded here because a reader will otherwise assume it.** Branch
  protection on `main` enumerates its required contexts — `rustfmt`, `clippy`, `tests`,
  `conformance (spec fixtures)` — and `lint` is **not** among them. A red `lint` makes a pull
  request visibly red; it does **not** prevent a merge. `docker`/`build` and `coverage` are already
  non-blocking in exactly this way, so this is the repo's existing pattern rather than something
  introduced here. Making it blocking is a protected-branch settings change, not a change to any
  file in the tree; the exact call is in `lint.yml`'s header, including the warning that
  `contexts` is replaced wholesale, so omitting an existing entry silently un-requires it.

- **A trap named while it is cheap:** those four job names are an external contract with branch
  protection. Renaming one without updating protection in the same change removes a required
  context, and every subsequent PR waits forever on a check that will never report.

- **`actionlint`'s own workflow checks found nothing.** Worth stating plainly, because the unit was
  scoped expecting latent bugs of the `v*`-tag-that-never-matched kind this repo has already hit
  once: there are none. All three workflow findings were `shellcheck` on embedded `run:` blocks.
  The value delivered here is the standing gate, not this batch of four fixes, and three style nits
  should not be dressed up as a vindication of the search.

<!-- unit U-503 (lane-3) — the `sha-` tag had two writers; BACKLOG C-D1 / C-D2 -->

### Fixed

- **`sha-<short>` was a mutable tag, and the release that exposed it moved one 19m44s after it
  was published.** `.github/workflows/docker.yml` publishes on a push to `main` *and* on an
  `acdp-registry-server/v*` tag, and release-plz tags the commit it merges
  (`release-plz.toml`'s `git_tag_name`), so a release gives one commit two publishing runs.
  `type=sha` sat in the shared `tags:` rules, ungated, so both runs computed and pushed
  `sha-<short>`. Measured on v0.1.3 (`f8b6d9e`): run 34734028111 published `sha-f8b6d9e` ->
  `sha256:b9315cc84f08` at 02:52:16Z, and run 34734871991 re-pointed it to
  `sha256:cf2f85068eb6` at 03:12:00Z. GHCR confirms the move by *state* rather than by
  inference — `b9315cc8` now carries only `[latest, main]`, having lost the `sha-` tag it was
  published with. A `sha-`-prefixed tag reads as content-addressed and was not: anyone who
  pinned it inside that window is running different bytes than they pinned.
  `concurrency: docker-${{ github.ref }}` does not help, because the two runs are different
  refs — the file already says so about a related case.

- **The fix is one gate, not a new mechanism.** `type=sha` now carries
  `enable={{is_default_branch}}` — the same gate the `latest` rule already used — so the three
  main-line tags (`main`, `latest`, `sha-<short>`) share one gate and one writer: the
  default-branch push. The release run publishes only its version tags and has nothing left with
  which to re-point a `sha-` tag. **This is a single-writer guarantee, not registry-level
  immutability.** GHCR tags stay mutable, and re-running a `main` build by hand will rebuild that
  commit and move its `sha-` tag. Closing *that* needs a pre-push existence check whose
  fail-closed behaviour would block a legitimate re-run after an infrastructure flake; it was
  priced and deliberately not taken, and the docs are worded so they stay true without it.

- **The same commit is still built twice, and that is correct — this is the half of the reported
  defect that was declined, with reasons.** The two digests for one commit are not flakiness and
  not a reproducibility failure; they differ *deterministically*. Read off both runs' `buildx`
  command lines: metadata-action stamps `org.opencontainers.image.version` as `main` on the
  main-push build and as `0.1.3` on the release build, `image.created` differs, and buildx
  attaches `--attest type=provenance,mode=max,builder-id=…/runs/<run-id>`. Labels and provenance
  live in the config blob, so the manifests cannot agree. The obvious way to collapse the builds
  — promoting the main digest with `buildx imagetools create` — would therefore publish a
  release image whose own OCI `version` label reads `main` and whose provenance names the main
  run. That trades a cosmetic problem for a mislabelling one, so the release rebuild earns its
  keep: it is what stamps release identity into the release artifact.

### Added

- **A guard that rejects the exact bytes of the incident, rather than a lint that would have
  passed on them.** `docker/assert-image-tags.sh` asserts the invariant as an **iff** — a
  `sha-` tag is present if and only if this is a push to the default branch — and `docker.yml`
  runs it after `metadata-action` and *before* `build + push`, so a bad tag set blocks
  publication instead of being discovered in the registry afterwards. The iff form is the point:
  a pull request is not the default branch, so if the gate is ever deleted, the PR that deletes
  it computes a `sha-` tag and the guard fails **there**, before merge, rather than staying
  silent until the next release. The reverse direction catches a gate that over-fires and
  quietly stops publishing a documented tag.

- **The guard's falsification is wired in rather than claimed.** `--self-test` runs on every
  workflow event over a ten-case table; five cases assert that the guard *rejects*, and two of
  those replay the real pre-fix tag sets from runs 34734871991 and 34725795501. Writing it this
  way paid for itself immediately: the first draft silently dropped the last tag in its input
  (`while read` returns false on a final line with no trailing newline), so it *passed* the
  release run it was written to reject — and two other cases passed anyway for the wrong reason,
  so no single green case would have localised it. This is also the repository's first shell
  script, into a tree with no `shellcheck`; the self-test step is what stands in for the absent
  linter.

### Changed

- **`docker/RAILWAY.md` now tells an operator which tag to deploy and why two digests for one
  source is expected.** The tag list said `sha-<7-hex>  # every push`, which was never true of
  pull requests and is now not true of release tags either. The deploy recipe led with
  `:latest` while the file's own warning three paragraphs earlier said `:latest` moves on every
  merge; it now leads with a version tag and keeps that warning intact. Added the fact an
  operator would otherwise have to discover from the registry: a release tag and `:latest` are
  different digests of identical source, by design, and
  `org.opencontainers.image.revision` is what confirms two tags came from one commit.

<!-- unit H-U (lane-1) — the store parameter is named for what the predicate consumes -->

### Changed

- **The store's disclosure parameter is renamed `anonymous_public_reads` -> `public_arm_open`,
  at every trait method, implementation and call site.** No behaviour change. The old name
  described one caller's situation rather than what the predicate reads: the public arm is
  `public_arm_open || requester.is_some()`, and `handlers/admin.rs` passes `true` for a caller who
  is authenticated but unnamed. Under the old name that read as a bypass, and **three readers in a
  row — an audit, the coordinating session, and another lane — took it for one.**

- **Both struct fields keep their names, and that is the load-bearing part of the diff.**
  `AuthConfig.anonymous_public_reads` is a config-file key, and
  `CapabilitiesDocument.anonymous_public_reads` is canonical per RFC-ACDP-0007 §3.3 and lives in
  the external `acdp` crate. `server/src/main.rs` is the translation point between them and is
  untouched. The field names are the vocabulary the spec and deployed configs fixed; only the
  parameter was free to change.

### Fixed

- **The config key had no guard, and this unit proved it the hard way.** The mechanical rename
  renamed the serde field. It **compiled**, `clippy --all-targets -D warnings` passed on **six**
  feature configurations, and 28 test suites stayed green — because every other use in the repo
  sets the field programmatically and moved with it. Nothing in the codebase observed the contract
  with deployed `registry.toml` files. `anonymous_public_reads_is_a_stable_config_key` now
  deserialises the literal key, which is the only way to observe the name serde matches on, and it
  fails against exactly that rename.

- **A correction to the hazard as it was described.** The risk was stated as `#[serde(default)]`
  causing an old key to be *ignored*, so a registry would boot at `false` while the operator
  believed otherwise — silent. `AuthConfig` also carries `#[serde(deny_unknown_fields)]`, so the
  old key instead **fails to parse and the registry refuses to start**. The conclusion is
  unchanged, the mechanism is not, and a doc that gets the mechanism wrong is the harder defect to
  find later.

- **A stale doc citation in `tests/common/mod.rs`** named a test another lane had renamed. Fixed —
  and only that one. The three other occurrences, in `ASSUMPTIONS.md` and this file, are **dated
  records** that correctly name the test as it stood on that date; editing them would falsify the
  record rather than update it.

<!-- unit H-A2-w (lane-1) — total_estimate returns for tenant-scoped callers -->

### Changed

- **`GET /contexts/search` now returns `total_estimate` to a tenant-scoped caller, and the number
  is that tenant's count.** Additive and wire-visible: a response that previously omitted the key
  for any request asserting a tenant now carries it. No field changed meaning for an un-scoped
  caller, and nothing was removed.

- **Why it was withheld, and why that reason expired.** The count came from the store, which
  counted §4.5-visible rows *before* the tenant predicate ran — the filter lived in the handler,
  post-query — so the number described rows across every tenant and handed a tenant-scoped caller
  a population size for data it could not see. `search_in_tenant` moved the predicate into the
  same statement as the keyset and the count, so `COUNT(*) OVER ()` rides a scan that only ever
  sees the caller's own rows. There is no cross-tenant number left to withhold, and withholding
  one would hide a figure the caller is entitled to.

### Fixed

- **`docs/HTTP-API.md` stated the opposite, and its stated REASON was false independently of its
  claim.** It explained the omission as "the count is taken in the store before the tenant
  predicate is applied in the handler", which stopped being true when the predicate moved. Both
  halves are rewritten rather than just the conclusion — a correct claim resting on a false
  mechanism is the harder defect to notice later.

- **The assertion that guarded the old behaviour was inverted, not deleted.**
  `search_omits_total_estimate_for_tenant_scoped_caller` was a tripwire naming its own removal
  condition, and this is the commit that makes it false, so it changes here and nowhere else. It
  is now `search_reports_a_tenant_scoped_total_estimate`, and it asserts the **value**: presence
  alone would pass against a handler reporting the registry-wide count and against one reporting
  `matches.len()`. The fixture makes those three numbers distinct (2, 5, 1) and each wrong value
  was produced by a real mutation. `search_still_reports_total_estimate_without_tenant` was left
  untouched and kept passing throughout, which is what shows the change did not go too wide.

<!-- unit H-H-w (lane-3) — wiring H-H's dormant tenant predicate into /contexts/search.
     The fix a doc sentence said still needed building. -->

### Fixed

- **Security (cross-tenant disclosure): a tenant-scoped `/contexts/search` could hand back a
  `next_cursor` anchored on another tenant's row, and that is now closed.** `next_cursor` is
  unsigned plaintext base64 of `{mint_ms}:{anchor_ms}:{ctx_id}` anchored on the last row the store
  **scanned**, not the last row returned. While tenant narrowing was a handler-side post-filter the
  scan saw other tenants' rows, so the retain dropped the row while the anchor had already been
  computed from it — a tenant-scoped caller could walk cursors to recover foreign `ctx_id`s and
  their ordering. No post-filter could have fixed it: dropping a row after the fact does not
  un-scan it.

  A tenant-scoped request is now served by `ExtendedRegistryStore::search_in_tenant`, which carries
  the predicate in the same statement as the keyset and the count, so the scan never sees another
  tenant's rows and the anchor can only be one of the caller's own.

  **Nothing was built for this.** `search_in_tenant` shipped with H-H and sat merged-and-dormant
  because no caller existed — the same shape as `visible_ctx_ids` before P9. The docs said *"closing
  it requires the tenant predicate in the store's search SQL"*, which was accurate only while the
  wiring was missing, so that sentence is corrected in the same change.

- **`search_cursor_oracle_remains_open_for_tenant_scoped_caller` is deleted**, as its own comment
  instructed: *"If you are reading this because it just failed: that is the good outcome. Confirm
  the cursor now only anchors on the caller's own rows, then delete this test and say so in the
  changelog."* This is that note.

  **Deleted, not relaxed, and confirmed causal before removal.** A test that asserts a defect still
  exists is the acceptance criterion for its fix, written by whoever could still reproduce it — so
  it was worth more than a guard written afterwards, and it was checked properly rather than assumed
  to be the reason: with the predicate in place it failed with its own designed message ("EXPECTED
  the residual leak and did not observe it") while its setup demonstrably still ran (three foreign
  rows created); changing **only** the tenant argument from `Some(tenant)` to `None` made it pass
  again. That isolates the failure to the single value that closes the oracle rather than to
  anything else that moved on `main`.

### Changed

- **`total_estimate` stays omitted for a tenant-scoped request, but the reason changed.** It was
  omitted because it was *wrong* — the store counted before the handler applied the tenant
  predicate. `search_inner` puts `AND tenant_id = ?` in the same statement as `COUNT(*) OVER ()`,
  so the count is now tenant-scoped and honest. The key is withheld **conservatively rather than
  necessarily**; re-enabling it is wire-visible and ships as its own reviewable unit rather than
  buried in a wiring change.

- **One gate was carried by hand, deliberately.** `RegistryServer::search` is not a thin wrapper: it
  rejects an anonymous search with **403 `not_authorized`** when `caps.anonymous_public_reads` is
  false *before* delegating to the store (RFC-ACDP-0008 §6.3, fixture `vis-009`) — not an empty
  `200`, which would still confirm the registry exists and that the query ran. `search_in_tenant` is
  a store entry point with no such gate, so routing through it means carrying the gate or silently
  downgrading a normative 403. The untenanted path is left on `server.search` untouched, and the
  gate is replicated on the tenant path only, reading the flag from `capabilities()` — the same
  field `server.search` reads, so the two agree by construction rather than while config and caps
  happen to match.

<!-- unit H-A, phase P10 (lane-1) — the route-classification guard names what it cannot parse -->

### Fixed

- **A route deleted from the core router while its `NON_DATA_ROUTES` row remained was invisible
  to every assertion.** The classification test ran `mounted ⊆ classified` in one direction only,
  so it noticed a route added without a cache posture and was blind to the opposite. A row
  matching no route is not inert: it reads as coverage, and it silently widens what a later route
  reusing that path would inherit. `DATA_PLANE_ROUTES` never had this gap — assertion 1 compares
  it to the `data` group with `assert_eq!` on two sets, which fails both ways — so this closes
  the one table that had no partner. Deliberately not a count: `len() == mounted - tabled` would
  pass if one row went stale while another was added, which is precisely what a rename does.

- **The equality assertion reddened on a non-literal route path but would not say which one.**
  It reported "26 != 27" and left the reader to diff two lists by hand. It now names the form it
  could not interpret, e.g. `.route(DEBUG_PATH, ...)`.

### Changed

- **Recorded because the bug was in this phase's own new code.** The first draft of the scanner
  that names non-literal forms worked line by line and reported seven false positives: `lib.rs`
  registers seven routes with `.route(` at the end of one line and the path on the next, and a
  per-line scan sees an empty argument. That is the same line-wrap failure that defeated a grep
  earlier in this unit, written again hours later in a different medium — knowing the failure
  mode did not prevent repeating it. The scanner is now the exact **inverse** of
  `mounted_route_paths`: same scan, same whitespace test, opposite branch, so the two cannot
  disagree about what a literal is instead of agreeing by inspection.
<!-- unit H-O (lane-2) — the caps/config split, and a premise that was mostly
     already satisfied. Three of the assign's claims were refuted by measurement. -->

### Changed

- **The test harness gained one call that cannot set half of the `anonymous_public_reads`
  invariant.** `RegistryServer` gates that flag off the `CapabilitiesDocument` baked in at
  `try_new`, not off `RegistryConfig`, so a test that flips the config value and observes a
  200 has measured the harness's caps/config split and nothing about the binary — the invalid
  "wire probe" that let a shipping anonymous-disclosure bug reach review (#255). The config
  value is not dead either: it is what the binary's `build_capabilities` derives the caps value
  *from*, so a test that sets only caps is asserting against a state no deployment can reach.
  `common::with_anonymous_public_reads` takes one input and returns both, so divergence is
  unrepresentable through it.

  **Three of the four premises behind this unit were refuted by measurement, and the refutation
  is most of the result.** `tests/common/mod.rs` already parameterised caps completely — every
  constructor takes it, and there is no `anonymous_public_reads` literal in that file's code at
  all. `conformance.rs` already overrode caps in three independent paths, not zero: the Shape D
  driver, `vis009_anonymous_public_reads_gates_anonymous_not_authenticated`, and
  `seeded_harness_rebuild_changes_router_behavior_and_preserves_seeded_state`, which sets caps
  false and asserts the same anonymous search is now refused. And a `caps=false` visibility test
  already existed and had already been falsified, in #255. So only the fourth premise — that
  nothing makes the two-knob divergence loud — survived, and that is all this ships.

  **Enforcement was deliberately not shipped.** A `debug_assert` in the shared wiring would have
  reddened `admin_list_returns_rows_under_the_shipped_disclosure_default` in a file another lane
  holds, so the invariant is constructive here and the divergent site is reported instead.

### Fixed

- **Recorded, not fixed — a third instance of the "probe that cannot fail" class, in this same
  area.** `admin_list_returns_rows_under_the_shipped_disclosure_default` is framed as testing
  "the SHIPPED DEFAULT `anonymous_public_reads = false`", and `admin_list` reads that flag from
  **neither** config nor caps: `handlers/admin.rs` hardcodes `let admin_sees_public_arm = true;`.
  The test varies a value its code path never consults, so it cannot fail for the reason it was
  written. Its actual assertion — an admin bearer still sees public rows — remains meaningful;
  only the framing overstates what is exercised. Both files are outside this lane's grant, so
  this is reported rather than edited.


<!-- unit H-A, phase P9 (lane-1) — /log/entries answers a page with one visibility query -->

### Fixed

- **Security (anonymous disclosure): `/log/entries` would have echoed every public `leaf` to an
  unauthenticated caller on the shipped default configuration.** Caught by this repo's own
  verification gate before merge, not in production — recorded because the way it survived
  three tracked files is the part worth keeping. The batched rewrite below passed a hardcoded
  `anonymous_public_reads: true`, justified by a "wire probe" that flipped
  `cfg.auth.anonymous_public_reads` and observed a 200. That probe measured nothing.
  `RegistryServer::retrieve` gates on `self.caps`, **not** on `RegistryConfig` — this repo
  already documents that split as GAP 3 in `crates/acdp-registry-server/tests/common/mod.rs` —
  and the default test harness hardcodes `caps.anonymous_public_reads: true`. So the 200 came
  from the caps value the probe never touched. Since `AuthConfig::default()` ships
  `anonymous_public_reads: false` *and* `auth.enabled: false`, on the default config every
  caller is anonymous and every public leaf would have been disclosed. The flag is now read
  from `state.server.capabilities()` — the same field `retrieve` reads — so the batched call
  equals the per-record retrieve **by construction** rather than only while config and caps
  agree. `log_entries_honours_anonymous_public_reads_from_caps` overrides the caps, which is
  the only thing that moves the real predicate, and it fails on the hardcoded value.

### Changed

- **`GET /log/entries` answers a whole page with one visibility query.** It resolved `leaf`
  visibility one record at a time: a blocking `RegistryServer::retrieve` dispatch per entry,
  plus — under an `X-Tenant-Id` header — a `tenant_of_ctx` per entry. On a full 256-record page
  (RFC-ACDP-0012 §8.3 RECOMMENDS a cap of at least 256) that is 256 blocking-pool dispatches and
  up to 512 store round-trips to answer one request. It is now a single
  `ExtendedRegistryStore::visible_ctx_ids` call, which both SQL backends override with one
  query. That method has been merged and dormant since #246; the plan for this phase predates
  it and prescribed a three-part workaround on the explicit grounds that a batched store method
  was out of scope. That premise expired, so the workaround was not built.

- **Responses are byte-identical on the SUCCESS path only, for a given
  `anonymous_public_reads` — two things change, and one of them changes a status code.** The
  earlier version of this sentence claimed byte-identity flatly and hedged with "but this is not
  a pure refactor", which narrows the wrong property: it qualifies refactor-ness while leaving
  byte-identity standing, so a reader who notices the hedge still comes away believing something
  false. `ASSUMPTIONS.md` had the four words that do the work — "on the success path only" — and
  this entry had dropped them. First, the flag above: get it wrong and the bytes differ
  enormously, which is the whole of the security entry. Second, which store errors can reach
  the caller — and the change is one-directional:

  **No longer able to surface: everything `RegistryStore::get` does.** The old path ran
  `server.retrieve` — and therefore a full `get`, including `events_for_ctx`,
  `reconcile_retraction` and `body_json` deserialization — on *every* record, because that call
  **was** the visibility check. The batched query is `SELECT ctx_id FROM contexts WHERE …`, so a
  decode failure or an events-table error on any row of the page used to 500 the request and now
  cannot.

  **Scope of that claim.** It is a property of the two SQL overrides, which is every backend that
  can serve this endpoint today — `MemoryStore` does not override `log_entries`, so
  `/log/entries` is `NotImplemented` there before this code is reached. It is *not* a property of
  `visible_ctx_ids` as a trait method: the default impl still runs a full `get` per id, so an
  external implementor that overrides `log_entries` but not `visible_ctx_ids` would see an
  unchanged error surface, not a smaller one.

  **Nothing is newly able to surface.** Two earlier drafts of this entry got this wrong in
  opposite ways, so the reasoning is spelled out rather than asserted. The first said a store
  error could reach the caller "only for rows that were already visible", which is false —
  `get` ran on every row. The second corrected that but claimed the change ran in **both**
  directions, with `tenant_of_ctx` newly reachable on hidden rows. Also false, in every
  implementation: the SQLite and Postgres overrides never call `tenant_of_ctx` at all (the
  predicate is `AND tenant_id = ?` inside the same statement), and the default trait impl still
  gates it behind `if !retrieve_visible(…) { continue; }` — the identical gate the old handler
  had. So the honest net is one-directional: strictly **fewer** classes of store error can
  surface than before, and the earlier "strictly more honest" framing had it backwards.

- **The guard is the deliverable.** The improvement is invisible in the response, so nothing in
  the suite could have noticed a revert. A `CountingStore` test wrapper counts `get`,
  `visible_ctx_ids`, `tenant_of_ctx` and `tenants_of_ctxs`; the tests pin one batched query and
  zero per-record reads with and without a tenant header, pin that no separate tenant lookup is
  paid for either, and pin that the reserved `default` sentinel is refused before any row is
  read. Every assertion was falsified individually against a mutation violating it alone,
  rather than as a group — the first failing assertion masks every one below it.

<!-- unit H-A, phase P8 (lane-1) — tenant-scoped search stops reporting a cross-tenant count -->

### Fixed

- **Security (cross-tenant disclosure, partial): a tenant-scoped search reported a population
  count for rows across every tenant.** `total_estimate` is produced by the store, which counts
  §4.5-visible rows *before* the tenant predicate is applied — tenant narrowing happens
  afterwards, in the handler, as a post-query filter. So a caller asserting `X-Tenant-Id`
  received the number of matches across the whole registry while seeing only their own rows: an
  O(1) population count for data they cannot read.

  The key is now **omitted entirely** for a tenant-scoped request. Omitted rather than
  recomputed, because an honest tenant-scoped count needs the predicate in the store's SQL, which
  is a different change in a different crate. `Option<u64>` with `skip_serializing_if` means the
  key is *absent* rather than `null` or `0`, so a client cannot read "withheld" as "none found".
  An un-scoped caller still receives it — removing it for everyone would have passed a naive
  "tenant caller sees no count" test while breaking conformance, which is why
  `search_still_reports_total_estimate_without_tenant` exists.

  **This is partial and is not described as closed.** `next_cursor` is unsigned plaintext base64
  of `{mint_ms}:{anchor_ms}:{ctx_id}` anchored on the last row the *store scanned*, which may
  belong to another tenant, so foreign `ctx_id`s and their ordering remain recoverable by paging.
  The fix moves that from one request to one request per row; it does not remove it. Closing it
  requires the tenant predicate in the store's search SQL.
  `search_cursor_oracle_remains_open_for_tenant_scoped_caller` asserts the residue, so "partial"
  is machine-checked rather than a sentence someone has to re-read, and it fails deliberately
  when the underlying fix lands.

  Two claims that were overstated have been corrected rather than carried forward. The code
  comment described the cursor as "a low-grade ordering/existence oracle" where "no context DATA
  leaks" — a `ctx_id` is a durable identifier, not low-grade, and "no data" was true only of
  bodies. And `docs/HTTP-API.md` stated `total_estimate` was "the count of §4.5-visible matches
  for the caller", which was exactly the falsified claim.
<!-- unit H-P (lane-3) — H-G's last two items. Both claims were about a property no
     test could see, and in both cases a test NAMED for that property already existed. -->

### Fixed

- **`Retry-After` reported a range, never a value.** Three separate copies of
  `WINDOW.saturating_sub(elapsed).as_secs().max(1)` exist in `rate_limit.rs`
  (`check_global_at`, `peek_at`, `check_at`). Replacing the subtraction with
  `elapsed.saturating_sub(WINDOW)` at all three pins every `Retry-After` to exactly **1
  second** and left all 24 tests in the module green, plus the HTTP integration assertion. A
  client throttled for a full minute would be told to come back in one second, retrying ~60×
  more often than intended — under precisely the load the limiter exists to shed.

  Three tests looked like they covered this and none did.
  `peek_retry_after_matches_check` pins `peek == check`, which is internal consistency between
  two paths and silent when both share one wrong arithmetic. `allows_up_to_limit_then_rejects`
  and the integration test both assert `(1..=60).contains(&retry)` — a range spanning every
  value the function can return. And `retry_after_never_reports_zero` *does* assert an exact
  value, but at 59.5s in, where the expected answer **is** the clamp floor: the module's one
  exact assertion sat at the single point where a wrong mechanism produces the right number.

  `retry_after_reports_the_seconds_actually_remaining` asserts the remaining seconds at five
  points across the window, on all three paths, with the expectation derived from the
  documented 60s contract rather than from `WINDOW` (computing both sides from one constant
  lets a change move them together). Falsified per site: mutating each of the three
  arithmetic copies individually fails it at a **different** assertion line, so no one path's
  coverage is masking another's.

  **Partly refuting the audit that raised it:** the 1s floor is *not* an unasserted comment.
  `retry_after_never_reports_zero` pins it exactly. What was missing was every other point in
  the window.

- **The constant-time compare had a test named for non-short-circuiting that could not
  detect short-circuiting.** `ct_eq_does_not_short_circuit_on_position` asserted only return
  values, and a short-circuiting comparison returns identical values for every input.
  Measured: replacing `ct_eq`'s body with `a == b` — the exact comparison the helper exists to
  avoid — left all three tests green. The claim that "every input reaches the fold" was
  stated in the test's own doc comment and asserted nowhere.

  The property is *work performed*, so it cannot be asserted on a result. The fold now lives
  in a private `ct_fold` generic over an iterator of byte pairs, which lets
  `ct_fold_consumes_every_pair` pass a counting iterator and assert every pair was consumed
  even when the **first** pair already differs — deterministic, where a wall-clock timing
  test on a shared runner is a flake that eventually gets deleted rather than fixed. It also
  asserts the fold is still correct, so a loop that consumes everything and computes nothing
  fails too.

  `ct_eq` is `ct_fold`'s only caller, so a rewrite that stops folding makes it dead code.
  Verified rather than assumed: CI's `cargo clippy --locked --workspace --all-targets -- -D
  warnings` exits **101** with `function ct_fold is never used`. `--all-targets` is what makes
  that work, by compiling the lib target separately from the test target — the test module's
  use of `ct_fold` does not keep it alive in the lib's own compilation.

<!-- unit H-N (lane-2) — H-G's JWT-issuer item. The claim was TRUE and true in its
     specifics: deleting the issuer check left the whole workspace green. -->

### Fixed

- **The JWT issuer was validated and never asserted**, and the gap was measured rather than
  argued: commenting out `v.set_issuer(&[&self.issuer])` in `jwt.rs::validate` left
  **all 619 workspace tests passing**. Nothing in this repo could distinguish a working issuer
  check from an absent one, so the guard against a federated token from a trusted-but-wrong
  issuer rested on a line no test was watching.

  `rejects_token_from_a_different_issuer` now closes that. The audience sibling
  (`rejects_token_for_a_different_audience`) already existed; this is the missing half.

  **The fixture is built so it can actually reach the issuer check** — the failure mode for a
  JWT test is that signature, expiry or audience validation short-circuits first and the test
  passes having proved nothing. Every other property of the token is deliberately correct:
  signed by the same signer, unexpired, matching `aud`, matching `acdp.registry`. And the
  assertion **names `InvalidIssuer`** rather than accepting any error, because "rejected" and
  "rejected for the reason under test" are different claims.

  Three mutations, each RED at a **different** assertion, which is what makes them three guards
  rather than one: deleting the check and *adding the attacker's issuer to the trusted list*
  both redden the primary assertion; setting the trusted issuer to a value nothing matches
  reddens the **control** — proving the control is a live guard and not decoration. The widened
  trust list is the realistic defect and it is now caught.


<!-- unit H-A, phase P7 follow-up (lane-1) — the 415 ruling applied -->

### Fixed

- **The `415` from a missing or wrong `Content-Type` now answers an RFC-ACDP-0007 §5 envelope**,
  closing the one case the extractor-rejection work shipped without. It carries
  `"code": "unsupported_media_type"` at the unchanged `415` status.

  **That code is not in the canonical §5 registry, and minting it was a deliberate decision by
  the project owner rather than an oversight.** `AcdpError::from_wire_error` recognises 25 codes
  and none describes a media-type failure, while `WireErrorBody::code` is a required field — so
  a registry answering `415` must put *something* there. The nearest canonical option,
  `schema_violation`, is documented as "malformed body, missing field, schema mismatch", and on
  a `415` the body was never parsed at all; it would have stated something false, made `415`
  indistinguishable from `400` at the code level, and become unfixable once clients coded
  against `AcdpError::SchemaViolation`. Unrecognised codes route to `AcdpError::Registry(wire)`
  "for forward compatibility", so existing clients keep the status and the message and lose only
  the typed variant.

  The divergence has a closing path rather than being permanent: **acdp-rs#268** asks the canon
  to adopt the code. Decision 16 in `DECISIONS.md` records the standing precedent — this repo may
  mint a wire code when the canon lacks an honest one, provided the name follows the canon's
  idiom and an upstream issue is filed.

<!-- unit H-E (lane-2) — the auth/webhook quartet. All four audit findings
     confirmed real with exact citations, which inverted the expectation the
     assign was written with. -->

### Fixed

- **A revoked token was accepted again for up to 30 seconds after its own expiry.**
  `JwtSigner::validate` accepts a token until `exp + leeway` (default 30s) — that is what leeway is
  for — but every revocation backend judged the tombstone against a bare `now`, so it went cold at
  `exp`. In the window `(exp, exp + leeway]` the token still decoded **and** the revocation check had
  already gone false. Revocation un-revoked itself.

  Not an authentication bypass: a control lapsing inside a window, which is narrower and is what the
  evidence supports. One helper, `tombstone_cutoff(now, leeway)`, now owns the rule, and **both**
  halves of the lifecycle take it — the check and the eviction. Fixing only the check would have left
  eviction deleting the row at `expires_at`, defeating revocation through the other door while a
  check-only test stayed green. The evictor reads the leeway from the signer rather than re-reading
  config, because a validator and an evictor disagreeing about the window is precisely the defect
  being fixed.

- **The revocation poller followed redirects.** Feeds were fetched with a bare `reqwest::Client`,
  which follows up to ten redirects, so a hostile or compromised peer could bounce the poller — with
  the admin bearer token it uses to read the feed — at an internal address. It now uses
  `acdp::safe_http::safe_client`, as webhook delivery already did.

  **Operator-visible consequence:** `safe_client` installs a DNS resolver that refuses hosts
  resolving into private, loopback or link-local ranges, so a peer registry reachable only on an
  internal hostname will now be refused rather than polled. Deliberate and consistent with webhook
  delivery. Note also what `safe_client` does *not* do: it consults its policy only for DNS, so
  `allow_http` and `reject_ip_literals` are unenforced — an `http://` or IP-literal feed URL still
  works, and an IP literal bypasses the resolver check entirely.

- **The challenge nonce was logged at INFO.** The event stays and now carries `agent_id`, which is
  what an operator correlates on. Severity is hygiene rather than exploitability — the nonce is the
  value the agent must *sign*, so disclosure alone forges nothing without the agent's key — but it is
  a short-TTL credential-shaped value and logs are routinely shipped off-box.

### Added

- **Opt-in replay protection for webhooks — offered, not enforced.** `X-ACDP-Signature` covers the
  body alone, so a captured delivery can be replayed forever, and because `X-ACDP-Event-Id` is
  deliberately stable across retries a replay is indistinguishable from a retry. Deliveries now also
  carry `X-ACDP-Timestamp` and `X-ACDP-Signature-Timestamped`, the latter covering
  `"<timestamp>." + body`.

  **`X-ACDP-Signature` is unchanged** and is now pinned by a test, because it is a documented public
  contract every deployed receiver verifies; widening it would break all of them. An unsigned
  timestamp would be rewritable and therefore worthless, which is why there is a second signature
  rather than just a header.

  **This does not close the replay hole on its own.** A receiver that ignores the new headers is
  exactly as exposed as before — the registry cannot make a receiver check freshness. Adoption means
  verifying the second signature and bounding the timestamp. Documented that way in
  `WEBHOOKS.md` deliberately: "the registry now has replay protection" would be false on the day it
  shipped.

<!-- unit H-I-s (lane-2) — batched retrieval-visibility for the audit log,
     storage half. PARTIAL BY DESIGN: nothing calls it until H-I-w wires the
     handler. -->

### Added

- **A batched retrieval-visibility check, `ExtendedRegistryStore::visible_ctx_ids`.**
  `GET /log/entries` gates every `ctx_id` it echoes on the full
  `GET /contexts/{ctx_id}` rule, and it did so **one context at a time** — a blocking
  `RegistryServer::retrieve` per entry, up to the 256-entry page cap. One query now answers a
  whole page. Measured on SQLite over a full 256-entry page: **991µs batched against 21.9ms for
  the per-id path**, ≈22×. The test asserts only that the two paths *agree*; the timing is
  printed rather than asserted, because a timing assertion on shared CI is a flake waiting to
  happen.

  **This ships dormant. The fan-out is still live on the deployed path.** Nothing calls
  `visible_ctx_ids` until unit H-I-w wires `handlers/log.rs`, which is in a crate this change
  must not touch. Partial by design, not an oversight.

  **It reproduces `retrieve` semantics, NOT `search` semantics** — and that distinction is the
  whole unit rather than a detail. Under RFC-ACDP-0008 §4.5 a named audience member **may**
  retrieve a `private` context, while search deliberately requires *ownership* for `private` and
  excludes the audience (conformance `vis-004`, the "private/audience retrieval asymmetry"; a
  listed **contributor** who is not in the audience is refused, because contributors are
  provenance, not authorization). Reusing a search predicate here would therefore silently
  **under**-disclose — hiding audit entries a caller is entitled to, with no error. The two
  predicates are correct for their own surfaces and must not be harmonized. Confirmed three
  independent ways before a line was written: the conformance fixture, the shape of the existing
  LIST predicate, and the upstream `can_retrieve` rule itself.

  Two further consequences that read as hardening and are actually defects, both guarded by
  tests that fail if someone "fixes" them: **status is absent from §4.5**, so a retracted context
  remains retrievable and must not be filtered; and **the tenant gate is half the contract**, so
  batching only the visibility half is a cross-tenant disclosure.

  The §4.5 rule is now expressed **three times** in this workspace — SQLite's predicate,
  Postgres's, and `retrieve_visible` in Rust. That is forced rather than chosen: the
  authoritative `can_retrieve` is `pub(crate)` in the upstream `acdp` crate and
  `RegistryStore` exposes only a raw `get`. The duplication is named where it lives and
  contained by a three-way differential test that makes all three answer for the same rows.

  No new index and no migration, measured rather than assumed: `ctx_id` is the primary key in
  both backends, so the id lookup is a PK index scan (`contexts_pkey`, bitmap index scan on
  Postgres) with visibility and tenant applied as a filter over at most N rows. Adding an index
  here would be redundant.

### Changed

- **Postgres's retrieval-visibility predicate is now the named constant `LIST_VISIBILITY_PG`.**
  It had been written inline inside `list_contexts`, so the batched method above would have made
  a *fourth* copy of the §4.5 rule. Extracting it and pointing both methods at the one
  expression keeps the count at three, and matches the structure SQLite already had.

<!-- unit H-A, phase P7 (lane-1) — extractor rejections speak the §5 envelope -->

### Fixed

- **Extractor rejections answered `Content-Type: application/acdp+json` over a plain-text body
  with no `error.code`.** A malformed body, a wrong `Content-Type`, a schema mismatch or an
  unparseable query value were all rejected by axum before any handler ran, and the outermost
  media-type backstop then stamped the ACDP media type onto axum's prose. So the wire promised an
  ACDP error envelope and delivered a parser message — and that message leaked serde type paths,
  internal struct field names, and byte offsets (`Failed to deserialize the JSON body into the
  target type: agent_id: invalid type: integer 123, expected a string at line 1 column 15`).

  All four now answer a real RFC-ACDP-0007 §5 envelope **at their original status**: `400`
  malformed body, `422` schema mismatch, `400` bad query value, and `413` for an oversized body
  that reaches the extractor. Messages are now this registry's own, stable, and describe the
  request rather than the parser — axum's and serde's text is theirs to change, so echoing it
  grows an accidental wire contract that breaks on a dependency bump.

  **Statuses are preserved, not collapsed.** Mapping these onto the internal error type would
  have forced `415` and `422` down to `400`, because that type has no 415-bearing variant. That
  would have been an artifact of the mechanism rather than a decision, so the mechanism was
  changed instead: a local rejection type carrying the rejection's own status. The catch-all arm
  carries `rej.status()` for the same reason — `JsonRejection` is `#[non_exhaustive]` and its
  `BytesRejection` variant is a **413**, so a hard-coded 400 would have downgraded an oversized
  body on `/auth/*` while every other test stayed green.

  **Still outstanding:** the `415` from a missing or wrong `Content-Type` is *not* yet enveloped.
  Enveloping it requires choosing a §5 `code`; the canonical 25-code registry has none for a
  media-type failure, and this repo has never emitted a code outside that registry. The question
  is policy, not engineering, and it is with the project owner. The status is `415` either way.
  `marker_the_415_rejection_is_not_yet_enveloped_pending_a_ruling` fails the moment the ruling is
  applied, so the gap cannot quietly outlive the question.

<!-- unit H-A, phases P4 + P5 (lane-1) — rate-limit scope taxonomy, and the
     publish bucket charged on success rather than on an unverified attempt -->

### Changed

- **`acdp_registry_rate_limit_rejections_total` now distinguishes a global challenge-ceiling
  rejection from a per-agent one.** **This changes the meaning of an existing label.** A
  `/auth/challenge` rejection caused by the process-global ceiling was previously recorded as
  `scope="challenge_per_agent"` — the same label as one noisy agent. It is now
  `scope="challenge_global"`.

  **If you alert on `challenge_per_agent`, that series will drop** by whatever share of its
  volume was actually global-ceiling rejections; the missing volume reappears under
  `challenge_global`. Splitting them is the point: the two mean opposite things. A per-agent
  rejection is one caller to throttle. A global rejection is a flood rotating `agent_id` to
  defeat the per-key limit — the precise attack the global ceiling was added for (#24), which
  the collapsed label rendered invisible by making it look like ordinary per-agent noise.

  The `scope` values are now generated from a single list rather than written out at each call
  site, so the emitted set, the `ALL` constant and the documented set cannot drift apart. This
  taxonomy had already drifted twice: `lifecycle_per_agent` was emitted but undocumented, while
  the `metrics.rs` docstring advertised a `challenge_global` that nothing emitted.

### Fixed

- **Security: the per-agent publish budget was charged against an UNVERIFIED `agent_id`.** The
  publish limiter ran `check` — which tests and charges in one step — before the verify/persist
  pipeline, at a point where `req.agent_id` is whatever the caller wrote in the body. Two
  consequences, both reachable without any credential: an unauthenticated caller could **spend
  another agent's budget** by naming them, and could **grow the limiter's bucket map without
  bound** by naming a fresh id each request, because the map key was attacker-controlled.

  `check` is now split. `peek` runs in the same place, answers the only question that position
  needs — "is this agent already over budget?" — and **never writes**: no insert, and no window
  rollover, so a caller who never earns a charge leaves no trace. `record` charges on the success
  path, where the `agent_id` has been verified. The 429, its `Retry-After`, and the
  `publish_per_agent` metric are unchanged.

  Two consequences are stated rather than left to be discovered. The enforced bound is now
  `limit + concurrent in-flight publishes for that agent`, not `limit + 1`, because peek and
  charge are separated by the whole pipeline; reserving to close that was rejected because a
  reservation must live somewhere, and that somewhere is the attacker-keyed map entry this change
  exists to remove. And a publish that fails *late* is no longer throttled at all — disclosed and
  tracked as #242 rather than papered over, because both available fixes are worse than the gap
  (see the issue).

<!-- #240 (lane-2) — merged to main while this file was being retired from CHANGELOG.md -->

### Fixed

- **Security (cross-tenant disclosure narrowing): a tenant-scoped search could hand back a
  pagination cursor anchored on another tenant's row.** The store layer's `search` carried no
  tenant predicate in either backend, so tenancy was applied in Rust to the *result set*. The
  foreign rows were removed from the page — no context data was ever served — but
  `acdp::pagination` anchors `next_cursor` on the last row the **scan touched**, not the last
  row served, deliberately, so that a page emptied by post-SQL filters cannot halt pagination
  early. The surviving cursor therefore encoded a foreign row's `(created_at, ctx_id)`: an
  ordering and existence oracle over rows the caller must not know exist, walkable one page at
  a time rather than a single-shot leak.

  `ExtendedRegistryStore::search_in_tenant` is new and puts `tenant_id = ?` in the `WHERE`
  clause on both SQLite and Postgres, which fixes the anchor **by construction** — every row
  the scan touches already belongs to the caller — and makes `total_estimate` tenant-correct on
  the same scan, with no extra query (measured: 100 of 2000 rows on SQLite, 2 of 506 on
  Postgres). Both backends run it through a single shared query builder rather than a second
  copy of the ~260-line filter chain, and a cross-backend assertion in
  `acdp_registry_store::parity` decodes the returned cursor and asserts the anchored row's
  tenant, so a future divergence fails both backends' suites.

  **Scope, stated plainly: this is not yet reachable, and the underlying leak is not yet
  closed.** No caller invokes `search_in_tenant` — the HTTP search handler still uses the
  protocol-level `RegistryStore::search` plus a post-query filter, so the cursor oracle remains
  live on the deployed path until the handler is wired (a separate change, in a crate this one
  deliberately does not touch). What ships here is the storage-layer half plus the guard that
  proves it: a **partial** fix, named as such. `cursor.rs`'s module docs previously claimed a
  cursor holds only "an identifier the requester was already shown"; that claim was false under
  tenant narrowing and is now replaced with a per-dimension account of what the anchor can
  name. Thanks to lane-1 for finding that while working on a neighbouring unit and flagging it
  rather than silently rewording it.

  No migration: the tenant predicate is already index-assisted in both backends
  (`idx_ctx_tenant` on a selective tenant; Postgres correctly prefers `idx_ctx_created` when the
  tenant is not selective), so a new index would be write cost for no measured gain.

<!-- H-D (lane-3) — docs & release truth -->

### Documentation

- **Nine documentation claims that the code contradicted, and seven guards so the
  next nine cannot hide** (unit H-D). The unit's method was that a corrected
  sentence is worth less than a checkable one: every fix here ships with a command
  that fails when the claim goes stale, and every guard was falsified — the defect
  reintroduced, the named test confirmed RED with its expected message, reverted,
  confirmed GREEN — per assertion rather than per test.

  **`/livez` was undocumented and `/healthz` was described as "Storage liveness".**
  That wording is the push toward wiring a storage-gated probe to a Kubernetes
  `livenessProbe`, which restart-loops healthy pods through a database outage and
  discards the in-memory webhook queue on each cycle. Corrected to
  readiness-vs-liveness, with the missing row and a quick-start `curl` whose body
  shape was read off the handler rather than assumed.

  **`docs/AUTHENTICATION.md` anchored thirteen claims to line numbers, and two had
  already slipped.** `context.rs:1350-1367` for `caller_from_headers` (really 1362;
  1350 is the tail of an unrelated handler) and `lib.rs:154-156` for the `/metrics`
  mount (really ~110 lines away — that range is the `auth` subrouter). The prose
  around both was correct. A pin that reads as precise and points at the wrong code
  is worse than no pin, so all thirteen became symbol citations, and a guard now
  rejects any `.rs:<digits>` pin reappearing in that file.

  **`docs/MULTI-TENANCY.md` attached a paging hazard to an endpoint that cannot
  page.** It said search *and lineage* post-filter the tenant binding "with a
  bounded refill loop … so a page may come back shorter than `limit`".
  `run_search_with_refill` has exactly one caller. `lineage` does post-filter in the
  handler, but returns the whole lineage in one unpaginated array — no loop, no
  cursor, no short page to misread — and `/lineages/{id}/current` 404s
  `no current version` on a foreign tenant, deliberately indistinguishable from "no
  such lineage". All three are now documented separately with the consequence
  attached only where it applies, and the refill cap is pinned to the constant.

  **The root `CHANGELOG.md` claimed nothing in it had shipped.** 3,596 lines under a
  single `## [Unreleased]` heading across three released versions. Retired to a
  pointer; this file is where it went. Reasoning, and why splitting by version was
  rejected as ill-defined rather than merely expensive, in `DECISIONS.md` decision 15.

  **Operator gaps.** The section titled "Backup and restore" documented backup only.
  It now carries a restore procedure (restore before first boot, since migrations run
  before the listener binds; verify with `/healthz`, not `/livez`, which answers 200
  against a dead database), a capacity section, a transparency-log-inconsistency
  runbook, and rotation for the two credentials that had none — admin tokens, which
  rotate through the list without a refusal window, and the webhook secret, which has
  no overlap window at all and must therefore be rotated from the receiving side
  first. Two `ACDP_REGISTRY_*__*_JSON` escape hatches were documented nowhere despite
  being the only way to configure `auth.tenant_agents` and `playground.pinned_keys` on
  a deployment without a config file.

  **The finding that outlived the unit.** The first route scanner keyed on the literal
  `.route("` and so missed every rustfmt-wrapped mount: it found **19 of 27** routes,
  and a `>= 15` count floor passed it, reporting six never-checked routes as covered.
  A floor cannot detect under-counting, which is precisely what a broken scanner
  produces. It was replaced with an equality against the number of `.route(` calls —
  which then immediately caught a second miss, this file's own prose being counted as
  a mount. Two of the audit's own claims also failed verification, both flattering the
  audit: receipt-key rotation was already documented, and the "four undocumented
  features" were all documented as TOML keys.

### Fixed

- **Cache posture: `/metrics` and the `did.json` 404 arm were uncacheable in principle and
  unlabelled in practice; `/healthz` was doing two jobs under one name.** Three fixes.

  `GET /metrics` now answers `Cache-Control: no-store` on **every** arm — 200, 401 and 405 —
  closing #218. The 401 is the arm that mattered: a cached 401 is what a shared cache would
  hand to an authorized scraper. The 405 is the arm that fixes the mechanism in place: the
  router emits it before any handler runs, so a handler-set header could not reach it. The directive is attached to that route alone, not to the
  group it shares with the `/.well-known/*` documents — applying it group-wide was tried and
  demonstrated to clobber `jwks.json`'s `public, max-age=300`.

  `GET /.well-known/did.json`'s **404 arm** now answers `no-store`. That 404 means "no receipt
  signing key is configured" and flips to 200 on an operator action, so a heuristically cached
  404 masks the newly available document from every resolver that saw the miss, with no way for
  the operator to observe or flush it. `no-store` rather than a short `max-age` because the 404
  carries no `ETag` to revalidate against.

  **New `GET /livez`** — always 200, never touches storage, `no-store`. `/healthz` reports
  storage *readiness* and 503s during a database outage, which is right for a load balancer and
  catastrophic for a Kubernetes `livenessProbe`: it restart-loops a process that is alive and
  cannot fix the database by restarting, discarding the in-memory webhook queue each cycle.
  Nothing in this repo wired `/healthz` that way, so this was a latent trap rather than a live
  bug — but `docker/RAILWAY.md` told operators to point Railway's healthcheck at `/healthz`
  without mentioning the 503 arm, and that half is fixed too.

  Guards falsified, not merely added. The `did.json` assertion previously **pinned the defect**
  as intended behaviour ("the 404 arm sets no Cache-Control"); it is now inverted. The `/livez`
  guard is a wire test against a closed storage pool, because the route-classification test is a
  source scan that fails on routes present-but-unclassified and never on the reverse — deleting
  `/livez` entirely leaves it green, which was confirmed by deleting it. The `/metrics`
  classification row was **replaced**, not deleted: the prior comment claimed deleting it was
  "how the fix announces itself", and deleting it was shown to fail the build instead.

- **Observability: the registry minted `x-request-id` values that reached no response,
  and middleware-generated `413`s carried no error envelope.** Two defects in the
  middleware stack, both in `build_router`.

  `SetRequestIdLayer` was applied before `PropagateRequestIdLayer`, and `Router::layer`
  makes the *later* call the *outer* one — the inverse of `tower::ServiceBuilder`, whose
  doc example the stack was written against. So `PropagateRequestId` ran first and read a
  request header that `SetRequestId` had not written yet, and the generated UUID reached
  **no response at all** — not merely the middleware-generated ones, but plain `200`s too.
  A client-supplied `x-request-id` echoed back correctly, which is what hid this: any test
  or manual check that sent its own id saw the header and concluded the feature worked.
  The pair is now inverted and hoisted outside the body limit, the timeout and CORS, so a
  `413`, a `408` and a CORS preflight all carry an id.

  Separately, **both** `413` paths violated RFC-ACDP-0007 §5. With `Content-Length` set,
  `RequestBodyLimitLayer` short-circuits before calling inner and hard-sets
  `Content-Type: text/plain`, which the outermost `if_not_present` media-type layer cannot
  correct. Streamed (no `Content-Length`), the response reached the per-route layer and so
  advertised `application/acdp+json` — over a plain-text body. A response-rewriting layer
  now gives both the `payload_too_large` envelope; it keys on whether the body actually
  parses as an envelope rather than on the media type, because the streamed path already
  claimed the right type while carrying the wrong payload.

  The existing regression test asserted status and `Content-Type` only, and passed against
  a non-JSON body via a code path its own comment misidentified. It is split into
  `_with_content_length` and `_chunked`, both asserting `error.code`. The Content-Length
  variant was run red before the fix.

  **`408` is deliberately not covered.** RFC-ACDP-0007 §5 has no wire code for a timeout,
  so the envelope would have to say `internal_error` — worse than silence, because it
  misattributes a client-side timeout to a server fault. Minting a `request_timeout` code
  is a change to the shared §5 registry and is tracked separately.

- **SQLite swallowed a corrupt `contributors` column and skipped an admission check while
  reporting success.** The predecessor's `contributors` was decoded with
  `.ok().and_then(...).unwrap_or_default()`, so an unreadable value became an empty list. That
  is not cosmetic: `contributors` feeds the RFC-ACDP-0014 §4 predecessor-admission check, so a
  legitimate contributor was refused with `SupersededTarget::NotFound` **while the check
  itself reported success**. Postgres already errored here (`TEXT[]` decoded with `?`), so this
  was a backend divergence as well as a silent wrong answer. SQLite now fails loudly.

  The finding that surfaced this claimed it contradicted the existing test
  `admission_is_not_skipped_when_the_predecessor_body_is_undecodable`. It does not — that test
  corrupts `body_json`, a different field. It establishes the principle rather than covering
  the case, which is why this ships with its own test.

- **SQLite relied on sqlx's implicit busy timeout.** `commit_publish` holds `BEGIN IMMEDIATE`
  across the receipt-minter callback, so a writer can legitimately hold the write lock for as
  long as that callback plus an fsync takes; a concurrent writer waiting less than that
  surfaced `SQLITE_BUSY` as a 500 with no retry — a spurious failure under ordinary
  contention. The timeout is now set explicitly and named. Mapping busy to a retryable 503 is
  a separate change in a different crate and is not included.

- **Postgres narrowed `version` to `i32`, where SQLite used `i64`.** `PublishRequest.version`
  is a client-supplied `u32`; Postgres bound it `as i32` (wrapping above 2^31-1, silently,
  since Rust's `as` truncates rather than panicking) with an `INTEGER` column to match.
  `contexts.version` is now `BIGINT` and the casts are `i64::from`, so the two backends agree
  by construction rather than by both being narrow.

  **Scope, stated precisely:** this is a parity fix and defence in depth, **not** a live
  exploit closed. The finding called it unreachable "because `put()` has no production
  callers", which was wrong — the casts were in `commit_publish` and the row INSERT, both on
  the live publish path. It is nonetheless unreachable, for a different and measured reason:
  the request builder requires `version == 1` for a first publish and `prev + 1` for a
  supersession, so a publish carrying 3_000_000_000 is refused before the store sees it on
  both backends. Reaching 2^31 would take ~2 billion sequential supersessions.

### Added

- **Indexes on `contexts(domain)` and `contexts(expires_at)` in both backends.** Both columns
  are used by `GET /contexts/search` filters and neither was indexed, so both filters were
  full scans. The `expires_at` index is partial (`WHERE expires_at IS NOT NULL`), matching the
  predicate the query actually writes. The `data_period` filters remain scans deliberately —
  they read out of `body_json` through a conversion, so indexing them needs an expression or
  generated column, which is a schema decision with no measured volume behind it yet.

- **Retrieval could serve a context as `active` while also serving its `retracted` event.**
  `get()` and `lineage()` read the context row and its lifecycle events as **two separate
  queries with no shared snapshot**, on *both* backends. A retraction committing between the
  two reads produced a response carrying `registry_state.status: "active"` alongside a
  `retracted` lifecycle event — contradicting the RFC-ACDP-0013 §7.2 precedence
  (`retracted > superseded > expired > active`) that both backends' `row_to_context` is
  documented to guarantee. A consumer trusting `status` would act on withdrawn data.

  Unlike the search bugs above, this one was present on **both** SQLite and Postgres; it is
  not a divergence but a shared defect.

  Fixed by reconciling the row-derived status against the events actually loaded, in one
  shared helper (`acdp_registry_store::lifecycle::reconcile_retraction`) applied at all four
  call sites. Retraction wins from either source, so the served pair is **self-consistent by
  construction** — a future read path that forgets to take a snapshot cannot reintroduce the
  contradiction. Failing closed is deliberate and asymmetric: being briefly stale about a
  republish is safe, serving `active` for retracted data is not.

  A transaction-per-read would also have fixed it and was the first design; reconciliation
  was chosen because it fixes the shape rather than the one call site. Two preconditions were
  checked against the code before relying on the cheaper fix — `lifecycle_events` has no
  `DELETE` in either backend (append-only), and the event and the denormalized flag are
  written in the same transaction — so the event log can never be missing a retraction the
  flag knows about.

- **Search: `q=` returned different results on SQLite and Postgres, and a code comment
  claimed it did not.** SQLite indexed with FTS5's default `unicode61` tokenizer — no
  stemmer, no stopwords — while Postgres used `plainto_tsquery('english', …)` over an
  `english` tsvector, which stems and drops stopwords. Measured on both engines:

  | query | sqlite (before) | pg |
  |---|---|---|
  | `q=running` against "run report" | 0 rows | 1 row |
  | `q=the` against "the quarterly figures" | 1 row | 0 rows |

  **Postgres's semantics win, and SQLite was raised to them.** Postgres is the production
  backend and stemming is better search behaviour, so degrading it to reach agreement would
  have been a product regression rather than a fix. The cost lands on SQLite's FTS index,
  which is derived data rebuilt from `contexts` — reversible, and no context data is touched.

  Two mechanisms, matched in two places: migration `013_fts5_porter.sql` switches
  `contexts_fts` to `tokenize = 'porter unicode61'` (stemming), and `fts5_escape` drops
  stopwords query-side because porter does not (measured). **This changes user-visible search
  results on SQLite:** inflected queries now match, and stopword-only queries now match
  nothing.

  Three of FTS5's four operator keywords (`NOT`, `AND`, `OR`) are themselves English
  stopwords, so they are now dropped before quoting rather than quoted — which is exactly
  what Postgres does (`plainto_tsquery('english', 'NOT hack')` → `'hack'`). That is strictly
  safer: a caller cannot obtain operator semantics either way, and with `OR` removed they
  cannot widen a query to a disjunction. `NEAR` is the one keyword Postgres keeps, so it is
  still quoted, and it now carries the operator-neutralization property in the tests.

  **The honest limit:** FTS5 `porter` and Postgres's snowball `english` are different
  implementations and will not agree on every word in the language. The parity suite pins the
  *mechanisms* — a stemmed match happens, stopwords are dropped, dropping them does not empty
  the rest of the query, terms are AND-ed, case folds — rather than claiming stemmer
  identity, which would be the same kind of overclaim as the comment this removes.

  The stopword list is PostgreSQL 16's own `tsearch_data/english.stop` (127 entries) and is
  **verified against Postgres at test time** rather than trusted as a hand-copied table: the
  pg suite asserts every entry is still a stopword according to the server. A hand-maintained
  list whose staleness nobody notices was the failure mode worth designing out.

- **Search: `data_period_start_after` / `data_period_end_before` returned wrong results on
  SQLite.** The two predicates compared RFC 3339 timestamps **lexicographically as TEXT**,
  because they read out of `body_json` (chrono serde output — `Z` suffix, 0/3/6/9 fractional
  digits) while the bound was bound as `to_rfc3339()` (`+00:00` suffix). Two different
  serializers for the same instant, and since `'+'`(0x2B) `< '.'`(0x2E) `<` digits `<
  'Z'`(0x5A), a stored whole-second value sorted *after* a bound naming that very same
  instant. Measured directly in sqlite3:

  ```
  '2026-01-01T00:00:00Z' <= '2026-01-01T00:00:00+00:00'   ->  0
  ```

  So an inclusive upper bound **excluded** a context whose period ended exactly on it, and
  the mirror case **wrongly included** one that started before a lower bound. Postgres was
  always correct — it casts to `timestamptz`. **This changes user-visible search results on
  SQLite: queries that silently returned the wrong set now return the right one.**

  Fixed by comparing numerically via `unixepoch(…, 'subsec')` on both sides, which
  normalizes suffix and fractional width together. Binding a `Z`-normalized string instead
  would *not* have been enough — `'…00Z'` still sorts after `'…00.500Z'`. The fix is
  query-side only: no migration, and `body_json` is untouched because its exact bytes are
  the `content_hash` preimage.

  `created_at` / `expires_at` filters were checked and are **not** affected — both their
  stored and bound sides go through `to_rfc3339()`, so their lexical order does hold. That
  was measured rather than assumed by analogy.

### Added

- **A cross-backend parity suite (`acdp_registry_store::parity`), so storage divergence
  fails a test instead of shipping.** The bug above survived a green CI because the
  conformance suite is SQLite-only and *neither* backend's contract suite exercised a single
  search filter — 114 stored rows across the pg suite carried zero `data_period` values.
  Assertions now live once, generic over `ExtendedRegistryStore`, and each backend
  contributes a thin `tests/parity.rs` that runs them; a divergence fails both suites rather
  than hiding in whichever one nobody duplicated it into. Demonstrated by reverting the fix:
  the shared test goes red on SQLite and stays green on Postgres.

- **CI: 23 Postgres contract tests reported success when no Postgres was present.** Every
  test in `crates/acdp-registry-pg/tests/store_contract.rs` opens with
  `let Some(url) = pg_url_or_skip() else { return };`, and an early `return` from a
  `#[tokio::test]` is a **pass** — so an absent database was indistinguishable from a
  passing one in CI output. Measured on the same binary, both exiting 0 and both printing
  `23 passed`: **0.01s** with `ACDP_REGISTRY_TEST_PG_URL` unset versus **0.62s** with it
  set. A 62x gap behind an identical green summary, and nothing in the log to tell them
  apart. Deleting or mis-spelling the URL in `ci.yml` would have taken the required `tests`
  job permanently and invisibly green on zero Postgres assertions.

  Fixed by `ACDP_REQUIRE_PG`, mirroring the `ACDP_REQUIRE_CONFORMANCE` gate this repo
  already built for the same bug class and never applied here: when set, a missing
  `ACDP_REGISTRY_TEST_PG_URL` panics instead of skipping. It is set on the two CI steps
  that provide a Postgres service. The gate is deliberately **opt-in rather than
  unconditional**, because `cargo test --workspace` runs this suite with no database on
  purpose — an unconditional panic would break a step that is correct as written.

  Currently gates the 23 tests in `acdp-registry-pg`; the 11 in
  `acdp-registry-server/tests/pg_integration.rs` hold a second copy of the helper and are
  covered by the same CI env var once that copy is updated.

- **Two CI guards that were never guarding, plus three claims that were false.**
  Unit H-C. Every finding below was reproduced by experiment before being fixed —
  the audit that produced them could not compile or run anything, and running them
  refuted part of what it reported.

  **The receipt key the registry published was never checked against the key it
  signs with.** RFC-ACDP-0010 §8 has a consumer verify a receipt against the key it
  resolves from `/.well-known/did.json`. Setting `receipt.rs:100` to `&[0u8; 32]` —
  publishing an all-zero key while still signing with the real one, so no consumer
  could verify any receipt — left the workspace suite at 524 passed / 0 failed,
  byte-identical to baseline. The only existing read of `publicKeyMultibase`
  asserted `starts_with('z')`; the HTTP test asserted fragment ids; and the
  receipt-verifying test used a key derived from its own seed rather than the served
  document. Two guards added, each falsified against that mutation; the end-to-end
  one takes its verification key only from the served bytes.

  **A wrongdir `ACDP_SPEC_DIR` silently disabled four conformance ratchets even
  under require-mode.** `spec_families()` returned `None` on a missing
  `registries/profiles.json` without consulting `require_conformance()`, unlike its
  neighbour `spec_fixtures()`. It now asserts.

  Narrower claim than it first appears, and the correction matters: this does **not**
  make CI catch a wrongdir spec directory — CI already caught it. The run was failed
  by `registry_advertisable_profiles_matches_spec_derived_set`, which gates on
  `spec_root()` alone, proceeds, and dies in `read_json`. **The last tripwire was one
  test being inconsistent with its four neighbours**, so the obvious tidy-up — making
  all five skip uniformly — would have produced exactly the fully-green failure the
  audit imagined. That trap is now documented at the test itself, which keeps an
  independent require-mode check rather than sharing one.

  Also fixed there: a bare-fixtures `ACDP_SPEC_DIR` in default mode used to fail hard
  via that same panic, despite being a layout `resolve_fixture_dir` explicitly
  supports. And `replays_spec_fixtures_when_present` was not skipping but *degrading*
  — bucketing fixtures by filename heuristic instead of spec-declared families while
  reporting ok. The silently-affected set was six tests, not five.

### Changed

- **The published Docker image now builds from the committed lockfile.** `--locked` on
  `cargo chef cook` and `cargo build --release` in `docker/Dockerfile`. `Cargo.lock` was
  tracked and in the build context but neither build step used it, so the ghcr image could
  compile dependency versions no CI run ever tested. The pre-existing `--locked` on
  `cargo install cargo-chef` pins the *tool*, not this repo's build — it reads as though the
  build were already locked, which is how this stayed open, and it is now annotated to say so.
  Verified in a real build: current lockfile compiles, a deliberately stale one fails with
  `the lock file /app/Cargo.lock needs to be updated but --locked was passed to prevent this`.
  Completes the half of the CI-lockfile change (#227) that was reported not-done rather than
  closed.

- **CI: supply-chain and reproducibility gates that actually assert something.**
  `[sources] unknown-registry`/`unknown-git` flipped to `deny` (free today: zero
  git-sourced and zero non-workspace path dependencies). `--locked` added to all 18
  CI cargo invocations that resolve dependencies, so a build can no longer silently
  use versions the lockfile never pinned.

  Two settings were deliberately **not** flipped, each for a measured reason.
  `yanked` stays `warn` because `deny` fails today — this workspace already depends
  on two yanked crates (`spin`, `wnaf`) and nobody noticed, which is the finding
  rather than a reason to shrug; clearing them needs a `Cargo.lock` update and should
  land in the same change that flips the setting. `[bans] multiple-versions` stays
  `warn` because duplicate versions exist today and are normal in a Rust graph.

- **CI: the memory test step was renamed, not fixed, and the name now says so.**
  `cargo test (memory)` → `cargo test (memory build; storage backend NOT covered)`.
  Under those flags all four integration files are cfg-gated away and report 0 tests
  each; the 71 that run are config validation and workspace scans, none touching
  `MemoryStore`. **The fix the finding proposed cannot work**:
  `acdp-registry-server` is bin-only, `MemoryStore` lives in a module of that binary,
  and integration tests link only against a lib target — a probe importing it fails
  with `error[E0433]: unresolved module or unlinked crate`. Covering `MemoryStore`
  requires a test module inside `memory_ext.rs` or a crate restructure; neither is in
  this change. A `MemoryStore::get` that always returned `Ok(None)` still passes this
  step. It is now labelled accurately rather than left overclaiming.

- **CI: coverage gained a floor** (`--fail-under-lines 75`), where before it rendered
  a summary and uploaded lcov while asserting nothing and could only ratchet down
  invisibly. 75 is deliberately conservative: the local baseline is 79.91% lines, but
  that run had no Postgres, so CI's figure will be higher than the number the floor
  was derived from. This fails the coverage *job*; it does not block a merge, because
  `coverage` is not a required check.

- **CI: `acdp-registry-types` is now built and tested at `feat=[]` deliberately**
  (closes #221), with a correction #221 does not carry — that configuration was
  *already* compiled there incidentally, by `cargo test -p acdp-registry-pg`, which
  declares the dependency `default-features = false`. A stale comment in `ci.yml`
  claiming otherwise has been corrected. The incidental coverage depends on that step
  keeping its `-p` form, since `--workspace` unifies features.

- **Security (availability): `GET /contexts/search?limit=` could abort the registry
  process from an unauthenticated request.** The handler sized its accumulator with
  `Vec::with_capacity` directly from the caller-supplied `limit`, using `.max(1)` — a
  floor, with no upper bound. `limit` is a `u32` off the query string and the route is
  reachable without an `Authorization` header, so `?limit=4294967295` requested 893 GB in
  one allocation and the process aborted with SIGABRT. The store-side `.min(100)` did not
  help: it runs after the accumulator is allocated. `limit` is now clamped in the handler
  to the same cap the stores enforce. Regression test landed deliberately red first, so
  the PR's own CI history shows it catching the live defect.


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
  **Update (U-502): that fix has landed for a bounded scope** — `.cargo/mutants.toml`
  and the scheduled `.github/workflows/mutants.yml`, baseline 74 mutants / 48 viable /
  46 caught / 2 survivors. See the U-502 entry at the end of this file. `#216` remains
  open for the rest. Left standing rather than rewritten: it was true when written.

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
    **Two updates, appended rather than rewritten, since both were true as
    written:** the mutation-oracle thread moved from `#130` to `#216` when
    `#130` closed; and the oracle now EXISTS for a bounded scope (U-502 —
    `.cargo/mutants.toml`, scheduled `mutants.yml`, 46 of 48 viable mutants
    caught, 2 survivors). See the U-502 entry at the end of this file.
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

### Added

<!-- U-502 #216 mutation oracle (lane-2) -->

- **A mutation oracle, the number it ratchets against, and the harness bug that
  made the first number a lie** (`#216`). The conformance file has never been able
  to prove that a test it vouches for *asserts* anything. Two mechanisms guard
  those claims and both are PRESENCE oracles: the substring guards over
  `include_str!` (`covered_direct_families_have_present_test_functions`,
  `partial_direct_test_functions_are_present`) and the compile-checked
  `DIRECT_FNS` table added by `#249`. A test that exists, compiles, runs and
  asserts nothing satisfies every one of them. Breaking the code and watching the
  test go red is the only thing that does not, and that is now wired.

  **THE BASELINE, at `52c0111`, `cargo-mutants 27.1.0`, 6m at `-j4`:**

  | outcome | count |
  |---|---|
  | mutants in scope | **74** |
  | viable (the honest denominator) | **48** |
  | **caught** | **46** |
  | **survivors (missed)** | **2** (one since killed → budget **1**) |
  | timeout | 0 |
  | unviable (does not compile) | 26 |

  **The committed survivor budget is 1** (survivor 1 below was killed rather than
  accepted), in
  `.github/workflows/mutants.yml`'s `env`, alongside an exact-equality check on
  the scope size.

  **Read the denominator honestly: 48, not 74.** The 26 unviable mutants are
  mutations that do not compile — every one is
  `replace <fn> -> <T> with Ok(Default::default())` (or similar) where `T` has no
  `Default`; the five axum handlers `log_entries`/`log_proof`/`log_checkpoint`/
  `inclusion_proof_response`/`consistency_proof_response` account for 20. There is
  nothing there for a test to catch, so they are excluded from the claim rather
  than counted toward it.

  **A FIRST BASELINE OF "48 CAUGHT, 0 SURVIVORS" WAS MEASURED, BELIEVED, AND WAS
  WRONG.** It is recorded here because the way it was wrong is the most useful
  thing this unit produced. `cargo-mutants` tests each mutant in a `$TMPDIR` copy
  of the tree and does not copy `.git` by default. `conformance_gate.rs`'s
  `no_tracked_file_contains_a_conflict_marker` shells out to `git ls-files`, which
  fails there; `cargo test` stops at the first failing test binary; and
  `conformance_gate` runs *before* `http_integration`. So every mutant whose real
  killer lived later was scored CAUGHT by that unrelated panic — **41 of the 48**,
  i.e. every `handlers/log.rs` verdict. Only 7 were genuine, all in `receipt.rs`,
  and only because core's unit tests happen to run before the poisoned gate. Had
  that gate run earlier, all 74 would have been false.

  The hazard was *already written down* in `.cargo/mutants.toml` — "a pre-existing
  failure elsewhere in the workspace would mark mutants CAUGHT for the wrong
  reason" — **and it was checked**: `cargo test --locked --workspace`, 632 passed,
  0 failed, quoted as evidence the zero was real. The check was sound and its
  answer was true. It was about **the tree we were standing in**, and the verdicts
  come from **the tree the tool builds**. Naming a hazard is not checking it, and
  checking something is not checking *it*. Fixed by `copy_vcs = true`; `.git` here
  is a 4 KB worktree pointer, so it costs nothing.

  What exposed it was not doubting the number — a 0 reads as success — but reading
  the killing test's *name*: `no_tracked_file_contains_a_conflict_marker` cannot
  possibly be killed by mutating `root_for`.

  **THE HARNESS CONTROL, because a budget with no control is a number on trust.**
  Under the harness and in the copy tree, `root_for -> String::new()` is CAUGHT by
  the ten `http_integration` log tests that genuinely exercise it — matching a
  by-hand mutation of the same line run *outside* the harness. Two independent
  derivations agreeing. `.github/workflows/mutants.yml` also carries a standing
  check for the same class: it fails if any single test is the SOLE failing test
  for more than half the caught mutants. Falsified on the two real runs, not on
  fixtures — the pre-`copy_vcs` run FAILS it (41 of 48 = 85%), the corrected run
  passes (largest sole killer 8 of 46 = 17%). `cargo-mutants` cannot do this for
  us: its unmutated baseline runs PACKAGE-scoped even under `test_workspace`
  (`baseline.log` says `--package=acdp-registry-core`, the mutant logs say
  `--workspace`), so it never builds the binary that was failing and a green
  baseline is compatible with every verdict being noise.

  **THE TWO SURVIVORS, enumerated with a reason each, not totalled.**

  1. `handlers/log.rs:117:19` — `replace != with ==` in `requester_can_retrieve`.
     **A real unasserted branch, and security-relevant.** Line 117 is
     `if stored != tenant {`, the tenant gate. Inverted it is wrong both ways: a
     matching tenant is DENIED, and a **mismatched tenant falls through to
     `Ok(true)`** — disclosure across the tenant boundary. The function is live
     (`:269`, `:303`) and gates the §8.2 `ctx_id` proof surface and every `leaf`
     echo — i.e. `/log/proof?ctx_id=…`, not `/log/entries`, which moved to the
     batched predicate in H-I-s (`:474` records that). No test reaches it: every
     `/log/proof` test in `http_integration.rs` fetches via `get_json` with **no
     `X-Tenant-Id`**, so `requested_tenant` is always `None` and the
     `if let Some(tenant)` block never executes.
     `log_entries_leaf_presence_is_tenant_scoped` looks like the guard and is not —
     it exercises the batched `/log/entries` path, a different predicate.
     **KILLED, not budgeted.** The claim on `http_integration.rs` was granted and
     the two tests landed in the same commit as the ratchet:
     `log_proof_ctx_id_is_served_to_the_owning_tenant` and
     `log_proof_ctx_id_is_withheld_from_a_foreign_tenant` — two tests rather than
     one because inverting the operator breaks **both** directions, and a single
     test would stop at whichever assertion ran first and never evaluate the other.
     Falsified: with `==` the first fails `left: 404, right: 200` (the owning tenant
     denied) and the second `left: 200, right: 404` with the response body carrying
     the other tenant's full `leaf` — `ctx_id`, `content_hash`, `key_fingerprint`,
     `receipt_hash`. Reverted, both green. **Budget therefore 1, not 2.**

     **Say what this is accurately.** The code is CORRECT: `!=` is the right
     operator and no cross-tenant disclosure ships. What the oracle found is an
     *unguarded correct property* — nothing executed the branch, so nothing would
     have noticed if it stopped being correct. "Mutation testing found a
     cross-tenant disclosure" would be false.
  2. `handlers/log.rs:131:18` — `replace == with !=` in `root_for`.
     **Accepted: an equivalent mutant.** Line 131 is `if tree_size == current {`
     and it guards *only* `log.cache_root(...)`. `root_for` returns the same
     `root` on both branches, and append-only makes any `(size → root)` pair
     immutable, so a cached historical root is still correct and an uncached
     current root is merely recomputed. Nothing observable changes — only which
     sizes are cached. No assertion over responses can kill it; doing so would
     need instrumentation counting merkle computations, which is a performance
     harness, not a correctness one.

  **WHAT THIS NUMBER DOES NOT COVER, stated where the number is rather than in a
  footnote.** The scope is two files, chosen to be ones no other unit is editing:
  `acdp-registry-core/src/receipt.rs` (9) and `src/handlers/log.rs` (65). It is
  **not** the workspace, which is **1398** mutants at this sha — roughly 2.4h at
  the ~6.2s/mutant marginal cost in `DECISIONS.md` #17. It deliberately excludes
  `src/handlers/context.rs` (**134** at this sha), held by another unit this wave:
  a budget keyed to a file being rewritten underneath it goes red for reasons
  unrelated to what it guards, and a red check nobody can explain gets disabled.
  The drift alone shows the exclusion was right — #17 measured `context.rs` at 132
  and the workspace at 1383 one day earlier.

  **And the conformance tests only assert under `ACDP_SPEC_DIR`.** 42 of the 70
  tests in `conformance.rs` — essentially every `Direct`-registered family test the
  coverage tables name — return early without it:
  `let Some(fixtures) = spec_fixtures() else { … return; }`. The suite prints
  `69 passed` either way (0.09s skipping versus 0.36s doing the work), so the
  omission is invisible in a green log. The first baseline ran that way, meaning
  the tests `#216` is *named after* contributed nothing to it. Both the local
  re-measurement and the scheduled job now set `ACDP_SPEC_DIR` (pinned
  `d1f06d0d…`, the same ref `ci.yml`'s conformance job uses) and
  `ACDP_REQUIRE_CONFORMANCE=1`, so a broken spec checkout fails loudly instead of
  42 tests quietly skipping. Verified by reading the work done rather than the
  pass line: for the same mutant, the conformance binary takes **0.55s** in
  require mode against **0.12s** in default, while a control binary of the same
  test count is 0.04s in both. One conformance test remains out of reach —
  `playground_compiled_in_but_runtime_disabled_keeps_admin_route`, which needs the
  non-default `playground` feature. A three-command wrapper would triple every
  mutant's cost (#17: ~6.2s → ~22s) to recover one test, so it is declined
  deliberately rather than overlooked.

  **`#216` stays open.** Its item 1 is fault injection over `src/` generally; this
  is a bounded 74-mutant ratchet. PARTIAL BY DESIGN.

## U-501 — #242: publishes that fail late are now charged on two of four branches

`P5` (`H-A`) split the publish limiter into `peek` (read-only, never inserts) before the
pipeline and `record` (charges) on the success path. That removed a real vulnerability — an
unauthenticated caller could spend another agent's budget by naming them, and grow the bucket
map without bound because the map key was attacker-controlled — and disclosed, as `#242`, the
gap it left: a publish that fails *after* the limiter costs a full verify plus a store
round-trip and is never charged.

`#242` recorded two candidate fixes and rejected both. Post-hoc classification of the error at
the central `record_publish(e.wire_code())` wrapper is a denylist over a `#[non_exhaustive]`
enum, so it **fails open** as variants are added. Reserving at `peek` time puts the reservation
in an attacker-keyed map entry, which is the unbounded growth `peek`-not-inserting exists to
remove. Both rejections still stand and neither was shipped.

### What changed

A `PublishCharge` drop guard (`rate_limit.rs`). Once armed it charges on **every** exit —
`?`, explicit return, panic, a cancelled request future. It classifies nothing, which is
precisely the property the rejected denylist could not have: an error variant that does not
exist yet is charged the day it is introduced, with no edit. Fail-closed by construction.
Arming is monotonic, so the pre-flight arm and the success-path arm collapse into one
mechanism without double-charging.

The guard is armed only where the signer is **proven**, so it never inserts on an
unauthenticated path. `peek` is untouched and `peek_does_not_create_a_bucket` still passes
unmodified.

### The finding that made this bigger than one branch

`#242` assumed only `enforce_pinned_signature` was a clean charge site — one branch of four —
and that anything more needed an SDK change. That was not true. `acdp-server`'s own `did:key`
pipeline (`crates/acdp-server/src/registry/server.rs:492-518`) establishes identity with two
functions that are already public and already outside the `client` feature gate:
`compute_content_hash` and `verify_publish_request_signature_offline`. Composing them in the
handler proves the signer offline, before the SDK call, with no new dependency and no
cross-repo write.

**Both halves are load-bearing, and the order is not cosmetic.**
`verify_publish_request_signature_offline` verifies the signature over `content_hash` but never
binds `content_hash` to the body. Keyed on it alone, a captured `(agent_id, content_hash,
signature)` triple replayed under a *different* body reads as "identity proven" and spends the
real agent's budget. Recomputing the hash first kills that. This was not a theoretical worry:
deleting the hash comparison reddened **nothing** in the entire suite until
`a_replayed_envelope_over_a_different_body_does_not_spend_the_budget` was written for it.

### The four-way split, stated rather than left to be discovered

| branch | late failure charged? | |
|---|---|---|
| `did:key` | **yes** | proven offline in the handler before the SDK call |
| playground, pinned | **yes** | `enforce_pinned_signature` proved it before the SDK call |
| playground, unpinned | no — **by design, permanently** | nothing is verified at all, so there is no identity to charge |
| production `did:web` | no — **remaining gap** | identity is established only inside the resolver-backed SDK call |

The two "no" rows are not the same kind of thing and must not be collapsed. Arming the
unpinned playground branch would key an insertion on an attacker-supplied `agent_id` — the
shape `#242` rejected. The `did:web` row is outstanding work: closing it from this side would
need a second DID-document resolution per publish (network cost, a second SSRF surface, a cache
that can disagree with the SDK's), all worse than the gap. It needs an SDK seam, designed in
`plans/cross-repo/acdp-rs-publish-charge-seam.md` and filed upstream against `acdp-rs`.

`late_failures_are_charged_on_exactly_two_of_the_four_publish_branches` pins the split with an
`assert_eq!` on the count, not a `>=` floor — a floor passes the very regression it exists to
catch. It reddens in both directions: removing an arm, and adding one to an unauthenticated
branch.

### Cost, priced rather than hidden

The `did:key` branch now pays one extra JCS canonicalization + SHA-256 and one extra signature
verification per publish, because the SDK redoes both. That is the price of keeping the fix
in-repo, and the cross-repo seam above is what removes it — along with the `did:web` gap.

### A test renamed because this change made its name false

`a_publish_that_fails_late_does_not_consume_the_agents_budget` is now
`a_publish_that_fails_before_the_signer_is_proven_does_not_consume_the_agents_budget`. The
failure it exercises is a tenant-check rejection, which runs before the branch dispatch — the
unproven side of the line. Its old name would have read as a direct contradiction of the new
tests sitting beside it.

## U-505 — making the repo's deferred work measurable

The repo's real backlog was one open issue plus an unknown number of follow-ups recorded in
prose inside two cumulative files that nothing scans. A follow-up nobody measures is
indistinguishable from one that does not exist — and worse, because the prose records it, so it
*reads* as tracked.

### The count

| source | deferred items | how bounded |
|---|---|---|
| `ASSUMPTIONS.md` (2535 lines) | **35** | exact; scanner bound-checked to 0 unexplained markers |
| `DECISIONS.md` (2328 lines) | **42 candidate blocks** | superset by construction; prose mentions included deliberately |

`DECISIONS.md` is **2328 lines, not the 6400+** the unit assignment estimated. All eight
`file:line` citations in the assignment were exact.

### Why the enumeration took three attempts, which is the transferable part

A line-anchored `grep '^- \*\*Status:\*\*'` finds **88** of the 120 status markers in
`ASSUMPTIONS.md`. The 32 it misses are not exotic:

- markers wrapped mid-paragraph, because `grep` is line-based
  (`... would have stayed green with the guard deleted outright. **Status: CONFIRMED`);
- `**Status (updated 2026-09-01):**`, where a parenthetical sits between the key and the colon;
- entries with **no `Status` line at all**, whose bullet *is* the status
  (`- **UNCONFIRMED — awaiting human ruling:** ...`) — **9 of these**, structurally invisible to
  any status-line scan;
- one item that is an `###` **heading**, not a bullet
  (`### UNCONFIRMED: the four new steps run clippy, not cargo build`);
- one recorded only as an update inside another entry
  (`- **Update, 2026-09-11 — PARTIALLY narrowed, still UNCONFIRMED.**`).

The last two were found **only** by a bound check: assert that every `UNCONFIRMED` token in the
file falls inside a counted block, then read the ones that do not. That check turned up five
stragglers, of which three were genuine prose and two were real items the parser had missed. A
scanner that is not bound-checked reports a confident number that is simply the number of items
matching its own assumptions.

### An open item is a claim about the past, and half of them had expired

Every item verified against the tree rather than inferred from the record. Of the ones checked,
**more were already fixed than were still live:**

| recorded as deferred | actual state | evidence |
|---|---|---|
| `/metrics` sets no cache headers, dismissal deserves revisiting | **ALREADY DONE** | `lib.rs:265-271` sets `Cache-Control: no-store` via a route layer, with a comment covering exactly the 200-vs-401 concern raised |
| EdDSA PEM case still fails late; `validate_config` narrowed to `jwt_secret` | **ALREADY DONE** | the EdDSA/PEM check is in `validate_config` (`main.rs:111-117`), and `validate_config` runs at `:83`, before every `store.migrate()` (`:677`, `:725`, `:758`) |
| `acdp-playground` types webhooks as a closed `Literal` of three, dropping two lifecycle events | **ALREADY DONE** | `acdp_client/models.py` `WebhookType` now lists all four |
| 14 cursor-error literals duplicated across two store crates | **ALREADY DONE** | consolidated into `acdp-registry-store/src/cursor.rs`; the `DECISIONS.md:1021-1022` line pins are dangling and now point at unrelated code |
| `storage-memory` uncovered by CI | **ALREADY DONE** | `ci.yml:59`, `:154`, `:324` |
| `dtolnay/rust-toolchain@master` — the loosest pin in the repo | **ALREADY DONE** | SHA-pinned at `6c977a6c…` in all 8 uses |
| add `bump-spec.yml` to this repo | **ALREADY DONE** | `.github/workflows/bump-spec.yml` exists |

### Still open, verified live, filed

- **#265** — CI's four feature-configuration steps run `cargo clippy`, not `cargo build`
  (`ci.yml:146-165`). Clippy does not run codegen or link, so a monomorphization or linker
  failure passes all four. A disclosed deviation from #200 that nothing ever decided.
- **#266** — `docker.yml` sets no `jwt_secret` and never boots the stack the quickstart ships.
  This is the specific hole the W3-U5 defect escaped through, still open.
- **acdp-website#43** — `webhooks.mdx` documents 2 of the registry's 4 webhook event types;
  `context_retracted` and `context_republished` are absent from the public docs.

### `plans/` is now partially tracked, and the exception is load-bearing

`.gitignore` ignored all of `plans/`. `/plan`'s cross-repo handoff writes a design for another
repo *here* (writing into a sibling needs a human gate; reading one never does) and then files an
issue *there* linking a GitHub blob URL — which 404'd for every such handoff, because the file was
never committed. acdp-rs#273 was filed that way and had to carry its design inline.

Ruling: **`plans/cross-repo/` is tracked; the rest of `plans/` stays ignored.** Per-feature plans
are per-run working documents and committing them adds churn; a cross-repo plan is a contract with
another repo and has to be linkable.

The form matters and was tested, not assumed: git does not descend into an excluded **directory**,
so a bare `plans/` makes `!plans/cross-repo/` unreachable. Verified by reverting to the bare form
and watching `git check-ignore` call the cross-repo file IGNORED again.

### Added

<!-- U-502 #216 verification -->

- **The mutation ratchet, verified by re-measurement rather than by assertion**
  (`#216`, U-502). The entry above records the baseline at `52c0111`
  (74 mutants / 48 viable / **46 caught / 2 survivors**) and states that one
  survivor was killed. This is the confirmation that it actually was, run at
  `6da5b7d` after the two tests landed:

  | outcome | at `52c0111` | at `6da5b7d` |
  |---|---|---|
  | mutants in scope | 74 | **74** |
  | caught | 46 | **47** |
  | **survivors** | 2 | **1** |
  | timeout | 0 | **0** |
  | unviable | 26 | **26** |

  8m at `-j6`. `handlers/log.rs:117:19` moved from MISSED to CAUGHT, and the log
  for that mutant names its killers: exactly
  `log_proof_ctx_id_is_served_to_the_owning_tenant` and
  `log_proof_ctx_id_is_withheld_from_a_foreign_tenant` — the two tests written for
  it, and nothing else. A mutant that changes verdict while the killing tests are
  named is the whole claim; "we added a test and the number went down" would not
  have been.

  The one remaining survivor is `handlers/log.rs:131:18`, the `root_for` cache
  equivalent mutant, accepted with its reasoning in the entry above.

  The **committed** ratchet and harness checks were then run against this report,
  extracted out of `.github/workflows/mutants.yml` itself so the text checked is
  the text that runs: ratchet `rc=0` at `scope=74 / budget=1`, harness check `rc=0`
  with the largest sole killer at 8 of 47 (17%), well under the 50% ceiling.

  **Appended rather than edited into the entry above, deliberately.** That entry was
  corrected in place while it existed only on an unmerged branch, and
  `DECISIONS.md` #18 records that the licence for doing so expired the moment it was
  pushed. It is pushed. So this is an append — which is the rule being applied to
  its author rather than merely written down by them.
## U-509 — #266: the documented quickstart is now a CI property

`.github/workflows/docker.yml` already booted a registry and hit `/healthz`. It did so by
hand-rolling `docker run` with `docker/config.docker.toml` mounted, so it never opened
`docker/docker-compose.yml` — and `cd docker && docker compose up --build` is what README.md's
"Production (Postgres + Docker)" section tells operators to run.

That distinction is the finding. W3-U5 existed because the quickstart did not boot: the compose
file shipped `ACDP_REGISTRY_AUTH__JWT_SECRET=changeme`, which `validate_config` rejects by name
(`crates/acdp-registry-server/src/main.rs:166`), so the stack exited rc=1 before serving a
request. **The defect lived in the one file CI never read**, which is why a user hit it before CI
did. It was fixed in `abfebf7` with nothing guarding it since.

### What landed

Three steps in `docker.yml`, driving `docker/assert-quickstart-boots.sh`:

- `--check` — boots the recipe through `docker compose`, asserts `/healthz` and `storage:true`
  (a stack that silently fell back to SQLite would still answer `/healthz`).
- `--check-auth-on` — boots it with auth enabled and a real secret, reaching `validate_config`'s
  auth-gated branch (`main.rs:129-142`) that the shipped `auth.enabled = false` never executes.
- `--self-test` — two negative controls, wired into CI rather than asserted in a PR body, matching
  the convention `assert-image-tags.sh --self-test` already set.

`compose.ci.yml` points the service at the image the job already built, so this costs no second
release build. It overrides **only** `image:`/`build:`; the environment block, config mount,
`depends_on` and postgres service are the recipe's own, verified by reading `docker compose config`.

### The negative control caught a decorative check — mine

The first draft of `--check-auth-on` exported `ACDP_REGISTRY_AUTH__ENABLED=true` before
`docker compose up`. **Compose forwards only the variables named in a service's own `environment:`
block**, and that one is not among them, so it never reached the container. The step booted the
auth-**off** stack and reported success. It could not have failed.

Nothing about the passing check revealed this. What revealed it was negative control (2) — auth on
with an empty secret *must* be refused — declining to go red. The control was right and the check
was wrong. This is the same shape as U-501's hash-bind (a security property fully implemented,
fully green, completely unguarded) and U-505's grep undercount, three units running: **the check
that passes is not evidence; the control that fails to fail is.**

Fixed with a CI-only overlay (`compose.ci-auth-on.yml`). Its effect is proven by control (2) now
firing, not by inspection.

### Falsified against the real historical defect

Not a synthetic break. `${ACDP_REGISTRY_JWT_SECRET:-}` in the shipped compose file was changed back
to `${ACDP_REGISTRY_JWT_SECRET:-changeme}` — W3-U5's literal defect — and `--check` exited 1 with
"the documented quickstart (docker compose up) did not come up healthy". Reverting restored green,
and `git diff --quiet docker/docker-compose.yml` confirms the file shipped byte-identical.

### The recipe gap this exposed, reported not papered over

The compose header tells operators to "set a real secret before enabling auth" but gives no env
path to enable auth. The obvious fix — a `${VAR:-false}` passthrough — is a **security regression**:
compose renders an unset variable as set-to-empty rather than absent, and env beats TOML, so an
operator who set `auth.enabled = true` in the file would have it silently forced back to `false`.
That is the same precedence trap the header already documents for `jwt_secret`. Logged as
`UNCONFIRMED` in `ASSUMPTIONS.md` for a unit that can design the passthrough safely, rather than
taken here to make a CI step convenient.

## U-511 — #271: an empty env override is treated as absent

Two behaviours composed into a footgun: an env var beats the TOML file, and `docker compose`
renders an *unset* variable as **set-to-empty** rather than absent. So every `${VAR:-}` passthrough
in a compose `environment:` block silently replaced whatever the operator wrote in their config
file. The shipped recipe carried **two separate caveats about this one rule**, which is the signal
that the rule was the defect rather than its documentation.

### The measurement that decided "correction, not breaking change"

The unit could have gone either way, and the deciding question — *could anyone be relying on the
old behaviour?* — turns entirely on what empty did today. It is not uniform:

| field type | empty override, BEFORE |
|---|---|
| number | hard ERROR (`invalid type: string ""`) |
| bool | hard ERROR |
| `Vec`, not a list-parse key | hard ERROR (`expected a sequence`) |
| `Vec`, list-parse key | `[""]` — a one-element list of nothing |
| `String` | overrode with `""` |

Four of five arms are a refusal to boot or a value nobody wants, so no deployment can have depended
on them. Only the `String` arm was leanable-on, and nothing in the repo documents it. Treating
empty as absent therefore **fixes three hard errors and one garbage value** and changes one arm
from "override with empty" to "fall through" — decided under the autonomy ladder rather than
escalated.

It is still made **loud**: `RegistryConfig::empty_env_overrides_ignored()` plus a startup `warn!`
names every dropped variable. A behaviour change nobody can see is the part that becomes a support
ticket.

### `${VAR:-false}` is not the safe form, and that is the counter-intuitive part

The obvious way to give the recipe an env path to enable auth is
`ACDP_REGISTRY_AUTH__ENABLED: ${VAR:-false}`. **That is still a security regression after this
change.** Verified with `docker compose config` rather than reasoned about: `${VAR:-false}` renders
the literal string `"false"`, which is *non-empty*, so it is a real override and would force an
operator's `auth.enabled = true` back to `false`. Only `${VAR:-}` — which renders `""` — becomes
safe. The recipe uses that form, and both the inline caveat and the header paragraph it replaced
now say what is true instead of warning about a trap that no longer exists.

### The test-fixture defect the falsification pass exposed

Falsifying the JSON-hatch filter reddened **four** tests, only one of which touched the broken
code. The other three inherited a leaked `ACDP_REGISTRY_AUTH__TENANT_AGENTS_JSON=""` — the failing
test panicked before reaching its own cleanup, and `cargo test` runs tests as threads sharing one
environment. A falsification run that names the wrong culprit is worse than no falsification,
because it sends the next reader to the wrong file.

Fixed with an `EnvGuard` that holds the lock and restores every variable it touched **on drop,
including on panic**. Re-running the same falsification now reddens exactly one test.

Two comments were corrected as collateral, both of which had become false: the existing env test's
`SAFETY` note claimed "no other test in this crate reads or writes process env" — already false
before this unit, since that same test does two separate env round-trips — and its inner note about
"preserving the only-one-test invariant". The invariant is now a lock, not an assurance.

### Upgrade ordering, stated because it is a real hazard

The recipe now passes an empty `ACDP_REGISTRY_AUTH__ENABLED`. A binary from before this change
rejects an empty boolean outright, so pulling the new `docker-compose.yml` against an older image
breaks the boot. Called out in README's Configuration section. Not mitigated in code — the
alternative is omitting the passthrough, which leaves the gap #271 was filed about.

## U-512 — #267: declining to make `sha-` tags immutable, and documenting what actually is

#267 asked for a pre-push existence check so a hand-run re-run of a `main` build could not repoint
`sha-<short>`. **Declined, with the argument recorded in `DECISIONS.md` and the residual risk
accepted explicitly rather than left implied.**

The deciding fact was checked rather than assumed: GHCR's package API exposes no tag-immutability,
tag-protection or retention setting — the returned keys are `created_at, html_url, id, name, owner,
package_type, repository, updated_at, url, version_count, visibility`. There is nothing at the
registry to turn on, so anything shipped here would be a *workflow* check, and the package is
repo-scoped, so anyone with package write can push over a tag without touching the workflow. Written
as one sentence with its limit inside — which is how the assign asked for it — the guarantee would
read: *"this workflow will not repoint a `sha-` tag, though anyone with package write access still
can."* That is not what #267 asked for, and shipping it under the name "immutability" is the
overclaim class this board keeps finding.

Meanwhile the immutable identifier already exists and costs nothing: the digest. Verified
end-to-end rather than asserted —

```
docker buildx imagetools inspect :sha-33bb3a3 --format '{{.Manifest.Digest}}'
  -> sha256:002469d2dc7d6f1263a058010976f0ec7e4a2ba52c6db3cdece1e866a9a29df3
```

— which matches the digest the packages API records for that tag. So the real problem #267 names,
that the `sha-` prefix *invites* being read as content-addressed, is a documentation problem.
`docker/RAILWAY.md` now answers it where operators actually choose a tag, including the cost of
pinning a digest (it never picks up a fix) so the trade is stated rather than sold.

### The counter-argument, kept rather than buried

A CI check **would** stop the realistic accident — a maintainer clicking *Re-run all jobs*. That is
the honest case for implementing, and it is recorded in `DECISIONS.md` next to the reasons for
declining rather than omitted to make the decision look cleaner. It does not carry because the harm
is bounded: the rebuild is from the same commit by construction, so what differs is build metadata,
`image.created` and the provenance attestation — reproducibility and audit, not behaviour.

### On not manufacturing mechanism

The escape hatch that would have made the fail-closed cost tolerable (a `workflow_dispatch` input
permitting overwrite) is pullable by anyone who could re-run the job in the first place. It would
have converted the guarantee into "immutable unless someone chose otherwise" — which is the status
quo with more moving parts and a more reassuring name. A decision recorded with its reasoning beats
a mechanism nobody wants.

No workflow change, so U-503's `type=sha` single-writer gate, `flavor: latest=false`, `assert image
tags` and its `--self-test` are all untouched and still running.

## U-514 — #276: making an operator hazard reach the operator

The #271 upgrade-ordering hazard — pull the image and `docker-compose.yml` together or the stack does
not boot — lived in a commit body, `ASSUMPTIONS.md`, the README and an issue. Release notes are
generated by `release-plz` from commit **subjects**, and a subject cannot carry it. So it reached
nobody upgrading.

### A premise that had to be disproved first

Mid-unit the board reported that v0.1.4's release PR (#278) had **dropped the change entirely** —
`acdp-registry-types` had no 0.1.4 section despite `config.rs` gaining 430 lines — and framed that as
upstream of the whole question.

It is staleness, not a defect. #278's branch parent is `0e5bd14`; `33bb3a3` (#275) is **not an
ancestor of it**. The numbers close it completely: commits touching `crates/acdp-registry-types/`
since its `v0.1.3` tag are **0 at #278's base** and **1 at current `main`**; for the server crate,
**2 and 3**, matching exactly the two entries #278 shows. The release PR's content is correct output
for the tree release-plz actually read.

Worth recording because the caveat was *stated and then not used*: the same message that reported the
finding also said the PR was BEHIND. Naming a limitation is not the same as gating on it.

### What was checked before designing (Rule 147)

`release-plz generate-schema` — the tool's own schema rather than recollection of its docs — confirms
`[changelog]` accepts `body`, `commit_parsers`, `commit_preprocessors` and `protect_breaking_commits`.
So option 1's mechanism **exists**; it was not rejected as impossible.

Two facts then shaped the choice:

- The schema carries **no default for `body`**, so adopting a template means authoring a full
  replacement for release-plz's built-in one without having that text authoritatively. The generated
  format today is good; silently regressing it is the realistic failure.
- **Squash bodies on `main` are multi-commit concatenations** — `0e5bd14`'s body is **1920 lines**.
  Rendering `commit.body` wholesale is unusable, so any viable template design reduces to a *targeted
  footer convention*: a convention needing enforcement, exactly like option 2, plus a template rewrite.

### Why option 2, in one sentence

A `body` template only runs during changelog generation, verifying it locally needs
`release-plz update` (which runs `cargo package --verify` across eight crates and failed on this
toolchain), and a release PR is open right now — so shipping it would have meant an undischarged
"it will work at the next release" with the release pipeline as the blast radius.

### What landed

`docs/UPGRADING.md`, one section per version, newest first, with 0.1.4 carrying the #271 hazard as a
table of which pull combinations boot. A version with no operator-visible change **says so** — an
absent section is indistinguishable from one nobody wrote.

Named from where readers and contributors actually land: `CHANGELOG.md`'s lookup table *and* its
"Where new entries go" section, `README.md`, and `docs/README.md`'s index.

`docker/assert-upgrade-notes.sh` is what stops this being a convention with nothing behind it. It
fails the build when the workspace version has no section, and it sits in `docker.yml` because that
workflow builds the operator-facing artifact and runs on `pull_request` — so the **release PR**, where
the version bump happens, is gated before it merges. Verified against a simulated 0.1.4 tree: the gate
passes and the section carries the hazard.

Four negative controls wired into `--self-test`. The one worth naming: `## 0.1.40` must **not**
satisfy a check for `0.1.4`. During a release those differ by a single trailing character, and a
prefix match would make the gate pass on the wrong section.

### The limit of this choice, stated rather than left to be found

The hazard text is not inside the rendered GitHub Release body. A reader who reads only the release
notes and follows no link still does not see it. Option 1 remains available and its design is written
up in `DECISIONS.md`; the convention established here is what such a template would render.

## U-516 — the gate that reported: making an enforcement claim testable

`docker/assert-upgrade-notes.sh` was added in U-514 with four negative controls, all of which fired
in real CI. Every one of them tested whether the *script* discriminates. None tested whether the
*workflow it ran in* could stop a merge. The unit then wrote "blocks" — a claim about the second
thing, resting on evidence about the first.

The controls were not weak; they were aimed one layer below the claim. A script that correctly
rejects a missing section, running in a job nobody requires, produces exactly the same green
transcript as a working gate. That is the whole failure mode, and it is invisible to any amount of
testing of the script itself.

What made it visible was a question the unit never asked: *which check name does this workflow
publish, and is that name in `required_status_checks.contexts`?* Two `gh api` calls. The answer was
`build`, and the list was `["rustfmt","clippy","tests","conformance (spec fixtures)"]`.

Two things worth keeping separate here, because conflating them is how this recurs:

**An equality, not a floor.** "At least one required job runs the script" cannot catch the case that
actually occurred, where the name assumed to be required is spelled differently from the one GitHub
publishes. The check has to be membership of a quoted name in a quoted list. `build` versus `docker`
versus `rustfmt` is precisely that distance.

**Observation and cause are separate sentences.** In the same window this unit ran, two sessions
independently misattributed one real failure — `release-plz update` failing locally — first to the
toolchain, then to a malformed macOS 27.0 SDK. Neither attribution was tested and both were wrong.
Measured here with `RUSTC_WRAPPER=""`:

```
cargo package -p acdp-registry-types   -> rc=0     (0 intra-workspace deps)
cargo package -p acdp-registry-server  -> rc=101   no matching package named `acdp-registry-auth`
                                                   found; location searched: crates.io index
```

The cause is `release-plz.toml`'s `publish = false`: nothing in this workspace is on crates.io, so
`cargo package` cannot resolve a path dependency on a sibling through the registry index. The
discriminator is exact — the one crate with no intra-workspace dependency packages cleanly; the seven
with at least one do not. Both wrong attributions pointed the same direction, at "local quirk,
someone could work around it", when the truth is structural and would reproduce on a clean Linux
runner. "Fails here, cause not established" would have been a complete and honest report; a confident
wrong cause was not.

This changes nothing about U-514's conclusion, and strengthens its stated reason: a `[changelog].body`
template cannot be verified short of an actual release for seven of the eight crates, anywhere.

## U-504 — #216: the mutation ratchet extended to `handlers/context.rs` (2026-09-13, lane-2)

Measured with `cargo mutants -j1`, scope from `.cargo/mutants.toml` (`receipt.rs` +
`handlers/log.rs` + `handlers/context.rs`), `ACDP_REQUIRE_CONFORMANCE=1`.

**Measured twice, at two spec pins, and the result is identical.** `main` moved the conformance
pin from `d1f06d0d49b73d411a3983d3877321ccaccd38e7` to `16211e64cf54973526a7af71adc8aed8996c3ae1`
during this unit (U-518's merge), and that diff touches the replayed fixtures — five modified
(`can-004`, `dk-001`, `dk-002`, `dk-004`, `data-ref-007`) plus a new `err-002`. Since
`mutants.yml` derives its pin from `ci.yml`, a baseline measured at the old pin would have been
true of a spec CI no longer uses. So the full 8-shard run was repeated at `af6647d` against the
new pin: **213 / 131 caught / 8 survivors / 1 timeout / 73 unviable, with the survivor set
content-identical across both pins.** The caveat is therefore closed rather than carried.

The new pin was obtained with `git -C ../acdp-spec-pinned archive <sha> | tar -x` into scratch —
a pure READ of the sibling repo, which stayed at `d1f06d0` with a clean tree throughout. Moving
another repo's checkout is a cross-repo write and is not a lane's call, least of all for a fixture
other lanes may be running against.

| | U-502 (74-mutant scope) | **U-504 (213-mutant scope)** |
|---|---|---|
| mutants in scope | 74 | **213** |
| viable | 48 | **140** |
| caught | 47 | **131** |
| **survivors** | 1 | **8** |
| timeout | 0 | **1** |
| unviable | 26 | 73 |

**The comparable figure is 28, not 8.** The first run over the new scope found **28
survivors**; 20 were killed with additive tests in `http_integration.rs` and these 8 are what
is left. `caught` moved 111 → 131, exactly +20, which is the independent arithmetic check that
the kills are real rather than the scope having shifted under them.

### Method: the completeness claim is an equality, not a floor

Run as 8 shards at `-j1`. Every shard's `outcomes.json` carries an `end_time` (a shard without
one is an incomplete run, not a result — two were discarded on that basis across this unit), the
**union of the shards' own mutant lists equals the full `cargo mutants --list` set exactly**
(213 == 213) with **zero overlap**, and `viable == caught + missed + timeout` (140 == 131+8+1).
A `>=` check cannot detect undercounting, which is precisely how this lane's first baseline was
wrong.

Slice sharding **nests** for even multiples (`0/4` == `0/8` + `1/8`, verified by set equality),
so a completed coarse shard stays valid when subdivided — which is what makes a partial run
resumable.

**`-j1`, measured, not assumed.** On one 18-mutant shard: `-j6` and `-j8` each exceeded 10
minutes with every mutant still building; `-j1` finished in 1m55s (~6.4s/mutant). Parallel jobs
cannot share incremental artifacts, so N simultaneous rebuilds of core plus every dependent
contend instead of pipelining. `mutants.yml` changed from `-j2` to `-j1` on that basis. Full
scope ≈ 23 min plus 5 min for the one timeout.

### What the oracle found: 20 real gaps, in five clusters

Framing as in U-502, and it is the honest one: **the code is correct, no disclosure ships, these
were unguarded correct properties.** Every gate below worked; nothing would have noticed if one
stopped working.

1. **Five tenant gates with no coverage at all** — `/contexts/{id}/body` (`:1018`),
   `/lineages/{id}` (`:1322` retain skipped, `:1328` retain inverted), `/lineages/{id}/current`
   (`:1361`), and `retract`/`republish` (`:1511`). The last protects a **write**: with `!=`
   inverted, a caller scoped to another tenant successfully retracted the context, event
   recorded. It looked covered — `X-Tenant-Id` appears 26 times in these tests and
   `retrieve_with_tenant` exercises this exact gate on `/contexts/{id}`, the *envelope* route —
   but the tenant-header sites and the requests to those three routes did not overlap on one line.
2. **Seven survivors behind an unasserted webhook payload.** Three handler values reach nothing
   but the webhook: `context_type_str` (one caller), the validated `x-run-id` (one use), and the
   retract delivery's reserved-tenant filter. `count_connections_and_reply_ok` counts connections
   and discards the bytes. **A value with exactly one consumer is untested if that consumer is
   untested.** Deleting the webhook `Retracted` arm delivered a retraction as
   `"type":"context_republished"` — telling a consumer the context came back.
3. **Six survivors in the search refill loop, whose body was dead to the whole repo.**
   Instrumenting `iterations` and running the entire server suite produced *not one* iteration
   beyond the first. Reaching it needed `?visibility=private`, which empties every page. The
   matches cannot discriminate (empty either way) — `next_cursor` can, so the test resumes an
   unfiltered search from it and counts what remains: 10 of 70 when the cap holds, 60 when the
   loop gives up after one page, none at all when it runs to exhaustion.
4. **Two on `Idempotency-Key`, the validation whose comment records it as the fix for #20.** The
   existing test sends a 257-char key and asserts only `200` — but a key wrongly *honored* also
   returns 200. Only the `ctx_id` distinguishes "ignored" from "used". The fix for #20 shipped
   with a test that could not see the behaviour it was named for.
5. **A did:web/lifecycle finding that is a gap rather than a kill** — see survivor 7 below.

### The 8 accepted survivors, each argued

**Equivalent — the mutation changes nothing observable (4):**

- **`log.rs:131:18` `== -> !=` in `root_for`.** Carried unchanged from U-502, not re-argued.
  Re-measured here and still MISSED, which matters: an equivalence claim expires when the suite
  changes, and this one survived 20 new tests.
- **`context.rs:1223:16` `delete !` on `if !matches.is_empty()`.** H-H-w moved the tenant
  predicate into the search SQL (`search_in_tenant`), so every row reaching this retain already
  belongs to the caller's tenant and skipping the block removes nothing. **Positive evidence, not
  inference:** the retain's own comparison `t == tenant` was CAUGHT at the same site. Inverting
  drops every row and reddens a count assertion; skipping drops nothing.
- **`context.rs:81` / `:82`, deleting the `"public"` / `"restricted"` arms of `parse_visibility`.**
  Measured: search returns only PUBLIC rows to *every* requester — probed with an anonymous
  caller, an audience member, and the row's own producer, all of which saw only the public row.
  So `?visibility=public` selects everything already visible and `?visibility=restricted` selects
  nothing, for anyone. The same site's `"private"` arm was **CAUGHT** — the one arm still doing
  work, since it must yield zero where no filter yields the visible public rows. **These become
  coverage gaps the moment search serves restricted rows to entitled requesters, and the budget
  must then drop to 6.**
- **`context.rs:627:39` `>` -> `>=` on `rec.expires_at > Utc::now()`.** Differs only when a stored
  expiry equals the clock to the nanosecond. `expired_idempotency_key_is_not_matched` pins the
  behaviour either side; the boundary itself is not reachable deterministically and a test that
  waited for it would be a flake generator.

**Blocked on test infrastructure this suite does not have (4):**

- **`context.rs:1542:13`, deleting the `Retracted` arm of the did:web dispatch** — a retract
  processed as a **republish**. The entire did:web lifecycle branch has zero coverage:
  `signed_event_envelope` can only sign as did:key, so `actor.starts_with("did:key:")` is true in
  every lifecycle test in the repo. **Verified unreachable rather than assumed** — a did:web-signed
  retract was written and fails at `key_resolution_unreachable`, because `retract_verified`
  resolves the actor through a real `WebResolver` and playground mode does not bypass it.
  *Follow-up: an HTTPS fixture serving `agents.test`'s did.json, which also unblocks did:web publish.*
- **`context.rs:1399:5` -> `""` and -> `"xyzzy"` on `lifecycle_outcome`.** The function is
  `e.wire_code()` behind a metrics label, so its only observer is a `/metrics` scrape. `/metrics`
  is deliberately not mounted in the `http_integration` harness (404, measured), and
  `metrics_integration.rs` is a separate test binary precisely so "the process-global `metrics`
  recorder is isolated from other integration tests" (its own module doc). Asserting it there
  would install a global recorder into a binary built without one and put 158 tests behind shared
  mutable state. The right home is `metrics_integration.rs`, outside this unit's grant.
  *Follow-up: a rejected-transition label assertion there.*

**New floor: 8.** Three of the eight are retired by one HTTPS did:web fixture plus one metrics
assertion; two more go if search ever serves restricted rows. Only the three genuine equivalents
(`log.rs:131`, `context.rs:1223`, `context.rs:627`) are permanent.

### The one timeout is budgeted, not folded into "not missed"

`context.rs:1277:12` `delete !` on `if !should_refill { break; }` loops forever whenever the loop
should *not* refill — every unfiltered search. Non-termination cannot be converted into a fast
assertion failure, so the verdict is TIMEOUT and a timeout here **is** the detection. The ratchet's
previous `timeout != 0` rule was reasoned about *slow tests*, which is a different thing; it is now
a named ceiling of 1, and a second timeout still fails the job. Measured directly: `:1223:16`
MISSED in 10s, `:1277:12` TIMEOUT at the full 300s.

I had reasoned the new refill test would *catch* `:1277` — under the mutation `should_refill` is
true on the first pass, so it breaks immediately and the assertion fires. That reasoning was wrong:
the other tests still hang, and a hanging binary times out whatever one failing test says. Hence
the measurement.

### Lesson: a survivor is a fact; "a survivor means a missing test" is an inference

Eight of the 28 were not gaps. The other branch is that the mutation changes nothing observable,
and **defence-in-depth manufactures that branch on purpose** — so the redundant-guard case is
commonest in exactly the most-hardened code, which is where a tenant-isolation audit points an
oracle first.

The cheap discriminator: **two mutations at one site with opposite verdicts is positive evidence of
equivalence.** `t == tenant` CAUGHT beside `delete !` SURVIVED; `"private"` CAUGHT beside `"public"`
and `"restricted"` SURVIVED. Both times the caught sibling proved the site was reachable and the
survivor unobservable. **For any survivor that deletes a fast-path or short-circuit guard, check for
upstream enforcement before writing the test.**

I got this wrong first and corrected it on the board: `:1223` was reported as an uncovered tenant
gate before `search_filters_by_tenant` — a *green* test that should have reddened — turned out to be
the evidence rather than the noise.

### AC-8: the spec-pin coupling now has a guard

`mutants.yml` derives its acdp-spec pin by grepping `ci.yml`'s 40-hex `ref:`. Nothing in `ci.yml`
says another workflow parses it, and `mutants.yml` is schedule-only, so a PR restructuring that step
could not turn red — the breakage would surface on the next Monday cron, detached from its cause.
CHARTER Rule 48: a doc artifact no command can check is a defect while it is still correct.
`conformance_gate.rs` now asserts four invariants as a pure function over both files' text (forced:
`ci.yml` is outside this grant, so falsifying by editing it was never possible), each falsified by a
valid-YAML restructuring — and the falsification test itself falsified by disabling each check.

**One invariant changed shape on contact.** As proposed it was "no hardcoded 40-hex ref anywhere in
`mutants.yml`". Measured: `mutants.yml` legitimately carries four 40-hex **action** pins, because
pinning actions by SHA is correct. Narrowed to a literal on a `ref:` line, with a test asserting
action pins are not flagged. A guard that fails against the correct file is a guard that gets deleted.
## U-518 — when the compiler names the site and not the cause

`axum::handler::Handler` has two halves: the extractors must implement `FromRequestParts`/
`FromRequest`, and the returned **future must be `Send`**. When the second half fails, the error
points at the `.route(...)` line and says nothing about why:

```
the trait bound `fn(...) -> ... {retrieve::<...>}: Handler<_, _>` is not satisfied
  --> crates/acdp-registry-core/src/lib.rs:79:42
= note: Consider using `#[axum::debug_handler]` to improve the error message
```

`#[axum::debug_handler]` does not apply to a generic handler, so that suggestion is a dead end here.

Two plausible causes were tested and eliminated before the right one — both about the *return* type,
which is where the eye goes first. What made them cheap to eliminate was asserting the bound directly
rather than reasoning about it:

```rust
fn assert_ir<T: axum::response::IntoResponse>() {}
assert_ir::<axum::Json<acdp::types::body::FullContext>>();                    // compiles
assert_ir::<Result<axum::Json<..::FullContext>, RegistryError>>();            // compiles
```

Both passed, which killed both hypotheses in one build and pointed at the future.

**The technique worth keeping.** A future's type cannot be written down, but it can be *named by a
call expression*, and that is enough to demand `Send` of it:

```rust
fn probe<S: ExtendedRegistryStore + 'static>(
    st: State<Arc<AppState<S>>>, h: HeaderMap, p: Path<String>,
) {
    fn is_send<T: Send>(_: T) {}
    is_send(handlers::retrieve::<S>(st, h, p));
}
```

That converts axum's "not a `Handler`" into the full chain: the offending type, every `async fn`
body it passes through, and the upstream file and line where it is held across an await — here
`acdp-client-0.13.2/src/revocation.rs:459`, a `&dyn Fn(&KeyRevocation) -> bool` missing `+ Sync`.
About twenty lines of output, all of it load-bearing. **Reach for this whenever a `Handler` bound
fails and the extractors and return type are unchanged.**

**And then prove it.** Naming a plausible cause is not establishing one — the two dead hypotheses
were also plausible. The cause was confirmed by *repairing* it: `+ Sync` on the two parameters in a
local copy of the upstream crate, wired in with `[patch.crates-io]`, and the symptom disappeared
while nothing else changed. A one-line edit that makes the failure go away is an attribution;
reading a diff and finding something that looks related is not. See `DECISIONS.md` for the decision
this fed, and acdp-rs#279 for the upstream report.

**The shape to remember:** a private helper's parameter can change a *public* future's auto-traits.
`acdp-client`'s own build stayed green — nothing in that crate observes `Send`-ness of its public
futures — so the regression could only ever surface in a downstream axum consumer. Auto-traits are
part of an async API's contract even though they appear in no signature.

## U-520 PR A — the reuse that would have broken something else

The obvious fix for an ungated `POST /contexts` was to route `publish` through the `AcdpJson`
extractor that already gates `/auth/*`. It is the right instinct — one implementation of the
accept-set, one envelope, one minted code — and it was wrong here for a reason that only showed up on
the way to being measured.

`AcdpJson` delegates to axum's `Json`, whose `JsonDataError` rejection carries **422**. So routing
`publish` through it fixed the 415 and simultaneously moved *every wrong-shaped publish* from 400 to
422. RFC-ACDP-0007 §5's status table pins `schema_violation` to 400. The reuse would have closed one
conformance violation by opening another with a much wider blast radius.

It surfaced as a one-line test failure — scenario A expected 400, got 422 — and the temptation at that
moment is to adjust the expectation, because A is only supposed to assert "not rejected". Reading the
spec's table instead is what turned a puzzling status into the reason not to take the obvious path.

The fix that shipped keeps the reuse where it pays (`AcdpRejection`, the envelope, the code constant)
and declines it where it costs (the deserializer and its status). The cost of that choice is a
re-implemented six-line predicate, which is a genuine drift risk — so it is pinned by a test asserting
both gates agree across a matrix of content types, rather than by the comment claiming they do.

**Two things worth keeping from the measurement.**

*The 104.* Gating an absent `Content-Type` reddened 104 of 159 integration tests. The fixture allows
either reading, so the number is not a bug report — it is the shape of the client breakage the other
reading would have caused, and it converted a coin-flip into an obvious decision. Scenario E accepts.

*The two-line diff.* The entire non-comment change to `handlers/context.rs` is the import and the
parameter type; the `body` binding is unchanged. That is also the evidence that nothing about hashing
or signature verification moved — not a claim that it didn't, but a diff in which it could not have.
When a change touches a signing path, the argument to reach for is one that makes the risky thing
structurally absent rather than reviewed and found safe.

## U-520 PR B — the falsification that failed, and what it caught

Five of the six falsifications behaved. The sixth did not: mis-grading `err-002` as
`RequiredByProfile` left the test **green**.

The assertion was fine. The test never reached it. `registries/profiles.json` stores `profiles` as an
**array** of objects with an `id` field, not a map keyed by profile name, so a `.get("acdp-registry-core")`
returned `None`, the helper returned `None`, and the caller took its "spec unavailable, skipping"
branch — reporting PASS while asserting nothing.

Two things are worth separating here.

The first is that **the skip branch is the hazard, not the lookup bug.** A lookup bug that panicked
would have been obvious. What made it survive was a well-intentioned pattern copied from the
surrounding tests: gracefully skip when the spec is unreachable. That pattern is correct for a
developer running `cargo test` with no `ACDP_SPEC_DIR`, and it is exactly wrong in CI, where the spec
IS reachable and a skip can only mean something is broken. The fix is not to remove the skip but to
condition it: `assert!(!require_conformance())` before returning, which is the idiom `spec_fixtures()`
already uses. A green skip and a green pass are indistinguishable in the summary line, and only one of
them means anything.

The second is that **I had already hit this exact shape an hour earlier and it did not transfer.**
While measuring, my first Python probe of the same file crashed with `'NoneType' object is not
subscriptable` on the same map-vs-array assumption; I fixed the probe, got my numbers, and then wrote
the Rust against the assumption the probe had just disproved. The scratch tool and the shipped code
were treated as different problems because they were written in different languages twenty minutes
apart. When a throwaway script teaches you the shape of a file, that lesson belongs in a note, not in
the script you are about to delete.

**What this says about the practice, not the bug.** Falsifying every assertion individually is what
separated these: had I falsified "the test suite" rather than each assertion, five reds would have
drowned the one green that mattered. The green falsification is the informative one — a red proves the
assertion works, a green proves the *test* does not — and it is the one that is easy to skim past,
because a passing test after a deliberate break looks like a test that is merely lenient rather than
one that never ran.

## U-523 — a falsification that reddened nothing, and why that is the useful outcome

Four assertions, four falsifications. Three reddened. The one for **415** reddened nothing, and the
green was the informative result again — for a different reason than last time.

Last unit a green falsification meant the test never ran. This time the test ran fine and asserted
exactly what it claimed; **the thing I broke was not the thing it reads.** `AcdpBytes` hard-coded
`StatusCode::UNSUPPORTED_MEDIA_TYPE` in its own rejection while `status_for_code` carried a 415 arm
used only by `AcdpJson`. Breaking the shared arm left the publish path untouched, because the publish
path never consulted it.

That is worth more than the bug it revealed. The whole decision behind this unit is *the code decides
the status, so code and status cannot disagree* — and the code contained two independent places where
a status was chosen, which is precisely the drift the decision exists to prevent. **A guard against
divergence that is itself duplicated has not removed the divergence; it has added a second copy of
it.** The falsification is what surfaced that, and only because it was aimed at one arm rather than at
"the suite".

The fix was to make `AcdpBytes` derive its 415 from the same function, after which breaking that arm
reddens both the publish matrix and `/auth/*` — one edit, two paths, which is the property the design
claimed from the start and did not have.

**The generalisable form:** when a falsification comes back green, resist reading it as "the assertion
is lenient". Ask which of two different things happened — *the test never ran*, or *the test never
reads what I varied*. Both are silent, both look like leniency, and they have opposite fixes: condition
the skip, versus point the code at the single source it claims to use. See also `probe must read what
you vary` — this is that rule applied to the guard rather than to the test.

## U-506 — making the coverage tables name what actually guards each family (2026-09-13, lane-2)

`conformance.rs`'s family tables said the `log` family's emission half was covered by two
golden-recompute tests. It is not. This unit makes the tables truthful and pins the citations that
were holding nothing.

### The finding, reproduced before anything was planned

Mutating `handlers/log.rs`'s `root_for` to `String::new()` — gutting the Merkle root every log
endpoint serves:

| | with the root gutted |
|---|---|
| the whole conformance suite | **73 passed, 0 failed** |
| the two tests `PARTIAL_DIRECT` names for `log` | **both pass** |
| `http_integration.rs` | **11 failed** |

**`EXCUSED`'s `log` entry argued that a direct pass "would assert something about acdp-crypto's
merkle code, not about this registry" — and then, four lines later, offered two golden-recompute
tests as proof that "the emission half IS covered". Its own argument applied to its own golden
tests, and the entry did not notice.** `log001_leaf_root_and_inclusion_golden_recomputed` and
`log003_consistency_proof_golden_recomputed` reach only `merkle::*`; they never enter
`handlers/log.rs`.

`PARTIAL_DIRECT` was **not** wrong, which is worth stating: it pins exactly what it claims to pin.
The defect was prose inviting a stronger reading than the mechanism supports.

**Classified as a DOCUMENTATION defect, not a coverage defect.** The tests that hold the handler
path exist; they were simply unnamed. The opposite conclusion would have sent someone writing
duplicate tests — which is why the survivor/gap distinction from U-504 is applied here explicitly.

### The number in the file was wrong, and it was my own doing

`source_test_fn_body`'s doc comment said the mutation was "CAUGHT by ten tests" and referred to "the
whole 69-test suite". It is **eleven**: U-502 added
`log_proof_ctx_id_is_served_to_the_owning_tenant` to that log suite *after* the sentence was
written. Both figures were true when written, neither had anything holding it, and the later edit
that expired the first was mine. The count now lives in `LOG_HANDLER_GUARD_COUNT` where an assertion
reads it, and the suite size is no longer restated in prose at all.

### The structural constraint, and the limit it forces

`conformance.rs` and `http_integration.rs` are **separate integration-test binaries**. Rust cannot
reference a `#[tokio::test]` function across them, so **#249's `direct_fn!`/`DIRECT_FNS` compile-time
binding is structurally unavailable here** — not merely unused. Cross-binary names can only be
verified by reading the other file's text: existence plus a test attribute, with the same substring
ceiling documented on `covered_direct_families_have_present_test_functions`.

**So this unit makes the tables more TRUTHFUL without making the guarantee STRONGER, and those are
different axes.** Stated at the check rather than left for a reader to infer, because a more accurate
table reads like a stronger guarantee and is not one.

### The second, more general defect: a hand-maintained list cannot catch omissions

`this_file_cites_constructs_and_never_line_numbers` already reads sibling files from disk and asserts
a construct is present — the right idiom, already in the file. But its list is hand-maintained, and
it named **1 of the 9** `http_integration.rs` test functions this file cites. The other eight —
including `publish_enforces_the_err002_media_type_matrix`, cited as the test that enforces the
`err-002` gate — were pinned by nothing and would rot silently on a rename.

So `CROSS_BINARY_GUARDS` is checked by a **derived equality** rather than by a longer list: read the
sibling from disk, compute its present test-attributed functions, intersect with what this file cites
by word-boundary match, require the result to equal the table. A `>=` floor would pass the very
scanner that is silently missing citations.

**Two design corrections found while building the falsifications**, both recorded in the code rather
than fixed quietly:

1. **The set equality does not protect the `log` group.** Those eleven names are cited *only* by the
   table, so deleting one shrinks both sides together and the equality stays satisfied.
   `LOG_HANDLER_GUARD_COUNT = 11` is that group's guard. The nine prose-cited names need no count —
   prose keeps citing them, so the set difference fires.
2. **`assert_eq!(cited, tabled)` was structurally unfireable and was removed.** Tabling a name *is*
   citing it, so `tabled ⊆ cited` always and the reverse half could never fail. An assertion that
   cannot fire reads as coverage and provides none.

### The survey of the other 21 entries, and why eight look wrong but are not

19 `Direct` blocks and 3 `PARTIAL_DIRECT` entries were classified by `log`'s own signature: a test
that reads fixture `vectors` and recomputes through a library without building a router cannot be
holding a handler. **The parser was checked against the known count of 19 before its output was
trusted** — its first version matched exactly one family and reported "no families at risk", which is
what a broken extraction looks like: a confident zero. That is U-504's own lesson, applied to myself
one unit later.

Nine families' named tests never touch the HTTP surface. **Eight are correct anyway, for three
different reasons, and the reasons matter more than the count:**

- **`can`, `lin`, `caps`** — pure-vector families. Canonicalisation, lineage derivation and
  capabilities validation *are* recomputations; no registry path exists to hold, so a golden test is
  the complete and correct test.
- **`rcpt`, `lhr`** — word-for-word the same "The producer half IS covered and stays pinned"
  construction `log` used, and **sound**. Measured, not read: a `panic!` in `receipt.rs`'s
  `build_signer` reddens both `rcpt001_…_and_remintable` and `lhr001_…_and_remintable`, so they
  genuinely traverse this registry's producer code. `log`'s goldens reach only `acdp-crypto`. That is
  the whole difference, and it is why identical wording was not enough to convict them.
- **`wit`, `dk`, `err`** — their registry-side path is held in a **third** place neither test binary
  can see. A `panic!` in `witness.rs`'s `verify_cosignature_against_own_log` leaves conformance (73
  pass) and `http_integration` (0 failures) entirely green and reddens **five `witness::tests::*`
  unit tests inside the core crate**. The table credits no coverage that does not exist; it never
  claimed to enumerate in-crate unit tests, and the comment now says so.

**So `log` was the only family whose table asserted something its named tests did not hold** — a
measured claim about the other 21, not an assumption that the first defect found was the only one.

**The bound, stated rather than implied:** this is a structural discriminator plus three targeted
probes, **not** a per-family mutation sweep. That is the ratchet's job (#216, U-504). A family whose
named tests *do* build a router could still assert the wrong thing about it and nothing here would
notice.

### Falsification

Five assertions, each individually, each firing its own message: the fn scanner broken →
"scanner is broken"; one log test dropped → "expected exactly 11"; a nonexistent name tabled → "no
longer defines it"; a test listed twice → "appears twice"; a prose-cited test untabled → "does not
list them". And **green against the unmodified tree**, because a guard that flags correct entries is
a guard someone reverts.

`http_integration.rs` was **READ ONLY** for this unit (lane-1 was writing it concurrently under
U-523), so the "rot" assertion was falsified by varying the **table** rather than the sibling file —
which exercises the same assertion. The guard was then re-run after merging lane-1's #293, which
touched that file: it passes, so no name this unit depends on was renamed.

`log`'s own claim was falsified the way the finding was made: with `root_for` gutted, the conformance
suite stays green and the failing `http_integration` set is **set-identical** to the tabled eleven —
equality, not a matching count.

### A note on where the decisions are recorded

U-506's grant covered `conformance.rs`, `docs/**` and `CHANGELOG.md`; it did **not** include
`ASSUMPTIONS.md` or `DECISIONS.md`. None of this unit's four judgement calls is a one-way door — the
derived equality over a hand list, removing the unfireable assertion, the count const, and Phase 3's
scope bound are each one commit to reverse — and all four are documented at the code they govern. So
they are recorded here rather than by reaching outside the grant.
## An agreement test is blind to a uniform regression

Two tests written earlier in this arc — `the_two_media_type_gates_agree` and
`the_admin_media_type_gate_matches_the_publish_gate` — compare one route's verdict against another's.
Breaking `status_for_code`'s 415 arm, the single shared centre both routes consult, left **both
green**. The break moved both sides equally, so the property they assert was preserved while the
behaviour they exist to protect was destroyed.

This is not leniency and it is not hard-coding; I initially mislabelled it as the latter. It is
structural: an assertion of the form `a == b` cannot see a change that maps `a -> a'` and `b -> b'`
together, and a shared implementation guarantees that changes to it are exactly of that shape. **The
more centralised the code, the blinder its agreement tests become** — which inverts the usual
intuition that consolidating logic makes it easier to test.

The fix is not to delete them. They still catch the thing they were written for: one route drifting
away from another. The fix is that at least one test must pin the **absolute** verdict — what status
this input actually produces — so a uniform move has something to break. That test is
`every_acdp_bytes_route_shares_one_media_type_gate`, and breaking the shared arm reddens it.

**How to tell in advance:** ask what happens if the code under test is *replaced wholesale* with
something wrong. If every assertion still passes, the suite is measuring internal consistency rather
than behaviour. Relational assertions (agree, match, round-trip, idempotent) all share this blind
spot and all read as strong coverage.

## Check the name against the thing before shipping the name

`every_body_bearing_route_shares_one_media_type_gate` covered five of eight body-bearing routes, and
the missing three did not use the gate it named. The test was correct; the name was a false claim
about the system, and a name is the most quotable unit a test has — it is what a future reader greps
for and what a summary repeats.

Verifying it cost one grep of the route table and one read of the other extractor. That grep is what
surfaced the actual finding of this unit: the two accept predicates are independent implementations
that agree by coincidence. **The overclaiming name was the only thing pointing at it** — the code
compiled, the tests passed, and nothing else in the run would have asked whether `/auth/*` shared
the gate.

## A falsification can be absorbed by an earlier assertion

Four falsifications, three mechanisms visible. The third — remapping
`JsonRejection::MissingJsonContentType` off 415 — was aimed at the absent-header assertion at the end
of the test, but reddened the present-type loop above it instead, because axum returns that same
rejection variant for a *wrong* content type as well as an absent one. The run came back red, the
mechanism named was real, and the assertion actually targeted was never evaluated.

A red falsification is therefore not proof that the assertion you aimed at works. **Read which
assertion fired, not merely that one did.** Where an earlier assertion absorbs the change, the later
one needs a separate falsification chosen to leave the earlier one satisfied — here, making
`AcdpJson` infer an absent header, which is also the precise "cleanup" the assertion exists to block.

## Verifying code against code agrees with itself

Two places in `conformance.rs` recorded that `did-ssrf-*` was "not HTTP-replayable", and both said so
*carefully*. One noted it was "confirmed for this phase by re-reading `extract_shapes` directly rather
than trusting the prior `DEFERRED` reason's claim on faith (it held up)". The other was headed "The
prior `DEFERRED` reason's claim, **verified before building on it**". Both then walked the dispatcher
shape by shape and concluded correctly that nothing matched.

Every step was accurate and the conclusion was wrong. `did-ssrf-001`..`004` are ordinary HTTP
publishes with concrete bodies; they now replay. **Re-reading the dispatcher can only ever establish
what the dispatcher does.** It cannot distinguish *"this fixture is not an HTTP request"* from *"the
dispatcher does not parse this spelling of one"* — and those two have opposite fixes. The check was
diligent, repeated, and pointed at the wrong artifact: to catch this, the code had to be checked
against the **fixture**, not against itself.

The tell was available and unread: the reason string said "vectors / schema / informative" about a
file containing `"endpoint": "POST /contexts"`. A classification that contradicts the thing it
classifies is visible without any tooling, and it survived two deliberate verification passes because
both passes asked "does the dispatcher reach the fallback?" instead of "is the fallback's claim
true?".

**How to apply:** when a check concludes that some input is out of scope, verify the *predicate
against the input*, not the code path that produced it. "I re-read the function" is evidence about the
function. See also `probe must read what you vary` and `assert the mechanism, not the symptom`.

## A floor is satisfied by every number above it

`MIN_REPLAYED_EXCHANGES: usize = 30` guarded the conformance replayer with `replayed >= 30`, and its
own comment named the hazard correctly — "a fidelity gate may be over-matching and silently shrinking
coverage". It could not catch that hazard. Coverage was 30 while `extract()` silently declined 12
parseable fixtures, and 30 satisfies `>= 30`. The guard was calibrated to exactly the broken state and
would have gone on passing as coverage decayed anywhere above its floor.

Replaced with `REPLAYED_EXCHANGES_AT_PIN = 38` and `assert_eq!`. Falsified by dropping a single
fixture: **37 passes the old floor and fails the new equality.** Movement in either direction is now a
human decision — fewer means a dispatch gate started over-matching, more means fixtures became
replayable and the coverage tables were not updated.

This is the same lesson `TOTAL_FIXTURES_AT_PIN` already carries one level up, which is the useful
part: the repo had *written down* that a `>=` floor "passes the very scanner that is silently missing
items", pinned its fixture total as an equality on that reasoning, and left the exchange count a
floor. **Knowing the rule did not propagate it to the neighbouring constant.** Worth a sweep when a
lesson is recorded: find the other guards of the same shape, not just the one that prompted it.

## Fixing the harness is half of curing a wrong-reason pass

`pub-011` expects 400 `invalid_signature`. It was unreachable because the conformance harness bypassed
DID verification — so the obvious cure was to make the harness verify signatures. That cure alone
would have left the defect standing, and the fixture would have looked cured.

Its `content_hash` is the literal placeholder `sha256:<recomputes-correctly-against-this-body>`, and
the replayer pinned **no error code** for publishes, on the reasonable-sounding grounds that validation
ordering is impl-defined. Measured with the harness fixed but the code still unpinned: `pub-011`
**passes**, receiving `schema_violation: content_hash digest must be 64 lowercase hex chars`. A
fixture whose entire purpose is signature verification, scored green by a schema error.

**A wrong-reason pass has two independent causes — the check that cannot run, and the assertion too
weak to notice.** Removing either one alone leaves a green test. They have to be counted separately,
because fixing the dramatic one feels like completion: the harness change is the hard, interesting
work, and it is exactly the moment you stop looking.

The general form: whenever a test asserts a *class* of outcome (any 4xx, an error occurred, it threw)
rather than the specific outcome it names, restoring the capability it was missing does not make it
discriminating. Ask what else could produce the same class.

## Pinning the codes said four of my own fixtures had never been right

Pinning the expected error code turned up five publish fixtures passing for the wrong reason. Four
were `did-ssrf-001..004` — fixtures **I had lit up in the previous unit** and reported as a coverage
win. They return `schema_violation` because their bodies omit a required member, so they never reach
the DID resolution they exist to exercise. The fifth, `pub-002`, was changed *by this unit*: pinning
the producer key makes signature verification run before the hash gate, so it now fails on the
signature rather than the hash its fixture names.

Both went into a `CODE_DIVERGENCES` table that records the code actually returned, with a reason, and
**asserts it**. The three available responses were: skip them (loses the coverage), leave them
unpinned (keeps the wrong-reason pass), or pin the truth and name the gap. Only the third leaves a
reader able to tell what is covered from what merely runs.

The uncomfortable part is the useful part: "+8 fixtures replaying" was true last unit and **half of it
was not coverage**. A count of things that execute is not a count of things that check. When reporting
newly-covered items, the honest figure is how many now assert the thing they were written to assert —
and you only learn that by pinning the specific outcome and seeing what breaks.

## Four instances, one shape, three units

The same defect has now been found four times: `pub-011`, `did-ssrf-001..004`, `pub-008`, and
`cur-001` caught in advance. Every one had an identical structure — **the fixture names a specific
error, the replayer pinned only the status, the registry returned a different 400 for an unrelated
reason, and the fixture was scored as coverage of a rule it never reached.**

What makes it worth a log entry is that the instances were *not* found by looking for the class. Each
surfaced while doing something else, and the search that would have found all four at once —
`grep 'want_error_code: None'` — was one command and was never run until the fourth. After the second
instance the class was named, after the third it had a table, and the sweep still only happened
because a reviewer asked for it explicitly.

**When you find the same defect twice, stop fixing instances and enumerate the class.** The cost is
usually one grep; the cost of not doing it is that the fourth instance is found by someone reading a
green test and wondering.

`pub-008` is the one that should sting: it predates all of this work. Every unit that touched the
replayer ran it, saw it green, and moved on.

## Inverting an invariant is a decision, and must read as one

The sweep's fix required reversing an existing assertion — `want_error_code.is_none()`, documented as
*"Shape A's publish branch never pins an error code (validation ordering is impl-defined), this must
still hold"*. Someone wrote "this must still hold" on the exact property that was hiding the bug.

The reasoning behind it was **correct**: RFC validation ordering genuinely is implementation-defined,
so demanding a specific first-failing code genuinely can be wrong. The error was in what that licenses.
*"We cannot assert THIS particular thing"* was silently widened into *"we assert nothing"*, and
nothing is what let an unrelated rejection pass as coverage. The middle option — assert the code we
DO return, and record why it differs from the fixture — is strictly stronger than silence and was
available the whole time.

So the assertion was inverted rather than deleted, and the comment now says it was inverted, by which
unit, and why the original reasoning was sound but insufficient. **An invariant that turns out to be
wrong should leave a scar, not a clean surface** — the next reader needs to know the property was
considered and reversed, not that it never existed.

## A guard's first act was to correct its author

The new sweep guard asserts a known-positive bound: *N replayed fixtures name an error code*, so that
an empty scan cannot read as a clean sweep. I wrote 14 from my own reading. The real number is 15, and
the assertion failed on its first run.

That is the bound check working exactly as intended, on the person who wrote it, thirty seconds after
writing it — and it is an argument for putting the number in as an equality even when you are
confident. A `>=` would have accepted 14 silently forever.

## U-521 — the mutation floor drops 8 -> 5, and the "blocked on infrastructure" class is empty

*2026-09-13, lane-2. Supersedes "**New floor: 8**" in "### The 8 accepted survivors, each argued"
(U-504) — that section stays as written because this log is append-only, and it was accurate at its
own sha. This entry is the current baseline.*

### The measurement

**213 total / 134 caught / 5 survivors / 73 unviable / 1 timeout**, at `8768b07`, from the documented
command `cargo mutants` (scope and test command come from `.cargo/mutants.toml`).

Against U-504's **131 caught / 8 survivors**: exactly **+3 caught / -3 survivors**, with unviable
(73), timeout (1) and total (213) all unchanged. That is the arithmetic three retirements should
produce, and nothing else moved.

**It was measured over six shards, not one run, and how it was judged matters more than the number.**
Two long single runs were killed partway (see below), and an interrupted cargo-mutants run writes a
partial `missed.txt` that is indistinguishable from a result by eye. So:

- every shard's `outcomes.json` was checked to carry an **`end_time`** — the null case is the tell;
- the four tallies were checked to **sum to 213**;
- the shard sets were checked to **partition exactly**: sizes summing to 213 **and** the union of
  unique mutant names equalling 213. The sum alone cannot catch an overlap and the union alone cannot
  catch a dropped shard, so both are needed;
- `4/8 + 5/8 == 2/4` and `6/8 + 7/8 == 3/4` were verified by listing before substituting finer shards
  for a coarser one, rather than trusting that slice shards nest.

One real contamination was caught this way: a leftover output directory from the *killed* shard run
was still matched by a `*/mutants.out/caught.txt` glob, making the concatenated count 135 against a
true 134. The per-shard sum disagreed with the glob, which is what surfaced it. **The totals above were
computed from six explicitly named shards, never from a glob.**

### The three retirements

Both SITES were "BLOCKED ON MISSING TEST INFRASTRUCTURE" in U-504's list — three mutants across
the two of them. Both blockers are now gone.

**1. The did:web `LifecycleEventType::Retracted` arm** (`context.rs`, cited `:1542:13` then, `:1577:13`
now). The blocker was real and structural: `signed_event_envelope` can only sign as `did:key`, so
`event.actor.starts_with("did:key:")` was true in *every* lifecycle test in the repo, and
`retract_verified` resolves the actor through a real `WebResolver` that playground mode does not
bypass. Covering it needed a genuine HTTPS endpoint.

`tests/didweb/mod.rs` now generates a **CA + leaf chain at test runtime with `rcgen`** and serves
`agents.test`'s `did.json` over TLS on loopback, with `WebResolver::with_test_endpoint` pointed at it
and trusting the generated CA. The DID keeps its real authority; only DNS is faked.

**Why generated rather than committed, because it is a policy fact worth recording:** `.gitignore`'s
`# TLS material` block forbids `*.pem`/`*.crt`/`*.key` repo-wide, its only negation is an empty
`.gitkeep`, and `git ls-files` finds **zero** committed TLS material in this repo. Committing a fixture
would have required an exception to a secret-bearing ignore rule — repo policy, not a lane's call and
not the lane leader's either. Generating needs no exception, commits no private key, and deletes the
expiry problem outright: nothing persists, so nothing can lapse, so there is no expiry guard to
maintain.

**And a correction to U-504's entry for this mutant, in the alarming direction.** It said the deleted
arm meant "a retract would be processed as a REPUBLISH". Running the mutant disproves it:
`republish_verified` validates `event_type` itself, so the request returns **400 `schema_violation`**.
did:web retracts break *loudly and entirely* rather than silently succeeding. A quietly readvertised
retracted context would have been a data-integrity hole; this is a denial of function. Worth fixing
either way — not the same finding, and the wrong version was the scarier one.

The mutant is killed by the **acceptance** assertion (the retract must return 200), not by the
status assertion beside it. That status assertion guards a different class — a retract accepted but not
persisted — which no mutation at this site can produce; it is written as a before/after pair on the
context's state so it cannot pass against a response that never carries a status.

**2 and 3. `lifecycle_outcome` -> `""` and -> `"xyzzy"`** (cited `:1399:5` then, `:1424:5` now). Two
mutants at one line, which is why this retirement is three mutants at two sites.
`a_rejected_lifecycle_transition_is_counted_under_its_wire_code` in `metrics_integration.rs` — the home
U-504 named — asserts the `outcome` label equals the wire code the response actually carried, read from
the response rather than hardcoded.

**The witness deliberately needs no publish.** That binary's counters are process-global and a single
test owns the accumulation-sensitive assertions, including `publish_total{outcome="inserted"} == 2`. The
on-the-nose witness (a double retract, yielding `invalid_lifecycle_transition`) requires a published
context, which turns that 2 into a 3 — the suite said so: *"9 passed, 1 failed, two accepted publishes:
left 3.0, right 2.0"*. Retracting a `ctx_id` that does not exist exercises the same mechanism, because
the metric wraps the whole `lifecycle_transition` call so any error flows through `lifecycle_outcome`,
and it perturbs no series that test pins. Preserving that documented split was worth more than the more
quotable wire code.

### What is left is five, and all five are equivalent

There is **no survivor in this scope blocked on missing infrastructure any more**. All five are
equivalent mutants — `log.rs:131:18`, `context.rs:81:9`, `:82:9`, `:642:39`, `:1238:16` — each argued in
`.github/workflows/mutants.yml` beside the budget. The ratchet cannot fall further without a code
change, or without search beginning to serve restricted rows to entitled requesters, at which point the
two `parse_visibility` arms become genuine coverage gaps and the floor drops to 3.

**Line numbers in U-504's list were stale and are corrected in `mutants.yml`**: #295 inserted above
them, shifting sites past ~line 600 by +15 and `lifecycle_outcome` by +25. Verified by reading each
line, not by arithmetic, and now cited as `file:line` *plus the expression* so the next shift does not
strand them.

### The ratchet was re-falsified at the new value

A check verified at 8 proves nothing about 5. Driving the workflow's three conditions against the env
values read back out of the file: 5 survivors **passes**, 6 **fails**; scope 212 and 214 both **fail**
(it is an equality, not a floor); 2 timeouts **fails**. The load-bearing pair is 5/6.

`MUTANTS_EXPECTED_SCOPE` stays **213** — re-confirmed by the partition check, not assumed — and
`MUTANTS_TIMEOUT_BUDGET` stays **1**, still the same provably non-terminating mutant
(`:1292:12`, was `:1277:12`). U-521 added a test that makes the refill loop's other survivors
observable and it did **not** change that verdict: a hanging binary times out whatever any single test
asserts. I predicted otherwise, measured, and was wrong.

### Two process failures from this unit, recorded because they nearly cost more than they did

**A long background job was killed twice, and I misdiagnosed it.** Free disk had genuinely fallen
82Gi -> 8Gi during the first run, so I diagnosed ENOSPC and had a full report drafted before reading
`mutants.out/debug.log`, which ends `cargo_mutants::interrupt: interrupted` with **zero** ENOSPC matches
anywhere. The disk fact was true and unrelated. A true, strongly-correlated fact survives every check
you would run against a guess — only the causal link was invented. **Read the failing thing's own record
before admitting correlated evidence**, because once you hold a mechanism you read the log for
confirmation instead of for cause. (Separately: `du -sh` on a 208k-entry directory reported 77G where
summing its entries gave ~4G. Sum the parts before a total drives a decision.)

**A stale hand-saved copy silently reverted four corrections.** Restoring a file from a scratch copy
taken *before* later edits reverted a doc-comment fix, a before/after assertion pair and two assertion
messages. Nothing could have detected it: the file was valid and the suite had been green before those
corrections too. It surfaced only because a mutant's failure output quoted the **old** message text. A
restore is not a revert to known-good; it is a jump to an arbitrary past state whose contents you must
remember, and what it eats is the most recent work — the work you are least likely to re-derive because
you believe it is done.

## U-535 — a differential with a shared centre points at the wrong file

`parity.rs` cross-checked the RFC-ACDP-0008 §4.5 disclosure rule across what its own comment called
three implementations. Two of the three were the same function: the trait default body is
`retrieve_visible` plus a tenant check, and the N-call reference calls `retrieve_visible` directly.

The interesting part is the failure mode. A shared-centre differential does not just miss a defect in
the centre — it **misattributes** it. With `retrieve_visible` broken, the leg that consults it agrees
with itself and stays green; the SQL, derived separately, is the only thing that can disagree; so the
report reads "the SQL disagrees". Falsified at baseline: the old legs printed
`over-disclosure: []` and blamed the batch for under-disclosing, which is precisely backwards.

Fix: `EXPECTED_BY_SPEC`, a literal per-requester table transcribed from the RFC, with **both**
implementations compared against it. Falsification, per backend and per leg:

| mutation | anchor names the Rust rule | anchor names the SQL |
|---|---|---|
| `retrieve_visible`: `None => false` → `None => true` | 4 | 0 |
| SQLite `LIST_VISIBILITY_SQLITE`: second-arm `AND` → `OR` | 0 | 6 |
| Postgres `LIST_VISIBILITY_PG`: second-arm `AND` → `OR` | 0 | 6 |

Attribution is correct in both directions, on both backends. The two SQL mutations also fired the new
named outsider pins (2 each), which is what those pins exist for.

The generalisable lesson: **count the distinct expressions a suite can bottom out in, not the number
of comparisons it performs.** Two differentials terminating at the same leaf give the coverage of one,
and the doc comment claiming otherwise is itself the defect.

## U-533 — required-but-unexercised 6 -> 1, and a retirement that cannot be faked

*2026-09-13, lane-2.*

### The counts

`required` **6 -> 1**; the survivor is `pub-007`. `conditional` **3**,
`TOTAL_FIXTURES_AT_PIN` **144** and `REPLAYABLE_FIXTURES_AT_PIN` **22** all unchanged **and asserted
as such** — in a diff, "did not move" and "was not checked" are the same thing, so each is a live
assertion rather than a claim in prose.

### Why AC6's "measure against `git archive <pin>`" is load-bearing

There are **three** spec trees on this box and **neither checkout is the pin**:

| tree | HEAD | fixtures |
|---|---|---|
| nested clone `agentcontextdistributionprotocol/` | `a0adda7` | **145** (adds `rev-003`) |
| `acdp-spec-pinned/` | `d1f06d0` | **143** |
| `git archive 16211e64…` | — | **144** |

The drift runs in **both directions**, so using either checkout fails `TOTAL_FIXTURES_AT_PIN` in a
different way — and the directory whose name says `-pinned` is the one that is behind. Two sessions
reported different shas for "the spec checkout" and **both were right about different trees**. Measure
the archive; never the working tree.

### The five retirements, and why four are direct tests rather than replays

The obvious repair was to widen the replayer. Measuring each fixture body showed that would have been
actively harmful:

* **`pub-006` / `pub-009`** carry `signature.value` of **96 base64 chars where ed25519 needs 88**.
  Shape validation rejects them before RFC-ACDP-0001 §5.11 step 2, so they would have replayed green
  on `400 invalid_signature` while asserting nothing about `key_not_authorized` — the identical
  `pub-008` defect from U-531 and the `did-ssrf-001..004` defect before it. Widening Shape A would
  have **manufactured two new wrong-reason passes inside the unit whose purpose is removing them.**
  `pub-006` has a second blocker: both its DIDs use `did:agent:`, unsupported here.
* **`pub-010`** has no inline body at all — its excerpt's signature is the literal placeholder
  `<base64 signature that verifies under alice's did:web key>`.
* **`pub-003`** is blocked by seeding only: `input.preconditions` matches no seeding path. Widening
  Shape D's shared seeder was rejected because `REPLAYABLE_FIXTURES_AT_PIN` is an equality and one
  fixture is not worth changing what others replay.
* **`ret-002`** needs lineage seeding with per-version statuses, which `parse_shape_d` cannot express.

Every blocker is **asserted, not described**, so a spec bump that fixes one reddens the test that
depends on it.

### `pub-007` stays, and `pub-010` is not a second instance of it

Both expect **201** where this repo returns **200**. The first read was that U-526 blocks two fixtures
and the target should be 6 -> 2 — **nearly escalated, and wrong.** This file already carries the
`anc-001` / `idem-001` precedent: assert the corrected status, assert the fixture's own literal
separately so the deviation is demonstrably real, record it, neither fake nor fix it. That covers
`pub-010`, whose subject is `contributors[]`.

It does **not** cover `pub-007`, whose entire subject *is* the response shape and the `Location`
header — a corrected status there deletes the fixture's point. Its row stays, with that reasoning in
it.

### The defect worth more than the retirements

`fixture_accounting_totals_are_exact`'s own doc comment said moving a fixture OUT "requires editing
the list and the count, **and nothing else**". Nothing checked that a retired fixture was exercised.
Deleting five rows and changing a `6` to a `1` went green unaided — a hand-maintained list cannot
catch an omission, the same shape as the `>=` floor this file has fixed twice.

`EXERCISED_FIXTURES` now binds each retirement to the test that requests it: **compile-time** via a
`stringify!` macro over the same token (rename/delete/comment-out = compile error), and **at runtime**
by calling each registered test and asserting the fixture id reached `find_fixture_by_id`.

**Testing that guard found it closes only half the hole**, and that is recorded rather than papered
over: a retirement that registers *nothing* is still green, because the two tables are not joined by an
invariant. A conservation law (`required + retired == 6`) was considered and **rejected** — a spec bump
legitimately adding a required fixture would redden it for a correct reason, and a guard that fails on
correct input is one someone deletes. The real closure is deriving the set from `profiles.json`'s 72
`required_fixtures`, named as the follow-up.

### `ret-002` scenario 1: undriven, on the fixture's own authority

All-versions-superseded cannot be produced over HTTP — superseding a head makes the superseding
version the new non-superseded head. The fixture's own note calls it "Abnormal state: reachable only
via admin correction or data corruption". Rather than fabricate it with a store-level insert, which
would assert against a state no client can reach, the test **asserts the note still says so**, so the
scenario becomes visibly owed if the spec changes its mind. `expired` proved producible via
`expires_at`, so no `blocked` was needed in this unit.

<!-- unit U-536 (lane-2) — the spec pin becomes one declarative source; three trees, one digest -->

## U-536 — three spec trees disagreed, and the harness accepted whichever one it was handed

`spec_root()` took whatever `ACDP_SPEC_DIR` named and said nothing about it. Three spec
checkouts exist on this machine and they disagree **in both directions** — 143, 144 and 145
conformance fixtures — and the directory named `-pinned` is the one two revisions *behind*
the pin. Each wrong tree produces a plausible count that fails `TOTAL_FIXTURES_AT_PIN`, which
sends the reader to debug the fixture ratchet instead of their own checkout. That cost two
false alarms and one wasted audit before this unit.

`.spec-pin` is now the single declarative source: `repository:`, `ref:`, and a
`conformance-digest:` of the fixtures at that revision. `ci.yml`, `mutants.yml`, the spec
bumper and the conformance harness all read it, and none of them restates the sha.

### The verdict comes from content, and that is forced rather than preferred

The way to materialise an exact revision locally is `git archive <sha> | tar x`, whose output
carries **no git metadata at all**. So the tree that must PASS is precisely the one
`git rev-parse` cannot identify, and any git-based check would reject the correct input. Git
is used only to *name* a revision once one is found, never to decide.

One digest covers both worlds, measured rather than assumed: an archive extract, an
independent second extract, and a real `git clone` + `checkout` of the pin — what CI's
`actions/checkout` produces — all hash to
`rfc6962-sha256:03644a90cc643fd5bd5fe3c762389c16def704a874f94716619f7a949b965f85`. There are
zero `.gitattributes` in the 253-file tree at the pin, so no filter can make checkout and
archive content diverge; that grep was run against a known positive (the same scan finds 145
`schemas/conformance` paths) before its zero was trusted.

RFC 6962 via `acdp::crypto::merkle` rather than a hand-rolled hash, and specifically **not**
`std`'s `DefaultHasher`, whose output `std` does not promise is stable across releases — fatal
for a value committed to a file. No new dependency: `leaf_hash`/`merkle_tree_hash` were
already reachable. `acdp::crypto::canonical_preimage` is unusable here because it strips an
RFC-ACDP-0001 §5.7 EXCLUDE-set key by *name*, and fixtures legitimately carry `signature` and
`ctx_id` at top level.

### The two guards that had to be falsified at the guard itself

**A tree inside another repository must report "not nameable", not the outer repo's HEAD.**
With the `rev-parse --show-toplevel` comparison removed, the message confidently reports the
enclosing repository's `63164d99` as the spec's revision. A `.git`-directory test would also
be wrong: one spec checkout here is a linked worktree whose `.git` is a 127-byte **file**.

**`merkle_tree_hash(&[])` is SHA-256("")** — a real, confident-looking digest for "no files".
The fixture count is therefore asserted where the digest is computed rather than inherited
from `resolve_fixture_dir`, and that path is reachable, not decorative: `has_json` accepts a
**directory** named `*.json`, so a directory whose only `.json` entry is a subdirectory
resolves as a fixture dir and hashes nothing.

Ten mutations in all, each confirmed RED at its own assertion with its own message, then
reverted and confirmed green. The decisive ones name `wanted 16211e64 / found d1f06d0` rather
than failing on a fixture count — a count failure would have been the right outcome for the
wrong reason. Renaming one fixture, with identical content and an identical file count,
changes the digest and restoring the name restores it, which is what the `0x00` between name
and bytes is for.

### The bumper's anchor count, and an invariant that exists because a falsification failed

`spec_pin_violations` (`tests/conformance_gate.rs`) was four invariants about the old
derivation and is now ten about the new wiring. Invariant 2 counts column-0 `repository:`
declarations — what the reader action parses. I wrote a falsification in which a **comment**
spells the anchor form, expected invariant 2 to fire, and it stayed silent. The failure was
correct: acdp-ci's `bump-spec-ref` counts anchors with

```awk
index($0, "repository: " SPEC) || index($0, "acdp-ci/actions/checkout-spec@")
```

a substring search over every line, comments included — and it then **refuses to bump the
file at all** rather than rewrite one anchor and leave the rest stale. So a helpful comment
freezes the pin with nothing going red, and invariant 2 was citing that external contract in
its own failure message while being unable to enforce it. Invariant 4 counts the bumper's
way, and is verified against the bumper's own awk rather than my reading of it: with a spelled
anchor the test reports lines `[7, 57]` and that awk independently reports `ANCHOR_COUNT=2`;
restored, both say 1. This is why `.spec-pin`'s comments *describe* the two anchor forms
instead of quoting them.

Line order is load-bearing for the same external reason. `conformance-digest:` must stay
**below** `ref:`, because a 64-hex digest contains a 40-hex run and that awk would otherwise
return the digest's first 40 characters as the current pin. Falsified against the real awk.

All ten invariants are falsified **twice** — once on a synthetic fixture, then again by
mutating the real `.spec-pin`/`ci.yml`/`mutants.yml`/`bump-spec.yml` and confirming the
real-file test goes red with that invariant's number and no other. A synthetic pass alone is
not enough: an invariant can hold on a fixture and be structurally unfireable on the shape
the repository actually has.

### What the move costs, in the direction it runs

Adopting a revision now changes **three** values and the bumper rewrites only the first:
`ref:`, `conformance-digest:`, and `TOTAL_FIXTURES_AT_PIN`. So a `bump spec` PR arrives RED on
`conformance` until the other two are updated. `TOTAL_FIXTURES_AT_PIN` already had that
property, the bump PR is held for review and never auto-merged, and the failure message names
the replacement digest itself — but it is a real added cost, stated in
`.spec-pin` where the value lives rather than in a footnote. `CONTRIBUTING.md` carries the
three-step procedure.

### The lint gap this unit created, and closed

`.github/actions/read-spec-pin/` is this repo's first composite action, and **neither existing
linter covered it**: actionlint enumerates `.github/workflows/*` and cannot parse an
`action.yml` at all — pointed at one it reports `"jobs" section is missing` and exits 1,
measured. The `actionlint` step's own comment claimed every `run:` block was covered, so this
unit would have made that comment false. `lint.yml` now extracts each composite-action `run:`
block and shellchecks it, with `${{ … }}` replaced by a placeholder (the substitution
actionlint performs internally; a quoting defect *inside* an expression is therefore not
visible — a stated limit). Falsified by unquoting one variable in the action, which turns the
step RED with SC2086 at the right line.

**The extractor asserts its own yield, and it earned that.** The first version was an awk
one-liner whose indentation was off by two: it extracted **zero** lines, and all six of its
negative cases reported PASS. An extraction expression that matches nothing fails to a
confident zero, so the step now refuses a run that extracts 0 blocks from a non-zero number of
actions — and that guard fired for real during development, when `git ls-files` correctly
reported 0 because the action was not yet staged. The step's embedded heredoc was also
verified to survive YAML round-tripping, by parsing the `run:` block back out of `lint.yml`
with a YAML parser and executing it: the Python body must land at column 0, and an
IndentationError there would have been visible only in CI.

### Two references this unit made stale, outside its path grant

- `ASSUMPTIONS.md:2872` — entry 9, "`mutants.yml` derives the spec pin from `ci.yml` instead
  of restating it". The derivation is gone; both files now read `.spec-pin`.
- `ASSUMPTIONS.md:315` — records that `bump-spec-ref.yml` requests `permission-workflows:
  write` **because** the pin lives under `.github/workflows/`. That *because* no longer holds:
  the bump PR now touches a root-level data file, so the scope is no longer load-bearing for
  the spec bump. The scope is still requested by the shared workflow in acdp-ci, and the
  installation grants it, so nothing breaks — the reasoning is what went stale, and it moved
  in the safe direction.

Earlier entries in this log describe the retired derivation as current — including the U-521
baseline note at line 5301, which explains why a mutation baseline had to be re-measured
across a pin move. That reasoning was true when written; the mechanism it names was replaced
here.

## U-539 — a comment that outlived its decision, and #216's last open question

`.cargo/mutants.toml` spent a paragraph explaining why `handlers/context.rs` was excluded
from the mutation scope. The glob list three lines below it had included that file since
U-504. The prose also pinned the scope at 74 mutants while the workflow that consumes it
pinned 213.

**Measured, not inferred** (`cargo mutants --list`, at `60b08b7`): 213 = context.rs 139 +
log.rs 65 + receipt.rs 9; workspace 1427 under `--no-config`; and `--file` unions with the
globs rather than replacing them (213 + admin.rs 49 = 262 exactly).

The instructive part: **the stale number was correct for a configuration that no longer
existed.** Deleting the `context.rs` glob reproduces exactly 74. So the failure was not a
miscount but a comment that kept describing a tree it no longer matched — and the arithmetic
that looked like it would recover the truth (74 + 134) gives 208, because `context.rs` had
grown to 139 in the meantime. Every figure in that file now carries the command that
reproduces it.

**#216 item 4 — keep the substring guards or retire them — is settled: KEPT.** The reasoning
and the three measured grounds are in `DECISIONS.md`. The measurement that decided it:

* over 65 `handlers/log.rs` mutants run against the pinned spec, **17 distinct tests**
  reddened and the two substring guards reddened **zero** times — while provably running in
  41 of those runs (= 40 caught + 1 missed; the 24 unviable never compile);
* conversely, three falsifications turned them red on things no `src/` mutation can express:
  a body gutted to `{}`, a golden gutted to `assert!(true)`, and a `PARTIAL_DIRECT` family
  also listed in `COVERED`.

That third one is the load-bearing find: `partial_direct_test_functions_are_present` is not
purely a text search. It enforces table invariants an oracle structurally cannot see, so
"retire the substring guard" would have silently retired those as well.

Independent confirmation worth recording: the run's single survivor was exactly the
`log.rs:131:18 == -> !=` mutant the workflow already enumerates as equivalent — a budgeted
entry re-derived rather than re-read.

## U-545 — a guard is only as wide as the filter that sized it

U-542 (#309) stopped the SQLite sidecar leak in `acdp-registry-server` and, by existing, made the
class look closed. It was not. Two crates were still leaking, and the reason the second one was
invisible is the part worth keeping.

**The census that sized U-542 used an `acdp-*` filename filter.** The surviving sites call
`tempfile::NamedTempFile::new()` with no prefix, so their files land as `.tmpXXXXXX` and that filter
could never have counted them: 17,538 `-wal` + 17,538 `-shm` = **35,076 files**, against 174,003
`*-wal` in the directory in total. The filter was not wrong about what it measured; it was silently
narrower than the question being asked of it. **A tool default is a filter you never typed** — and so
re-reading your own pipeline cannot reveal the omission, because the omission is not in anything you
wrote. The same failure arrived from the opposite direction moments later, when an `ls -1` reading
omitted dotfiles and returned a clean-looking 0. What settled it was arithmetic over two independent
enumerations: 156,465 + 17,538 = 174,003, exactly.

**The other crate was invisible for a structural reason.** `acdp-registry-core` leaked from a
`#[cfg(test)]` module inside `src/`. No integration test can reach that — it is a different
compilation target — and the crate had no `tests/` directory at all, so there was nowhere the defect
could have been caught and nobody listed it as a candidate. Meanwhile the starting list *included*
`acdp-registry-pg`, which has zero `tempfile` references and is Postgres-backed. The list was wrong
in both directions, which is the argument for re-deriving a class from the tree rather than
inheriting it.

**Two mechanisms, because neither closes the class alone.** The source scan keys on the call shape —
a file guard bound and connected within ten lines — and contains no prefix anywhere, since a
no-prefix call is the *default* form and therefore the most likely shape of the next instance. The
runtime guard proves that shape actually cleans up, in the crate that had never had a test target.

**Both were falsified separately, and that is the lesson that cost the most.** The scan went red on
the real defect reintroduced in `witness.rs`, naming the exact `file:line`. On the strength of that
red I described both guards as working. The core guard did not compile — `store.migrate()` is a
trait method and its trait was not in scope — and had never executed once. `cargo test -p <pkg>
--test <name>` compiles that package's target and nothing else, so a red from one guard is **zero
evidence** about a guard beside it in the same commit. It does not feel that way: a clean
falsification reads as "the unit is in good shape" rather than "this one assertion in this one binary
fires". A brand-new test file in a crate with no prior `tests/` directory is the worst case, because
nothing in that package had ever proven its dev-dependencies or trait imports. `cargo test
--workspace --no-run` costs one command and would have caught it.

The runtime falsification left exactly **two** files behind, `-wal` and `-shm` — not three. The guard
cleaned the parent and orphaned both siblings, reproducing on demand the 14-parents-vs-286,894-
sidecars signature that identified the defect in the first place. The leaked names were
`.tmpFXt9XC-*`: the blind spot, caught by name in the failure output.

Both guards carry their limits in their own doc comments, including that a source scan fails on a
pattern being present and never on a correct-but-absent test.

## U-554 — a release PR that breaks CI is a process defect, not an accident

The v0.1.4 release PR failed three checks that pass on main. All three fail on one assertion,
`root_changelog_stays_a_pointer`: the workspace is at 0.1.4 but `acdp-registry-auth`, `-pg` and
`-webhook` have no `## [0.1.4]` section.

**Three rules, any two of which are compatible.** Every crate is `version = { workspace = true }`, so
a release bumps all eight. release-plz writes a changelog section only for crates with commits in
their own directory, and no configuration option exists to change that. The guard requires every
crate to document the version it ships. The three crates that fail are exactly the three with zero
commits since v0.1.3.

**It recurs, and that is measured rather than predicted.** In the previous window all eight crates had
at least two commits, so every one got a section and the guard passed. It has simply never been
exercised against "a crate did not change" — and it fires on every release where one hasn't.

Two traps worth keeping from the measurement itself. **`git tag -l 'v*'` matches nothing in this
repo**, because `git_tag_name = "{{ package }}/v{{ version }}"` puts the package first; anchoring a
range on that glob yields a degenerate range and a confident zero for every crate. The counts here
were cross-checked against each crate's own tag, and all eight `v0.1.3` tags resolve to the same
commit, so the two anchors are equivalent.

**The fix documents the non-change instead of narrowing the guard.** A consumer upgrading auth
0.1.3 → 0.1.4 would otherwise find no record at all and could not tell "nothing changed" from
"someone forgot". Narrowing would also have undone U-538's hardening of that same walk, which was
written for precisely this neighbourhood.

**Ordering mattered more than speed.** release-plz does not force-push its branch: it opens a new
timestamped branch and PR each run and abandons the old one — #278 and #286 are two PRs for the same
version, the first closed in favour of the second. A hand-edit on the open PR would have been
stranded on an abandoned branch and the replacement would have gone red again with nobody watching.
So the fix lands on main and lets regeneration carry it.

**Two testing notes that generalise.** First, the fix's own PR is green at 0.1.3 and that green
proves nothing, because every changelog already carries its section — the real proof was reproducing
the failure at the PR head and clearing it there. Second, the script was tested twice: once
standalone and once **extracted from the parsed YAML**, because escaping differs between the two. It
did differ — a backtick pair inside a double-quoted string would have executed `pr` by command
substitution in the runner but never in the standalone copy.

### Postscript — the linter was right about the family, not the line

`actionlint`'s shellcheck pass rejected the generator twice with `SC2016`, "expressions don't expand
in single quotes", pointing at the two lines that write the stub's markdown code spans. The code was
correct — a backtick inside single quotes is a literal, which is precisely why single quotes were
used — so a `# shellcheck disable=SC2016` would have been defensible and one line long.

It was the wrong fix, because the suppression would have covered the *adjacent* form too. In a
double-quoted string or an unquoted heredoc those backticks are command substitution, not text: a
later contributor adding another `$VERSION` to that paragraph would reasonably switch to `"`, and the
step — which runs with a push token against the release branch — would then execute
`version = { workspace = true }` and `crates/<name>/` as commands. The warning named the family; this
line happened to be its safe member.

The fix costs the same and removes the family: hold the backtick in `BT` and pass it as a `printf`
argument, so it never sits inside a quoted string under any quoting style. `actionlint` goes from two
findings to zero, and the dangerous refactor becomes impossible by construction rather than merely
absent today.

Verification was done against the **parsed YAML**, not the file — this is the second defect in this
block that exists only after YAML parsing, the first being a backtick that would have invoked the `pr`
paginator. The extracted script was re-run against an eight-crate fixture (six missing a `0.1.4`
section, two already written by release-plz): six stubbed, two skipped, and the generated prose
asserted byte-identical to before the change. A canary defining `version()` and `crates()` as failing
shell functions stayed silent, which is what makes "nothing executed" a measurement rather than a
reading of the diff.
