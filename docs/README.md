# acdp-registry-rs documentation

Reference documentation for the Agent Context Distribution Protocol (ACDP)
registry. Two axes matter here and they are **not** the same thing:

- **Advertised version** — the `acdp_version` string served at
  `GET /.well-known/acdp.json`. As of REG-3 (RFC-ACDP-0016 §10 anchors
  support), this is unconditionally `"0.5.0"` for every configuration of
  the shipped binary, because anchors handling has no admin-config gate
  and its version claim always wins the capability-ladder max(). It no
  longer tells you which of the sections below are actually active.
- **Functional capabilities** — what the registry actually enables and
  enforces, which still lights up per-section exactly as before: v0.2.0
  trust-hardening (registry receipts, did:key), v0.3.0 (lifecycle events,
  the transparency log, head receipts), and v0.4.0 (witness cosignature
  aggregation) each activate only when their config sections are
  configured. Check `profiles` and the response bodies (not
  `acdp_version`) to see which of these a given deployment actually runs.

Start with the [project README](../README.md) for a quick start; these docs
go deeper.

## Map

| Doc | What it covers |
|-----|----------------|
| [ARCHITECTURE.md](ARCHITECTURE.md) | Crate graph, storage trait, publish pipeline, the request lifecycle. |
| [HTTP-API.md](HTTP-API.md) | Every endpoint: inputs, response shapes, status codes, and the error envelope. |
| [AUTHENTICATION.md](AUTHENTICATION.md) | DID challenge-response, JWT claims, HS256 vs EdDSA, token revocation, cross-issuer federation. |
| [CONFIGURATION.md](CONFIGURATION.md) | The full config tree — every TOML key, its env-var name, type, and default. |
| [MULTI-TENANCY.md](MULTI-TENANCY.md) | Tenant resolution precedence, strict mode, agent→tenant bindings, the SQL filter. |
| [WEBHOOKS.md](WEBHOOKS.md) | Event payloads, the GitHub-compatible signature scheme, delivery and retry semantics. |
| [RECEIPTS.md](RECEIPTS.md) | ACDP 0.2.0 registry receipts: enabling, serving `/.well-known/did.json`, the key-retention rule, rotation, did:key, the lineage audit. |
| [OPERATIONS.md](OPERATIONS.md) | Deploying, observability, backup/restore, key rotation, federation ops. |
| [UPGRADING.md](UPGRADING.md) | **Read before upgrading a deployment.** Operator-visible changes per version — ordering requirements, moved defaults, config whose meaning changed. |
| [MAINTAINING.md](MAINTAINING.md) | For maintainers: required vs advisory checks, the protection drift job, the enforce_admins / tag-ruleset runbook, the release flow, the mutation oracle. |
| [ENGINEERING-LOG.md](ENGINEERING-LOG.md) | The narrative record of what changed and why — reasoning, rejected alternatives, evidence. Was the root `CHANGELOG.md` until #220; per-release notes live in `crates/*/CHANGELOG.md`. |
| [advertisable-profiles.json](advertisable-profiles.json) | The profile names this registry can advertise, as data for non-Rust consumers; kept identical to the code by `advertisable_profiles_json_matches_const` (see [CONFIGURATION.md](CONFIGURATION.md)). |
| [MUTATION-SCOPE-CANDIDATES.md](MUTATION-SCOPE-CANDIDATES.md) | Which files the mutation oracle should cover next, and the measured survivor bill for each — the companion to `.cargo/mutants.toml` for #216 item 1. |
| [mutation-runs/README.md](mutation-runs/README.md) | Index of the committed cargo-mutants run ledgers (`outcomes.json` per run): which one each was measured against and which are superseded. A dated record. |

## Where the protocol ends and this registry begins

These docs cover **this service** — its HTTP surface, storage, auth, tenancy,
webhooks, and operations. They deliberately do **not** restate the protocol the
service implements. Anything about the wire format, signing/hashing,
verification, SSRF defenses, or the canonical error-code registry lives in the
`acdp` protocol library docs and the RFC spec — we link out rather than copy:

| For… | See |
|------|-----|
| The publish pipeline algorithm (RFC-ACDP-0003 §2.1), `RegistryServer` / `RegistryStore` | [acdp-rs · Implementing a Registry](https://github.com/agentcontextdistributionprotocol/acdp-rs/blob/8a888edaa15c4475bbaeccff45567921e3153730/docs/registry.md) |
| Building/signing a `PublishRequest`, `content_hash`, supersession | [acdp-rs · Producing](https://github.com/agentcontextdistributionprotocol/acdp-rs/blob/8a888edaa15c4475bbaeccff45567921e3153730/docs/producing.md) |
| The verification pipeline, `VerifiedContext`, retrieval | [acdp-rs · Consuming & Verifying](https://github.com/agentcontextdistributionprotocol/acdp-rs/blob/8a888edaa15c4475bbaeccff45567921e3153730/docs/consuming.md) |
| The `AcdpError` ↔ RFC-ACDP-0007 §5 wire-code registry, retry guidance | [acdp-rs · Errors & Retries](https://github.com/agentcontextdistributionprotocol/acdp-rs/blob/8a888edaa15c4475bbaeccff45567921e3153730/docs/errors.md) |
| SSRF defenses, HTTPS/size/redirect caps, algorithm-downgrade rejection (`WebResolver`) | [acdp-rs · Security Model](https://github.com/agentcontextdistributionprotocol/acdp-rs/blob/8a888edaa15c4475bbaeccff45567921e3153730/docs/security.md) |
| The three-layer model (what is hashed/signed/mutable) | [acdp-rs · Architecture](https://github.com/agentcontextdistributionprotocol/acdp-rs/blob/8a888edaa15c4475bbaeccff45567921e3153730/docs/architecture.md) |
| API reference for the `acdp` crate | [docs.rs/acdp 0.14.3](https://docs.rs/acdp/0.14.3/acdp/) |
| The IANA-style registries (profiles, error codes, lifecycle event types, signature algorithms) | [spec · registries](https://github.com/agentcontextdistributionprotocol/agentcontextdistributionprotocol/tree/9deb7e7bdabfa7416fcc0e25a7fcac6eb642b6dd/registries) |
| Normative protocol rules | [RFC set](https://github.com/agentcontextdistributionprotocol/agentcontextdistributionprotocol/tree/9deb7e7bdabfa7416fcc0e25a7fcac6eb642b6dd/rfcs) |

`acdp-rs` documents its own registry building blocks, not this registry. Where
its [Implementing a Registry](https://github.com/agentcontextdistributionprotocol/acdp-rs/blob/8a888edaa15c4475bbaeccff45567921e3153730/docs/registry.md) guide describes a deployment
choice, this registry's docs state what it actually does: for example, it
enforces its own per-agent publish budget rather than plugging a limiter into
`RegistryServer` (see [ARCHITECTURE.md](ARCHITECTURE.md#publish-pipeline)),
and it can advertise the seven profiles listed in
[advertisable-profiles.json](advertisable-profiles.json), not only the three
that guide names.

### Link convention

Every link to a sibling repository is **pinned** and written **inline**
(`[text](url)`), never reference-style and never as a relative `../` path —
the website rewriter skips reference-style links, and relative paths break on
both GitHub and the website.

- **Spec (RFCs, `registries/`, schemas, `VERSIONING.md`):**
  `https://github.com/agentcontextdistributionprotocol/agentcontextdistributionprotocol/blob/<ref>/rfcs/RFC-ACDP-00NN-<slug>.md#<anchor>`, where `<ref>` is
  the `ref:` value in `.spec-pin` (today `9deb7e7bdabfa7416fcc0e25a7fcac6eb642b6dd`). Non-normative
  pages under the spec's `docs/` may instead use
  `fb76f6d54ba25f583ce526b2bdee30503a5d8e59`, the docs refresh that followed
  the pin. No other spec ref — no `main`, tag, or short SHA.
- **SDK guides:** `https://github.com/agentcontextdistributionprotocol/acdp-rs/blob/<ref>/docs/<page>.md#<anchor>`,
  where `<ref>` is a release tag `acdp-v<semver>` or a full 40-hex SHA — today
  `8a888edaa15c4475bbaeccff45567921e3153730` (the guides refresh, not yet in a
  tag). Never `main`.
- **SDK API:** docs.rs with an explicit version matching `Cargo.lock`, e.g.
  `https://docs.rs/acdp/0.14.3/acdp/`. For re-exported modules use the
  sub-crate path (`https://docs.rs/acdp-client/0.14.3/acdp_client/verified/`);
  the `acdp/client/...` form does not exist on docs.rs.
- **Links to this repository** may use `main`.

**Re-pointing on a spec bump.** A PR that changes `ref:` in `.spec-pin` must
replace the old SHA in every pinned spec link in the same PR, and re-check
each `#anchor` against the new revision's headings (sections get renumbered).
`sibling_repo_links_are_pinned` in `conformance_gate.rs` enforces these rules,
so such a PR fails CI until the links are re-pointed;
[MAINTAINING.md](MAINTAINING.md#spec-bumps) has the one-command rewrite.
An `acdp` bump re-points docs.rs versions the same way; guide links move only
when a newer tag or SHA carries the page being cited.

## Conventions used throughout

- **Authority** — the registry's bare lowercase DNS name (e.g.
  `registry.example.com`). It is both the `ctx_id` minting authority and the
  `did:web` identifier for the registry.
- **`ctx_id`** — a fully-qualified context identifier scoped to an authority.
- **Wire envelope** — every ACDP data/auth endpoint returns
  `application/acdp+json`; errors follow the RFC-ACDP-0007 §4 envelope
  (see [HTTP-API.md](HTTP-API.md#error-envelope)).
- **RFC-ACDP-XXXX** references point at the
  [protocol spec](https://github.com/agentcontextdistributionprotocol/agentcontextdistributionprotocol/tree/9deb7e7bdabfa7416fcc0e25a7fcac6eb642b6dd) at the revision pinned in `.spec-pin`.

## Spec profiles implemented

The profile names, their status, and their prerequisites are defined
canonically in the spec's [profile registry](https://github.com/agentcontextdistributionprotocol/agentcontextdistributionprotocol/blob/9deb7e7bdabfa7416fcc0e25a7fcac6eb642b6dd/registries/profiles.md) — this section only
records **which** of them this implementation advertises, not what they mean.

`acdp-registry-core` and `acdp-registry-discovery` by default (an operator-set
`registry.profiles` replaces that pair, and is the only way to advertise
`acdp-registry-federated`); `acdp-registry-receipts`,
`acdp-registry-head-receipts`, `acdp-registry-lifecycle`, and
`acdp-registry-transparency-log` are advertised when their config sections are
enabled. The full allowlist is
[advertisable-profiles.json](advertisable-profiles.json). All are served at
`GET /.well-known/acdp.json`.
See [HTTP-API.md](HTTP-API.md#get-well-knownacdpjson).
