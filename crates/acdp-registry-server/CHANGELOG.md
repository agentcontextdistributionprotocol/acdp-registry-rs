

## [0.1.1](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/compare/acdp-registry-server/v0.1.0...acdp-registry-server/v0.1.1) - 2026-09-11

### Added

- *(http)* Cache-Control posture on requester-relative responses ([#205](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/205)) ([#219](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/219))

### Fixed

- *(docker)* ship no jwt_secret so the quickstart actually boots (W3-U5) ([#211](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/211))

### Other

- build every valid feature configuration of the server binary ([#200](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/200)) ([#222](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/222))
- *(conformance)* reclassify rcpt/lhr/log as EXCUSED, closing #130 ([#217](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/217))
- close #190 and #191, and repair four pins nobody had swept ([#206](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/206))


## [0.1.0](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/releases/tag/acdp-registry-server-v0.1.0) - 2026-06-13

### Added

- ACDP 0.2.0 trust hardening — registry receipts, did:key producers, lineage audit ([#39](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/39))
- *(auth,store)* admin pinned-key reload, tenant-bound tokens, durable revocation cursors, tenant-scoped pagination ([#21](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/21))
- *(auth)* cross-issuer revocation poller (consumer of CP feed) ([#20](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/20))
- extend tenant_id filter to search / lineage / list paths ([#18](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/18))
- tenant_id schema + opt-in tenant filter on contexts ([#17](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/17))
- *(auth)* EdDSA signing + JWKS endpoint (registry mirror of CP §2 follow-up) ([#15](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/15))
- *(registry-types)* key rotation with overlap windows ([#9](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/9)) ([#8](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/8))
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
