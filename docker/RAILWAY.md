# Deploying the registry to Railway

The CI pipeline (`.github/workflows/docker.yml`) builds a `linux/amd64` image and
pushes it to the GitHub Container Registry (GHCR) on every push to `main` and on
every `acdp-registry-server/v*` release tag:

```
ghcr.io/agentcontextdistributionprotocol/acdp-registry:latest        # tip of main
ghcr.io/agentcontextdistributionprotocol/acdp-registry:main          # tip of main
ghcr.io/agentcontextdistributionprotocol/acdp-registry:0.1.0         # a release tag, leading `v` stripped
ghcr.io/agentcontextdistributionprotocol/acdp-registry:0.1           # rolling major.minor
ghcr.io/agentcontextdistributionprotocol/acdp-registry:sha-<7-hex>   # every push
```

> **`:latest` tracks the tip of `main`, not the last release.** Pin a version tag
> (`:0.1.0`, or `:0.1` to follow patches) for anything you care about keeping
> stable: `:latest` moves on every merge to `main`, a version tag does not.
> Both version tags above are **real and pullable** — the release pipeline
> published its first one on 2026-09-10. They are no longer illustrative.
> The GitHub Release corresponding to `:0.1.0` is
> [`acdp-registry-server/v0.1.0`](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/releases/tag/acdp-registry-server%2Fv0.1.0)
> (slash namespace). The older hyphen-named `acdp-registry-server-v0.1.0`
> Release is a June 2026 baseline, built 124 commits earlier, and does **not**
> describe this image.

Pull-request builds compute a `pr-<n>` tag but never push it — the login and push
steps are skipped for `pull_request` events.

## Deploy the prebuilt GHCR image (recommended)

`acdp` is consumed from crates.io, so Railway *could* build this repo from source
directly. We still recommend deploying the **prebuilt GHCR image**: it's the
exact `linux/amd64` artifact CI already built and tested, so deploys are fast and
reproducible instead of recompiling the Rust workspace on every push.

## One-time GHCR setup

Railway needs to pull from GHCR. Either:

- **Make the package public** — GitHub → repo → *Packages* → `acdp-registry` →
  *Package settings* → change visibility to **Public**. Then Railway pulls with
  no credentials. (Simplest.)
- **Or keep it private** and give Railway a registry credential: a GitHub PAT
  with `read:packages` scope, configured on the Railway service.

## Creating the Railway service (later)

1. New Project → **Deploy from a Docker image**.
2. Image: `ghcr.io/agentcontextdistributionprotocol/acdp-registry:latest`
   (pin a version tag such as `0.1.0` for production stability — the image tag
   carries no leading `v`).
3. Add a **PostgreSQL** plugin (the image is built with `STORAGE_FEATURE=storage-pg`).
4. Set the env vars below.
5. Networking → expose the service; set the target port (see `$PORT` note).

### Required env vars

| Variable | Value |
|----------|-------|
| `ACDP_REGISTRY_STORAGE__BACKEND` | `postgres` — the GHCR image is compiled Postgres-only and mounts no config file on Railway, so the backend must be selected here (the default is `sqlite`, which the image refuses to run) |
| `ACDP_REGISTRY_STORAGE__POSTGRES_URL` | `${{ Postgres.DATABASE_URL }}` (Railway reference) |
| `ACDP_REGISTRY_AUTH__JWT_SECRET` | a real secret — `openssl rand -base64 32`. Validated at startup **even with auth off**: on the default HS256 signing algorithm, any non-empty value must not be the literal `changeme` and must be base64 decoding to at least 32 bytes, or the binary refuses to boot. Only an *empty* secret skips these checks, and an empty secret is accepted only while auth stays off. Setting a real value now means enabling auth later is one variable, not two |
| `ACDP_REGISTRY_REGISTRY__BIND` | `0.0.0.0` |
| `ACDP_REGISTRY_REGISTRY__ALLOW_PUBLIC_BIND` | `true` (non-loopback bind opt-in) |
| `ACDP_REGISTRY_REGISTRY__PORT` | `${{ PORT }}` — Railway injects `$PORT`; the registry config key is `registry.port`, so map it explicitly |
| `ACDP_REGISTRY_REGISTRY__AUTHORITY` | your public hostname (e.g. `acdp-registry.up.railway.app`) |
| `RUST_LOG` | `info,acdp=info,acdp_registry=info` |

> **This recipe leaves authentication OFF.** It does not set
> `ACDP_REGISTRY_AUTH__ENABLED`, `auth.enabled` defaults to `false`
> (`AuthConfig::default()`), and the GHCR image mounts no config file on Railway
> — so `config.docker.toml` does not apply either. Two consequences worth being
> explicit about. First, the `JWT_SECRET` above **is** validated at startup
> despite auth being off: the `changeme` and base64/≥32-byte checks run whenever
> the secret is non-empty and the signing algorithm is HS256 (the default) —
> only the *empty-secret* check is gated on `auth.enabled` (see `validate_config`
> in `crates/acdp-registry-server/src/main.rs`). Second, the
> `ALLOW_PUBLIC_BIND = true` in the table waives a startup guard that otherwise
> refuses a non-loopback bind with TLS *and* auth both disabled. That guard's
> stated precondition is a trusted proxy that terminates TLS **and
> authenticates** in front of the registry; Railway's edge does the former, not
> the latter. Requests reaching the registry are therefore unauthenticated.
> Publishes and lifecycle events remain bound to DID-signature verification;
> nothing else is. Run this recipe as written only where something in front of
> Railway is genuinely doing the authenticating.

> **Read posture — decide this deliberately; the recipe's defaults serve no
> reads.** A `public` context is retrievable only when
> `auth.anonymous_public_reads = true` **or** the caller presents a valid bearer
> token; `restricted` and `private` additionally require the caller to be the
> producer or a named audience member. This recipe sets neither flag, and with
> auth off every caller is anonymous (`caller_from_headers` returns "no caller"
> unconditionally) — so `retrieve`, `search` and `list` return nothing to
> anybody, `public` contexts included. Publishing still works. To make the
> registry readable, opt in to exactly what you mean:
>
> - `ACDP_REGISTRY_AUTH__ANONYMOUS_PUBLIC_READS = true` — `public` contexts
>   become world-readable with no credential. `restricted` and `private` stay
>   unreadable, because without auth there is no identity to entitle.
> - `ACDP_REGISTRY_AUTH__ENABLED = true` — bearer-token authentication.
>   **Understand what this opens before choosing it.** Any caller holding a
>   valid token can read **every** `public` context, and token issuance has no
>   agent allowlist: it gates on the challenge binding, expiry, signature
>   algorithm and DID method (`auth.did_methods`, default `["did:web"]`), so the
>   bar for a token is control of any domain serving a `did.json`. In effect the
>   `public` corpus becomes readable by anyone willing to register a domain.
>   Enabling auth does **not** make credentials mandatory — a request with no
>   `Authorization` header is still served as an anonymous caller; only a
>   present-but-invalid token is rejected. It *does* make a non-empty
>   `JWT_SECRET` mandatory at boot, and it mounts `/auth/challenge` and
>   `/auth/token` as new **unauthenticated** endpoints (rate-limited per client
>   IP — but `rate_limit.trusted_proxies` defaults to empty, so behind Railway's
>   edge all traffic shares one bucket until you configure it).
>   `[[auth.tenant_agents]]` does **not** narrow any of this: it binds agents to
>   tenants, it does not restrict who can obtain a token.
>
> If you enable auth and still want anonymous readers of `public` contexts, set
> both flags. If you want `public` contexts unreadable, set neither — which is
> what this recipe does.

> **TLS:** terminate TLS at Railway's edge and run the container with
> `registry.tls.enabled = false` (as in `config.docker.toml`). The
> `ALLOW_PUBLIC_BIND` opt-in exists so a non-loopback bind without in-process
> TLS is a deliberate choice.

### Healthcheck

Point Railway's healthcheck at **`/healthz`** — it reports storage
**readiness** and answers `503` when the database is unreachable, which is what
you want for gating traffic and for gating a deploy.

**Do not point a *liveness* probe at `/healthz`.** It returns `503` during a
database outage, so a liveness probe there restarts a process that is perfectly
alive and cannot fix the database by restarting — and every restart discards the
in-memory webhook queue, which has no outbox and no replay. Use **`/livez`** for
liveness: it always answers `200` and never touches storage.

## Local parity

`docker/docker-compose.yml` runs the same image against a local Postgres — use it
to validate config before promoting to Railway.
