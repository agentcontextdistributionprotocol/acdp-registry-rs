# Multi-tenancy

The registry can scope every operation — publish, retrieve, search, lineage,
pagination — to a **tenant**. Tenancy is off by default (V0-compatible): with no
bindings configured and `require_tenant = false`, requests run unscoped, gated
only by visibility.

Resolution is centralized in `tenant_for_request()` and `tenant_for_publish()`
(`crates/acdp-registry-core/src/handlers/context.rs`). Handlers must never
branch on tenant ad hoc — they call these functions.

## Resolution precedence

Both functions use the same order:

```
signed JWT `tenant` claim  >  [[auth.tenant_agents]] binding (writes)  >  TRUSTED X-Tenant-Id  >  None
```

- The JWT `tenant` claim is **authoritative** — it's issuer-signed. It is set
  only for agents bound via `[[auth.tenant_agents]]` (see
  [AUTHENTICATION.md](AUTHENTICATION.md#jwt-claims)).
- On writes, the producer's `[[auth.tenant_agents]]` binding comes next. Publish
  is producer-authenticated by the signature over `content_hash`, not by a
  bearer, so the binding is what proves which tenant a producer writes into.
- `X-Tenant-Id` selects a tenant only when it is **trusted** — when it crossed
  the boundary the operator declared with
  [`auth.tenant_header_trust`](#who-may-send-x-tenant-id) — and only when no claim
  or binding applies. A trusted header never overrides a claim or a binding.
- A header that agrees with the claim or binding is accepted in every mode
  (it corroborates; it decides nothing). A header that **disagrees** with them is
  rejected (`403 not_authorized`, "tenant assertion mismatch").
- `None` means "no tenant asserted" → the tenant filter is disabled (V0), or,
  in strict mode, the request is default-denied.

## Who may send `X-Tenant-Id`

Nothing in the header authenticates it: any client can send any value. RFC-ACDP-0008
§6.4 (pinned spec) says an unauthenticated tenant indicator MUST NOT be trusted
unless deployment policy at a trust boundary, an authenticated gateway, or a
signed claim stands behind it. The registry cannot see that boundary, so the
operator declares it:

```toml
[auth]
tenant_header_trust = "trusted_proxies"   # "none" | "trusted_proxies" | "any_peer"

[rate_limit]
trusted_proxies = ["10.0.0.0/8"]          # the gateway(s) that stamp X-Tenant-Id
```

| `tenant_header_trust` | The header is trusted when… |
|---|---|
| `none` | never |
| `trusted_proxies` | the **immediate TCP peer** is inside `rate_limit.trusted_proxies`. `X-Forwarded-For` plays no part, and a request with no recorded peer is untrusted. The gateway must strip or overwrite any `X-Tenant-Id` a client sends; the registry cannot check that. |
| `any_peer` | always — you assert a boundary the registry cannot observe (a network policy whose ingress addresses you cannot enumerate, or loopback-only development) |

When the key is absent, 0.3.x applies `none` under `require_tenant = true` and
`any_peer` otherwise (the pre-#374 behaviour), and warns at startup in the
`any_peer` case. **The default becomes `none` for every mode in 0.4.0** — set the
key explicitly. See [UPGRADING.md](UPGRADING.md).

A header that is present but **untrusted** is rejected, not ignored — unless it
equals the claim or binding the request already carries:

```
403 not_authorized
X-Tenant-Id is not trusted from this peer (auth.tenant_header_trust = "<mode>"); use a tenant-bound token or send the request through the declared gateway
```

Ignoring it instead would silently run the request, or place the write, in the
untenanted bucket the caller did not ask for. Each rejection is logged at WARN
with the peer address and the mode. The reserved `default` value is still
refused first (400 `schema_violation`), and a claim/binding mismatch is still
its own 403.

### Every case

T = trusted header, U = untrusted header present, – = no header.

| Case | Reads (`tenant_for_request`) | Writes (`tenant_for_publish`) |
|------|------------------------------|-------------------------------|
| `auth.enabled = false` | T → header; – → unscoped; U → 403 | T → header; – → untenanted; U → 403 |
| Lax mode, no valid bearer or an unbound token | T → header; – → unscoped; U → 403 | bound agent: the binding (a different header → 403); unbound agent: T → header, – → untenanted, U → 403 |
| Strict mode, no valid bearer | T → header; – or U → 403 | bound agent: the binding; unbound agent: T → header, – or U → 403 |
| Strict mode, unbound token | T → header; – or U → 403 | as the row above |
| Tenant-bound token | the claim; a different header → 403, in every mode | the claim; a different header → 403 |

On reads the visibility rules still apply on top: an anonymous caller only ever
sees `public` rows, and only when `anonymous_public_reads` is on. A trusted
header picks *which tenant's* rows those are.

The one case that is looser than before #374 is strict mode with a **trusted**
header and no claim or binding. That is deliberate: under `trusted_proxies` the
gateway is the authenticator (§6.4's second bullet), and strict mode with no
`[[auth.tenant_agents]]` has no other way to name a tenant.

**`any_peer` is the operator's declaration, not the registry's.** With
`any_peer` (the 0.3.x default in lax and auth-off mode) the registry trusts
every client's header, so it meets §6.4 only if something in front of it
strips or overwrites `X-Tenant-Id` from clients (or sets it from an
authenticated identity). Startup warns when `any_peer` is in effect on a
non-loopback bind. Prefer `trusted_proxies`, or tenant-bound tokens with
`none`.

## Strict mode (`auth.require_tenant = true`)

On an enforced multi-tenant deployment:

- A request that resolves to **no tenant** is default-denied (`403
  not_authorized`) — serving it would run with the filter off and could surface
  cross-tenant rows.
- A caller's tenant comes from the JWT `tenant` claim, the producer's binding
  (writes), or a header trusted under `auth.tenant_header_trust`. With the key
  absent, strict mode distrusts the header (`none`).
- Configuring any `[[auth.tenant_agents]]` requires `require_tenant = true`;
  startup validation enforces this so tenancy can't be half-enabled.
- `require_tenant = true` with no `[[auth.tenant_agents]]` and
  `tenant_header_trust = "none"` is refused at startup: no request could ever
  resolve a tenant.
- Strict mode itself requires a tenancy-aware storage backend (`sqlite` or
  `postgres`): startup validation refuses `storage.backend = "memory"` when
  **either** `require_tenant = true` or a non-empty `[[auth.tenant_agents]]`
  is configured. See [Backend support](#backend-support).

## The reserved `default` sentinel

`default` is the column value for untenanted rows. It is **rejected** as an
explicitly-asserted tenant from any source — header or token claim. Allowing a
caller to assert `default` would alias the entire untenanted bucket, a
cross-boundary read/write. Untenanted rows remain reachable only through the
*absence* of any tenant assertion (`None`).

## Backend support

Tenancy requires the `sqlite` or `postgres` backend. The `memory` backend is
**not** tenancy-aware, and startup is refused when `storage.backend = "memory"`
is combined with **either** tenancy signal: a non-empty `[[auth.tenant_agents]]`,
or `require_tenant = true`. An untenanted memory registry — neither of those set
— still starts, which is the ephemeral demo case the backend exists for.

`MemoryStore` overrides none of the three tenancy methods below, so it inherits
their untenanted defaults: `set_tenant_of_ctx` is a no-op, and `tenant_of_ctx` /
`tenants_of_ctxs` report `default` for every row. Because `default` is the
reserved sentinel above and cannot be asserted by any caller, no tenant a caller
*can* assert would ever match a row — every tenant-scoped **read** returns zero
rows. (Publishes still succeed; they simply record no tenant, which is what makes
the reads empty.) A warning would therefore have nothing working to preserve on
the read path: the registry would start cleanly and then serve nothing.

Both signals are covered because either one alone is enough to break reads.
`require_tenant = true` with an *empty* `tenant_agents` is a real configuration:
with no agent bindings no registry-issued token ever carries a `tenant` claim, so
a caller's tenant comes from an `X-Tenant-Id` header stamped by the gateway
declared with `auth.tenant_header_trust`. On this backend those requests then fail
the same way as the `tenant_agents` arm — no asserted tenant can match the
`default` each row reports. (The original refusal keyed on `[[auth.tenant_agents]]` alone and
left that arm starting cleanly and serving nothing; closed in #156.)

## How the binding is stored and filtered

The store carries the tenant binding alongside each context:

- `set_tenant_of_ctx` / `tenant_of_ctx` / `tenants_of_ctxs` — write and read the
  binding.
- `list_contexts(tenant)` and `/admin/contexts` filter at the **SQL level**, so
  pagination pages don't short.
- `list_contexts` also carries `anonymous_public_reads`, the same
  RFC-ACDP-0008 §4.5 term `search` uses: for an anonymous caller
  (`requester = None`), `public` rows are listed only when it is `true`.
  This is orthogonal to tenant filtering — the two are ANDed together, so
  an anonymous caller with `anonymous_public_reads = false` sees zero rows
  regardless of tenant, and a non-`None` requester's results are unaffected
  by this flag.
- `GET /contexts/search` with a tenant asserted is filtered, paged and counted
  **in SQL** on SQLite and Postgres: the handler calls
  `search_in_tenant`, which puts the tenant predicate in the same statement as
  the keyset cursor and the `total_estimate` count. Pages therefore fill to
  `limit` with the caller's own rows, the cursor is anchored on one of them, and
  `total_estimate` counts only that tenant. The handler still re-checks each
  row's binding (`tenants_of_ctxs`) as defence in depth; against those backends
  it drops nothing. (The memory backend is not tenancy-aware; see
  [Backend support](#backend-support).)
- The optional `?visibility=` narrowing is different: it is applied **after**
  the query, in the handler, so it can short a page. Search then runs a bounded
  refill loop, capped at `SEARCH_REFILL_MAX_PAGES` inner pages (**6** today,
  `handlers/context.rs`). Hitting that cap returns **fewer than `limit`** rows
  while still emitting a non-`None` `next_cursor`, so a short page is NOT an
  end-of-results signal — keep paging until `next_cursor` is absent.
- Lineage reads post-filter the tenant binding in the handler, but neither
  paginates, so neither has a short page to misread:
  - `GET /lineages/{lineage_id}` returns the complete lineage in one unpaginated
    array and filters it in the handler. There is no refill loop and no cursor;
    a fully-foreign lineage comes back as an empty array, not a 404.
  - `GET /lineages/{lineage_id}/current` filters the single resolved version and
    returns **404 `no current version`** when it belongs to another tenant —
    deliberately indistinguishable from "no such lineage", so the endpoint does
    not confirm existence across a tenant boundary.

  The RFC-ACDP-0008 §4.5 visibility rule (who may see a `public`/`restricted`/
  `private` row) is a separate axis, enforced in the search SQL, and never
  causes short pages. Only the caller's own `?visibility=` narrowing does.

## Configuration

```toml
[auth]
enabled        = true
require_tenant = true            # strict: deny requests that resolve to no tenant

[[auth.tenant_agents]]
agent_did = "did:web:agents.acme.example:billing-bot"
tenant_id = "acme"

[[auth.tenant_agents]]
agent_did = "did:web:agents.globex.example:ingest"
tenant_id = "globex"
```

A bound agent's tokens carry `"tenant": "acme"`; its publishes write into the
`acme` namespace regardless of any `X-Tenant-Id` header. See
[CONFIGURATION.md](CONFIGURATION.md#authtenant_agents) for the field reference.
