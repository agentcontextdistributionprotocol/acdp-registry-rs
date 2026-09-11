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
| `ACDP_REGISTRY_AUTH__JWT_SECRET` | a real secret — `openssl rand -base64 32`. The binary rejects the literal `changeme` only once auth is enabled on HS256; this recipe does not enable auth, so the value is never validated — see the note below |
| `ACDP_REGISTRY_REGISTRY__BIND` | `0.0.0.0` |
| `ACDP_REGISTRY_REGISTRY__ALLOW_PUBLIC_BIND` | `true` (non-loopback bind opt-in) |
| `ACDP_REGISTRY_REGISTRY__PORT` | `${{ PORT }}` — Railway injects `$PORT`; the registry config key is `registry.port`, so map it explicitly |
| `ACDP_REGISTRY_REGISTRY__AUTHORITY` | your public hostname (e.g. `acdp-registry.up.railway.app`) |
| `RUST_LOG` | `info,acdp=info,acdp_registry=info` |

> **This recipe leaves authentication OFF.** It does not set
> `ACDP_REGISTRY_AUTH__ENABLED`, `auth.enabled` defaults to `false`
> (`AuthConfig::default()`), and the GHCR image mounts no config file on Railway
> — so `config.docker.toml` does not apply either. Two consequences worth being
> explicit about: the `JWT_SECRET` above is never validated (the `changeme`
> check is gated on auth being enabled), and the `ALLOW_PUBLIC_BIND = true` in
> the table waives a startup guard that otherwise refuses a non-loopback bind with TLS
> *and* auth both disabled. That guard's stated precondition is a trusted proxy
> that terminates TLS **and authenticates** in front of the registry; Railway's
> edge does the former, not the latter. Requests reaching the registry are
> therefore unauthenticated. Publishes and lifecycle events remain bound to
> DID-signature verification; nothing else is. Authentication is turned on with
> `ACDP_REGISTRY_AUTH__ENABLED = true`; run this recipe as written only where
> something in front of Railway is genuinely doing the authenticating.

> **TLS:** terminate TLS at Railway's edge and run the container with
> `registry.tls.enabled = false` (as in `config.docker.toml`). The
> `ALLOW_PUBLIC_BIND` opt-in exists so a non-loopback bind without in-process
> TLS is a deliberate choice.

### Healthcheck

Point Railway's healthcheck at **`/healthz`**.

## Local parity

`docker/docker-compose.yml` runs the same image against a local Postgres — use it
to validate config before promoting to Railway.
