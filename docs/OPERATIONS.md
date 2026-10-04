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

### Inbound TLS was broken before 0.1.4, and the smoke tests could not see it

Setting `registry.tls.enabled = true` used to abort the process at startup with
**exit code 101**: the dependency graph enables two rustls crypto providers
(`aws-lc-rs` via `axum-server`'s `tls-rustls`, `ring` via `reqwest`'s
`rustls-tls`), neither of them selected by a feature this workspace controls,
and nothing installed one explicitly. rustls refuses to guess.

The symptom was worse than a failed start. `main` logs `listening` with the bind
address **before** entering the TLS branch, so an operator saw a normal
startup line and then a dead process with the port refusing connections. If you
ever saw that and concluded the config was wrong, it was not — `tls.enabled` was
simply unusable.

Fixed by installing `ring` explicitly, before the `listening` log.

**Why no test caught it, which is the part worth keeping.**
`docker/config.docker.toml` sets `tls.enabled = false`, so the container smoke
tests exercise the plaintext path only and would not have caught this — nor will
they catch a regression. Nor could an ordinary in-process test: the failure was
in the *shipped binary's* dependency set, not in library code a test links
against, so a test build could import a crypto provider the real binary did not
have. The guard is `crates/acdp-registry-server/tests/tls_startup.rs`, which
**spawns the real binary** and requires it to serve HTTPS — its red state was
measured on the unfixed tree (exit 101) before the test was written.

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

**Rolling back a release (Postgres).** The Postgres migrator tolerates a
database that is *ahead* of the running binary — it can roll back one release
without crash-looping, because migrations are required to stay additive (see
CONTRIBUTING.md). Applied migrations remain checksum-verified, so a corrupted
or edited migration still fails startup. The orphaned table/columns from the
newer migration are left in place and simply unused by the older binary; to
fully retire a migration, delete its row from `_sqlx_migrations` and drop its
objects by hand.

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
  `public_arm_open` parameter — the local is spelled `admin_sees_public_arm`
  where it is declared in `admin_list`
  (`crates/acdp-registry-core/src/handlers/admin.rs`) and is passed as the
  fifth positional argument, binding to `ExtendedRegistryStore::list_contexts`'s
  `public_arm_open` parameter (`crates/acdp-registry-store/src/lib.rs`), so it
  reaches the RFC-ACDP-0008 §4.5 public arm only — restricted and private
  contexts are never disclosed to it. (The configured
  `auth.anonymous_public_reads` is not consulted on this path: it scopes only
  unauthenticated requests, and an admin bearer is authenticated.)

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

Per-agent, per-process, in-memory fixed-window counters:
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

By default all of these are per-process. Behind a load balancer set
`trusted_proxies` so per-IP limits track real clients; the `global_per_minute`
ceiling is then per replica. With more than one replica you have three options
for the `/auth/*` bound:

1. **Gateway / proxy limits** in front of the registry. Required in any case if
   you need protection against a *volumetric* flood: every limit the registry
   enforces runs after the request has already reached the registry process.
2. **`[rate_limit] backend = "postgres"`** — the per-IP and global ceilings are
   counted once across every replica sharing the database (table
   `rate_limit_windows`, an `UNLOGGED` table). Postgres storage only; see
   [CONFIGURATION.md · `[rate_limit]`](CONFIGURATION.md#rate_limit-feat-06) for
   the keys and the startup checks. The in-memory limiter stays in front as a
   pre-filter, so a request it rejects never touches the database and database
   writes stay bounded by the in-memory limits regardless of attacker volume.
   If the database cannot answer, `backend_unavailable = "allow"` (the default)
   falls back to per-process limiting rather than failing token issuance;
   `"deny"` refuses instead. Watch
   `acdp_registry_rate_limit_shared_total{outcome="unavailable"}` and the
   `acdp_registry_rate_limit_shared_seconds` latency histogram
   ([HTTP-API.md · metrics](HTTP-API.md#get-metrics-feat-10)): the single
   `auth_global` row is updated by every `/auth/*` request fleet-wide, so its
   latency is the figure to check before raising `global_per_minute` into the
   tens of thousands across several replicas.
3. **Accept per-replica bounds.** Each replica enforces its own limits, so the
   effective cluster-wide ceiling is `replicas × global_per_minute`.

The Postgres backend is a fairness and abuse bound, **not** a DDoS defence: do
not rely on it instead of an edge limiter. It is also not needed on a single
replica — every deployment recipe in this repo runs exactly one. Its counters are
ephemeral by design (the table is `UNLOGGED`): after a database crash or standby
promotion one window's budget resets once.

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
3. **If `[log]` is enabled, settle the log instance before the first boot.**
   A restore from any backup older than the last publish rolls the log back,
   and the first `GET /log/checkpoint` after boot signs the restored tree under
   whatever `log_id` is configured. If the restored tree may be smaller than a
   checkpoint the registry already served, change `[log] instance` *before*
   starting — see the [runbook below](#runbook-transparency-log-inconsistency).
4. **Start the registry and watch the first boot.** Migrations run before the
   listener binds, so a schema failure is a startup failure, not a 500 later.
5. **Verify readiness, not liveness.** `GET /healthz` reports storage
   readiness and answers `503` if the backend is not usable; `GET /livez` says
   only that the process is up and will answer `200` against a broken database.
   Check `/healthz`.

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
| Search page size | clamped in both stores | A caller-supplied `limit` is clamped to 100. A page can also come back **shorter** than requested when the request uses `?visibility=`, which is applied after the query (tenant scoping is not: it runs in SQL) — see [MULTI-TENANCY.md](MULTI-TENANCY.md#how-the-binding-is-stored-and-filtered) on the refill cap. Short is not "end of results"; page until `next_cursor` is absent. |
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

**The rule every recovery must follow.** [RFC-ACDP-0012 §7.4](https://github.com/agentcontextdistributionprotocol/agentcontextdistributionprotocol/blob/9deb7e7bdabfa7416fcc0e25a7fcac6eb642b6dd/rfcs/RFC-ACDP-0012-transparency-log.md#74-log-instantiation-and-reset)
(pinned spec): a registry whose tree is lost **MUST NOT serve a reconstructed
history under the same `log_id`**; it MUST start a new instantiation with a new
`<instance>` component, and SHOULD publish an operational notice. "Serve" here
includes reads: `GET /log/checkpoint` is public and signs the current tree on
demand, so a registry that merely boots on a rolled-back database is already
signing the reconstructed history under the old `log_id`. (That reading of
"serve" is this runbook's, not the RFC's wording.) Restoring an older backup
and carrying on under the same `log_id` is therefore not a valid recovery
unless step 4 shows the restored tree is consistent with everything already
served.

This registry's `log_id` is `did:web:<authority>/log/<instance>`, where
`<instance>` is `[log] instance` (default `"1"`; env
`ACDP_REGISTRY_LOG__INSTANCE`; must match `[a-z0-9-]{1,32}`, checked at
startup). Changing it is a config change and a restart — no migration, no
manual SQL. What it does to existing data, which you should state in your
notice:

- **The surviving leaves become the new log's history.** `log_leaves` has no
  instance or `log_id` column, so nothing is deleted or renumbered: the rows
  still present keep `leaf_index` 0 onward, the new log's first checkpoint
  commits to all of them (`tree_size` is the row count), and new publishes
  append after them. Under the new `log_id` those positions say nothing about
  when the entries were first published; the new log anchors them only from
  its own first checkpoint.
- **Witness cosignatures stop matching.** `log_witness_cosignatures` rows are
  keyed by `log_id`; the old rows stay in the table but are never attached to
  a new-log checkpoint, and the registry polls each `[[witnesses]]` entry for
  the new `log_id`. The new log starts un-witnessed until the witnesses cosign
  it, so tell their operators.
- **Old proofs do not carry over.** Consistency proofs exist only within one
  `log_id`. Inclusion proofs and checkpoints consumers retained for the old
  `log_id` cannot be checked against the new one. That break is the intended,
  visible signal.

**Do, in order:**

1. **Preserve the evidence before touching anything.** Snapshot the database and
   keep the failing checkpoint pair (`first`, `second`) and both responses. A
   second restore attempt destroys the only record of what the log claimed.
2. **Take the registry out of service** (stop it, or remove it from the load
   balancer so clients cannot reach `/log/*`). Every checkpoint it serves on the
   bad tree, and every entry appended on top of it, widens the set of
   checkpoints that can never be reconciled. The registry has no mechanism to
   repair a log in place, and none to reconcile two histories.
3. **Find the largest checkpoint the registry may have served** under the
   current `log_id`: from your monitors, a witness, a consumer report, or the
   pre-incident database (its `SELECT COUNT(*) FROM log_leaves` is the
   `tree_size` it was serving). Compare it with the tree you are about to serve
   (the same count on the database you will run).
4. **Decide the instance.** Keep the current `log_id` only if the tree you will
   serve is at least as large as the largest served checkpoint **and** a
   consistency proof from that checkpoint to the new head verifies (fetch
   `GET /log/proof?first=<served>&second=<current>` with the registry reachable
   only by you, and check the proof with any RFC 6962 consistency-proof verifier;
   the shapes are in RFC-ACDP-0012 §6). Otherwise — the restored tree is smaller, the root differs, or
   you cannot show either way — **set a new `[log] instance`** (for example
   `"1"` → `"2"`) before the registry is reachable again. On a public registry
   you can rarely rule out an unseen checkpoint, so expect to change it after
   any rollback. (This is operator guidance, stricter than §7.4's wording.)
5. **If it was a rollback from a restore:** contexts published after the backup
   are gone from this registry, along with their leaves. Their producers have
   to re-publish them, which mints new `ctx_id`s and new leaves in the new log.
   Say so explicitly to consumers.
6. **If it was not a restore:** treat it as a possible compromise of whatever
   has write access to the log table, and follow the rotation steps below. A
   rewrite that nobody performed deliberately means something else can write
   there. The instance decision in step 4 applies here too.
7. **Tell the monitors and witnesses** (the operational notice §7.4 asks for).
   A consistency failure is the signal the transparency log exists to produce;
   a registry that resolves one silently has removed the only reason a consumer
   would trust it. Publish what happened, the old and new `log_id`, the affected
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
