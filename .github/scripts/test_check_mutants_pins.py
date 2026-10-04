#!/usr/bin/env python3
"""Unit tests for check_mutants_pins.py.

These run in the required `tests` job on EVERY pull request, next to the
classifier's tests, for the same reason: the PR job that calls the script
(`mutants-pins.yml`) skips its check on PRs that cannot move a pin, so a PR
that breaks the script itself must still go red somewhere.

Every fixture is a small but complete repo -- a mutants.yml with the real env
keys, a .cargo/mutants.toml with the real table shape, and a prior ledger in
cargo-mutants' real outcomes.json shape -- because the readers are part of what
is under test.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
SCRIPT = os.path.join(HERE, "check_mutants_pins.py")
REPO = os.path.abspath(os.path.join(HERE, "..", ".."))
sys.path.insert(0, HERE)
import check_mutants_pins as cmp  # noqa: E402

EXIT_OK, EXIT_DRIFT, EXIT_UNSOUND = 0, 10, 11

A = "crates/acdp-registry-core/src/handlers/context.rs"
B = "crates/acdp-registry-sqlite/src/store.rs"

SURVIVOR = "%s:83:9: delete match arm \"public\" in parse_visibility" % A
TIMEOUT = "%s:1258:12: delete ! in run_search_with_refill" % A
LISTING = [
    "%s:10:5: replace f -> bool with true" % A,
    SURVIVOR,
    TIMEOUT,
    "%s:20:9: replace > with >= in g" % B,
    "%s:30:9: replace == with != in h" % B,
]


def workflow(scope=5, survivors=(SURVIVOR,), budget=1, ledger="ledger.json"):
    body = "\n".join("    # reason for the next line\n    " + s for s in survivors)
    return (
        "name: mutants\n"
        "on:\n  workflow_dispatch:\n"
        "env:\n"
        '  MUTANTS_EXPECTED_SCOPE: "%d"\n'
        '  MUTANTS_PRIOR_LEDGER: "%s"\n'
        "  MUTANTS_SURVIVORS: |\n%s\n\n"
        '  MUTANTS_TIMEOUT_BUDGET: "%d"\n'
        "jobs: {}\n" % (scope, ledger, body or "    # none", budget)
    )


def config(rows=((A, 3), (B, 2)), total=5, globs=(A, B)):
    table = "\n".join("#     %-50s %4d" % r for r in rows)
    return (
        "# header prose\n#\n#     cargo mutants --list\n#\n%s\n"
        "#     ------------------------------------\n"
        "#     total                                              %d\n#\n"
        "# acdp-registry-pg/src/store.rs (136) -- prose that must not parse as a row\n"
        "test_workspace = true\n"
        "examine_globs = [\n%s\n]\n"
        % (table, total, "\n".join('    "%s",' % g for g in globs))
    )


def ledger(timeouts=(TIMEOUT,)):
    outs = [{"scenario": "Baseline", "summary": "Success"}]
    outs += [{"scenario": {"Mutant": {"name": n}}, "summary": "Timeout"} for n in timeouts]
    outs += [{"scenario": {"Mutant": {"name": SURVIVOR}}, "summary": "MissedMutant"}]
    return {"outcomes": outs}


class Base(unittest.TestCase):
    def run_check(self, *, wf=None, cfg=None, led=None, listing=LISTING, raw_listing=None):
        d = tempfile.mkdtemp()
        os.makedirs(os.path.join(d, ".github", "workflows"))
        os.makedirs(os.path.join(d, ".cargo"))
        with open(os.path.join(d, ".github", "workflows", "mutants.yml"), "w") as fh:
            fh.write(workflow() if wf is None else wf)
        with open(os.path.join(d, ".cargo", "mutants.toml"), "w") as fh:
            fh.write(config() if cfg is None else cfg)
        if led is not False:
            with open(os.path.join(d, "ledger.json"), "w") as fh:
                fh.write(json.dumps(ledger() if led is None else led) if not isinstance(led, str) else led)
        lp = os.path.join(d, "list.txt")
        with open(lp, "w") as fh:
            fh.write(raw_listing if raw_listing is not None else "\n".join(listing) + "\n")
        p = subprocess.run([sys.executable, SCRIPT, "--root", d, "check", "--listing", lp],
                           capture_output=True, text=True)
        return p.returncode, p.stdout + p.stderr

    def run_relevant(self, changed, cfg=None):
        d = tempfile.mkdtemp()
        os.makedirs(os.path.join(d, ".cargo"))
        with open(os.path.join(d, ".cargo", "mutants.toml"), "w") as fh:
            fh.write(config() if cfg is None else cfg)
        cp = os.path.join(d, "changed.txt")
        if changed is not None:
            with open(cp, "w") as fh:
                fh.write("\n".join(changed) + "\n")
        p = subprocess.run([sys.executable, SCRIPT, "--root", d, "relevant", "--changed-files", cp],
                           capture_output=True, text=True)
        self.assertEqual(p.returncode, EXIT_OK, p.stdout + p.stderr)
        return p.stdout.strip()


def shifted(lines, file, from_line, by=1):
    """What one inserted line does to a listing: every mutant at or below it moves."""
    out = []
    for ln in lines:
        m = cmp.NAME_RE.match(ln)
        if m.group("file") == file and int(m.group("line")) >= from_line:
            ln = "%s:%d:%s: %s" % (file, int(m.group("line")) + by, m.group("col"), m.group("desc"))
        out.append(ln)
    return out


class Pass(Base):
    def test_matching_pins_pass(self):
        rc, out = self.run_check()
        self.assertEqual(rc, EXIT_OK, out)
        self.assertIn("mutation pins OK", out)

    def test_a_shift_below_every_cited_line_is_line_neutral_and_passes(self):
        """Only cited lines matter: moving uncited mutants after the last cited
        one changes nothing the cron reads."""
        rc, out = self.run_check(listing=shifted(LISTING, A, 2000))
        self.assertEqual(rc, EXIT_OK, out)

    def test_an_empty_survivor_list_is_legitimate(self):
        rc, out = self.run_check(wf=workflow(survivors=()))
        self.assertEqual(rc, EXIT_OK, out)


class Falsification(Base):
    """Each pin, broken on its own, must fail -- a check that cannot fail is not one."""

    def test_shifting_one_line_in_a_scoped_file_fails(self):
        """THE CASE #384 EXISTS FOR: a one-line insertion above a survivor, with
        no change in the number of mutants. Scope and split still match; only
        the verbatim names catch it."""
        rc, out = self.run_check(listing=shifted(LISTING, A, 1))
        self.assertEqual(rc, EXIT_DRIFT, out)
        self.assertIn("MUTANTS_SURVIVORS line not in the listing verbatim: " + SURVIVOR, out)
        self.assertIn("same file and description now listed at: %s:84:9:" % A, out)
        self.assertIn("budgeted TIMEOUT (from MUTANTS_PRIOR_LEDGER) not in the listing", out)
        self.assertNotIn("scope:", out)
        self.assertNotIn("per-file split:", out)

    def test_an_added_mutant_fails_scope_and_split(self):
        rc, out = self.run_check(listing=LISTING + ["%s:40:1: replace k with ()" % B])
        self.assertEqual(rc, EXIT_DRIFT, out)
        self.assertIn("scope: `cargo mutants --list` has 6 mutants", out)
        self.assertIn("per-file split: %s lists 3, the table" % B, out)

    def test_a_split_that_moves_between_files_fails_even_with_the_total_unchanged(self):
        rc, out = self.run_check(cfg=config(rows=((A, 2), (B, 3))))
        self.assertEqual(rc, EXIT_DRIFT, out)
        self.assertNotIn("scope:", out)
        self.assertIn("per-file split: %s lists 3, the table in .cargo/mutants.toml says 2." % A, out)

    def test_a_table_whose_total_row_is_stale_fails(self):
        rc, out = self.run_check(cfg=config(total=4))
        self.assertEqual(rc, EXIT_DRIFT, out)
        self.assertIn("its `total` row is 4 but its rows sum to 5", out)
        self.assertIn("its `total` row is 4 but MUTANTS_EXPECTED_SCOPE is 5", out)

    def test_a_table_row_outside_examine_globs_fails(self):
        rc, out = self.run_check(cfg=config(globs=(A,)))
        self.assertEqual(rc, EXIT_DRIFT, out)
        self.assertIn("%s is not matched by `examine_globs`" % B, out)

    def test_a_survivor_whose_description_changed_says_so(self):
        gone = "%s:50:1: replace q with () in gone" % A
        rc, out = self.run_check(wf=workflow(survivors=(SURVIVOR, gone)))
        self.assertEqual(rc, EXIT_DRIFT, out)
        self.assertIn("no listed mutant has this file and description", out)

    def test_more_ledger_timeouts_than_budget_fails(self):
        rc, out = self.run_check(wf=workflow(budget=0))
        self.assertEqual(rc, EXIT_DRIFT, out)
        self.assertIn("records 1 timeout(s), above MUTANTS_TIMEOUT_BUDGET=0", out)

    def test_findings_accumulate_rather_than_stopping_at_the_first(self):
        rc, out = self.run_check(wf=workflow(scope=9), listing=shifted(LISTING, A, 1),
                                 cfg=config(total=9))
        self.assertEqual(rc, EXIT_DRIFT, out)
        for needle in ("scope:", "rows sum to 5", "MUTANTS_SURVIVORS line", "budgeted TIMEOUT"):
            self.assertIn(needle, out)
        self.assertIn("make the edit line-neutral", out)
        self.assertIn("gh workflow run mutants.yml", out)

    def test_duplicate_listing_lines_fail(self):
        rc, out = self.run_check(wf=workflow(scope=6), cfg=config(rows=((A, 4), (B, 2)), total=6),
                                 listing=LISTING + [LISTING[0]])
        self.assertEqual(rc, EXIT_DRIFT, out)
        self.assertIn("duplicate line", out)


class Unsound(Base):
    """Inputs that cannot support a conclusion must never read as a pass."""

    def test_empty_listing(self):
        rc, out = self.run_check(raw_listing="")
        self.assertEqual(rc, EXIT_UNSOUND, out)
        self.assertIn("the listing is empty", out)

    def test_malformed_listing_line(self):
        rc, out = self.run_check(raw_listing="\n".join(LISTING) + "\nerror: could not parse\n")
        self.assertEqual(rc, EXIT_UNSOUND, out)
        self.assertIn("not `file:line:col: description`", out)

    def test_a_coloured_listing_is_refused_not_compared(self):
        """Measured on this job's first CI run: CARGO_TERM_COLOR=always makes
        `--list` wrap descriptions in ANSI escapes, so every survivor 'vanished'
        while scope and split still matched. That is an input fault, not drift."""
        coloured = [ln.replace(": ", ": \x1b[35m", 1) + "\x1b[0m" for ln in LISTING]
        rc, out = self.run_check(listing=coloured)
        self.assertEqual(rc, EXIT_UNSOUND, out)
        self.assertIn("--colors never", out)

    def test_no_table(self):
        rc, out = self.run_check(cfg='examine_globs = ["%s"]\n' % A)
        self.assertEqual(rc, EXIT_UNSOUND, out)
        self.assertIn("no per-file table rows", out)

    def test_no_examine_globs(self):
        rc, out = self.run_check(cfg=config().replace("examine_globs", "examine_re"))
        self.assertEqual(rc, EXIT_UNSOUND, out)

    def test_missing_env_key(self):
        rc, out = self.run_check(wf=workflow().replace("MUTANTS_TIMEOUT_BUDGET", "X"))
        self.assertEqual(rc, EXIT_UNSOUND, out)
        self.assertIn("does not declare MUTANTS_TIMEOUT_BUDGET", out)

    def test_missing_ledger(self):
        rc, out = self.run_check(led=False)
        self.assertEqual(rc, EXIT_UNSOUND, out)
        self.assertIn("MUTANTS_PRIOR_LEDGER", out)

    def test_ledger_without_outcomes(self):
        rc, out = self.run_check(led={"total_mutants": 5})
        self.assertEqual(rc, EXIT_UNSOUND, out)

    def test_codes_are_not_the_interpreters_own(self):
        """1 = unhandled exception, 2 = argparse error: neither may mean drift."""
        self.assertNotIn(EXIT_DRIFT, (1, 2))
        self.assertNotIn(EXIT_UNSOUND, (1, 2))
        p = subprocess.run([sys.executable, SCRIPT, "check"], capture_output=True, text=True)
        self.assertEqual(p.returncode, 2)


class Relevance(Base):
    def test_a_scoped_source_is_relevant(self):
        self.assertEqual(self.run_relevant(["README.md", B]), "true")

    def test_the_pins_and_the_ledgers_are_relevant(self):
        for p in (".cargo/mutants.toml", ".github/workflows/mutants.yml",
                  ".github/workflows/mutants-pins.yml", ".github/scripts/check_mutants_pins.py",
                  "docs/mutation-runs/run1-outcomes.json"):
            self.assertEqual(self.run_relevant([p]), "true", p)

    def test_an_unrelated_pr_is_not(self):
        self.assertEqual(self.run_relevant(
            ["README.md", "crates/acdp-registry-core/src/handlers/admin.rs",
             ".github/workflows/ci.yml"]), "false")

    def test_an_unreadable_diff_fails_toward_running(self):
        self.assertEqual(self.run_relevant(None), "true")

    def test_an_unparseable_config_fails_toward_running(self):
        self.assertEqual(self.run_relevant(["README.md"], cfg="nothing here\n"), "true")


class TheRealRepo(unittest.TestCase):
    """The readers against the COMMITTED files. A reformatted table or env block
    must fail here, in the required `tests` job, not only in the path-gated PR job."""

    def test_the_committed_pins_parse_and_agree_with_each_other(self):
        with open(os.path.join(REPO, cmp.WORKFLOW_REL)) as fh:
            env = cmp.read_workflow_env(fh.read())
        with open(os.path.join(REPO, cmp.CONFIG_REL)) as fh:
            globs, table, total = cmp.read_config(fh.read())
        self.assertEqual(total, int(env["MUTANTS_EXPECTED_SCOPE"]))
        self.assertEqual(sum(table.values()), total)
        self.assertEqual(sorted(table), sorted(globs))
        self.assertTrue(cmp.committed_survivors(env["MUTANTS_SURVIVORS"]))
        with open(os.path.join(REPO, str(env["MUTANTS_PRIOR_LEDGER"]))) as fh:
            timeouts = cmp.ledger_timeouts(json.load(fh))
        self.assertLessEqual(len(timeouts), int(env["MUTANTS_TIMEOUT_BUDGET"]))

    def test_the_relevance_list_names_this_workflow_and_it_exists(self):
        self.assertTrue(os.path.exists(os.path.join(REPO, ".github/workflows/mutants-pins.yml")))


if __name__ == "__main__":
    unittest.main(verbosity=2)
