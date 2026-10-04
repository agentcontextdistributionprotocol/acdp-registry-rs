#!/usr/bin/env python3
"""PR-time drift check for the mutation ratchet's pins (#384).

WHY THIS EXISTS. `mutants.yml` is `schedule:` + `workflow_dispatch` only: a full
mutation run takes ~2 hours, and #216 lists blocking PRs on it as a non-goal. But
three of its pins can be checked WITHOUT running a single mutant, because they are
statements about the mutant LISTING, not about verdicts:

  1. `MUTANTS_EXPECTED_SCOPE` -- must EQUAL `cargo mutants --list | wc -l`;
  2. the per-file table in `.cargo/mutants.toml` -- must equal the listing's
     per-file split (it is prose, so nothing else ever fails when it goes stale);
  3. every `MUTANTS_SURVIVORS` line, and every TIMEOUT recorded in
     `MUTANTS_PRIOR_LEDGER` (the budgeted one), must appear in the listing
     VERBATIM. Each is a `file:line:col: description` string, so an edit that
     only moves code in a scoped file breaks it.

Any of these going stale turns the following Monday's cron red, detached from the
PR that caused it. `cargo mutants --list` parses source only (no build), so this
check costs seconds and can run on the PR instead.

WHAT IT CANNOT SEE. Whether a mutant SURVIVES. A PR that adds a mutant nothing
kills, or kills a committed survivor, still only shows up on the cron. This
check removes the mechanical failures (counts and line numbers), which are the
ones that have actually turned the cron red (#341, #371, #373-#376).

Subcommands:
  check     --listing FILE   compare a saved `cargo mutants --list` to the pins.
  relevant  --changed-files FILE   print `true` if any changed path can move a
            pin, else `false`. Used by the PR job to skip the check (in-step,
            so the job still reports a status) on PRs that cannot affect it.

python3 + PyYAML only (preinstalled on GitHub runners; `pip3 install --user
pyyaml` on a stock macOS python3). Python 3.9 compatible, so no `tomllib`:
`examine_globs` is read with a narrow regex that FAILS LOUD if it cannot find it.
"""

from __future__ import annotations

import argparse
import fnmatch
import json
import os
import re
import sys
from collections import Counter

import yaml

# Same exit-code convention as classify_removed_survivors.py, for the same
# reason: an unhandled exception exits 1 and an argparse error exits 2, so those
# values must never mean "drift found" -- a crashed checker would read as a
# routine finding.
EXIT_OK = 0
EXIT_DRIFT = 10    # the pins disagree with the listing: an edit is required
EXIT_UNSOUND = 11  # the inputs cannot support a conclusion (missing/malformed)

ROOT = os.path.abspath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", ".."))
WORKFLOW_REL = ".github/workflows/mutants.yml"
CONFIG_REL = ".cargo/mutants.toml"

# Paths, besides the scoped sources themselves, whose edit can move a pin or this
# check's verdict. Kept as patterns so a new ledger or a renamed test is covered.
ALWAYS_RELEVANT = (
    CONFIG_REL,
    WORKFLOW_REL,
    ".github/workflows/mutants-pins.yml",
    ".github/scripts/check_mutants_pins.py",
    ".github/scripts/test_check_mutants_pins.py",
    "docs/mutation-runs/*",
)

# `<file>:<line>:<col>: <description>` -- the shape of every `--list` line.
NAME_RE = re.compile(r"^(?P<file>[^:]+):(?P<line>\d+):(?P<col>\d+): (?P<desc>.+)$")
# One row of the per-file table: `#     crates/x/src/y.rs   144`.
TABLE_ROW_RE = re.compile(r"^#\s+(?P<file>crates/\S+\.rs)\s+(?P<n>\d+)\s*$")
TABLE_TOTAL_RE = re.compile(r"^#\s+total\s+(?P<n>\d+)\s*$")
GLOBS_RE = re.compile(r"^examine_globs\s*=\s*\[(?P<body>.*?)\]", re.S | re.M)


class Unsound(Exception):
    """The inputs cannot support a conclusion. Never guess past this."""


def err(msg: str) -> None:
    print("::error::" + msg)


# --------------------------------------------------------------------------
# Readers. Each raises Unsound rather than returning an empty value, because an
# empty pin compared against an empty listing is a vacuous pass.
# --------------------------------------------------------------------------

def read_workflow_env(text: str) -> dict:
    try:
        doc = yaml.safe_load(text)
    except yaml.YAMLError as e:
        raise Unsound("mutants.yml is not valid YAML: %s" % e)
    env = (doc or {}).get("env") if isinstance(doc, dict) else None
    if not isinstance(env, dict):
        raise Unsound("mutants.yml has no top-level `env:` mapping")
    for key in ("MUTANTS_EXPECTED_SCOPE", "MUTANTS_SURVIVORS", "MUTANTS_TIMEOUT_BUDGET",
                "MUTANTS_PRIOR_LEDGER"):
        if key not in env:
            raise Unsound("mutants.yml's env does not declare %s" % key)
    for key in ("MUTANTS_EXPECTED_SCOPE", "MUTANTS_TIMEOUT_BUDGET"):
        if not re.fullmatch(r"\d+", str(env[key]).strip()):
            raise Unsound("%s is not an integer: %r" % (key, env[key]))
    return env


def committed_survivors(block: str) -> list:
    """The workflow's own normalisation: strip trailing whitespace, drop `#`
    lines and blanks (mutants.yml's `survivors_expected`)."""
    out = []
    for line in str(block).splitlines():
        line = line.rstrip()
        if not line or line.lstrip().startswith("#"):
            continue
        out.append(line)
    return out


def read_config(text: str):
    """(examine_globs, {file: count}, total) from .cargo/mutants.toml."""
    m = GLOBS_RE.search(text)
    if not m:
        raise Unsound("could not find `examine_globs = [...]` in %s" % CONFIG_REL)
    globs = re.findall(r'"([^"]+)"', m.group("body"))
    if not globs:
        raise Unsound("`examine_globs` in %s is empty" % CONFIG_REL)
    table, totals = {}, []
    for line in text.splitlines():
        row = TABLE_ROW_RE.match(line)
        if row:
            f = row.group("file")
            if f in table:
                raise Unsound("the per-file table in %s lists %s twice" % (CONFIG_REL, f))
            table[f] = int(row.group("n"))
            continue
        tot = TABLE_TOTAL_RE.match(line)
        if tot:
            totals.append(int(tot.group("n")))
    if not table:
        raise Unsound("found no per-file table rows (`#   crates/.../x.rs   N`) in %s" % CONFIG_REL)
    if len(totals) != 1:
        raise Unsound("expected exactly one `#   total   N` row in %s, found %d"
                      % (CONFIG_REL, len(totals)))
    return globs, table, totals[0]


def read_listing(text: str) -> list:
    lines = [ln.rstrip() for ln in text.splitlines() if ln.strip()]
    if not lines:
        raise Unsound("the listing is empty -- `cargo mutants --list` produced nothing,"
                      " which is a broken run, not a scope of zero")
    bad = [ln for ln in lines if not NAME_RE.match(ln)]
    if bad:
        raise Unsound("%d listing line(s) are not `file:line:col: description`, e.g. %r"
                      % (len(bad), bad[0]))
    return lines


def ledger_timeouts(ledger: dict) -> list:
    outcomes = ledger.get("outcomes") if isinstance(ledger, dict) else None
    if not isinstance(outcomes, list):
        raise Unsound("MUTANTS_PRIOR_LEDGER has no `outcomes` list")
    names = []
    for o in outcomes:
        scen = o.get("scenario") if isinstance(o, dict) else None
        if isinstance(scen, dict) and o.get("summary") == "Timeout":
            name = (scen.get("Mutant") or {}).get("name")
            if not isinstance(name, str):
                raise Unsound("a Timeout outcome in the ledger has no mutant name")
            names.append(name)
    return names


# --------------------------------------------------------------------------
# The check itself. Pure: returns the list of findings so tests can assert on
# each one, and so every assertion runs even when an earlier one fails.
# --------------------------------------------------------------------------

def strip_pos(name: str) -> tuple:
    m = NAME_RE.match(name)
    return (m.group("file"), m.group("desc")) if m else (name, "")


def find_drift(env: dict, globs: list, table: dict, table_total: int,
               listing: list, timeouts: list) -> list:
    findings = []
    expected = int(str(env["MUTANTS_EXPECTED_SCOPE"]).strip())
    listed = set(listing)
    split = Counter(NAME_RE.match(ln).group("file") for ln in listing)

    # (1) Scope, as an EQUALITY -- mutants.yml's own rule.
    if len(listing) != expected:
        findings.append(
            "scope: `cargo mutants --list` has %d mutants, MUTANTS_EXPECTED_SCOPE in %s is %d."
            % (len(listing), WORKFLOW_REL, expected))
    if len(listed) != len(listing):
        findings.append("scope: the listing contains %d duplicate line(s)"
                        % (len(listing) - len(listed)))

    # (2) The per-file table, which is prose and fails nowhere else.
    if dict(split) != table:
        for f in sorted(set(split) | set(table)):
            if split.get(f, 0) != table.get(f, 0):
                findings.append("per-file split: %s lists %d, the table in %s says %d."
                                % (f, split.get(f, 0), CONFIG_REL, table.get(f, 0)))
    if table_total != sum(table.values()):
        findings.append("per-file table: its `total` row is %d but its rows sum to %d."
                        % (table_total, sum(table.values())))
    if table_total != expected:
        findings.append("per-file table: its `total` row is %d but MUTANTS_EXPECTED_SCOPE is %d."
                        % (table_total, expected))
    unglobbed = sorted(f for f in table if not any(fnmatch.fnmatchcase(f, g) for g in globs))
    for f in unglobbed:
        findings.append("per-file table: %s is not matched by `examine_globs`." % f)

    # (3) Survivor and timeout names, verbatim.
    by_identity = {}
    for ln in listing:
        by_identity.setdefault(strip_pos(ln), []).append(ln)

    def missing(kind, names):
        for n in names:
            if n in listed:
                continue
            msg = "%s not in the listing verbatim: %s" % (kind, n)
            moved = by_identity.get(strip_pos(n), [])
            if moved:
                msg += "  -- same file and description now listed at: %s" % "; ".join(moved)
            else:
                msg += "  -- no listed mutant has this file and description"
            findings.append(msg)

    # An EMPTY survivor list is legitimate (every survivor killed), so it is not
    # a finding; the scope equality above still keeps this check non-vacuous.
    survivors = committed_survivors(env["MUTANTS_SURVIVORS"])
    missing("MUTANTS_SURVIVORS line", survivors)
    budget = int(str(env["MUTANTS_TIMEOUT_BUDGET"]).strip())
    if len(timeouts) > budget:
        findings.append("the prior ledger records %d timeout(s), above MUTANTS_TIMEOUT_BUDGET=%d."
                        % (len(timeouts), budget))
    missing("budgeted TIMEOUT (from MUTANTS_PRIOR_LEDGER)", timeouts)
    return findings


REMEDY = (
    "This PR moves the mutation ratchet's pins, so the next Monday mutants.yml run would"
    " fail. Either make the edit line-neutral in the scoped files, or re-pin IN THIS PR:"
    " run mutants.yml on this branch (`gh workflow run mutants.yml --ref <branch>`),"
    " commit its outcomes.json under docs/mutation-runs/ as MUTANTS_PRIOR_LEDGER, and"
    " update MUTANTS_EXPECTED_SCOPE (`cargo mutants --list | wc -l`), the per-file table"
    " in .cargo/mutants.toml (`cargo mutants --list | sed 's/:[0-9]*:[0-9]*:.*//' | sort"
    " | uniq -c`) and MUTANTS_SURVIVORS (the run's missed.txt) from it. Survivor and"
    " timeout lines are verdicts and must match that ledger"
    " (`every_committed_survivor_is_in_the_prior_ledger`), so do not hand-edit a moved"
    " line to the position hinted above without the run. See"
    " docs/MAINTAINING.md#mutation-oracle.")


def cmd_check(args) -> int:
    root = args.root
    try:
        with open(os.path.join(root, args.workflow)) as fh:
            env = read_workflow_env(fh.read())
        with open(os.path.join(root, args.config)) as fh:
            globs, table, table_total = read_config(fh.read())
        with open(args.listing) as fh:
            listing = read_listing(fh.read())
        ledger_path = os.path.join(root, str(env["MUTANTS_PRIOR_LEDGER"]).strip())
        try:
            with open(ledger_path) as fh:
                ledger = json.load(fh)
        except (OSError, ValueError) as e:
            raise Unsound("MUTANTS_PRIOR_LEDGER (%s) is unreadable: %s" % (ledger_path, e))
        timeouts = ledger_timeouts(ledger)
    except OSError as e:
        err("cannot read an input: %s" % e)
        return EXIT_UNSOUND
    except Unsound as e:
        err(str(e))
        return EXIT_UNSOUND

    findings = find_drift(env, globs, table, table_total, listing, timeouts)
    print("listed=%d expected=%s survivors=%d timeouts=%d files=%d"
          % (len(listing), env["MUTANTS_EXPECTED_SCOPE"],
             len(committed_survivors(env["MUTANTS_SURVIVORS"])), len(timeouts), len(table)))
    if not findings:
        print("mutation pins OK: scope, per-file split, survivor and timeout lines all match"
              " `cargo mutants --list`")
        return EXIT_OK
    for f in findings:
        err(f)
    err(REMEDY)
    return EXIT_DRIFT


def is_relevant(changed: list, globs: list) -> bool:
    patterns = list(globs) + list(ALWAYS_RELEVANT)
    return any(fnmatch.fnmatchcase(p, pat) for p in changed for pat in patterns)


def cmd_relevant(args) -> int:
    # FAILS TOWARD RUNNING. A diff we cannot read, or a config we cannot parse,
    # answers `true`: the check costs seconds, and skipping it on a PR that did
    # touch the scope is the exact failure it exists to remove.
    try:
        with open(args.changed_files) as fh:
            changed = [ln.strip() for ln in fh if ln.strip()]
        with open(os.path.join(args.root, args.config)) as fh:
            globs, _, _ = read_config(fh.read())
    except (OSError, Unsound) as e:
        print("cannot decide relevance (%s); running the check" % e, file=sys.stderr)
        print("true")
        return EXIT_OK
    print("true" if is_relevant(changed, globs) else "false")
    return EXIT_OK


def main(argv=None) -> int:
    p = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    p.add_argument("--root", default=ROOT, help="repository root (default: this script's repo)")
    p.add_argument("--workflow", default=WORKFLOW_REL)
    p.add_argument("--config", default=CONFIG_REL)
    sub = p.add_subparsers(dest="cmd", required=True)
    c = sub.add_parser("check")
    c.add_argument("--listing", required=True, help="saved `cargo mutants --list` output")
    r = sub.add_parser("relevant")
    r.add_argument("--changed-files", required=True, help="one changed path per line")
    args = p.parse_args(argv)
    return cmd_check(args) if args.cmd == "check" else cmd_relevant(args)


if __name__ == "__main__":
    sys.exit(main())
