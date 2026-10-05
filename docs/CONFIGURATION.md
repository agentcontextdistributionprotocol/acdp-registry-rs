# Configuration

`acdp-registry` is configured by a TOML file layered with environment-variable
overrides. There is no `clap` CLI — config loading is the `config` crate plus
env vars (same dependency-minimization principle as `acdp-rs`). The schema is
`RegistryConfig` in `crates/acdp-registry-types/src/config.rs`; a worked example
is [`config/registry.example.toml`](../config/registry.example.toml).

## Loading and precedence

```
built-in defaults  <  TOML file  <  ACDP_REGISTRY_* env vars
```

- The TOML file path comes from `ACDP_REGISTRY_CONFIG`; when unset the binary
  falls back to its defaults (dev runs use SQLite under `./data/registry.db`).
- Env overrides use `ACDP_REGISTRY_<SECTION>__<FIELD>` — a **single** underscore
  after the `ACDP_REGISTRY` prefix, then **double** underscores between nesting
  levels:

  ```bash
  export ACDP_REGISTRY_STORAGE__POSTGRES_URL="postgres://acdp:acdp@db:5432/acdp"
  export ACDP_REGISTRY_AUTH__JWT_SECRET="$(openssl rand -base64 32)"
  export ACDP_REGISTRY_AUTH__JWT_SIGNING_ALG="EdDSA"
  ```

### List-valued fields from the environment

Exactly **two** fields accept a comma-separated env value, and they are named
explicitly in the loader (`with_list_parse_key`, `config.rs`):

```bash
export ACDP_REGISTRY_AUTH__DID_METHODS="did:web,did:key"
export ACDP_REGISTRY_REGISTRY__PROFILES="acdp-registry-core,acdp-registry-discovery"
```

Every other field — including base64 secrets, which is the reason the rule is a
whitelist rather than "split anything containing a comma" — deserializes from the
environment as a plain scalar. A comma in one of those values is part of the
value.

### Two settings that cannot be expressed as env vars at all — and their escape hatches

`auth.tenant_agents` and `playground.pinned_keys` are lists **of structs**.
Comma-splitting can only ever produce a `Vec<String>`, so the ordinary
`ACDP_REGISTRY_<SECTION>__<FIELD>` mechanism cannot represent them. On a platform
with no config-file mechanism — Railway "deploy from image" services are the
motivating case — they would otherwise be unreachable.

Two JSON escape hatches exist for exactly this. They are read directly rather
than through the env layer, and are applied **after** every other source, so they
win if both a TOML file and the variable are present:

| Variable | Sets | Value |
|----------|------|-------|
| `ACDP_REGISTRY_AUTH__TENANT_AGENTS_JSON` | `auth.tenant_agents` | JSON array of `{agent_did, tenant_id}` |
| `ACDP_REGISTRY_PLAYGROUND__PINNED_KEYS_JSON` | `playground.pinned_keys` | JSON array of `{agent_did, public_key_b64, algorithm?, valid_from?, valid_until?}` |

```bash
export ACDP_REGISTRY_AUTH__TENANT_AGENTS_JSON='[{"agent_did":"did:web:agent.example.com","tenant_id":"acme"}]'
```

Invalid JSON fails startup with the variable's name in the message; it is never
silently ignored. Note that these replace the whole list rather than merging
into a TOML-configured one.

### Environment variables outside the config tree

These are read directly by the binary and have no TOML equivalent, so they do
not appear in the Reference below:

| Variable | Effect | Default |
|----------|--------|---------|
| `ACDP_REGISTRY_CONFIG` | Path to the TOML file. Unset means built-in defaults only. | unset |
| `ACDP_LOG_FORMAT` | `json` or `pretty`. `pretty` is for interactive runs; `json` is what you want behind a log collector. | `json` |
| `RUST_LOG` | Standard `tracing` filter. When unset the binary uses `info,acdp=info,acdp_registry=info`. | unset |

## Startup validation

The binary validates config before serving and refuses to boot on a misconfig
(`validate_config` in `crates/acdp-registry-server/src/main.rs`). It enforces:

- **Auth** — `jwt_signing_alg` ∈ {`HS256`, `EdDSA`}. EdDSA requires a non-empty
  `jwt_private_key_pem`. HS256 with an empty `jwt_secret` requires
  `allow_ephemeral_secret = true`, otherwise it fails. A non-empty
  `jwt_secret` is checked whether or not auth is enabled: it is rejected if it
  is the literal `changeme` (case-insensitive, after trimming), and must decode
  to ≥32 bytes. Under EdDSA `jwt_secret` is not examined at all.

  Note the asymmetry, and note that it is only partial. Inside
  `validate_config` the non-empty-`jwt_secret` rule is the only one in this
  bullet not gated on `auth.enabled` — but "gated here" is not the same as
  "not enforced", and the three gated rules do not behave alike. Measured
  against the binary with `auth.enabled = false`:

  - **Algorithm check** — an unrecognised `jwt_signing_alg` boots, falling
    through to the HS256 default. Genuinely not enforced.
  - **Empty-secret rule** — an empty secret boots. Genuinely not enforced, and
    deliberately: that is a supported configuration.
  - **EdDSA `jwt_private_key_pem`** — an empty PEM still refuses to boot, from
    the serve path, *after* migrations have run. Gated here, enforced anyway,
    late.

  So a bad `jwt_secret` is refused either way and refused *before* migrations;
  a missing EdDSA PEM is refused either way but only *after* them. Closing
  that second case is not done here.
- **Admin tokens** — every entry in `auth.admin_tokens` must be non-blank and
  carry no leading or trailing whitespace. An empty *list* remains valid and
  still means "admin routes disabled"; it is a bad *entry* that is refused.
  One bad entry is enough: the allowlist compare folds over every entry, so a
  blank one is admitted alongside real tokens while the list still looks
  populated. A bare `Authorization: Bearer ` matches a blank entry over
  HTTP/2, which preserves trailing header whitespace (HTTP/1.1 strips it
  before the handler sees it, so the same request is refused there).
- **Webhook** — when `enabled`, `url` must be non-empty and pass the SSRF policy
  (HTTPS, no private/internal authorities), and `secret` must be non-empty.
- **Multi-tenancy** — a non-empty `[[auth.tenant_agents]]` requires
  `require_tenant = true` (you can't half-enable tenancy). **Either**
  tenancy signal — a non-empty `[[auth.tenant_agents]]` or
  `require_tenant = true` — also requires a tenancy-aware storage backend
  (SQLite/Postgres). The memory backend records no tenant and reports the
  reserved `default` for every row, so every tenant-scoped read would
  return zero rows; an untenanted memory registry still starts.
- **Tenant header trust (#374)** — `auth.tenant_header_trust =
  "trusted_proxies"` with an empty `rate_limit.trusted_proxies` is refused (no
  peer could ever be trusted). `require_tenant = true` with no
  `[[auth.tenant_agents]]` and the header distrusted (`"none"`, the default) is
  refused (no request could ever resolve a tenant). A `/0` entry in
  `rate_limit.trusted_proxies` (`0.0.0.0/0`, `::/0`) is refused: it would trust
  every peer (#391). One warning, not a refusal: `any_peer` on a non-loopback
  `bind`. (0.3.x also warned when the unset key fell back to `any_peer`; since
  0.4.0 an unset key means `none` in every mode, #386.)
- **Bind safety** — a non-loopback `bind` with neither TLS nor auth requires an
  explicit `allow_public_bind = true`.
- **TLS** — when `tls.enabled`, `cert_path` and `key_path` must exist on disk.
- **DID methods** — `auth.did_methods` entries must be `did:web` or `did:key`,
  and `did:web` must be present (RFC-ACDP-0007 §3.1).
- **Playground** — `playground.enabled = true` with `playground.pinned_only =
  true` and an *empty* `[[playground.pinned_keys]]` list is refused at startup,
  **whether or not `[receipt]` is configured**. Not because it is too strict:
  `pinned_only` has no effect while `pinned_keys` is empty, so the registry
  would fall through to the fully unverified publish path, accepting unverified
  publishes from every non-`did:key` agent (`did:key` identities are
  self-verifying and never reach this gate). Add a pinned key, or set
  `playground.enabled = false`.
- **Pinned-key entries** — each `[[playground.pinned_keys]]` entry must be
  *usable*, not merely present. Five refusals, each naming the offending entry
  by index and DID:
  1. `algorithm` is not `ed25519` or `ecdsa-p256` (exact match, case-sensitive).
  2. `public_key_b64` is not valid base64.
  3. an `ed25519` key does not decode to **32** bytes.
  4. an `ecdsa-p256` key does not decode to **65** bytes, or does not begin
     `0x04` (SEC1 uncompressed).
  5. `valid_from >= valid_until` — a window that can never be open.

  These are checked **regardless of `playground.enabled`**, so a latent typo
  surfaces before the flag is flipped rather than after. Startup is the only
  place the detail is legible: at request time such an entry fails as an
  `internal_error`, whose wire message is redacted to the static string
  `"internal error"` (RFC-ACDP-0007 §5), leaving the operator a 500 with no
  indication that the cause is a config typo.

  > **Upgrade note — a config that boots today can be refused after this
  > change.** Any entry matching one of the five above is now fatal, however
  > harmless it looked before. Three shapes account for essentially all of it,
  > in rising order of how quiet they were:
  >
  > - **A rotated-out entry left in place with broken key material** — expired
  >   *and* malformed. Completely inert before: `pinned_for_at` filters on the
  >   validity window, so nothing ever decoded it. Nothing was wrong at
  >   runtime, and now the registry will not start.
  > - **Any broken entry at all while `playground.enabled = false`.** These
  >   rules do not consult `enabled`, so a section you consider switched off
  >   is still validated.
  > - **A currently-live entry with a typo'd `algorithm` or bad key
  >   material.** This one *was* already failing — but only for that single
  >   agent's publishes, as an opaque 500, which is why it can sit unnoticed
  >   for a long time. It is now refused up front for everyone.
  >
  > Delete rotated-out entries rather than leaving them broken. If a registry
  > that has been running for months suddenly will not start after an upgrade,
  > this is the first thing to check.

  **Not a refusal — a startup warning:** a list where *no* entry is currently
  within its validity window (all expired, or all future-dated). This is not
  fatal because expiry is time-dependent — refusing would make bootability a
  function of the wall clock rather than of the config file — and because in
  lax mode the state is identical to having no pins at all, which is supported.
  The warning branches, because the two modes do opposite things:
  with `pinned_only = true` every `did:web` publish is **rejected** until a key
  is rotated in; with `pinned_only = false` publishes from those agents are
  **accepted with no signature check**, i.e. pinning is silently inert. The
  second is the one worth an alert.

  The same rules gate `POST /admin/pinned-keys/reload`, so a reload cannot
  install a config startup would refuse — see
  [HTTP-API.md](HTTP-API.md#post-adminpinned-keysreload-playground-feature).
  Both doors call one function, `validate_playground_config` in
  `crates/acdp-registry-core/src/playground.rs`, rather than each keeping its
  own copy of the rules.
- **Receipts** — a configured `[receipt]` key must parse (exactly one source,
  valid base64, 32 bytes), and is incompatible with `playground.enabled = true`
  **unless** `playground.pinned_only = true` with at least one
  `[[playground.pinned_keys]]` entry (RFC-ACDP-0010 §7: a receipts registry has
  no unverified publish path).
- **Profile allowlist (REG-5)** — every entry in `registry.profiles` must be
  one of the seven *registry* profiles the pinned ACDP spec defines
  (`REGISTRY_ADVERTISABLE_PROFILES` in `crates/acdp-registry-types/src/
  config.rs`: `acdp-registry-core`, `acdp-registry-discovery`,
  `acdp-registry-federated`, `acdp-registry-receipts`,
  `acdp-registry-head-receipts`, `acdp-registry-transparency-log`,
  `acdp-registry-lifecycle`). This is checked before any other
  `registry.profiles`-dependent guard below, so a typo or an out-of-scope
  profile is reported first. `acdp-log-witness` is explicitly rejected with
  a dedicated message: a witness is not a registry (RFC-ACDP-0015 §6.1) — a
  registry MAY aggregate cosignatures under `acdp-registry-transparency-log`
  without ever advertising `acdp-log-witness` itself.
  A non-Rust consumer (a fixture, a console, a validator) can read the same
  set as data from [`docs/advertisable-profiles.json`](advertisable-profiles.json)
  — vendor it at an `acdp-registry-types/vX.Y.Z` release tag
  (e.g. `https://raw.githubusercontent.com/agentcontextdistributionprotocol/acdp-registry-rs/acdp-registry-types/v0.2.0/docs/advertisable-profiles.json`)
  and diff it on a version bump. A unit test
  (`advertisable_profiles_json_matches_const`) keeps it identical to the const,
  so a profile removed or renamed here turns this repo's CI red until the file
  is updated.
- **0.3.0 profiles** — `receipt.head_receipts = true` requires a configured
  `[receipt]` signing key (RFC-ACDP-0011 §9: head receipts are signed with
  the receipt key). `log.enabled = true` likewise requires a `[receipt]` key
  (RFC-ACDP-0012 §11: leaves bind receipt hashes and checkpoints sign with
  the receipt key), a durable storage backend (SQLite/Postgres — the memory
  backend cannot honor the append-only history commitment), and a
  well-formed `log.instance` (`[a-z0-9-]{1,32}`). Listing
  `acdp-registry-head-receipts`, `acdp-registry-lifecycle`, or
  `acdp-registry-transparency-log` in `registry.profiles` without enabling
  the matching feature is refused as a false capability advertisement.
- **Witnesses (0.4.0)** — `[[witnesses]]` requires `log.enabled = true`
  (RFC-ACDP-0015 §6.1: there are no checkpoints to witness without a log);
  each `did` must be a `did:web` DID and each `url` must pass the SSRF
  policy (HTTPS, non-private host).
- **Rate limiting (FEAT-06)** — every `rate_limit.trusted_proxies` entry must
  be a valid CIDR (or bare IP), so a typo fails the boot rather than silently
  disabling XFF trust, and no entry may have a `/0` prefix (#391: it would
  trust XFF, and under `trusted_proxies` tenant trust, from every peer). A
  non-empty `trusted_proxies` with `rate_limit.enabled
  = false` is refused (the list would be parsed for nothing) — unless
  `auth.tenant_header_trust = "trusted_proxies"`, the list's second consumer.
- **Metrics (FEAT-10)** — when `metrics.enabled`, `duration_buckets` must be
  non-empty, positive, finite, and strictly increasing (Prometheus histogram
  bounds).

## Reference

Defaults below are the struct defaults; the example TOML may set different
values for illustration. Env var = `ACDP_REGISTRY_` + the bracketed path.

### `[registry]`

| Key | Type | Default | Notes |
|-----|------|---------|-------|
| `authority` | string | — | Bare lowercase DNS name. Mints `ctx_id` and is the `did:web` registry id. |
| `port` | u16 | `8443` | Listen port. |
| `bind` | string | `127.0.0.1` | Bind address. Non-loopback needs TLS/auth or `allow_public_bind`. |
| `allow_public_bind` | bool | `false` | Opt-in to bind a public interface without TLS/auth. |
| `base_url` | string | `https://{authority}` | Public URL advertised to consumers / federation control plane. |
| `profiles` | string[] | `["acdp-registry-core","acdp-registry-discovery"]` | Advertised in capabilities. Must be a subset of the seven `acdp-registry-*` profiles the pinned spec defines — any other value (including `acdp-log-witness`, which is not a registry profile) fails startup. |
| `cross_registry_resolution` | bool | `true` | Forward foreign `ctx_id`s to their home registry; `false` returns 404 instead. |

#### `[registry.tls]`

| Key | Type | Default | Notes |
|-----|------|---------|-------|
| `enabled` | bool | `false` | Serve HTTPS directly via rustls. Usually terminate TLS upstream instead. |
| `cert_path` | path | — | Required when enabled. |
| `key_path` | path | — | Required when enabled. |

#### `[registry.cors]`

| Key | Type | Default | Notes |
|-----|------|---------|-------|
| `allowed_origins` | string[] | `[]` | Empty disables CORS (no headers sent). List your UI origin(s) to opt in. |

### `[storage]`

| Key | Type | Default | Notes |
|-----|------|---------|-------|
| `backend` | enum | `sqlite` | `postgres` \| `sqlite` \| `memory`. Must match the compiled storage feature. |
| `postgres_url` | string | — | Required when `backend = "postgres"`. |
| `sqlite_path` | path | `./data/registry.db` | SQLite file. |
| `max_connections` | u32 | `20` | sqlx pool size. |

> The storage backend is also chosen at **compile time** via the
> `acdp-registry-server` Cargo features (`storage-sqlite` default, `storage-pg`,
> `storage-memory`). The `backend` config key must agree with the built binary.
> The memory backend is demo/ephemeral only (nothing survives a restart), and
> its `q=` search is a case-insensitive substring match of the whole query, not
> the stemmed all-words match of SQLite/Postgres.

### `[auth]`

| Key | Type | Default | Notes |
|-----|------|---------|-------|
| `enabled` | bool | `false` | Mounts `/auth/*` and turns on the bearer/visibility gates. |
| `did_methods` | string[] | `["did:web"]` | Allowed DID methods; advertised in capabilities. |
| `jwt_signing_alg` | string | `HS256` | `HS256` or `EdDSA`. See [AUTHENTICATION.md](AUTHENTICATION.md#signing-algorithms). |
| `jwt_secret` | string | `""` | HS256 secret — base64, ≥32 bytes. Never published. |
| `allow_ephemeral_secret` | bool | `false` | HS256 dev escape hatch: random process-lifetime key when `jwt_secret` is empty. |
| `jwt_private_key_pem` | string | `""` | EdDSA Ed25519 private key (PKCS#8 PEM). Required for EdDSA. |
| `jwt_kid` | string | `""` | Optional explicit JWKS key id; default is the public-key fingerprint. |
| `token_ttl_seconds` | u64 | `3600` | JWT lifetime. |
| `challenge_ttl_seconds` | u64 | `300` | Challenge nonce validity. |
| `token_leeway_seconds` | u64 | `30` | Clock-skew tolerance for `exp`. |
| `anonymous_public_reads` | bool | `false` | Allow unauthenticated reads of `public` contexts. Opt in for discovery hubs. |
| `require_tenant` | bool | `false` | Strict multi-tenancy: requests resolving to no tenant are denied. See [MULTI-TENANCY.md](MULTI-TENANCY.md). |
| `tenant_header_trust` | `"none"` \| `"trusted_proxies"` \| `"any_peer"` | unset: `"none"` in every mode (0.4.0, #386; 0.3.x used `"any_peer"` when `require_tenant = false`) | Who may select a tenant with `X-Tenant-Id` when no signed claim or agent binding applies. `trusted_proxies`: only when the immediate TCP peer (not `X-Forwarded-For`) is in `rate_limit.trusted_proxies`. An untrusted header that disagrees with the claim/binding, or has none to agree with, is 403 `not_authorized`. See [MULTI-TENANCY.md](MULTI-TENANCY.md#who-may-send-x-tenant-id). |
| `admin_tokens` | string[] | `[]` | Bearer tokens for `/admin/*`. Entries must be non-empty and not whitespace-only (startup validation). An empty *list* disables every admin-bearer-gated route: `/admin/status`, `/admin/lineages/{id}/audit`, `/admin/contexts/{id}/retract`, `/admin/contexts/{id}/republish`, `GET /admin/contexts`, `/admin/pinned-keys/reload`. See [HTTP-API.md#admin](HTTP-API.md#admin). |

#### `[[auth.tenant_agents]]`

Repeatable. Binds a producing/consuming agent to a tenant.

| Key | Type | Notes |
|-----|------|-------|
| `agent_did` | string | Full DID of the agent. |
| `tenant_id` | string | Tenant the agent is scoped to (`default` is reserved). |

#### `[[auth.revocation_feeds]]`

Repeatable. A peer registry whose revocations this registry mirrors.

| Key | Type | Default | Notes |
|-----|------|---------|-------|
| `issuer` | string | — | Peer DID; each fetched entry's `iss` must match. |
| `feed_url` | string | — | Peer's `/auth/revocations` URL. |
| `admin_token` | string | — | Bearer for the peer's feed. |
| `poll_seconds` | u64 | `300` | Poll interval. |

### `[webhook]`

| Key | Type | Default | Notes |
|-----|------|---------|-------|
| `enabled` | bool | `false` | |
| `url` | string | `""` | HMAC-signed POST target. SSRF-policy gated. |
| `secret` | string | `""` | HMAC-SHA256 key; must be non-empty when enabled. |
| `timeout_seconds` | u64 | `5` | Per-delivery timeout. |
| `max_retries` | u32 | `3` | Exponential backoff (250 ms → cap 15 s). |
| `queue_capacity` | usize | `1024` | Bounded in-memory queue; events drop (with a warn) when full. |

See [WEBHOOKS.md](WEBHOOKS.md).

### `[limits]`

| Key | Type | Default | Notes |
|-----|------|---------|-------|
| `max_payload_bytes` | u64 | `1048576` | Publish body cap (enforced by the body-limit layer on every route). |
| `max_embedded_bytes` | u64 | `65536` | Cap on an inline `data_ref` value. |
| `idempotency_key_ttl_seconds` | u64 | `86400` | Idempotency-Key replay window; advertised in capabilities. |
| `publish_rate_per_minute` | u32 | `60` | Per-agent `POST /contexts` cap; `0` disables. In-memory, per-process. |
| `challenge_rate_per_minute` | u32 | `60` | Per-agent `POST /auth/challenge` cap; `0` disables. In-memory, per-process. |

> These are per-process in-memory fixed-window counters — see
> [OPERATIONS.md · Rate limiting](OPERATIONS.md#rate-limiting) for the
> multi-replica caveat.

### `[rate_limit]` *(FEAT-06)*

Per-IP and process-global rate limiting on the `/auth/*` endpoints (token
issuance / refresh / revoke), applied as middleware over the whole `/auth/*`
subrouter — on top of the per-agent `[limits]` budgets. The `[limits]` buckets
key on the caller-supplied `agent_id`, which an unauthenticated attacker
controls and can rotate to defeat the per-key limit; these two bounds are
attacker-independent.

| Key | Type | Default | Notes |
|-----|------|---------|-------|
| `enabled` | bool | `true` | Master switch. On by default — `/auth/*` is the most attacker-controllable surface. |
| `per_ip_per_minute` | u32 | `60` | Per-resolved-client-IP cap on `/auth/*`; `0` disables the per-IP bound. |
| `global_per_minute` | u32 | `6000` | Whole-process ceiling across all IPs; `0` disables it. Bounds a source-IP-rotating flood. |
| `trusted_proxies` | list\<CIDR\> | `[]` | Reverse-proxy CIDRs whose `X-Forwarded-For` is trusted. Empty = never trust XFF. Also the gateway list for `auth.tenant_header_trust = "trusted_proxies"`, matched against the immediate TCP peer; allowed with `enabled = false` in that mode. Entries are matched as written against the IPv4-canonicalised peer: list IPv4 gateways in IPv4 form (an `::ffff:…` entry never matches). A `/0` entry is refused at startup. |
| `backend` | `"memory"` \| `"postgres"` | `"memory"` | Where the two ceilings above are counted. `memory`: per process (each replica counts for itself). `postgres`: counted once across every replica sharing the database — see **Shared backend** below. |
| `backend_unavailable` | `"allow"` \| `"deny"` | `"allow"` | Only with `backend = "postgres"`. What to do when the database cannot answer a check: `allow` falls back to the per-process limit; `deny` refuses with `429` + `Retry-After: 5`. |
| `backend_timeout_ms` | u64 | `250` | Only with `backend = "postgres"`. Per-check timeout against the database; must be `> 0`. |
| `backend_prune_seconds` | u64 | `300` | Only with `backend = "postgres"`. How often stale counter rows are deleted; must be `> 0`. |

**Shared backend (multi-replica).** With `backend = "postgres"`, the per-IP and
global ceilings are counted in the registry's own Postgres database (table
`rate_limit_windows`, an `UNLOGGED` table) so they hold across replicas instead
of per replica. The in-memory limiter stays in front as a pre-filter, with the
same limits: a request it rejects never touches the database, so database writes
are bounded by the in-memory limits (at most `2 × global_per_minute` statements per
limiter window per replica) however large a flood is, and enabling the shared backend can
only tighten enforcement relative to per-process limiting. Startup refuses the
combination unless `storage.backend = "postgres"`, `enabled = true` and
`global_per_minute > 0` (the global ceiling is what bounds database load when an
attacker rotates source IPs), and refuses a zero `backend_timeout_ms` or
`backend_prune_seconds`. If the database cannot answer, `backend_unavailable`
decides: the default `allow` degrades to exactly today's per-process limiting
rather than turning a database blip into a token-issuance outage; a stalled check
adds up to `backend_timeout_ms` to a request (two checks per `/auth/*` request).
The counters are ephemeral: after a database crash or standby promotion one
window's budget resets once. Nothing here is needed on a single replica.

**Client-IP resolution & the trusted-proxy decision (security).** The client
IP defaults to the TCP socket peer. `X-Forwarded-For` is caller-supplied and
is **never** trusted unless the socket peer is itself in one of the
`trusted_proxies` ranges — otherwise any client could spoof its source IP to
evade the per-IP budget or frame another address. When the peer *is* a trusted
proxy, the real client is taken from the rightmost `X-Forwarded-For` entry that
is not itself a trusted proxy (walking a chain of trusted hops from the right).
List **only** proxies you operate; a wrong entry is a spoofing hole. CIDRs are
validated at startup — a malformed entry fails the boot rather than silently
disabling XFF trust.

> Which ceilings are per replica depends on `backend`. With the default
> `backend = "memory"` this is the same per-process, in-memory caveat as
> `[limits]`: behind a load balancer with `trusted_proxies` set, each replica
> limits per real client IP, and both `per_ip_per_minute` and
> `global_per_minute` are per replica. With `backend = "postgres"` both are
> counted once across every replica sharing the database (see **Shared
> backend** above) — except while the database cannot answer and
> `backend_unavailable = "allow"`, when they fall back to per-replica counting.
> The `[limits]` per-agent budgets stay per-process under either backend.
> Requests are admitted or rejected *before* any DID resolution, so the
> SSRF/DNS path never runs for a throttled request.

### `[metrics]` *(FEAT-10)*

Prometheus `/metrics` endpoint (text exposition, `version=0.0.4`). Off by
default. When enabled, `GET /metrics` is mounted **outside** the ACDP auth
pipeline and the rate limiter so a scraper reaches it unimpeded, and a
process-global recorder captures HTTP request metrics plus domain counters.

| Key | Type | Default | Notes |
|-----|------|---------|-------|
| `enabled` | bool | `false` | Mount `/metrics` and start recording. |
| `bearer_token` | string | `""` | When set, `/metrics` requires `Authorization: Bearer <token>` and answers `401` otherwise. Empty = open, the default. A **whitespace-only** value is refused at startup when `enabled = true` ([#162](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/issues/162)): it would trim to empty and silently leave the endpoint open. Surrounding whitespace on an otherwise real token is accepted — both the configured and the presented token are trimmed before comparison. |
| `duration_buckets` | list\<f64\> | web-latency ladder | Buckets (seconds) for the request-latency histogram; must be positive and strictly increasing. |

See [HTTP-API.md · `GET /metrics`](HTTP-API.md#get-metrics-feat-10) for the
exposed metric names.

### `[playground]`

**Never enable in production** — setting `enabled = true` skips DID-signature
verification on publishes from non-`did:key` agents, regardless of which Cargo
features the binary was built with. (`did:key` publishes are verified before
the playground branch runs and are unaffected — see below — and a pinned
agent under `[[playground.pinned_keys]]` is still cryptographically verified
even with `enabled = true`; the bypass applies to unpinned, non-`did:key`
publishes.)

Only the two admin routes this feature unlocks (`GET /admin/contexts`,
`POST /admin/pinned-keys/reload`) are compiled in with the `playground` Cargo
feature — see [OPERATIONS.md](OPERATIONS.md#admin-endpoints) and
[HTTP-API.md](HTTP-API.md#endpoint-summary). `[playground] enabled = true`
itself is **not** feature-gated: the publish handler's DID-signature bypass
(`crates/acdp-registry-core/src/handlers/context.rs`, the
`playground_snapshot.enabled` branch) is a plain runtime `if`, compiled into
every build including a stock release binary with default features.
`did:key` producers are checked before this branch, unconditionally, through
acdp's offline verifier (the `starts_with("did:key:")` branch of
`publish_inner` in `context.rs`; rationale in the comment that opens that
branch) — a
`did:key` identity is self-verifying by construction, so `[playground]` never
affects how a `did:key` publish is authorized. Pinned agents
(`[[playground.pinned_keys]]`) are cryptographically verified inside the
playground branch itself (`crate::playground::enforce_pinned_signature`,
called at the top of that branch); the skip applies only to
publishes from non-`did:key` agents that aren't pinned.

| Key | Type | Default | Notes |
|-----|------|---------|-------|
| `enabled` | bool | `false` | Skip DID-signature verification for hands-on demos. |
| `pinned_only` | bool | `false` | Reject publishes from agents without a pinned key. **No effect while `pinned_keys` is empty.** Also the only playground mode compatible with a configured `[receipt]` key — and it then requires at least one pinned key. |

#### `[[playground.pinned_keys]]`

Repeatable.

| Key | Type | Default | Notes |
|-----|------|---------|-------|
| `agent_did` | string | — | Full DID. |
| `public_key_b64` | string | — | Standard base64 of the raw 32-byte key. |
| `algorithm` | string | `ed25519` | `ed25519` or `ecdsa-p256`. |
| `valid_from` | i64 | — | Unix seconds, inclusive; open-ended if omitted. |
| `valid_until` | i64 | — | Unix seconds, exclusive; open-ended if omitted. |

Hot-reload the `[playground]` section with `POST /admin/pinned-keys/reload`
(playground feature) — see [HTTP-API.md](HTTP-API.md#post-adminpinned-keysreload-playground-feature).

### `[receipt]` *(ACDP 0.2.0)*

Registry-receipt signing identity (RFC-ACDP-0010). Configuring a key enables
receipt minting, the `acdp-registry-receipts` profile, and
`GET /.well-known/did.json`. Leave unset to stay a receipt-less registry.
(`acdp_version` itself is unconditionally `"0.5.0"` as of REG-3 —
RFC-ACDP-0016 §10's anchors claim always wins the version max() — so it no
longer moves with `[receipt]`; the `acdp-registry-receipts` profile is what
actually signals receipt minting is active.) See [RECEIPTS.md](RECEIPTS.md)
for the operator runbook (rotation, retention, backfill policy).

| Key | Type | Default | Notes |
|-----|------|---------|-------|
| `signing_key_seed_b64` | string | `""` | Standard base64 of the raw 32-byte Ed25519 seed. Exactly one of the two key sources may be set. |
| `signing_key_path` | path | — | File (e.g. mounted secret) whose contents are that base64 string. |
| `key_id_fragment` | string | `receipt-key-1` | Fragment under the registry DID; `signature.key_id = did:web:<authority>#<fragment>`. Pick a fresh fragment per rotation. |
| `head_receipts` | bool | `false` | *(ACDP 0.3.0)* Mint a lineage-head receipt on every `GET /lineages/{id}/current` response (RFC-ACDP-0011). Requires a configured signing key (the same receipt key signs — no new key role); advertises `acdp-registry-head-receipts` (`acdp_version` itself is unconditionally `"0.5.0"` as of REG-3 and no longer moves with this flag). |

#### `[[receipt.retired_keys]]`

Repeatable. Rotated-out receipt keys, published in the DID document's
`verificationMethod` only (never `assertionMethod`). **Removing an entry
bricks every receipt that key signed** — RFC-ACDP-0010 §9 retains retired
keys indefinitely; remove only on confirmed compromise.

| Key | Type | Default | Notes |
|-----|------|---------|-------|
| `public_key_b64` | string | — | Standard base64 of the raw 32-byte Ed25519 **public** key. |
| `key_id_fragment` | string | — | The fragment the key was published under while active. |

### `[lifecycle]` *(ACDP 0.3.0)*

Lifecycle events & retraction (RFC-ACDP-0013). When enabled the registry
serves `POST /contexts/{ctx_id}/retract` / `/republish`, derives `status`
with the `retracted > superseded > expired > active` precedence, excludes
retracted contexts from default search and from `/current`, serves
`registry_state.lifecycle_events`, and advertises `acdp-registry-lifecycle`
(`acdp_version` itself is unconditionally `"0.5.0"` as of REG-3 —
RFC-ACDP-0016 §10's anchors claim always wins the version max() — and no
longer moves with this flag). When disabled (the default) both endpoints
answer `501 not_implemented` and the registry emits neither
`lifecycle_events` nor the `retracted` status — guaranteed by the startup
refusal below, not by the read paths.

**Turning the flag off over existing lifecycle state refuses to start.**
[RFC-ACDP-0013 §6](https://github.com/agentcontextdistributionprotocol/agentcontextdistributionprotocol/blob/34f14ab2ab454308e94fd6f137ef940db45c72c8/rfcs/RFC-ACDP-0013-lifecycle-events.md#6-retraction--republication-endpoints)
(pinned spec): a registry that does not advertise `acdp-registry-lifecycle`
MUST NOT emit `lifecycle_events` or the `retracted` status, with no carve-out
for state recorded while the profile *was* advertised. The stores project
stored lifecycle state on every read path regardless of the flag, so at
startup — after migrations, before binding — the binary asks the store whether
it holds any lifecycle state (`ExtendedRegistryStore::has_lifecycle_state`:
at least one `lifecycle_events` row, **or** at least one context with the
`retracted` flag set). With `enabled = false` and state present it logs at
`ERROR` and exits non-zero naming both ways out:

1. **Re-enable** — set `[lifecycle] enabled = true`
   (`ACDP_REGISTRY_LIFECYCLE__ENABLED=true`). The state is served exactly as
   before. This is the recommended exit.
2. **Purge** — back up the database first. Deleting the state is
   **irreversible**: every retracted context becomes `active` again and
   reappears in default search and as its lineage's `/current` head, reversing
   withdrawals the producers (or you) made. With the registry stopped:

   ```sql
   -- SQLite
   BEGIN;
   DELETE FROM lifecycle_events;
   UPDATE contexts SET retracted = 0 WHERE retracted <> 0;
   COMMIT;

   -- Postgres
   BEGIN;
   DELETE FROM lifecycle_events;
   UPDATE contexts SET retracted = FALSE WHERE retracted;
   COMMIT;
   ```

A republished context counts as state: it is no longer retracted but keeps
both events, which §6 equally forbids emitting. The check runs once at
startup; on a multi-replica Postgres deployment every replica must agree on
`[lifecycle]` (a flag-off replica already running keeps serving whatever a
flag-on replica records until its next restart, which then refuses). The
in-memory backend never trips it — nothing survives its restart. See
[UPGRADING.md](UPGRADING.md) for the pre-upgrade check.

| Key | Type | Default | Notes |
|-----|------|---------|-------|
| `enabled` | bool | `false` | Opt into the RFC-ACDP-0013 endpoint surface and status semantics. |

`enabled` also unlocks the **registry-attested** takedown path
(`POST /admin/contexts/{ctx_id}/{retract,republish}`, RFC-ACDP-0013 §6
registry-initiated events — see [HTTP-API.md](HTTP-API.md#post-admincontextsctx_idretract-post-admincontextsctx_idrepublish-acdp-030)).
That path adds **no new knob**: it is gated by the existing `auth.admin_tokens`
bearer, and its signing is driven by whether a `[receipt]` key is configured —
with a receipt key the registry MUST sign the event under it; without one the
event is recorded unsigned but still attributed to the registry DID.

### `[log]` *(ACDP 0.3.0)*

Registry transparency log
([RFC-ACDP-0012](https://github.com/agentcontextdistributionprotocol/agentcontextdistributionprotocol/blob/34f14ab2ab454308e94fd6f137ef940db45c72c8/rfcs/RFC-ACDP-0012-transparency-log.md)): the leaf
encoding, Merkle tree construction and checkpoint format are normative in
[§4–§6](https://github.com/agentcontextdistributionprotocol/agentcontextdistributionprotocol/blob/34f14ab2ab454308e94fd6f137ef940db45c72c8/rfcs/RFC-ACDP-0012-transparency-log.md#4-leaf-object-and-leaf-encoding-normative)
and not restated here. When enabled this registry appends one leaf per
accepted publish **in the same storage transaction as the context row and its
receipt** (a publish that cannot durably append its leaf fails), serves
`GET /log/checkpoint`, `GET /log/proof`, and `GET /log/entries`, signs
checkpoints with the `[receipt]` key (no new key to operate), and advertises
`acdp-registry-transparency-log` (`acdp_version` itself is unconditionally
`"0.5.0"` as of REG-3 and no longer moves with this flag). When disabled (the
default) the three `/log/*` endpoints answer `501 not_implemented`. There is
no degraded mode and no `log_unavailable` error.

Prerequisites (enforced at startup): a configured `[receipt]` signing key
and a durable storage backend (`sqlite` or `postgres`).

Storage: leaves live in the `log_leaves` table — dense 0-based
`leaf_index` in acceptance order, one leaf per `ctx_id`, and the **exact
JCS-canonical leaf bytes** plus their `sha256:` leaf hash, so every leaf
is byte-exactly reproducible forever. Roots and proofs are recomputed per
request from the ordered leaf hashes (O(n); the head root is cached).
Contexts published *before* enablement are not backfilled: no leaf is ever
appended for them, and there is no backfill command.

**Deviation from a SHOULD in [RFC-ACDP-0012 §7.3](https://github.com/agentcontextdistributionprotocol/agentcontextdistributionprotocol/blob/34f14ab2ab454308e94fd6f137ef940db45c72c8/rfcs/RFC-ACDP-0012-transparency-log.md#73-backfill)
(pinned spec):** a registry enabling the profile SHOULD backfill leaves for
contexts published before enablement (after backfilling their receipts, which
[RFC-ACDP-0010 §7](https://github.com/agentcontextdistributionprotocol/agentcontextdistributionprotocol/blob/34f14ab2ab454308e94fd6f137ef940db45c72c8/rfcs/RFC-ACDP-0010-registry-receipts.md#7-issuance)
likewise says SHOULD happen and this registry also does not do — see
[RECEIPTS.md](RECEIPTS.md#backfill-policy-none)). Backfilled leaves would be
time-unanchored under §7.3 anyway, so what is missing is their inclusion in the
log, not a timing guarantee.

| Key | Type | Default | Notes |
|-----|------|---------|-------|
| `enabled` | bool | `false` | Opt into the RFC-ACDP-0012 log: atomic leaf appends + the three `/log/*` endpoints. |
| `instance` | string | `"1"` | The `<instance>` component of `log_id = did:web:<authority>/log/<instance>` (matches `[a-z0-9-]{1,32}`). **Change only on catastrophic tree loss** (§7.4) — a new instance is an explicit, loudly detectable history reset. |

### `[[witnesses]]` *(ACDP 0.4.0)*

Transparency-log **witness cosignature aggregation** (RFC-ACDP-0015 §6.1).
An array of independent witnesses this registry polls and whose **verified**
cosignatures it attaches to its checkpoint responses (the reserved
top-level `witness_signatures` member — see
[HTTP-API.md](HTTP-API.md#get-logcheckpoint-get-logproof-get-logentries-acdp-030)).
A consumer then gets a checkpoint **and** its witness quorum in one fetch
and verifies *N-witnessed* locally.

For each configured witness a background poller GETs
`<url>?log_id=<this registry's log_id>` over the SSRF-guarded outbound
client (HTTPS-only, DNS-rebinding-guarded, no redirects — RFC-ACDP-0008
§4.8) and runs the
[RFC-ACDP-0015 §8](https://github.com/agentcontextdistributionprotocol/agentcontextdistributionprotocol/blob/34f14ab2ab454308e94fd6f137ef940db45c72c8/rfcs/RFC-ACDP-0015-witness-cosigning.md#8-consumer-verification-procedure-normative)
verification procedure on every returned cosignature **against this
registry's own checkpoint** at that `tree_size`, resolving the witness key
through the `did:web` resolver. A witness cosigning a *different* root (a
fork, or a lie) is logged and **dropped** — it is never stored and never
served. Only verified cosignatures are persisted (table
`log_witness_cosignatures`, keyed by
`(log_id, tree_size, root_hash, witness_did)`; a fresh re-observation
upserts, newest wins), so serving is a single indexed read with no blocking
network call in the request path.

Aggregation is a pure convenience: the registry never holds a witness key,
so it can neither forge a cosignature nor make itself a trust dependency —
a consumer MAY always fetch direct from a witness (§6.2). There is **no new
capability flag or profile**: a registry that aggregates does so under its
existing `acdp-registry-transparency-log` profile (§10). `acdp_version`
served at `.well-known/acdp.json` is unconditionally `"0.5.0"` as of REG-3
(RFC-ACDP-0016 §10's anchors claim always wins the version max()), so
configuring `[[witnesses]]` no longer moves it; whether aggregation is
active is visible in the response body (whether `witness_signatures`
accompanies a checkpoint) — see
[HTTP-API.md](HTTP-API.md#get-well-knownacdpjson).

Prerequisites (enforced at startup): `log.enabled = true` (there are no
checkpoints to witness without a log), each `did` a `did:web` DID, and each
`url` accepted by the SSRF policy (HTTPS, non-private host). Empty (the
default) disables aggregation entirely.

```toml
[[witnesses]]
did          = "did:web:witness.example.org"
url          = "https://witness.example.org/log/witness"
poll_seconds = 60                              # default 300

[[witnesses]]
did = "did:web:witness-2.example.org"
url = "https://witness-2.example.org/log/witness"
```

| Key | Type | Default | Notes |
|-----|------|---------|-------|
| `did` | string | *(required)* | The witness's `did:web` DID. Cosignatures whose `witness_id` ≠ this value are ignored (a witness endpoint only speaks for its own DID). |
| `url` | string | *(required)* | HTTPS URL of the witness's `GET /log/witness` endpoint (RFC-ACDP-0015 §6.2). SSRF-checked at startup and at DNS time on every poll. |
| `poll_seconds` | integer | `300` | Poll interval. |
