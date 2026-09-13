

## [0.1.3](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/compare/acdp-registry-sqlite/v0.1.2...acdp-registry-sqlite/v0.1.3) - 2026-09-13

### Fixed

- answer a whole page of audit-log visibility checks in one query, on retrieve semantics ([#246](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/246))
- *(store)* scope search by tenant in SQL so a cursor cannot name a foreign tenant's row ([#240](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/240))
- fail loudly on corrupt contributors, widen pg version, index the search filters ([#233](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/233))
- stop serving a context as active alongside its own retraction event ([#232](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/232))
- *(sqlite)* adopt Postgres full-text semantics so q= means one thing ([#231](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/231))
- *(sqlite)* compare data_period bounds numerically, and add a cross-backend parity suite ([#229](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/229))

### Other

- *(store)* name the disclosure parameter for what the predicate consumes ([#262](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/262))


## [0.1.2](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/compare/acdp-registry-sqlite/v0.1.1...acdp-registry-sqlite/v0.1.2) - 2026-09-11

### Other

- update Cargo.toml dependencies


## [0.1.1](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/compare/acdp-registry-sqlite/v0.1.0...acdp-registry-sqlite/v0.1.1) - 2026-09-11

### Other

- update Cargo.toml dependencies


## [0.1.0](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/releases/tag/acdp-registry-sqlite-v0.1.0) - 2026-06-13

### Added

- ACDP 0.2.0 trust hardening — registry receipts, did:key producers, lineage audit ([#39](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/39))
- *(auth,store)* admin pinned-key reload, tenant-bound tokens, durable revocation cursors, tenant-scoped pagination ([#21](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/21))
- extend tenant_id filter to search / lineage / list paths ([#18](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/18))
- tenant_id schema + opt-in tenant filter on contexts ([#17](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/17))
- *(registry)* harden auth, pagination, lineage, and operational surface
- initial acdp-registry workspace with hardened auth handshake

### Fixed

- *(pg,sqlite)* uniform supersession not-found message (close existence oracle) ([#25](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/25))
- P0/P1 security and RFC-conformance remediation ([#24](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/24))

### Other

- expand unit and integration coverage across all crates ([#38](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/38))
- Feat/registry remediation p0 p1 ([#22](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/pull/22))
- unbreak Docker build and cargo-deny on PR pipeline
