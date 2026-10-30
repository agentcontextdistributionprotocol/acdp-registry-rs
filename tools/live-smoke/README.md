# live-smoke

An end-to-end check of a **deployed** registry, done the way a remote consumer
does it. It runs these steps in order:

1. `GET /.well-known/acdp.json`. Fails unless the registry advertises
   `acdp-registry-receipts` and `acdp-registry-lifecycle` and has
   `anonymous_public_reads` on. When it does not advertise
   `acdp-registry-keyless` (RFC-ACDP-0021), fails unless `GET` and
   `POST /key-attestations` both answer 501 `not_implemented`. When it does,
   the probe is skipped: minting an attestation needs a real workload token.
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
