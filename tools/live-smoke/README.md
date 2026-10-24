# live-smoke

An end-to-end check of a **deployed** registry, done the way a remote consumer
does it. It runs these steps in order:

1. `GET /.well-known/acdp.json`. Fails unless the registry advertises
   `acdp-registry-receipts` and `acdp-registry-lifecycle` and has
   `anonymous_public_reads` on.
2. Publishes a `public` context, signed by a `did:web` producer. Fails unless
   the response carries a `registry_receipt`.
3. Resolves the context through `acdp::client::CrossRegistryResolver`. The
   resolver is configured like Seam's `seam-context-acdp`: an SSRF policy, an
   authority allowlist, and fail-closed RFC-ACDP-0014 revocation discovery.
   Fails unless the receipt verifies and the context is `not_retracted`.
4. Posts a producer-signed `retracted` lifecycle event (RFC-ACDP-0013).
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
