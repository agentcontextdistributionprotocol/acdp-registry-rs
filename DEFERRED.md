# Deferred items

Open assumptions and escalations recorded in [ASSUMPTIONS.md](ASSUMPTIONS.md). **None has an owner, an SLA or an issue.** Each is
revisited only when it blocks a trigger in [STATUS.md](STATUS.md). Nothing here is a to-do list.

The authoritative, current list is the census tool's output, not this file:

```sh
python3 docs/assumptions-status-census.py --open
```

At `main` @ a222e90 it reports 33 open status declarations (some are historical text the tool cannot tell from a live status). They fall
into four groups:

| Group | Meaning | Where (ASSUMPTIONS.md) |
|---|---|---|
| Waiting on a human ruling | A decision only the project owner can make; the code ships a safe default. Includes the RFC §5 `code` for a 415 and the `playground.refuse_on_no_live_pin` question. | around lines 746, 950, 1439, 1593, 2000–2021 |
| Unfalsifiable from this repo | Depends on how a client, proxy or sibling repo behaves; cannot be tested here. | around lines 1162–1170, 1633, 2397, 2489 |
| Logged on 2026-10-04/05 rounds | Per-PR assumptions recorded when W1–W8, #372–#376 and the supply-chain/spec bumps shipped; no action pending. | around lines 4710–4751, 4898–4931 |
| Historical text | Entries whose status line was later settled but still match the census pattern. | e.g. line 578 |

When one of these is settled, edit its status in `ASSUMPTIONS.md` in the same PR as the change that settled it, and note the decision in
`DECISIONS.md`. Do not open a PR only to tidy a status line.
