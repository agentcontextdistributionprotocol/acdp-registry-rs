# Architecture

How the 8-crate workspace fits together, and the path a request takes through
it. The per-topic docs linked below go deeper on each subsystem.

```
acdp (crates.io)  ── types / crypto / did / validator / RegistryServer
  │
  ▼
acdp-registry-types ......... leaf. EVERY crate below depends on it.
  │
  ├──▶ acdp-registry-store ... trait ExtendedRegistryStore
  │        │
  │        ├──▶ acdp-registry-pg      (Postgres backend)
  │        └──▶ acdp-registry-sqlite  (SQLite backend)
  │
  ├──▶ acdp-registry-auth ..... DID challenge → JWT, revocation
  │
  └──▶ acdp-registry-webhook .. HMAC POSTs

acdp-registry-core ......... axum + handlers, generic over S
  depends on: -types, -store, -sqlite, -auth, -webhook

acdp-registry-server ....... binary; picks S via Cargo features
  depends on: all seven
```

Edges above are the real `[dependencies]` graph, not a sketch. Re-derive it
with:

```bash
cargo metadata --format-version 1 --no-deps | python3 -c "
import json,sys
for p in sorted(json.load(sys.stdin)['packages'], key=lambda x: x['name']):
    deps = sorted({d['name'] for d in p['dependencies']
                   if d['name'].startswith('acdp-registry')})
    print(f\"{p['name']:<24} -> {', '.join(deps) or '(leaf)'}\")
"
```

This replaced a hand-drawn box diagram that asserted two dependency edges
which do not exist: `-store → -auth` and `-sqlite → -webhook`. Both of those
crates depend on `-types` alone. A drawing cannot be checked by anything, so
it drifted silently; the command above can be run in one line, which is why
the edges are written as text.

The protocol library [`acdp`](https://crates.io/crates/acdp) is consumed as a
**crates.io** dependency (not a path dep) — `types`, crypto, `did` resolution,
the publish validator, and the `RegistryServer` algorithm all live there. This
repo adds storage backends, HTTP wiring, auth, tenancy, webhooks, and the
binary on top.

## Storage trait

`ExtendedRegistryStore: acdp::registry::RegistryStore + Send + Sync` adds, on
top of the upstream sync trait:

- `list_contexts(limit, cursor, requester, tenant, public_arm_open) ->
  Page<FullContext>` — visibility-filtered, tenant-scoped admin/debug
  pagination; `public_arm_open` gates whether a `requester = None` caller
  sees `public` rows, the same RFC-ACDP-0008 §4.5 term `search` already
  honors. It is the predicate's input, not the config flag: the caller
  derives it. The only production caller, `admin_list`, passes `true` (an
  admin bearer is authenticated); `search` instead derives the term from the
  capabilities' `anonymous_public_reads`.
- `health()` — ping the backend (drives `/healthz`).
- `migrate()` — apply pending migrations at startup.
- Tenant binding — `set_tenant_of_ctx` / `tenant_of_ctx` / `tenants_of_ctxs`,
  plus the durable revocation cursors used by federation. See
  [MULTI-TENANCY.md](MULTI-TENANCY.md).
- `search_in_tenant(params, requester, public_arm_open, tenant)` — search with
  the tenant predicate in the same SQL statement as the keyset cursor and the
  count, so a tenant-scoped page, cursor and `total_estimate` only ever see that
  tenant's rows. The search handler calls it whenever a tenant is asserted. The
  default implementation (untenanted backends) returns an empty page for any
  tenant but `default`; SQLite and Postgres override it.
- `visible_ctx_ids(ctx_ids, requester, tenant, public_arm_open)` — which of a
  batch of ids the caller may *retrieve* (RFC-ACDP-0008 §4.5 retrieve rules,
  not search rules, plus the tenant gate), in one query on the SQL backends.
  `/log/entries` uses it to decide which leaves to echo.

The sync `RegistryStore` methods inherited from `acdp` are required so the
upstream `RegistryServer` publish algorithm (see
[Publish pipeline](#publish-pipeline)) runs unchanged. The
Postgres and SQLite implementations bridge to async sqlx via
`tokio::task::block_in_place + Handle::current().block_on(...)`; HTTP handlers
wrap the sync calls in `tokio::task::spawn_blocking`.

`acdp-registry-core` is **generic** over `S`, not boxed — the server binary
monomorphizes the type when it builds `Arc<AppState<S>>`. The storage backend is
chosen at **compile time** by the `acdp-registry-server` Cargo features; the
`storage.backend` config key must agree with the built binary.

## Request lifecycle

Every request passes through the middleware stack assembled in `build_router()`
(`crates/acdp-registry-core/src/lib.rs`), outermost first: an `if_not_present`
media-type layer → `x-request-id` assignment + propagation → a `413` envelope
backstop → CORS → `RequestBodyLimitLayer` (capped at `limits.max_payload_bytes`)
→ 30 s `TimeoutLayer` → `TraceLayer` → request metrics (when `metrics.enabled`;
FEAT-10). The `/auth/*` routes additionally carry the FEAT-06 per-IP/global
rate-limit `route_layer` (`auth_rate_limit`), and ACDP data and auth routes carry an
`application/acdp+json` response-header layer.

`auth_rate_limit` charges each request to whatever `SharedRateLimitBackend`
`AppState` holds: the in-memory `AgentRateLimiter` by default, or, with
`[rate_limit] backend = "postgres"`, a `LayeredRateLimiter` that asks the
in-memory limiter first and only on an admit asks `PgRateLimitBackend` (one
atomic upsert per check on the `rate_limit_windows` table). The layering is
applied in `AppStateInner::with_shared_auth_limiter`, never by the binary, so a
shared backend cannot be installed without its in-memory pre-filter; the
layered limiter also applies the `backend_unavailable` posture. Details:
[CONFIGURATION.md · `[rate_limit]`](CONFIGURATION.md#rate_limit-feat-06).

Note the ordering rule that governs this list: `Router::layer` makes the **later**
call the **outer** one — the inverse of `tower::ServiceBuilder`, where the first
call is outermost. The request-id and envelope backstops are deliberately outside
the body limit and the timeout so that responses those layers synthesize
themselves (413, 408, CORS preflight) still carry an `x-request-id`; within the
request-id pair, `SetRequestId` must be applied last so it runs first, because
`PropagateRequestId` reads the id from the request headers. Full endpoint reference:
[HTTP-API.md](HTTP-API.md).

## Publish pipeline

The protocol-critical part of `POST /contexts` is **not** implemented here — it
is `acdp`'s `RegistryServer`, which implements the ordered algorithm of
[RFC-ACDP-0003 §2.1](https://github.com/agentcontextdistributionprotocol/agentcontextdistributionprotocol/blob/9deb7e7bdabfa7416fcc0e25a7fcac6eb642b6dd/rfcs/RFC-ACDP-0003-publish.md#21-registry-processing).
The steps and their one invariant are specified there and explained in
[acdp-rs · Implementing a Registry](https://github.com/agentcontextdistributionprotocol/acdp-rs/blob/8a888edaa15c4475bbaeccff45567921e3153730/docs/registry.md#the-publish-pipeline--the-one-rule);
this page does not restate them. We reuse it unchanged and add storage
adapters, **not** a parallel validator. One difference from that guide: we do
not plug a limiter into `RegistryServer` (`with_rate_limiter`) — the per-agent
publish budget is this registry's own, peeked and charged in steps 3 and 6
below.

The registry calls it in **two halves** rather than as one bundled call
(`acdp` 0.14's prove/commit split): a `prove_publish_identity*` call runs the
§2.1 validation and signature steps and persists nothing, and `commit_proven`
then runs the atomic store commit (idempotency lookup, predecessor checks,
insert, supersession marking). The split exists so the per-agent publish
budget can be charged at the exact point the signer becomes proven — between
the two halves — and not before.

What `publish` (`crates/acdp-registry-core/src/handlers/context.rs`) wraps
around those calls:

1. Body-size cap (the `RequestBodyLimitLayer`, uniformly across routes) and the
   `Content-Type` gate (`AcdpBytes`; see
   [HTTP-API.md](HTTP-API.md#request-bodies-and-content-type)).
2. JSON deserialization into `acdp::types::publish::PublishRequest`, then the
   RFC-ACDP-0016 `anchors` version gate.
3. A read-only **peek** at the per-agent budget
   (`limits.publish_rate_per_minute`) for the *claimed* `agent_id`: an agent
   already over budget gets `429`, but nothing is charged and no bucket is
   created, because the claim is not yet proven.
4. Tenant resolution for the write (`tenant_for_publish`; see
   [MULTI-TENANCY.md](MULTI-TENANCY.md)).
5. One of four branches, each pairing a proof with `commit_proven` except the
   last:
   - `did:key` producer (checked first, whatever `[playground]` says) →
     `prove_publish_identity_did_key`, offline;
   - playground, agent with a pinned key → the pinned-signature check, then
     `prove_publish_identity_pinned`;
   - production → `prove_publish_identity`, resolving the `did:web` document;
   - playground, unpinned agent → (refused outright when `pinned_only` is
     set) `publish_unverified_for_tests`, the SDK's
     test path that skips the §2.1 signature steps (a protocol violation
     reserved for demos), with idempotency and tenant stamping done around it
     by the handler.
6. The charge: a `PublishCharge` guard is **armed** as soon as a
   `prove_publish_identity*` call succeeds, so every later failure on those
   three branches (a store error, a duplicate-publish race) is charged too;
   it is armed again on success, which is the only charge on the unpinned
   playground branch. Arming is idempotent, so a publish is charged at most
   once, and a publish that fails before its signer is proven is never
   charged.
7. Receipt minting and the transparency-log leaf append, when `[receipt]` /
   `[log]` are enabled (see [RECEIPTS.md](RECEIPTS.md)).
8. A `context.published` webhook on success (see [WEBHOOKS.md](WEBHOOKS.md)).

The four-way charge split is pinned by
`late_failures_are_charged_on_exactly_three_of_the_four_publish_branches`
(`crates/acdp-registry-server/tests/http_integration.rs`).

The producer lifecycle routes (`/retract`, `/republish`) share that per-agent
bucket, keyed by the event `actor`, and follow the same rule: a read-only peek
before verification, a `PublishCharge` armed once the signer is proven. They
use the SDK's lifecycle prove/commit split (`acdp` 0.14.4): after the tenant
gate and the bearer check, `prove_lifecycle_identity*` runs RFC-ACDP-0013 §6
steps 1–3 (visibility, event validation and endpoint binding, actor ==
producer) and the signature verification, persisting nothing; the charge arms;
then `commit_lifecycle_proven` runs the store's locked strict-alternation check
and append. The signature is verified once, and only a proven producer is
charged; an event refused before its signature is checked (unknown or
invisible context, actor ≠ producer, wrong `event_type`) costs nobody.

DID verification reuses `acdp`'s `WebResolver` (LRU-cached, SSRF-policy-gated —
see [acdp-rs · Security Model](https://github.com/agentcontextdistributionprotocol/acdp-rs/blob/8a888edaa15c4475bbaeccff45567921e3153730/docs/security.md#ssrfpolicy)) for **both** publish and
auth-challenge verification; there is intentionally only one resolver per server
instance.

## Auth

A DID challenge-response flow mints a short-lived JWT bound to the agent DID and
this registry's authority; the validator checks the signature, `exp` (with
`auth.token_leeway_seconds` leeway), the `aud` / `acdp.registry` binding, and the
revocation store. This flow is registry-specific (it is not part of the `acdp`
protocol library). The full treatment — sequence, JWT claims, HS256 vs EdDSA,
revocation, cross-issuer federation — is in
[AUTHENTICATION.md](AUTHENTICATION.md).

## Visibility

Visibility enforcement is centralized: `RegistryServer::can_retrieve` for
single-object retrieval, and the SQL `WHERE` clause each store builds for
search (DESIGN-01 pushed the predicate into SQL on both backends). Both
implement the same RFC-ACDP-0008 §4.5 rule — handlers must never reimplement
it. When you add an endpoint that returns ACDP-typed data, route errors through
`RegistryError` (so the wire envelope lands automatically) and cover the
visibility rule in those shared paths, not in the handler.

## Crate map

| Crate | Role |
|-------|------|
| `acdp-registry-types`  | Leaf: config (TOML+env), errors with HTTP projection, webhook events. |
| `acdp-registry-store`  | `ExtendedRegistryStore` trait — extends `acdp::registry::RegistryStore`; also the `SharedRateLimitBackend` trait for the `/auth/*` ceilings. |
| `acdp-registry-pg`     | Postgres backend (native `TIMESTAMPTZ` / `TEXT[]` / `JSONB` / `tsvector`); `PgRateLimitBackend`, the cluster-wide `/auth/*` counter. |
| `acdp-registry-sqlite` | SQLite backend (FTS5 virtual table; arrays as JSON TEXT). |
| `acdp-registry-auth`   | DID challenge → JWT (HS256/EdDSA), revocation store + cross-issuer pollers. |
| `acdp-registry-webhook`| HMAC-SHA256-signed POSTs over a bounded mpsc channel. |
| `acdp-registry-core`   | axum router + handlers, generic over `S`; the in-memory rate limiters and `LayeredRateLimiter` (in-memory in front of a shared backend). |
| `acdp-registry-server` | Binary (`acdp-registry`); features select the storage backend. |
