

## [0.1.3](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/compare/acdp-registry-types/v0.1.2...acdp-registry-types/v0.1.3) - 2026-09-13

### Other

- *(store)* name the disclosure parameter for what the predicate consumes ([#262](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/262))
- test(types) + docs(decisions): the hash_mismatch arm, and a cargo-mutants feasibility measurement (H-M, #216) ([#252](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/252))


## [0.1.2](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/compare/acdp-registry-types/v0.1.1...acdp-registry-types/v0.1.2) - 2026-09-11

### Other

- update Cargo.toml dependencies


## [0.1.1](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/compare/acdp-registry-types/v0.1.0...acdp-registry-types/v0.1.1) - 2026-09-11

### Other

- close #190 and #191, and repair four pins nobody had swept ([#206](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/206))


## [0.1.0](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/releases/tag/acdp-registry-types-v0.1.0) - 2026-06-13

### Added

- ACDP 0.2.0 trust hardening — registry receipts, did:key producers, lineage audit ([#39](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/39))
- *(auth,store)* admin pinned-key reload, tenant-bound tokens, durable revocation cursors, tenant-scoped pagination ([#21](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/21))
- *(auth)* cross-issuer revocation poller (consumer of CP feed) ([#20](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/20))
- *(auth)* JWT tenant claim is authoritative; X-Tenant-Id is fallback ([#19](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/19))
- *(auth)* EdDSA signing + JWKS endpoint (registry mirror of CP §2 follow-up) ([#15](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/15))
- *(registry-types)* key rotation with overlap windows ([#9](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/9)) ([#8](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/8))
- *(registry)* enforce playground pinned-key signatures
- *(registry)* harden auth, pagination, lineage, and operational surface
- initial acdp-registry workspace with hardened auth handshake

### Fixed

- *(config)* apply ACDP_REGISTRY_* env overrides and unblock container boot ([#35](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/35))
- P0/P1 security and RFC-conformance remediation ([#24](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/24))
- *(core,auth)* enforce tenant isolation on publish and add strict tenant scoping ([#23](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/23))

### Other

- expand unit and integration coverage across all crates ([#38](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/38))
- Feat/registry remediation p0 p1 ([#22](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/22))
