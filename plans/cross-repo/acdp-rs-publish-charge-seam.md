# acdp-rs — a verification/charge seam so a registry can charge late failures

**Target repo:** `agentcontextdistributionprotocol/acdp-rs` (the `acdp` SDK).
**Written from:** `acdp-registry-rs`, unit U-501, closing the in-repo half of
[`acdp-registry-rs#242`](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/issues/242).
**Status here:** design only. This file is a *read* target for whoever picks the work up in
`acdp-rs`; nothing in this repo writes to that one.

## The problem, from the registry's side

`acdp-registry-rs` rate-limits publishes per signing `agent_id`. The limiter must only ever be
charged against an agent whose identity has been **proven**, because charging an unproven
`agent_id` lets an attacker spend a victim's budget by naming them, and lets them grow the
bucket map without bound.

That constraint is easy to satisfy on the success path and hard to satisfy on the failure
path, because the registry cannot see *where* inside an SDK publish call the failure happened.
So a publish that fails after a full verify plus a store round-trip currently costs the
producer nothing on the `did:web` production path.

U-501 closed this for two of the registry's four publish branches without any SDK change:

- **`did:key`** — the registry now proves identity itself, before the SDK call, by composing
  two already-public, already-ungated SDK functions:
  `acdp::crypto::hash::compute_content_hash` (bind `content_hash` to the body) and
  `acdp::crypto::verify::verify_publish_request_signature_offline` (bind `content_hash` to
  `agent_id`'s key). This mirrors `acdp-server`'s own did:key pipeline
  (`crates/acdp-server/src/registry/server.rs:492-518`) exactly.
- **playground pinned keys** — the registry already verifies the signature itself there.

It could **not** be closed for the `did:web` production path, which is the one that matters
most in production.

## Why did:web cannot be solved from the registry side

`publish_verified_in_tenant` resolves the producer's DID document over the network and verifies
inside the call. For the registry to establish identity before that call, it would have to
perform its **own** DID-document resolution — a second network round-trip per publish, a second
SSRF surface to secure, and a cache that can disagree with the SDK's. All three are worse than
the gap.

There is no public seam that reports "identity was established" independently of "the publish
succeeded".

## What is actually being asked for

A way for a caller to learn *whether the signing identity was established* on a publish that
failed. Sketched three ways, cheapest first — **the SDK owners should pick; this is a statement
of need, not a prescription.**

### Option A — an identity-established flag on the error path (smallest surface)

Expose whether the failure happened before or after step 7–8 of the RFC-ACDP-0003 §2.1
pipeline. For example a method on the error, or a richer result type:

```rust
impl AcdpError {
    /// True when the producer's signature over `content_hash` had already been
    /// verified when this error was raised.
    pub fn identity_was_established(&self) -> bool;
}
```

**This must be computed at the raise site, not classified after the fact.** A registry-side
`match` over error variants is exactly the fail-open denylist that `#242` rejected: `AcdpError`
is `#[non_exhaustive]`, so a new variant silently defaults to "not charged". Only the SDK knows
where in the pipeline it was, so only the SDK can answer this without a rot surface.

### Option B — a split publish API

```rust
let proven = server.verify_publish_identity(&req, &resolver).await?; // steps 7-8 only
let response = server.commit_proven(proven, idempotency_key, tenant).await?;
```

The caller charges between the two. Strictly more useful than A (it also removes the duplicated
hash+verify the registry now pays on the `did:key` path), and strictly more invasive.

### Option C — a callback invoked once identity is established

```rust
server.publish_verified_in_tenant_with(&req, ..., |proven_agent_id| { /* charge */ }).await
```

Least invasive to existing call sites, but a callback firing mid-pipeline is harder to reason
about than either of the above, and it inverts control for what is really just a fact the
caller wants reported.

## Acceptance, whichever option is chosen

1. A caller can distinguish "failed before identity was established" from "failed after",
   **without** matching on `AcdpError` variants.
2. The answer is produced where the pipeline knows it, so a newly added error variant cannot
   silently change it.
3. `did:key` callers can avoid re-doing hash recomputation and signature verification that the
   publish call will do anyway (Option B only).
4. No behaviour change for callers that do not opt in.

## What the registry will do once this exists

Replace `publish_identity_proven_offline` (a pre-flight duplicate of SDK work) with the SDK's
own signal, and arm the `PublishCharge` guard on the `did:web` production branch too — taking
`acdp-registry-rs#242` from two of four branches to three of four. The fourth (playground,
unpinned) verifies nothing at all and must stay uncharged by design.
