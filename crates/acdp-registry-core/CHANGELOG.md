

## [0.1.3](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/compare/acdp-registry-core/v0.1.2...acdp-registry-core/v0.1.3) - 2026-09-13

### Added

- *(search)* return total_estimate to tenant-scoped callers, as that tenant's count ([#260](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/260))
- *(search)* scan inside the tenant so the cursor cannot anchor on a foreign row (H-H-w) ([#259](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/259))

### Fixed

- envelope the 415, minting `unsupported_media_type` per the owner's ruling ([#247](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/247))
- extractor rejections speak the §5 envelope, at their original status ([#245](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/245))
- unspoofable publish budget, and a rate-limit taxonomy that cannot drift ([#243](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/243))
- make the cache posture honest and split liveness from readiness ([#239](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/239))
- make x-request-id reach responses, and give the 413 a real envelope ([#235](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/235))
- clamp ?limit= so an unauthenticated search cannot abort the process ([#226](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/226))

### Other

- *(store)* name the disclosure parameter for what the predicate consumes ([#262](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/262))
- *(server)* rename the admin disclosure test to what it pins, and say at both sites why the handler is right ([#261](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/261))
- one visibility query per /log/entries page, and no cross-tenant count in /search ([#255](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/255))
- *(rate_limit,secure_compare,gate)* assert three properties that had tests named for them but no assertion of them (H-P) ([#254](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/254))
- unit H-C — five CI guards that were never guarding, and four claims that were false ([#227](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/227))


## [0.1.2](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/compare/acdp-registry-core/v0.1.1...acdp-registry-core/v0.1.2) - 2026-09-11

### Other

- update Cargo.toml dependencies


## [0.1.1](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/compare/acdp-registry-core/v0.1.0...acdp-registry-core/v0.1.1) - 2026-09-11

### Added

- *(http)* Cache-Control posture on requester-relative responses ([#205](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/205)) ([#219](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/219))


## [0.1.0](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/releases/tag/acdp-registry-core-v0.1.0) - 2026-06-13

### Added

- ACDP 0.2.0 trust hardening — registry receipts, did:key producers, lineage audit ([#39](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/39))
- *(auth,store)* admin pinned-key reload, tenant-bound tokens, durable revocation cursors, tenant-scoped pagination ([#21](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/21))
- *(auth)* JWT tenant claim is authoritative; X-Tenant-Id is fallback ([#19](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/19))
- extend tenant_id filter to search / lineage / list paths ([#18](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/18))
- tenant_id schema + opt-in tenant filter on contexts ([#17](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/17))
- *(auth)* EdDSA signing + JWKS endpoint (registry mirror of CP §2 follow-up) ([#15](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/15))
- *(registry-types)* key rotation with overlap windows ([#9](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/9)) ([#8](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/8))
- *(registry)* accept ECDSA-P256 in pinned-key enforcement ([#10](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/10)) ([#7](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/7))
- *(registry)* enforce playground pinned-key signatures
- *(registry)* harden auth, pagination, lineage, and operational surface
- initial acdp-registry workspace with hardened auth handshake

### Fixed

- *(core)* set application/acdp+json on framework-generated error responses ([#26](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/26))
- P0/P1 security and RFC-conformance remediation ([#24](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/24))
- *(core,auth)* enforce tenant isolation on publish and add strict tenant scoping ([#23](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/23))
- *(ci)* unblock build and broaden HTTP integration tests

### Other

- expand unit and integration coverage across all crates ([#38](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/38))
- Feat/registry remediation p0 p1 ([#22](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/22))
- unbreak Docker build and cargo-deny on PR pipeline
