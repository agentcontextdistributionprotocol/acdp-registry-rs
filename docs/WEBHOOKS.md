# Webhooks

When `[webhook] enabled = true`, the registry POSTs HMAC-signed JSON events to a
configured receiver. Delivery is **best-effort and non-blocking** — events go to
a bounded in-memory queue and are delivered by a background worker; the HTTP
request that triggered an event never waits on (or fails because of) webhook
delivery. Implementation: `crates/acdp-registry-webhook/src/lib.rs`; event types:
`crates/acdp-registry-types/src/event.rs`.

## Events

Five event types. Each is delivered as a flattened envelope: the variant fields
plus `event_id`, `schema_version`, and a `type` discriminator.

Note the `type` value is snake_case (`context_published`), while the
`X-ACDP-Event` header uses the dotted form (`context.published`). Both name the
same event; the header is what you route on without parsing the body.

### `context.published`

```json
{
  "event_id": "e0f1...",
  "schema_version": "1.0",
  "type": "context_published",
  "registry_authority": "registry.example.com",
  "registry_base_url": "https://registry.example.com",
  "ctx_id": "acdp://registry.example.com/1234...",
  "lineage_id": "lin:sha256:9f2b...",
  "agent_id": "did:web:agents.example.com:my-agent",
  "context_type": "observation",
  "visibility": "public",
  "version": 1,
  "created_at": "2026-06-10T12:00:00Z",
  "derived_from": ["acdp://registry.example.com/5678..."],
  "run_id": "run-123",
  "key_fingerprint": "sha256:139e…",
  "registry_receipt": { "registry_did": "did:web:registry.example.com", "…": "…" }
}
```

`registry_base_url` lets a federation control plane bootstrap proxy routes;
`run_id` echoes the publisher's `X-Run-Id` header (omitted if absent).

`key_fingerprint` and `registry_receipt` (ACDP 0.2.0, additive — both omitted
on a receipt-less registry) carry the RFC-ACDP-0010 §6 fingerprint of the
producer key the registry actually resolved at publish time, and the full
signed receipt, so the control plane can correlate and re-verify without
re-fetching the context.

### `context.retrieved`

```json
{
  "event_id": "e0f3...",
  "schema_version": "1.0",
  "type": "context_retrieved",
  "registry_authority": "registry.example.com",
  "ctx_id": "acdp://registry.example.com/1234...",
  "requester_did": "did:web:agents.example.com:reader",
  "at": "2026-06-10T12:01:00Z"
}
```

`requester_did` is `null` for anonymous reads.

### `context.retracted`

A context was formally retracted (RFC-ACDP-0013 §6). **Mark-not-delete:** the body
stays retrievable, `status` becomes `retracted`, and the context drops out of
default searches and `/current`.

```json
{
  "event_id": "e0f5...",
  "schema_version": "1.0",
  "type": "context_retracted",
  "registry_authority": "registry.example.com",
  "ctx_id": "acdp://registry.example.com/1234...",
  "lineage_id": "lin:sha256:9f2b...",
  "actor": "did:web:agents.example.com:my-agent",
  "lifecycle_event_id": "0195a1c2-...",
  "reason": "superseded by a newer context",
  "at": "2026-06-10T12:03:00Z"
}
```

`actor` is the DID of the party that performed the retraction, and it is **not
always the producer**. For producer-submitted events it is the producer
(`body.agent_id`); for a registry-initiated retraction via
`POST /admin/contexts/:id/retract` it is the **registry's own DID**. Retract and
republish on one context may legitimately carry *different* actors — see
[HTTP-API.md](HTTP-API.md) on lifecycle transitions. Do not assume
`actor == agent_id`.

`lifecycle_event_id` is the **actor-minted** lifecycle event id (an RFC 9562
UUID) taken from the signed lifecycle event. It is *not* the same thing as the
envelope's `event_id`, which is minted per delivery and is what you dedupe on —
see [Two ids, and why they have different names](#two-ids-and-why-they-have-different-names).

`reason` is optional and is **omitted entirely** when absent, never `null`.

`at` is stamped when the webhook is constructed — it is the registry's
*delivery-side* clock, **not** the `occurred_at` of the signed lifecycle event.
The two are close but not equal, and neither is authoritative for the other. To
correlate a delivery with the signed event, match on `lifecycle_event_id`, never
on `at`.

### `context.republished`

A prior retraction was reversed (RFC-ACDP-0013 §6). `status` re-derives as though
the context had never been retracted; both events stay in the lineage history.

```json
{
  "event_id": "e0f2...",
  "schema_version": "1.0",
  "type": "context_republished",
  "registry_authority": "registry.example.com",
  "ctx_id": "acdp://registry.example.com/1234...",
  "lineage_id": "lin:sha256:9f2b...",
  "actor": "did:web:agents.example.com:my-agent",
  "lifecycle_event_id": "0195a1c3-...",
  "at": "2026-06-10T12:04:00Z"
}
```

Same fields as `context.retracted`; this example omits the optional `reason` to
show the omitted form.

#### Two ids, and why they have different names

These two event types are the only ones carrying an id of their own, and until
`#179` both ids were called `event_id` — the envelope's and the variant's. The
envelope serialises first and the flattened variant second, so the key appeared
**twice** in one object and receivers resolved it by parser accident: last-wins
(`serde_json`, JavaScript `JSON.parse`, Python `json`) saw the lifecycle id,
first-wins implementations saw the envelope id. One of the two meanings was
always silently lost.

They are now distinct on the wire:

| key | minted by | stable across retries | use it for |
|-----|-----------|----------------------|------------|
| `event_id` | the registry, per delivery | yes | de-duplicating deliveries |
| `lifecycle_event_id` | the actor, in the signed lifecycle event | yes | correlating with the signed event / lineage history |

`event_id` is also echoed in the `X-ACDP-Event-Id` header, so a receiver that
dedupes on the header needs no change at all.

**If you consume these two event types:** a receiver that read `event_id` and
relied on last-wins was reading the *lifecycle* id. It now reads the *delivery*
id. Read `lifecycle_event_id` instead to keep the old value.

`schema_version` deliberately stays `"1.0"`. It describes the **envelope**, which
did not change. It is also stamped on each *delivery*, so it says something about
that delivery rather than about the event stream — a `search.executed` body
carrying a bumped version would assert that something about *that* delivery
changed, when nothing did. Do not expect it to move for a per-variant field
change, however many variants that change touches; those are recorded here.

### `search.executed`

```json
{
  "event_id": "e0f4...",
  "schema_version": "1.0",
  "type": "search_executed",
  "registry_authority": "registry.example.com",
  "query": "weather",
  "result_count": 12,
  "requester_did": null,
  "at": "2026-06-10T12:02:00Z"
}
```

## Wire change history

`schema_version` tracks the **envelope** and moves only when the envelope shape
changes. Per-variant field changes do not move it, no matter how many variants
they touch: the value is stamped per *delivery*, so bumping it would assert that
something about that delivery's envelope changed, when the envelope is exactly
what did not change. Those changes are recorded here instead. Newest first.

| change | affects | `schema_version` |
|--------|---------|------------------|
| `#179` — the actor-minted lifecycle id moved from `event_id` to `lifecycle_event_id`, ending a duplicate JSON key. See [Two ids, and why they have different names](#two-ids-and-why-they-have-different-names). | `context.retracted`, `context.republished` | unchanged, `1.0` |

## Signature scheme

Matches GitHub's exactly — receivers that already verify GitHub webhooks reuse
the same code. Headers on every delivery:

```
Content-Type:      application/json
X-ACDP-Signature:  sha256=<hex of HMAC-SHA256(webhook.secret, raw_json_body)>
X-ACDP-Event:      context.published | context.retrieved | context.retracted
                   | context.republished | search.executed
X-ACDP-Event-Id:   <uuid, stable across retries>
X-Tenant-Id:       <tenant, when the event is tenant-scoped>
```

Verify by recomputing the HMAC over the **raw** request body bytes (not a
re-serialization) and comparing in constant time. Reject on mismatch.

> The signing input is the exact bytes posted. Don't parse-and-reserialize
> before verifying — key ordering or whitespace differences will break the MAC.

## Delivery and retries

- Events are queued on a bounded mpsc channel of `webhook.queue_capacity`
  (default 1024). When the queue is full the event is **dropped** with a warn
  log — back-pressure never blocks the request path.
- The worker retries on `429`, `5xx`, and transport errors with exponential
  backoff starting at 250 ms, doubling, capped at 15 s, up to
  `webhook.max_retries` (default 3).
- A non-429 `4xx` is treated as a permanent failure — the worker gives up
  immediately (the receiver rejected the payload; retrying won't help).
- `2xx` is success. After exhausting retries the failure is logged and dropped
  (fire-and-forget).

Queue depth is exposed operationally at `GET /admin/status`
(`webhook.queue_in_flight` / `queue_capacity`) — see
[HTTP-API.md](HTTP-API.md#get-adminstatus).

## SSRF protection

`webhook.url` is validated at startup against the same `SsrfPolicy` as DID and
cross-registry resolution: HTTPS only, no private/internal authorities, no
redirects to such (the policy is documented in
[acdp-rs · Security Model](https://github.com/agentcontextdistributionprotocol/acdp-rs/blob/main/docs/security.md)).
A webhook config that fails validation aborts startup rather than silently
disabling delivery.

## Configuration

```toml
[webhook]
enabled         = true
url             = "https://example.com/hooks/acdp"
secret          = "<random shared secret, non-empty>"
timeout_seconds = 5
max_retries     = 3
queue_capacity  = 1024
```

See [CONFIGURATION.md](CONFIGURATION.md#webhook) for the field reference.
