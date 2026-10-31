# live-smoke

An end-to-end check of a **deployed** registry, done the way a remote consumer
does it. It runs these steps in order:

1. `GET /.well-known/acdp.json`. Fails unless the registry advertises
   `acdp-registry-receipts` and `acdp-registry-lifecycle` and has
   `anonymous_public_reads` on. When it does not advertise
   `acdp-registry-keyless` (RFC-ACDP-0021), fails unless `GET` and
   `POST /key-attestations` both answer 501 `not_implemented`. When it does,
   the probe is skipped here; the `keyless` subcommand below covers it.
2. Publishes a `public` context, signed by a `did:web` producer. Fails unless
   the response carries a `registry_receipt`.
3. Resolves the context through `acdp::client::CrossRegistryResolver`. The
   resolver is configured like Seam's `seam-context-acdp`: an SSRF policy, an
   authority allowlist, and fail-closed RFC-ACDP-0014 revocation discovery.
   Fails unless the receipt verifies and the context is active and not
   retracted. A context with no events omits `lifecycle_events`, so the
   retraction reads `unknown` at this step.
4. Posts a producer-signed `retracted` lifecycle event (RFC-ACDP-0013).
   When the registry advertises `acdp-registry-changes` (RFC-ACDP-0018), it
   reads from the `GET /changes?since=head` cursor saved after step 1 with a
   `ctx_id` watch (up to 5 reads, 1 s apart) and fails unless the retraction's
   lifecycle entry is there and `acdp::client::verify_change` returns
   `Verified` and reads it `retracted`. It prints the entry's `seq`. When the
   profile is not advertised, `GET /changes` must answer 501 (or 404 from a
   build that predates the route) and the check is skipped.
5. Resolves again. Fails unless the context now reads `retracted` and the
   receipt still verifies.

It ends with `SMOKE OK <ctx_id>`. A failure exits non-zero and names the step
that failed.

The tool is excluded from the workspace (`exclude` in the root `Cargo.toml`)
and has its own lockfile. It talks to a live network, so CI does not build or
run it.

## Prerequisites

- **The registry must be reachable at its `registry.authority`.** The resolver
  routes by the `ctx_id` authority, not by `--base-url`. Run it against the
  public hostname, not a platform-internal URL.
- **The producer needs a `did:web` whose `did.json` is live.** Both the
  registry and the resolver fetch it. Generate the document from the seed and
  host it at `https://<host>/.well-known/did.json`:

  ```bash
  export SMOKE_PRODUCER_SEED_B64="$(openssl rand -base64 32)"   # keep it
  cargo run --release -- did-doc --did did:web:<host> > did.json
  ```

## Run

```bash
cd tools/live-smoke
SMOKE_PRODUCER_SEED_B64=… cargo run --release -- run \
  --base-url https://registry.example.com \
  --producer-did did:web:<host>
```

To run as the seed's own `did:key`, with nothing to host, pass
`--producer-did did:key`. That needs `did:key` in the registry's
`auth.did_methods`.

Every run leaves one retracted context titled `live smoke <timestamp>` behind.

For the reference deployment, the producer is
`did:web:acdp-smoke-producer.vercel.app` and its seed is in Doppler
(`acdp-registry` / `prd`):

```bash
doppler run --project acdp-registry --config prd -- \
  cargo run --release -- run \
    --base-url https://registry.agentcontextdistributionprotocol.io \
    --producer-did did:web:acdp-smoke-producer.vercel.app
```

## Keyless (RFC-ACDP-0021)

`keyless` checks a registry that advertises `acdp-registry-keyless` with a
real workload identity. It needs no seed. It reads an OIDC token minted with
the registry DID as audience from `SMOKE_OIDC_TOKEN`, then:

1. fails unless the capabilities advertise the profile and
   `keyless.accepted_oidc_issuers` lists `--issuer`;
2. mints an attestation for a fresh Ed25519 key (`POST /key-attestations`)
   and fails unless it is signed under the registry DID and carries a receipt;
3. publishes a public context under the attested key;
4. verifies it with `VerifiedContext` accepting this registry as attester,
   and fails unless it reads `CurrentlyAuthorized`. It also fails if a
   consumer accepting no attester verifies the same body.

It ends with `KEYLESS SMOKE OK <ctx_id>`. Refusals print the RFC-ACDP-0021
reason token.

The controller's `did.json` must list the identity in an
`AcdpKeylessSigning` service. `did-doc --keyless-issuer <iss>
--keyless-subject <sub>` adds one. For the reference deployment,
`did:web:acdp-smoke-producer.vercel.app` lists the GitHub Actions identity
of this repository's `main` branch:

```bash
doppler run --project acdp-registry --config prd -- \
  cargo run --release -- did-doc --did did:web:acdp-smoke-producer.vercel.app \
    --keyless-issuer https://token.actions.githubusercontent.com \
    --keyless-subject repo:agentcontextdistributionprotocol/acdp-registry-rs:ref:refs/heads/main
```

`.github/workflows/keyless-smoke.yml` runs the check from that identity
against the reference registry only (dispatch it on `main`). Every run leaves one active keyless context titled
`keyless smoke <timestamp>` behind. The ephemeral key cannot retract it
(keyless keys never sign lifecycle events); the controller's own key, the
`SMOKE_PRODUCER_SEED_B64` seed, can.
