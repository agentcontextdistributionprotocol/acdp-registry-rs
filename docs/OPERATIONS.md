# Operations Guide

Running `acdp-registry` in production. See [CONFIGURATION.md](CONFIGURATION.md)
for the full config reference and [AUTHENTICATION.md](AUTHENTICATION.md) for the
auth/federation model.

## Deploying with Docker

```bash
cd docker
docker compose up -d --build
```

The compose file boots Postgres + the registry server. Configuration is read
from `docker/config.docker.toml` (mounted read-only); the Postgres URL comes
from `ACDP_REGISTRY_STORAGE__POSTGRES_URL`. The image is built with the
`storage-pg` feature (`STORAGE_FEATURE` build arg), and `acdp` is pulled from
crates.io, so the build context is just this repo.

**Secrets** are sourced from the environment or a sibling `.env` file via
`${VAR:-default}` substitution. **The compose file ships no default for the JWT
secret**: `ACDP_REGISTRY_AUTH__JWT_SECRET` resolves to empty unless you set
`ACDP_REGISTRY_JWT_SECRET`, and that is what lets the quickstart boot as
documented. It previously defaulted to the literal `changeme`, which did not
boot at all — the stack this section described was never startable.

The startup validator rejects that literal (case-insensitively, after trimming)
**whenever `jwt_secret` is non-empty and `jwt_signing_alg` is not `EdDSA`** —
including with `auth.enabled = false`, which is the shipped compose posture. The
refusal happens before `store.migrate()`, so a rejected config creates no
database files. The two checks are gated differently, and the asymmetry is
deliberate rather than an oversight:

| check | gated on `auth.enabled`? |
|---|---|
| a **non-empty** `jwt_secret` that is `changeme`, or is not base64 of ≥32 bytes | **no** — always runs, for any non-`EdDSA` algorithm |
| an **empty** `jwt_secret` (refused unless `auth.allow_ephemeral_secret`) | **yes** — empty + auth off is a supported configuration, and is how the quickstart runs |

Under `EdDSA` the secret is not examined at all; the key comes from
`jwt_private_key_pem`. Set a real secret before enabling auth or promoting
beyond a disposable demo:

```bash
echo "ACDP_REGISTRY_JWT_SECRET=$(openssl rand -base64 32)" >> docker/.env
```

For multi-host deployments, put `acdp-registry` behind a TLS-terminating proxy
(Caddy, Nginx, ALB). See [RAILWAY.md](../docker/RAILWAY.md) for a Railway recipe.

## TLS

The `[registry.tls]` block can serve HTTPS directly via rustls, but the
recommended topology is to **terminate TLS upstream** and let the registry serve
plain HTTP behind it — cert rotation without restarts, standard ops. The example
`port = 8443` is a hint that HTTPS is expected at the edge; the binary serves
plain HTTP on any port.

Note the asymmetry: cross-registry resolution and webhook delivery are
**outbound** and require HTTPS (the `acdp` `SsrfPolicy` refuses HTTP and
private/internal authorities). That's independent of how you serve inbound
traffic.

### Binding a public interface

A non-loopback `bind` (e.g. `0.0.0.0`) with neither TLS nor auth enabled fails
startup unless you set `registry.allow_public_bind = true`. This is a guardrail
against accidentally exposing an unauthenticated registry — prefer enabling auth
or fronting with a proxy over flipping the flag.

## Configuration precedence

```
defaults  <  TOML file  <  ACDP_REGISTRY_* env vars
```

`ACDP_REGISTRY_<SECTION>__<FIELD>` uses a single underscore after the prefix and
double underscores between nesting levels:

```bash
export ACDP_REGISTRY_STORAGE__POSTGRES_URL="postgres://acdp:acdp@db:5432/acdp"
export ACDP_REGISTRY_AUTH__JWT_SECRET="$(openssl rand -base64 32)"
export ACDP_REGISTRY_WEBHOOK__URL="https://example.com/hooks/acdp"
export ACDP_REGISTRY_WEBHOOK__SECRET="$(openssl rand -base64 32)"
export ACDP_REGISTRY_WEBHOOK__ENABLED="true"
```

## Migrations

Migrations run automatically at startup and are idempotent — restarting an
already-migrated database is a no-op. To add one, drop a new sequential SQL file
into `crates/acdp-registry-<backend>/migrations/` and let CI cover the upgrade
path. Never edit an applied migration.

## Admin endpoints

All six are bearer-gated against `auth.admin_tokens` (constant-time compare;
an empty list disables every route that checks it). See
[HTTP-API.md](HTTP-API.md#admin) for the full picture. Generate admin tokens
out of band and distribute them to operators / monitoring.

- `GET /admin/status` — always shipped, admin-bearer gated. An operational
  snapshot: build identity, storage health, idempotency record count, webhook
  queue depth, configured revocation feeds, and migration state. Good for a
  readiness probe richer than `/healthz`. Shape in
  [HTTP-API.md](HTTP-API.md#get-adminstatus).

  The `build` group answers "exactly which binary is running": `version` (the
  same string `/healthz` serves unauthenticated), `commit`, and
  `storage_impl`. **`commit` is omitted when the binary was not built by
  `docker.yml`** — that absence is the signal that the build is not uniquely
  identified, because `version` then degrades to the bare package version that
  every locally built binary shares. When diagnosing "which build is in this
  pod", an absent `commit` means you cannot answer that from the service and
  must fall back to the image digest. `storage_impl` is an opaque diagnostic
  string (a Rust type path) — display it, do not parse it.
- `GET /admin/contexts`, `POST /admin/pinned-keys/reload` — only in builds with
  the `playground` Cargo feature. Both are admin-bearer gated. `GET
  /admin/contexts` authenticates the caller but names no agent DID, and
  `admin_list` unconditionally passes `true` for the store's
  `anonymous_public_reads` parameter — the local is spelled
  `admin_sees_public_arm` where it is declared
  (`crates/acdp-registry-core/src/handlers/admin.rs:87`) and is passed as the
  fifth positional argument (`admin.rs:101`), binding to
  `list_contexts`'s `anonymous_public_reads` parameter
  (`crates/acdp-registry-store/src/lib.rs:75`), so it
  reaches the RFC-ACDP-0008 §4.5 public arm only — restricted and private
  contexts are never disclosed to it.

```bash
curl -H "Authorization: Bearer $ADMIN_TOKEN" https://registry.example.com/admin/status
```

## JWKS and key distribution

With `auth.jwt_signing_alg = "EdDSA"`, the registry's Ed25519 public key is
published at `GET /.well-known/jwks.json` so federated peers verify your tokens
without a shared secret. With the default `HS256`, that endpoint returns an
empty key set and the symmetric `jwt_secret` is never exposed. See
[AUTHENTICATION.md](AUTHENTICATION.md#signing-algorithms).

## Revocation federation

This registry is a revocation **consumer**: it polls peers' feeds and mirrors
their revocations locally. Configure peers with `[[auth.revocation_feeds]]`
(`issuer`, `feed_url`, `admin_token`, `poll_seconds`). Cursors are durable
(unix-ms) and advance only when an entire page applies cleanly, so a restart or
a mid-page failure replays rather than skips. `GET /admin/status` reports the
configured feed count. Full behavior:
[AUTHENTICATION.md](AUTHENTICATION.md#cross-issuer-revocation-federation).

## Cross-registry federation

With `registry.cross_registry_resolution = true`, a `GET /contexts/{ctx_id}` for
a foreign authority is resolved against that registry **anonymously** (no
caller-credential forwarding), so only remote `public` contexts are surfaced.
The `acdp` `SsrfPolicy` (see [acdp-rs · Security Model][acdp-security]) rejects
private/internal authorities with `502 cross_registry_resolution_failed`. Set
the key to `false` to return `404` for foreign ids instead.

[acdp-security]: https://github.com/agentcontextdistributionprotocol/acdp-rs/blob/main/docs/security.md

## Rate limiting

Per-agent, per-process, in-memory token buckets:
`limits.publish_rate_per_minute` on `POST /contexts` and
`limits.challenge_rate_per_minute` on `POST /auth/challenge` (both default 60,
`0` disables; `429` + `Retry-After` when drained).

On top of these, the FEAT-06 `[rate_limit]` section adds **per-IP** and
**process-global** buckets over the whole `/auth/*` subrouter (token issuance /
refresh / revoke) — the most attacker-controllable surface. Unlike the
per-agent buckets, these key on the resolved client IP, which an unauthenticated
attacker can't rotate as freely as `agent_id`. The client IP is the TCP socket
peer unless `rate_limit.trusted_proxies` marks the peer as a trusted reverse
proxy, in which case it comes from `X-Forwarded-For` (see
[CONFIGURATION.md · `[rate_limit]`](CONFIGURATION.md#rate_limit-feat-06) for the
trust model — XFF is never honoured from an untrusted peer). The server is bound
with `into_make_service_with_connect_info`, so the peer IP is available behind
`axum_server`.

All of these are per-process. Behind a load balancer set `trusted_proxies` so
per-IP limits track real clients; the `global_per_minute` ceiling is per replica.
For a cluster-wide bound, still front the deployment with a shared limiter (or
proxy-level limits).

## Metrics

Set `metrics.enabled = true` to mount `GET /metrics` (Prometheus text
exposition). It sits outside the auth pipeline and the rate limiter so a scraper
reaches it unimpeded; gate it with `metrics.bearer_token` (or network policy)
when the endpoint is reachable from untrusted networks. Beyond HTTP
request-rate/latency/status by route, the registry exports domain counters
(publishes by outcome, receipts minted, log leaves appended, lifecycle events,
witness cosignatures, rate-limit rejections). Full metric list:
[HTTP-API.md · `GET /metrics`](HTTP-API.md#get-metrics-feat-10).

## Webhooks

Receivers verify `X-ACDP-Signature` (GitHub-compatible HMAC-SHA256 over the raw
body). Delivery is non-blocking with exponential backoff (250 ms → cap 15 s) up
to `webhook.max_retries`; a full queue drops events with a warn. Full reference:
[WEBHOOKS.md](WEBHOOKS.md).

## Observability

The binary emits JSON-structured `tracing` logs. The default filter is
`info,acdp=info,acdp_registry=info`; override with `RUST_LOG`. Every request
carries an `x-request-id` header (UUIDv4 if absent on input) propagated
downstream. At startup it logs the authority, port, storage backend, and
playground flag; on bind it logs the listen address; on `SIGTERM`/`Ctrl-C` it
drains in-flight requests for up to 30 s before exiting.

For numeric telemetry, enable the Prometheus `/metrics` endpoint (see
[Metrics](#metrics) above); `GET /admin/status` remains the JSON snapshot for
queue/idempotency/migration state.

## Backup and restore

### Backup

Postgres: logical (`pg_dump`) or physical (`pg_basebackup`) backups as usual.
The `contexts.body_json` column is the canonical projection — `body_json` plus
`status` is enough to reconstruct every other index.

SQLite: stop the writer and copy the `.db`, `.db-wal`, and `.db-shm` files
atomically (or use the `.backup` command). Copying the `.db` alone while the
writer is running yields a torn database: recent commits live in the `-wal`.

### Restore

The registry runs `store.migrate()` at startup, so a restored database is
brought to the current schema automatically. Restore the data **before** the
first boot against it; there is no online restore path.

1. **Stop the registry.** Restoring under a live writer is what corrupts a
   SQLite `-wal` set and what makes a Postgres restore race the migration.
2. **Restore the data.** Postgres: `pg_restore` into an empty database (not a
   partially-populated one — the migrations are not written to merge two
   datasets). SQLite: put the `.db`, `.db-wal` and `.db-shm` files back
   together, as a set.
3. **Start the registry and watch the first boot.** Migrations run before the
   listener binds, so a schema failure is a startup failure, not a 500 later.
4. **Verify readiness, not liveness.** `GET /healthz` reports storage
   readiness and answers `503` if the backend is not usable; `GET /livez` says
   only that the process is up and will answer `200` against a broken database.
   Check `/healthz`.
5. **Re-check the transparency log if it is enabled** — see the runbook below.
   A restore that rolls the log back to an earlier state is exactly the
   condition a consistency proof is designed to expose.

**What a restore does not bring back.** The webhook delivery queue is in-memory
and bounded, with no outbox and no replay. Every event queued and not yet
delivered at the moment of the crash or shutdown is gone, and a restore cannot
reconstruct it — the events were never persisted. Consumers that missed a
`context.published` must re-derive state from the API rather than wait for a
redelivery that will never come.

Nonces (`/auth/challenge`) and issued-token revocation state live in their own
stores; restoring an older snapshot restores an older revocation list with it,
which can un-revoke a token that has not yet expired. Prefer rotating the
signing key over relying on a restored revocation list.

## Capacity

There is no autoscaling story here and no built-in load shedding beyond the
rate limiter. Four bounds are worth knowing before they find you:

| Bound | Where | What happens at the limit |
|-------|-------|---------------------------|
| Webhook queue | `webhook.queue_capacity` | The queue is bounded and delivery is fire-and-forget. When it is full the event is **dropped** with a `warn` log (`webhook queue full; event dropped`) — the publish itself still succeeds. There is no Prometheus gauge for depth; `GET /admin/status` reports `queue_in_flight` and `queue_capacity`, and that plus the warn log is the whole signal. |
| Transparency log proofs | `[log]` | Tree computation is **O(n) per request** over the stored leaf hashes (only the current head root is cached). `/log/proof` and `/log/checkpoint` therefore get steadily more expensive as the log grows, and they are unauthenticated for hash-only queries. Rate-limit them deliberately rather than discovering the cost. |
| Search page size | clamped in both stores | A caller-supplied `limit` is clamped to 100. A page can also come back **shorter** than requested — see [MULTI-TENANCY.md](MULTI-TENANCY.md) on the refill cap. Short is not "end of results"; page until `next_cursor` is absent. |
| Storage pool | `storage.max_connections` (default 20) | Requests queue on the sqlx pool. `/healthz` issues a storage health query of its own, so it reports `degraded` + `503` once the pool can no longer serve one — which is the intended behaviour for a load balancer, and the reason `/livez` exists separately and never touches storage. |

Sizing the webhook queue is a trade between memory and loss: it holds whole
deliveries in memory, and raising it raises the amount of work a crash discards
(none of which is replayable). If deliveries are load-bearing, the honest answer
is a consumer that reconciles against the API, not a larger queue.

## Runbook: transparency-log inconsistency

Applies when `[log]` is enabled. A **consistency proof** (`GET /log/proof?first=<a>&second=<b>`)
is the mechanism that detects a log whose history changed underneath its
published checkpoints; RFC-ACDP-0012 §8.2 makes it REQUIRED for exactly that
reason. A monitor that has retained an earlier checkpoint and cannot verify it
against the current one has found a root rewrite.

**What it means.** The Merkle root is not stored independently — it is computed
from the stored leaf hashes on every request. So a failed consistency proof does
not indicate a corrupted root value. It indicates that the **leaves themselves
changed**: rows removed, reordered, or rewritten. The realistic causes are a
database restore that rolled the log back, a partial restore that mixed two
datasets, or direct writes to the log table.

**Do, in order:**

1. **Preserve the evidence before touching anything.** Snapshot the database and
   keep the failing checkpoint pair (`first`, `second`) and both responses. A
   second restore attempt destroys the only record of what the log claimed.
2. **Establish which direction it moved.** Fetch `GET /log/checkpoint` and
   compare `tree_size` against the retained checkpoint. A *smaller* current
   `tree_size` is a rollback — almost always a restore from an older backup. An
   equal-or-larger size with a non-verifying proof is a rewrite, which is more
   serious.
3. **Stop publishing.** Every new entry appended on top of a rewritten log
   extends the divergence and enlarges the set of checkpoints that can never be
   reconciled. The registry has no mechanism to repair a log in place, and none
   to reconcile two histories.
4. **If it was a rollback from a restore:** restore forward to the most recent
   good backup instead. Entries published between the two backups are not
   recoverable from the registry — they have to be re-published by their
   producers, which mints new entries at new positions. Say so explicitly to
   consumers; their retained inclusion proofs for the lost entries will not
   verify against the new log.
5. **If it was not a restore:** treat it as a possible compromise of whatever
   has write access to the log table, and follow the rotation steps below. A
   rewrite that nobody performed deliberately means something else can write
   there.
6. **Tell the monitors.** A consistency failure is the signal the transparency
   log exists to produce; a registry that resolves one silently has removed the
   only reason a consumer would trust it. Publish what happened, the affected
   `tree_size` range, and whether entries were lost.

## Key rotation

- **HS256** — set a new `ACDP_REGISTRY_AUTH__JWT_SECRET` and restart. Outstanding
  tokens become invalid immediately; clients re-run challenge-response.
- **EdDSA** — rotate `auth.jwt_private_key_pem` (and optionally `jwt_kid`) and
  restart; the new public key is published at `/.well-known/jwks.json`.
  Outstanding tokens stop verifying once the old public key is gone — coordinate
  with federated verifiers that cache the JWKS (300 s `max-age`).

For targeted revocation without rotating the signing key, use
`POST /auth/token/revoke` (see [AUTHENTICATION.md](AUTHENTICATION.md#token-revocation)).

### Admin tokens

`auth.admin_tokens` is a **list**, which is what makes rotation possible without
a window of refused admin requests:

1. Append the new token to `auth.admin_tokens` and restart. Both old and new are
   now accepted.
2. Move every caller — deploy scripts, dashboards, whatever calls `/admin/*` —
   to the new token.
3. Remove the old entry and restart.

Two things to know before you start. Emptying the list does **not** leave
`/admin/*` open: an empty list disables every admin-bearer-gated route outright
(`/admin/status`, `/admin/contexts`, the audit, retract and republish routes,
and `/admin/pinned-keys/reload`). And entries are compared with no trimming on
`/admin/*` — a token with stray leading or trailing whitespace is rejected there
while being accepted on other routes over HTTP/1.1, which is why startup
validation refuses such entries outright. See
[AUTHENTICATION.md](AUTHENTICATION.md) for the full parser comparison.

### Webhook signing secret

`webhook.secret` keys the HMAC-SHA256 in `X-ACDP-Signature` **and** in
`X-ACDP-Signature-Timestamped` (the opt-in freshness signature — see
[WEBHOOKS.md](WEBHOOKS.md#signature-scheme)). One secret keys both, so rotation
below covers both; a receiver verifying either one is affected identically.
There is no multi-secret list and no overlap window: the registry signs with
exactly one secret, so the moment you rotate, deliveries signed with the old
secret stop verifying at the receiver.

Rotate from the **receiving** side, not the sending side:

1. Teach the receiver to accept either secret — verify against the new one, and
   fall back to the old on mismatch.
2. Change `webhook.secret` and restart the registry.
3. Once no delivery has verified against the old secret for longer than your
   retry window, drop the fallback.

Rotating the registry first inverts this into an outage: every event in flight
during the change is signed with a secret the receiver rejects, and because the
delivery queue is in-memory with no outbox, a delivery that exhausts its retries
is gone rather than deferred.

### Receipt signing key

Receipt-key rotation has its own procedure and a retention rule that must not be
violated — a removed key invalidates every receipt it ever signed. It is
documented in [RECEIPTS.md](RECEIPTS.md#the-key-retention-rule-read-before-rotating),
not here.
