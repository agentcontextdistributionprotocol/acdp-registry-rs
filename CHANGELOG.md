# Changelog

This workspace does not keep a single root changelog. Release notes are
per-crate, because releases are per-crate: `release-plz` tags and releases each
crate independently (`acdp-registry-server/v0.1.2`, and so on).

## Where the release notes are

| You want | Look here |
|----------|-----------|
| What changed in a released version of a crate | `crates/<crate>/CHANGELOG.md` — written by `release-plz`, one section per released version, each linked to its comparison diff. |
| The same notes, rendered, with artifacts | [GitHub Releases](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/releases) — one per crate per version. |
| *Why* a change was made — reasoning, rejected alternatives, evidence | [`docs/ENGINEERING-LOG.md`](docs/ENGINEERING-LOG.md). |
| Why a cross-cutting decision went the way it did | [`DECISIONS.md`](DECISIONS.md). |

## Where new entries go

Narrative entries go to [`docs/ENGINEERING-LOG.md`](docs/ENGINEERING-LOG.md).
**Do not add version sections to this file.** It was one until
[#220](https://github.com/agentcontextdistributionprotocol/acdp-registry-rs/issues/220):
it had grown to thousands of lines under a single `## [Unreleased]` heading
while three versions had already shipped, so it asserted that nearly all of its
own content was unreleased — the one artefact in the repo contradicting the
per-crate changelogs that were correct all along. Decision 15 in
`DECISIONS.md` records why it was retired rather than split, and
`root_changelog_stays_a_pointer` (`crates/acdp-registry-server/tests/conformance_gate.rs`)
fails the build if it starts drifting back.
