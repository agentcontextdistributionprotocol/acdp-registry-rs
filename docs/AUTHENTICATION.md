# Authentication

The registry authenticates **agents** (clients) with a DID challenge-response
flow that mints a short-lived JWT, and authenticates **producers** (publishers)
implicitly via the signature over `content_hash` carried in the publish request.
This doc covers the first. Publish signing belongs to the protocol, not this
registry — how a producer builds and signs a `PublishRequest` is documented in
[acdp-rs · Producing][acdp-producing], and where the registry verifies it in
[ARCHITECTURE.md](ARCHITECTURE.md#publish-pipeline).

All of this lives in `crates/acdp-registry-auth/` and is mounted only when
`auth.enabled = true`. When auth is disabled, the `/auth/*` routes are not
mounted, any `Authorization` header is ignored, and every caller is treated as
anonymous (so the visibility gate runs against `None`).

## Challenge-response flow

```
client                                         registry
  │  POST /auth/challenge { agent_id }            │
  │ ────────────────────────────────────────────►│  validate did:web, mint nonce,
  │                                               │  persist ChallengeRecord(nonce, agent_id, expires_at)
  │  AuthChallenge { nonce, signing_input, ... }  │
  │ ◄──────────────────────────────────────────── │
  │                                               │
  │  sign signing_input with a DID assertionMethod key
  │                                               │
  │  POST /auth/token { agent_id, key_id, nonce,  │
  │                     expires_at, algorithm, signature }
  │ ────────────────────────────────────────────►│  consume nonce (one-shot),
  │                                               │  resolve DID doc, verify VM + signature,
  │                                               │  mint JWT, record jti as issued
  │  TokenResponse { token, token_type, expires_at }
  │ ◄──────────────────────────────────────────── │
  │                                               │
  │  GET /contexts/... Authorization: Bearer <jwt>│
  │ ────────────────────────────────────────────►│  validate_bearer (sig, exp, aud, revocation)
```

### The signing input is namespaced

```
acdp-registry-auth:v1:{nonce}:{agent_id}:{registry_authority}:{expires_at}
```

The `acdp-registry-auth:v1:` prefix and the `registry_authority` binding are
load-bearing: they stop a signature minted for one purpose or one registry from
being replayed as a challenge response elsewhere. **Do not** remove the version
prefix or the authority component (see CLAUDE.md → Conventions).

### Token issuance checks (`/auth/token`)

In order (`service.rs`):

1. Atomically **consume** the nonce — a second use of the same nonce is rejected
   (one-shot, replay-proof).
2. The request's `agent_id` and `expires_at` must match the stored challenge
   record (defeats nonce theft and tampering).
3. The challenge must not have expired.
4. `algorithm` must be supported (`ed25519` or `ecdsa-p256`).
5. `key_id` is split into a `did:web:` DID + fragment; the fragment is required.
6. The DID document is resolved via the shared `WebResolver` — HTTPS-only,
   SSRF-policy-gated, LRU-cached, the *same* resolver used for publish. Its
   defenses (IP-literal rejection, DNS-time SSRF filtering, size/redirect caps)
   are documented in [acdp-rs · Security Model][acdp-security].
7. The verification method named by the fragment must appear in the document's
   `assertionMethod` set.
8. If the verification method declares an algorithm, it must match the request's
   `algorithm` (algorithm-downgrade defense, RFC-ACDP-0001 §5.10 — enforced by
   `acdp`; see [acdp-rs · Security Model][acdp-security]).
9. The signature is verified against the resolved public key.
10. A JWT is minted and its `jti` is recorded as *issued* in the revocation
    store. If that write fails, the whole request fails — a token that can't be
    tracked is never handed out.

## JWT claims

```json
{
  "iss": "did:web:registry.example.com",
  "sub": "did:web:agents.example.com:my-agent",
  "aud": "registry.example.com",
  "jti": "<uuid-v4>",
  "iat": 1748000000,
  "exp": 1748003600,
  "acdp": {
    "registry": "registry.example.com",
    "key_id":   "did:web:agents.example.com:my-agent#key-1"
  },
  "tenant": "acme"
}
```

- `aud` and `acdp.registry` bind the token to this registry's authority — a
  token minted by a peer won't validate here.
- `exp` defaults to `iat + auth.token_ttl_seconds` (default 3600 s).
- `tenant` is present **only** for agents bound via `[[auth.tenant_agents]]`;
  it is the sole authority for an authenticated caller's tenant (see
  [MULTI-TENANCY.md](MULTI-TENANCY.md)).

### Validation

On every bearer-authenticated request the registry checks the signature, `exp`
(with `auth.token_leeway_seconds` of clock skew, default 30 s), the `aud` /
`acdp.registry` binding, and the revocation store. A revoked or expired `jti` is
rejected with `403 not_authorized`.

## Presenting a bearer

**Three** bearer parsers coexist and no two of them agree. Each is deliberate,
and the differences below are pinned by tests — `extract_bearer_accepts_two_casings_and_trims`
(`crates/acdp-registry-auth/src/service.rs`), `bearer_scheme_is_case_sensitive`
and `rejects_token_with_extra_whitespace` (`handlers/admin.rs:854-873`), and
`metrics_bearer_parser_shape_is_pinned`
(`crates/acdp-registry-server/tests/metrics_integration.rs`). The differences
were undocumented rather than accidental, and stating them is what this section
exists for.

The `/metrics` and `/contexts/*` halves of that coverage were added alongside
this section: before them, deleting `.map(str::trim)` from either parser, or
dropping `extract_bearer`'s `"bearer "` arm, left the entire workspace suite
green. Every behaviour claimed below is now locked; if you add a parser, pin its
differences in the same commit that documents them.

| Route group | Parser | Scheme prefixes accepted | Trims the token? | Unrecognised header shape |
|---|---|---|---|---|
| `/contexts/*`, `/lineages/*`, and the other ordinary read/publish routes | `extract_bearer` (`crates/acdp-registry-auth/src/service.rs:400-405`) | `Bearer ` **and** `bearer ` | yes | treated as **anonymous** |
| `/admin/*` | `require_admin_bearer` (`crates/acdp-registry-core/src/handlers/admin.rs:679-693`) | `Bearer ` only | **no** | rejected with **403** `{"error": "admin-only"}` (`admin.rs:741-745`) |
| `/metrics` | inline in `metrics_endpoint` (`crates/acdp-registry-core/src/metrics.rs:124-128`) | `Bearer ` only | yes | rejected with **401** + a `WWW-Authenticate` challenge (`metrics.rs:130-134`) |

The `/metrics` parser is a hybrid of the other two: case-sensitive on the scheme
like the admin one, trimming like the lax one. It is also the only one of the
three that is not part of the ACDP auth pipeline at all — see
[`/metrics` is gated separately](#metrics-is-gated-separately) below.

Its row describes `/metrics` **with its gate active** — `metrics.enabled = true`
and a non-blank `metrics.bearer_token`. Under the shipped defaults there is no
third parser in play at all: `metrics.enabled` is `false`, so the route is not
mounted and every request is a `404`; and with metrics on but the token empty,
the endpoint is open and no header shape is rejected.

No parser is case-insensitive on the scheme token. `extract_bearer` hardcodes
two casings, so `BEARER …` and `BeArEr …` are unrecognised by **all three**;
one parser is merely more permissive than the others, not more conformant.

### Unrecognised means anonymous on the ordinary routes

`caller_from_headers` (`crates/acdp-registry-core/src/handlers/context.rs:1348-1365`)
returns `Ok(None)` — an anonymous caller — in three cases:

- `auth.enabled = false`, regardless of what the client sent;
- no `Authorization` header, or a value the HTTP layer will not hand over as a
  string — `HeaderValue::to_str` rejects any byte outside visible ASCII, which
  is broader than "not valid UTF-8" (`Bearer café` is valid UTF-8 and still
  fails);
- any value `extract_bearer` does not recognise, including a non-`Bearer` scheme.

Only a **well-formed** bearer whose token then fails validation is rejected, with
`403 not_authorized` — auth failures **on the ACDP routes** are `403`, never
`401`, and no `WWW-Authenticate` challenge is emitted (see
[HTTP-API.md](HTTP-API.md#error-envelope)). A client whose token merely expired sees that
explicitly rather than being silently downgraded.

`/admin/*` also answers `403` and emits no challenge, though with its own body
(`{"error": "admin-only"}`) rather than the ACDP error envelope. **`/metrics` is
the exception to both halves** of that sentence: it answers `401` *and* sends
`WWW-Authenticate: Bearer realm="metrics"`. Scope any "this registry never
returns 401" reasoning to the ACDP routes and `/admin/*`.

The consequence worth knowing when debugging: **a typo in the scheme is not an
auth failure at all.** `Authorizaton: Bearer …` (misspelled header name),
`Basic …`, or `BEARER …` do not reach token validation — the request simply
proceeds as anonymous.

What the caller then sees depends on the route and on
`auth.anonymous_public_reads`, which ships as `false`. Anonymous access is
subject to the RFC-ACDP-0008 §4.5 visibility rules, so the same malformed header
can surface as a refusal on one route and as a short, successfully-filtered
result set on another. The invariant to hold onto while debugging is upstream of
that: the auth layer did not reject the header, it classified the caller as
anonymous. If a caller reports missing rows, or an authorization error that
names anonymity rather than a bad token, suspect the header shape before
suspecting the token.

On `/admin/*` the same inputs return `403` — absent, non-UTF-8, and unrecognised
headers are all refused, and an empty `auth.admin_tokens` list disables the routes
outright (`admin.rs:684`).

### What each parser accepts

`extract_bearer` strips `"Bearer "` or `"bearer "` and then trims the remaining
token. `require_admin_bearer` strips `"Bearer "` only, and does not trim. The
`/metrics` parser strips `"Bearer "` only, and does trim.

None of the three is case-insensitive: each hard-codes its prefixes, so `BEARER`
and `BeArEr` are unrecognised by all three.

The `/metrics` column below describes the endpoint **with its gate active** —
`metrics.enabled = true` and a non-blank `metrics.bearer_token`. With the token
empty the endpoint is open and every row is `200`; a whitespace-only token is
refused at startup rather than silently opening it (see
[`/metrics` is gated separately](#metrics-is-gated-separately)).

| `Authorization` value | `/contexts/*` | `/admin/*` | `/metrics` |
|---|---|---|---|
| `Bearer <token>` | `<token>` | `<token>` | `200` |
| `bearer <token>` | `<token>` | **403** | **401** |
| `Bearer  <token>` (two spaces) | `<token>` | token is `" <token>"`, so **403** | `200` |
| `Bearer <token>` + trailing space | `<token>` | **depends on protocol — see below** | `200` |
| `BEARER <token>`, `BeArEr <token>` | anonymous | 403 | **401** |
| `Bearer<TAB><token>` | anonymous | 403 | **401** |
| `Basic <token>`, bare `<token>`, empty | anonymous | 403 | **401** |

The three columns are not the same kind of thing: on `/contexts/*` an
unrecognised header yields an anonymous *caller*, whose eventual status depends
on the route and on the visibility rules; on `/admin/*` and `/metrics` it is the
response status itself.

Both behaviours on the admin side are pinned by tests, so loosening either is a
deliberate reviewed change rather than a refactor: `bearer_scheme_is_case_sensitive`
(`admin.rs:854-863`) and `rejects_token_with_extra_whitespace` (`admin.rs:866-873`).

#### Trailing whitespace depends on the HTTP version

Trailing whitespace in the header *value* never reaches any of the three parsers
over HTTP/1.1: `httparse` strips trailing SP/HTAB/CR/LF from every header value while
parsing the request. Over HTTP/2, HPACK carries the value verbatim and nothing
strips it. The registry serves both.

So `Authorization: Bearer <token> ` (one trailing space) is accepted everywhere
over HTTP/1.1 — the space is gone before any handler sees it — and on HTTP/2 it
is accepted by `/contexts/*` and `/metrics` (which trim) but **rejected by
`/admin/*`** (which does not). Only the *trailing* case is protocol-dependent,
and only on `/admin/*`; the two-space case above is internal to the value and
behaves identically on both protocols everywhere.

The practical consequence is that an admin token configured with stray trailing
whitespace can appear to work in one environment and fail in another, depending
on whether a proxy or client negotiated HTTP/2. Startup validation now refuses
such entries outright — see [#161](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/issues/161)
and the admin-token rules in [CONFIGURATION.md](CONFIGURATION.md).

**Practical rule: send exactly `Authorization: Bearer <token>`, one space, no
surrounding whitespace.** That form is accepted everywhere. Any other spelling
works on some routes and not others.

## `/metrics` is gated separately

`GET /metrics` does not go through the ACDP auth pipeline at all. It is mounted
on the un-authenticated, un-rate-limited `aux` router
(`crates/acdp-registry-core/src/lib.rs:153-155`), so no bearer it receives is
ever validated as an ACDP token — no signature check, no `exp`, no revocation
lookup, no tenant resolution. The handler applies its own gate instead, and that
gate is a plain string comparison against a configured value.

The gate is applied only when `metrics.bearer_token` is non-blank
(`crates/acdp-registry-core/src/metrics.rs:121-122`); the configured value and
the presented one are both trimmed before comparison (`:121`, `:128`). Three
consequences follow, and they are the ones that surprise people:

- **An empty `metrics.bearer_token` leaves `/metrics` open**, to anyone who can
  reach the port. This is the shipped default
  (`crates/acdp-registry-types/src/config.rs:701`) and it is deliberate — the
  endpoint is meant to be reachable from a trusted scrape network without ACDP
  credentials. It is not an oversight, but it is a decision your deployment
  inherits by default.
- **A whitespace-only token used to mean the same thing, silently.** `" "` trims
  to empty, so the gate was skipped entirely and the endpoint was served
  unauthenticated with no failed-auth signal in the logs — the request just
  looked like an authorized scrape. Startup validation now refuses that value
  when `metrics.enabled = true`, so the failure is a refused boot rather than a
  quietly open endpoint. See
  [#162](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/issues/162).
- **Padding is not refused here**, unlike `auth.admin_tokens`. Because both sides
  of the comparison are trimmed, `" tok "` and `"tok"` behave identically on
  HTTP/1.1 and HTTP/2 alike; there is no protocol-dependent credential to guard
  against, so the guard is deliberately narrower than the admin-token one.

Failures on this endpoint answer `401` with
`WWW-Authenticate: Bearer realm="metrics"` (`metrics.rs:130-134`) — the one place
in the registry that does. Everything else authenticated answers `403`.

What `/metrics` exposes is operational rather than secret — request counts,
latency histograms, publish outcomes — but an operator who sets
`metrics.bearer_token` has expressed an intent to gate it, and the point of the
startup check is that the intent is not silently discarded. Configuration
reference: [CONFIGURATION.md](CONFIGURATION.md).

## Signing algorithms

JWT signing is selected by `auth.jwt_signing_alg`:

| Alg | Key material | JWKS | Use it for |
|-----|--------------|------|------------|
| `HS256` (default) | `auth.jwt_secret` — base64, ≥32 bytes, symmetric | empty key set | Single registry; backward-compatible default. |
| `EdDSA` (Ed25519) | `auth.jwt_private_key_pem` — PKCS#8 PEM, asymmetric | publishes the public key | Federation — peers verify your tokens without sharing a secret. |

In HS256 mode the secret is never published; `GET /.well-known/jwks.json`
returns `{ "keys": [] }`. In EdDSA mode the public key is published there as an
OKP/Ed25519 JWK, with `kid` derived from the key fingerprint unless
`auth.jwt_kid` overrides it.

> **Dev convenience:** with HS256 and an empty `jwt_secret`, set
> `auth.allow_ephemeral_secret = true` to boot with a random process-lifetime
> key. Tokens won't survive a restart. Never use this in production — set a real
> `jwt_secret`. The startup validator refuses the literal `changeme` and refuses
> an empty secret unless `allow_ephemeral_secret` is set.

## Token revocation

`POST /auth/token/revoke` with `{ "jti": "..." }` and a valid bearer marks a
token revoked. The caller's DID must own the `jti` (you can only revoke your own
tokens). State lives in the revocation store
(`acdp-registry-auth/src/revocation_store.rs`):

- `record_issued` — written at mint time, `revoked = false` (never downgrades an
  existing `revoked = true`).
- `revoke` — flips the flag / writes a tombstone.
- `is_revoked(jti)` — checked on every bearer validation.
- `owner_of(jti)` — enforces the ownership check on revoke.
- `evict_expired(now)` — a background task prunes tombstones past `exp` (runs on
  a ~300 s tick alongside challenge eviction).

Recording the `jti` at *issuance* (not at revoke time) is what lets the registry
reject a revoked token that was never seen again — there's always a row to flip.

## Cross-issuer revocation federation

Revocation federation is **consume-only**. This registry does not expose a
`/auth/revocations` feed; it *polls* peers' feeds and applies their revocations
locally. Configure peers with `[[auth.revocation_feeds]]`:

```toml
[[auth.revocation_feeds]]
issuer       = "did:web:peer.example.com"          # must match each entry's `iss`
feed_url     = "https://peer.example.com/auth/revocations"
admin_token  = "<bearer for the peer's feed>"
poll_seconds = 300
```

A background poller per feed (`revocation_poller.rs`) fetches
`GET {feed_url}?since={cursor_ms}&limit=...`, sanity-checks each entry's `iss`
against the configured `issuer`, and applies remote revocations to the local
store. **Durable cursors** (`get_revocation_cursor` / `set_revocation_cursor`,
unix ms) survive restarts, and the cursor advances only when an entire page
applies cleanly — a partial failure replays that page on the next tick rather
than skipping revocations.

## Where it's wired

- Routes: `build_router()` in `crates/acdp-registry-core/src/lib.rs`.
- Flow + verification: `crates/acdp-registry-auth/src/service.rs`.
- JWT sign/verify and JWKS: `crates/acdp-registry-auth/src/jwt.rs`.
- Stores: `challenge_store.rs`, `revocation_store.rs` (in-memory / SQLite / PG).
- Startup wiring (signer choice, ephemeral secret, poller spawn):
  `crates/acdp-registry-server/src/main.rs`.

[acdp-producing]: https://github.com/agentcontextdistributionprotocol/acdp-rs/blob/main/docs/producing.md
[acdp-security]: https://github.com/agentcontextdistributionprotocol/acdp-rs/blob/main/docs/security.md
