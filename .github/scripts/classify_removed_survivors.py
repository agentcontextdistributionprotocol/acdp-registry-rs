#!/usr/bin/env python3
"""Classify survivor lines that vanished from `mutants.out/missed.txt`.

WHY THIS EXISTS. `mutants.yml`'s ratchet compares `MUTANTS_SURVIVORS` against
`missed.txt` -- the survivor SUBSET. When a committed line stops appearing there,
the set difference knows only that it is gone. It cannot say whether a test now
KILLS that mutant (delete the line, the ratchet tightens) or whether the mutant is
alive and merely MOVED (rewrite the line; deleting it drops the budget for nothing
and loses a live survivor).

The gate was never silent about this -- a drift lands in BOTH `added` and
`removed`, and `rc=1` fires for either. What it did was editorialise: the error
text led with "Usually GOOD NEWS -- a test now kills it. Delete the line" and only
reached the drift caveat afterwards, handing the reader no evidence with which to
decide. This script supplies the evidence and names the edit.

THE KEY FACT. `outcomes.json` already records EVERY mutant in scope with its
verdict, not just the survivors, and `scenario.Mutant.name` is byte-identical to
the `cargo mutants --list` line that `MUTANTS_SURVIVORS` stores (verified 213/213
and 138/138, same set and same order). So the question is decidable from the
report the run already produced -- no extra `cargo` invocation, and in particular
no attempt to re-run the mutant in isolation, which cannot even FIND a mutant
whose line moved.

EVERY CLASSIFICATION STILL FAILS THE JOB, including a proven kill. The committed
list is a SET EQUALITY; letting a proven kill pass green would leave the list
disagreeing with `missed.txt` and nothing forcing it back into agreement, which
re-opens the hole the equality was installed to close. This script's job is to
tell the reader WHICH edit to make, not to excuse them from making it -- so every
branch names an edit that makes the next run green. A red check nobody can act on
is how a gate gets disabled.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from collections import defaultdict

# `<file>:<line>:<col>: <description>` -- verified to hold for 213/213 mutants.
NAME_RE = re.compile(r"^(?P<file>[^:]+):(?P<line>\d+):(?P<col>\d+): (?P<desc>.*)$")

# DELIBERATELY NOT 1 AND 2. An unhandled Python exception exits 1 and an argparse
# error exits 2, so using those would make a CRASHED classifier indistinguishable
# from a normal classification -- the caller would read "1" and print the routine
# message while the script had in fact died. These values are outside anything the
# interpreter produces on its own, so the workflow can treat every other non-zero
# code as "the classifier itself broke".
EXIT_OK = 0           # nothing removed; nothing to classify
EXIT_CLASSIFIED = 10  # at least one removed line classified -- an edit is required
EXIT_UNSOUND = 11     # cannot classify: the inputs themselves are broken


class Unsound(Exception):
    """The inputs cannot support a classification. Never guess past this."""


def err(msg: str) -> None:
    """GitHub Actions error annotation, one line, wrapped like the ratchet's own."""
    print(f"::error::{msg}")


def note(msg: str) -> None:
    print(msg)


def check_scope(total: int, expected: int | None, path: str) -> None:
    """Refuse a SHARDED report.

    `cargo mutants --shard N/K` writes a report whose `total_mutants` is that
    shard's share, and `len(records) == total_mutants` holds PER SHARD -- so the
    truncation check passes and every committed survivor belonging to a different
    shard looks like a line that "names no mutant in scope". Classified naively
    that is `NO CANDIDATE -> delete the line`, for nearly every survivor at once.
    The scope pin is what makes a shard detectable at all.
    """
    if expected is None or total == expected:
        return
    raise Unsound(
        f"this report covers {total} mutants but MUTANTS_EXPECTED_SCOPE is {expected} "
        f"({path}). If the run was SHARDED, no single shard can classify the committed "
        f"list: a survivor belonging to another shard is absent from this report and is "
        f"indistinguishable from one whose expression was deleted. Merge the shards into "
        f"one report before classifying, or classify per shard against a per-shard list. "
        f"If instead the scope genuinely changed, the scope-equality check is the one to "
        f"read -- do not delete survivor lines on the strength of this run."
    )


def load_mutants(path: str, label: str) -> tuple[list[dict], int]:
    """Return (mutant records, total_mutants) from a cargo-mutants report.

    `scenario` is an OBJECT {"Mutant": {...}} for mutants but the bare STRING
    "Baseline" for baseline records -- indexing it blindly raises TypeError on
    every real report, so the type guard is load-bearing, not defensive.
    """
    try:
        with open(path) as fh:
            doc = json.load(fh)
    except FileNotFoundError:
        raise Unsound(f"{label} does not exist: {path}")
    except json.JSONDecodeError as exc:
        raise Unsound(f"{label} is not valid JSON ({path}): {exc}")

    outcomes = doc.get("outcomes")
    if not isinstance(outcomes, list):
        raise Unsound(f"{label} has no 'outcomes' list ({path}); the report format changed")

    records = []
    for outcome in outcomes:
        scenario = outcome.get("scenario")
        if not isinstance(scenario, dict):
            continue  # the "Baseline" string
        mutant = scenario.get("Mutant")
        if not isinstance(mutant, dict):
            continue
        fn = mutant.get("function") or {}
        span = (mutant.get("span") or {}).get("start") or {}
        fn_span = ((fn.get("span") or {}).get("start")) or {}
        records.append(
            {
                "name": mutant.get("name"),
                "file": mutant.get("file"),
                "function": fn.get("function_name"),
                "genre": mutant.get("genre"),
                "replacement": mutant.get("replacement"),
                "line": span.get("line"),
                "column": span.get("column"),
                "fn_start": fn_span.get("line"),
                "summary": outcome.get("summary"),
            }
        )

    total = doc.get("total_mutants")
    if not isinstance(total, int):
        raise Unsound(f"{label} has no integer 'total_mutants' ({path})")

    # A TRUNCATED report must fail, not classify against a partial name set --
    # a missing name would be classified "names no mutant in scope", which reads
    # identically to a real drift.
    # BOTH directions: fewer records than reported is a truncated write;
    # MORE is two reports merged (e.g. a glob), where name-keyed lookup is
    # already unsound. A one-sided `<` would pass the merge case silently.
    if len(records) != total:
        raise Unsound(
            f"{label} lists {len(records)} mutants but reports total_mutants={total} "
            f"({path}). The report is truncated or internally inconsistent; a partial "
            f"name set would misclassify every missing name as drift."
        )

    names = [r["name"] for r in records]
    if len(set(names)) != len(names):
        dupes = sorted({n for n in names if names.count(n) > 1})
        raise Unsound(
            f"{label} contains duplicate mutant names ({path}), e.g. {dupes[:3]}. "
            f"Name-keyed lookup is unsound here. If you pointed this at a GLOB of "
            f"ledgers, point it at exactly one: docs/mutation-runs/ holds VOID and "
            f"SUPERSEDED ledgers whose verdicts contradict the current ones."
        )
    return records, total


def offset_key(r: dict):
    """Identity anchored to the enclosing function's start line.

    Survives insertions BETWEEN functions (the whole function moves as a unit)
    and breaks on insertions INSIDE one.
    """
    if r["line"] is None or r["fn_start"] is None:
        return None
    return (r["file"], r["function"], r["genre"], r["replacement"],
            r["line"] - r["fn_start"], r["column"])


def ordinal_keys(records: list[dict]) -> dict[int, tuple]:
    """Identity anchored to position within the function, by (line, column).

    Survives insertions INSIDE a function and breaks when a mutant is added to or
    removed from it -- i.e. it fails on a DIFFERENT edit than `offset_key`, which
    is the entire reason both are computed and required to agree.
    """
    by_fn = defaultdict(list)
    for idx, r in enumerate(records):
        by_fn[(r["file"], r["function"])].append(idx)
    out = {}
    for group in by_fn.values():
        group.sort(key=lambda i: (records[i]["line"] or 0, records[i]["column"] or 0))
        for ordinal, i in enumerate(group):
            r = records[i]
            out[i] = (r["file"], r["function"], r["genre"], r["replacement"], ordinal)
    return out


def describe(name: str):
    """(file, description) -- the line-independent fallback. Deliberately COARSE.

    Measured: this collapses 351 mutants to 312 keys. It merges the three
    `delete ! in run_search_with_refill` mutants, which hold three DIFFERENT
    verdicts (Caught / Missed / Timeout). It is therefore only ever used to
    OFFER candidates to a human, never to conclude a pairing.
    """
    m = NAME_RE.match(name)
    return (m.group("file"), m.group("desc")) if m else None


# What to do about a verdict this script has never seen. cargo-mutants may add
# summary variants; landing here must still tell the reader something actionable
# rather than the word "Investigate".
UNKNOWN_ACTION = (
    "This script does not know this verdict, so it will not tell you to delete "
    "anything. Read mutants.out/outcomes.json for this mutant, decide whether the "
    "verdict means KILLED, and teach VERDICT_ACTIONS in "
    ".github/scripts/classify_removed_survivors.py about it IN THE SAME COMMIT."
)

# Verdict -> (label, demanded edit). Every entry names an edit; see module docstring.
VERDICT_ACTIONS = {
    "CaughtMutant": (
        "KILLED (proven by this run)",
        "Delete the line AND its # reason from MUTANTS_SURVIVORS. The budget drops "
        "by one -- it is derived from the list length, so do not edit a count.",
    ),
    "Timeout": (
        "TIMED OUT -- not a kill",
        "A timeout is an UNRESOLVED verdict, not a kill: the mutant's fate is "
        "unknown. Do NOT delete the line. Either make the test terminate, or move "
        "this mutant to MUTANTS_TIMEOUT_BUDGET's named list with a written reason.",
    ),
    "Unviable": (
        "UNVIABLE -- not a kill",
        "The mutant no longer compiles, so the surrounding code changed shape. "
        "Delete the line and record IN THE SAME COMMIT what change made it "
        "unviable -- an unviable mutant is a gap in the oracle, not a test win.",
    ),
    "MissedMutant": (
        "CONTRADICTION",
        "outcomes.json says this mutant is MissedMutant, but it is absent from "
        "missed.txt. The report contradicts itself; fix the run. Do not pick "
        "whichever of the two enumerations is convenient.",
    ),
}


def classify(removed, current, prior):
    """Classify each removed line. Returns the number classified."""
    cur_by_name = {r["name"]: r for r in current}

    cur_by_offset, cur_by_ordinal = defaultdict(list), defaultdict(list)
    cur_ordinals = ordinal_keys(current)
    for i, r in enumerate(current):
        k = offset_key(r)
        if k:
            cur_by_offset[k].append(r)
        cur_by_ordinal[cur_ordinals[i]].append(r)

    prior_by_name = {r["name"]: r for r in prior} if prior else {}
    prior_ordinals = ordinal_keys(prior) if prior else {}
    prior_ordinal_by_name = (
        {prior[i]["name"]: k for i, k in prior_ordinals.items()} if prior else {}
    )

    cur_by_desc = defaultdict(list)
    for r in current:
        d = describe(r["name"] or "")
        if d:
            cur_by_desc[d].append(r)

    for line in removed:
        note("")
        # The workflow already printed the raw list; this is the per-line verdict.
        err(f"classifying:  - {line}")

        # --- Case 1: the identifier still names a mutant in this scope. -------
        hit = cur_by_name.get(line)
        if hit:
            label, action = VERDICT_ACTIONS.get(
                hit["summary"], (f"UNKNOWN VERDICT {hit['summary']!r}", UNKNOWN_ACTION)
            )
            err(f"  {label}: the mutant still exists at this exact site and this "
                f"run recorded {hit['summary']}.")
            err(f"  ACTION: {action}")
            continue

        # --- Case 2: it names no mutant in scope. NEVER a kill. --------------
        err("  NOT A KILL -- this identifier names no mutant in the current scope. "
            "The count going down is not evidence; a mutant whose line moved "
            "disappears from missed.txt exactly like one that was killed.")

        # A line that does not even PARSE is a different failure from a line whose
        # mutant vanished, and it must never be told to delete: the committed text
        # is what is broken, not the source. Checked BEFORE pairing, because a
        # malformed line cannot be looked up in anything.
        if describe(line) is None:
            err("  MALFORMED -- this is not a `cargo mutants --list` line, so it "
                "identifies no mutant and never could. Expected "
                "`<file>:<line>:<col>: <description>`.")
            err("  ACTION: restore the line to the exact verbatim `cargo mutants --list` "
                "text. Do NOT delete it -- a line that cannot be parsed is not evidence "
                "that anything was killed.")
            continue

        prior_rec = prior_by_name.get(line)
        paired, how = None, None
        if prior_rec is not None:
            ok, ordk = offset_key(prior_rec), prior_ordinal_by_name.get(line)
            by_off = cur_by_offset.get(ok, []) if ok else []
            by_ord = cur_by_ordinal.get(ordk, []) if ordk else []
            # THE `== 1` IS THE LAST GUARD BEFORE A WRONG DELETION. Two mutants
            # can share an offset key when a file holds two same-named functions
            # (ordinary in Rust: one `f` per impl block). Taking by_off[0] there
            # picks one arbitrarily -- and if that one happens to be CaughtMutant
            # while the real match is still MissedMutant, the output flips from
            # "decide by hand" to "DELETE the line", against a live survivor.
            off1 = by_off[0] if len(by_off) == 1 else None
            # by_ord is structurally at most one element -- ordinals are unique
            # within a (file, function) group, so the key cannot collide. The
            # length check is kept anyway so this code does not silently depend
            # on that invariant if the key definition ever changes.
            ord1 = by_ord[0] if len(by_ord) == 1 else None

            if off1 and ord1 and off1["name"] == ord1["name"]:
                paired, how = off1, "both keys agree"
            elif off1 and ord1:
                # A GENUINE disagreement: both keys resolved, to different mutants.
                err("  the two identity keys resolved to DIFFERENT mutants, so neither "
                    "pairing is trusted:")
                err(f"    [offset ] {off1['name']}  ({off1['summary']})")
                err(f"    [ordinal] {ord1['name']}  ({ord1['summary']})")
                err("  ACTION: open mutants.out/diff/ for both and decide by hand. Do "
                    "NOT delete the line on the strength of either.")
                continue
            elif off1 or ord1:
                # Exactly ONE key survived the edit. This is NOT a disagreement --
                # the keys are designed to fail on different edits, so an edit
                # inside a function breaks `offset` while `ordinal` holds, and an
                # edit that adds a mutant to a function does the reverse. Pair on
                # the survivor and SAY which one, so the reader can weigh it.
                paired = off1 or ord1
                how = ("offset key only -- the ordinal key found no match, which is "
                       "what an edit that ADDS or REMOVES a mutant in this function "
                       "looks like") if off1 else (
                       "ordinal key only -- the offset key found no match, which is "
                       "what an edit INSIDE this function looks like")
            elif by_off or by_ord:
                # At least one key matched several mutants: ambiguous, not paired.
                err("  an identity key matched MORE THAN ONE mutant, so the pairing is "
                    "not unique:")
                for lbl, cands in (("offset ", by_off), ("ordinal", by_ord)):
                    for c in cands:
                        err(f"    [{lbl}] {c['name']}  ({c['summary']})")
                err("  ACTION: open mutants.out/diff/ for these and decide by hand.")
                continue
        elif prior:
            err("  NOTE: this line is absent from the prior ledger too, so its identity "
                "cannot be computed at all. Falling back to a coarse match.")

        if paired is not None:
            err(f"  paired via: {how}")
            if paired["summary"] == "MissedMutant":
                err("  DRIFT-STILL-MISSED: the same mutant is alive at a new location.")
                err(f"    - {line}")
                err(f"    + {paired['name']}")
                err("  ACTION: REWRITE the line in MUTANTS_SURVIVORS to the new text, "
                    "keeping its # reason. Do NOT delete it -- the budget must not "
                    "move, because nothing was killed.")
            elif paired["summary"] == "CaughtMutant":
                err("  DRIFT-NOW-CAUGHT: the mutant moved AND is now killed -- both "
                    "happened in the same window, which is why the line vanished.")
                err(f"    - {line}")
                err(f"    + {paired['name']}  ({paired['summary']})")
                err("  ACTION: DELETE the line and its # reason. The budget drops by one.")
            else:
                label, action = VERDICT_ACTIONS.get(
                    paired["summary"],
                    (f"UNKNOWN VERDICT {paired['summary']!r}", UNKNOWN_ACTION))
                err(f"  DRIFTED, and the mutant at its new location is "
                    f"{paired['summary']} -- {label}, not a kill.")
                err(f"    + {paired['name']}")
                err(f"  ACTION: {action}")
            continue

        # --- Stage 2: coarse fallback. INFERENCE, and labelled as such. ------
        d = describe(line)
        cands = cur_by_desc.get(d, []) if d else []
        if len(cands) == 1:
            c = cands[0]
            err("  PROBABLE DRIFT (INFERENCE, not proof -- matched only on "
                "file+description, which cannot tell sibling mutants apart):")
            err(f"    + {c['name']}  ({c['summary']})")
            err("  ACTION: confirm against mutants.out/diff/ before editing.")
        elif cands:
            err(f"  AMBIGUOUS -- {len(cands)} mutants share this file+description and "
                f"are indistinguishable from the committed text:")
            for c in cands:
                err(f"    + {c['name']}  ({c['summary']})")
            err("  ACTION: open mutants.out/diff/ for each and choose by hand.")
        else:
            err("  NO CANDIDATE: the mutable expression no longer exists in the "
                "source at all.")
            err("  ACTION: delete the line and record IN THE SAME COMMIT what "
                "removed the expression.")

    return len(removed)


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--outcomes", required=True, help="this run's mutants.out/outcomes.json")
    ap.add_argument("--removed-file", required=True,
                    help="committed survivor lines absent from missed.txt, one per line")
    ap.add_argument("--prior-ledger", default=None,
                    help="ONE committed ledger to pair drifted lines against. Never a glob.")
    ap.add_argument("--expected-scope", type=int, default=None,
                    help="MUTANTS_EXPECTED_SCOPE. Refuses a sharded report, which would "
                         "otherwise classify other shards' survivors as deleted.")
    ap.add_argument("--committed-file", default=None,
                    help="the full committed survivor list, to check the prior ledger is "
                         "fresh enough to pair against.")
    args = ap.parse_args(argv)

    try:
        with open(args.removed_file) as fh:
            removed = [ln.rstrip("\n").rstrip() for ln in fh]
        removed = [ln for ln in removed if ln.strip()]
    except FileNotFoundError:
        err(f"--removed-file does not exist: {args.removed_file}")
        return EXIT_UNSOUND

    if not removed:
        note("no removed survivor lines to classify")
        return EXIT_OK

    try:
        current, cur_total = load_mutants(args.outcomes, "this run's report")
        check_scope(cur_total, args.expected_scope, args.outcomes)
        prior = []
        if args.prior_ledger:
            prior, _ = load_mutants(args.prior_ledger, "the prior ledger")
            # FRESHNESS. Pairing can only ever work for a line the prior ledger
            # actually contains. Without this gate the first edit to
            # MUTANTS_SURVIVORS that forgets to commit a matching ledger degrades
            # every future drift to a coarse guess -- silently, while the wiring
            # still looks correct. Checked over the WHOLE committed list, not just
            # the removed lines, because the staleness predates any one removal.
            # ONLY GATE WHEN PAIRING IS ACTUALLY NEEDED. A line whose mutant is
            # still in scope is classified from this run's verdict alone and never
            # touches the prior ledger, so blocking it on a stale ledger would
            # withhold a provable answer and leave the ratchet unactionable.
            needs_pairing = [l for l in removed if l not in {r["name"] for r in current}]
            if args.committed_file and needs_pairing:
                with open(args.committed_file) as fh:
                    committed = [l.strip() for l in fh if l.strip()
                                 and not l.lstrip().startswith("#")]
                names = {r["name"] for r in prior}
                missing = [c for c in committed if c not in names]
                if missing:
                    raise Unsound(
                        f"STALE PRIOR LEDGER: {len(missing)} of {len(committed)} committed "
                        f"survivor line(s) do not appear in {args.prior_ledger}, so a "
                        f"drifted line cannot be paired to its new location and would "
                        f"degrade to a coarse guess. First missing: {missing[0]!r}. "
                        f"Commit the outcomes.json from the run that produced the current "
                        f"MUTANTS_SURVIVORS and point MUTANTS_PRIOR_LEDGER at it, in the "
                        f"same commit as any survivor-list edit."
                    )
        else:
            err("no --prior-ledger given, so a drifted line cannot be paired to its "
                "new location. Set MUTANTS_PRIOR_LEDGER to exactly one committed "
                "docs/mutation-runs/*-outcomes.json.")
    except Unsound as exc:
        err(str(exc))
        err("Refusing to classify against inputs that cannot support a conclusion.")
        return EXIT_UNSOUND

    classify(removed, current, prior)
    note("")
    note(f"classified {len(removed)} removed survivor line(s); each requires an edit")
    return EXIT_CLASSIFIED


if __name__ == "__main__":
    sys.exit(main())
