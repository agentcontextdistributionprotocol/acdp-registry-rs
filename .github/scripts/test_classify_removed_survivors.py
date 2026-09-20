#!/usr/bin/env python3
"""Unit tests for classify_removed_survivors.py.

These run on EVERY PULL REQUEST, which is the point. `mutants.yml` is
`schedule:` + `workflow_dispatch` with no `pull_request` trigger, so a PR that
breaks the ratchet's wiring cannot turn it red -- the breakage would surface on
the following Monday's cron, detached from the change that caused it. The same
argument is already written down in `conformance_gate.rs:1514-1527`.

Every fixture carries a bare-string "Baseline" scenario alongside its mutants,
because that is the shape cargo-mutants actually emits and a fixture without one
would not exercise the parser's type guard at all.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
SCRIPT = os.path.join(HERE, "classify_removed_survivors.py")

EXIT_OK, EXIT_CLASSIFIED, EXIT_UNSOUND = 0, 10, 11


def mutant(name, summary, *, file="crates/a/src/x.rs", fn="f", genre="BinaryOperator",
           replacement="==", line=100, column=9, fn_start=50):
    """One outcome record in cargo-mutants' real shape."""
    return {
        "scenario": {
            "Mutant": {
                "name": name,
                "file": file,
                "function": {"function_name": fn,
                             "span": {"start": {"line": fn_start, "column": 1}}},
                "span": {"start": {"line": line, "column": column}},
                "replacement": replacement,
                "genre": genre,
            }
        },
        "summary": summary,
    }


def report(mutants, *, total=None):
    """A full report, including the baseline record cargo-mutants always emits."""
    return {
        "total_mutants": len(mutants) if total is None else total,
        "outcomes": [{"scenario": "Baseline", "summary": "Success"}] + mutants,
    }


def name_for(file="crates/a/src/x.rs", line=100, col=9, desc="replace > with == in f"):
    return f"{file}:{line}:{col}: {desc}"


class Base(unittest.TestCase):
    def run_script(self, *, outcomes, removed, prior=None, extra=()):
        d = tempfile.mkdtemp()
        op = os.path.join(d, "outcomes.json")
        with open(op, "w") as fh:
            json.dump(outcomes, fh)
        rp = os.path.join(d, "removed.txt")
        with open(rp, "w") as fh:
            fh.write("\n".join(removed) + ("\n" if removed else ""))
        argv = [sys.executable, SCRIPT, "--outcomes", op, "--removed-file", rp]
        extra = list(extra)
        if "@COMMITTED@" in extra:
            cp = os.path.join(d, "committed.txt")
            with open(cp, "w") as fh:
                fh.write("\n".join(removed) + "\n")
            extra[extra.index("@COMMITTED@")] = cp
        argv += extra
        if prior is not None:
            pp = os.path.join(d, "prior.json")
            with open(pp, "w") as fh:
                json.dump(prior, fh)
            argv += ["--prior-ledger", pp]
        p = subprocess.run(argv, capture_output=True, text=True)
        return p.returncode, p.stdout + p.stderr


class TestVerdictClasses(Base):
    """A line whose mutant still exists at the same site: classify by verdict."""

    def test_caught_is_a_proven_kill_and_STILL_FAILS(self):
        n = name_for()
        rc, out = self.run_script(outcomes=report([mutant(n, "CaughtMutant")]), removed=[n])
        self.assertIn("KILLED (proven by this run)", out)
        self.assertIn("Delete the line", out)
        # The exit code is the assertion that matters: a proven kill must NOT turn
        # the job green, or the committed list stops matching missed.txt with
        # nothing forcing it back into agreement.
        self.assertEqual(rc, EXIT_CLASSIFIED)

    def test_timeout_is_not_a_kill(self):
        n = name_for()
        rc, out = self.run_script(outcomes=report([mutant(n, "Timeout")]), removed=[n])
        self.assertIn("TIMED OUT -- not a kill", out)
        self.assertIn("Do NOT delete the line", out)
        self.assertEqual(rc, EXIT_CLASSIFIED)

    def test_unviable_is_not_a_kill(self):
        n = name_for()
        rc, out = self.run_script(outcomes=report([mutant(n, "Unviable")]), removed=[n])
        self.assertIn("UNVIABLE -- not a kill", out)
        self.assertEqual(rc, EXIT_CLASSIFIED)

    def test_still_missed_is_a_self_contradicting_report(self):
        n = name_for()
        rc, out = self.run_script(outcomes=report([mutant(n, "MissedMutant")]), removed=[n])
        self.assertIn("CONTRADICTION", out)
        self.assertEqual(rc, EXIT_CLASSIFIED)


class TestDrift(Base):
    """A line that names no mutant in scope: pair it against the prior ledger."""

    def test_drift_still_missed_demands_a_rewrite_not_a_delete(self):
        old, new = name_for(line=100), name_for(line=115)
        prior = report([mutant(old, "MissedMutant", line=100, fn_start=50)])
        cur = report([mutant(new, "MissedMutant", line=115, fn_start=65)])
        rc, out = self.run_script(outcomes=cur, removed=[old], prior=prior)
        self.assertIn("DRIFT-STILL-MISSED", out)
        self.assertIn(f"- {old}", out)
        self.assertIn(f"+ {new}", out)
        self.assertIn("REWRITE", out)
        self.assertIn("Do NOT delete", out)
        self.assertEqual(rc, EXIT_CLASSIFIED)

    def test_drift_now_caught_demands_a_delete(self):
        """A mutant can drift AND be killed in the same window."""
        old, new = name_for(line=100), name_for(line=115)
        prior = report([mutant(old, "MissedMutant", line=100, fn_start=50)])
        cur = report([mutant(new, "CaughtMutant", line=115, fn_start=65)])
        rc, out = self.run_script(outcomes=cur, removed=[old], prior=prior)
        self.assertIn("DRIFT-NOW-CAUGHT", out)
        self.assertIn("DELETE the line", out)
        self.assertEqual(rc, EXIT_CLASSIFIED)

    def test_a_vanished_line_is_never_reported_as_a_kill(self):
        """The core property: absence from missed.txt is not evidence of a kill."""
        old = name_for(line=100)
        cur = report([mutant(name_for(line=115), "MissedMutant", line=115, fn_start=65)])
        rc, out = self.run_script(outcomes=cur, removed=[old],
                                  prior=report([mutant(old, "MissedMutant", fn_start=50)]))
        self.assertIn("NOT A KILL", out)
        self.assertNotIn("KILLED (proven by this run)", out)
        self.assertEqual(rc, EXIT_CLASSIFIED)

    def test_disagreeing_identity_keys_escalate_rather_than_pairing(self):
        """The whole reason two keys are computed.

        They fail on DIFFERENT edits -- offset survives insertions between
        functions and breaks inside one; ordinal is the mirror image. When they
        point at different mutants, neither is trusted. Constructed so the offset
        key resolves to the mutant at line 120 (same fn-relative offset 50 and
        column as the committed line) while the ordinal key resolves to the one at
        line 110 (first in the function by line).
        """
        old = name_for(line=100)
        prior = report([mutant(old, "MissedMutant", line=100, column=9, fn_start=50)])
        cur = report([
            mutant(name_for(line=110), "CaughtMutant", line=110, column=9, fn_start=70),
            mutant(name_for(line=120), "MissedMutant", line=120, column=9, fn_start=70),
        ])
        rc, out = self.run_script(outcomes=cur, removed=[old], prior=prior)
        self.assertIn("resolved to DIFFERENT mutants", out)
        self.assertIn("by hand", out)
        # It must not silently pick either one, and above all must not call it a kill.
        self.assertNotIn("DRIFT-STILL-MISSED", out)
        self.assertNotIn("KILLED (proven by this run)", out)
        self.assertEqual(rc, EXIT_CLASSIFIED)

    def test_no_candidate_when_the_expression_is_gone(self):
        old = name_for(line=100)
        cur = report([mutant(name_for(desc="replace + with - in g"), "CaughtMutant", fn="g")])
        rc, out = self.run_script(outcomes=cur, removed=[old],
                                  prior=report([mutant(old, "MissedMutant")]))
        self.assertIn("NO CANDIDATE", out)
        self.assertEqual(rc, EXIT_CLASSIFIED)

    def test_ambiguous_siblings_escalate_instead_of_guessing(self):
        """Three `delete !` mutants in one function is REAL -- and they held three
        different verdicts in the actual 213-mutant ledger. Coarse matching must
        offer candidates, never conclude."""
        old = name_for(line=100, desc="delete ! in f")
        sibs = [
            mutant(name_for(line=110, desc="delete ! in f"), "CaughtMutant",
                   line=110, column=39, replacement="", genre="UnaryOperator"),
            mutant(name_for(line=120, desc="delete ! in f"), "MissedMutant",
                   line=120, column=16, replacement="", genre="UnaryOperator"),
            mutant(name_for(line=130, desc="delete ! in f"), "Timeout",
                   line=130, column=12, replacement="", genre="UnaryOperator"),
        ]
        rc, out = self.run_script(outcomes=report(sibs), removed=[old], prior=report([]))
        self.assertIn("AMBIGUOUS", out)
        self.assertIn("by hand", out)
        self.assertNotIn("KILLED (proven by this run)", out)
        self.assertEqual(rc, EXIT_CLASSIFIED)


class TestIntegrity(Base):
    """Inputs that cannot support a conclusion must fail, never classify."""

    def test_truncated_report_is_unsound(self):
        n = name_for()
        rc, out = self.run_script(outcomes=report([mutant(n, "CaughtMutant")], total=213),
                                  removed=[n])
        self.assertIn("truncated", out)
        self.assertEqual(rc, EXIT_UNSOUND)

    def test_duplicate_names_are_unsound_and_name_the_glob_trap(self):
        """docs/mutation-runs/ holds VOID and SUPERSEDED ledgers; globbing them
        yields 66 duplicate names, 11 with CONFLICTING verdicts."""
        n = name_for()
        dup = report([mutant(n, "CaughtMutant"), mutant(n, "MissedMutant")])
        rc, out = self.run_script(outcomes=dup, removed=[n])
        self.assertIn("duplicate mutant names", out)
        self.assertIn("GLOB", out)
        self.assertEqual(rc, EXIT_UNSOUND)

    def test_unparseable_report_is_unsound(self):
        d = tempfile.mkdtemp()
        op = os.path.join(d, "outcomes.json")
        with open(op, "w") as fh:
            fh.write("{not json")
        rp = os.path.join(d, "removed.txt")
        with open(rp, "w") as fh:
            fh.write(name_for() + "\n")
        p = subprocess.run([sys.executable, SCRIPT, "--outcomes", op, "--removed-file", rp],
                           capture_output=True, text=True)
        self.assertIn("not valid JSON", p.stdout + p.stderr)
        self.assertEqual(p.returncode, EXIT_UNSOUND)

    def test_missing_prior_ledger_is_unsound(self):
        n = name_for()
        d = tempfile.mkdtemp()
        op = os.path.join(d, "outcomes.json")
        with open(op, "w") as fh:
            json.dump(report([mutant(n, "CaughtMutant")]), fh)
        rp = os.path.join(d, "removed.txt")
        with open(rp, "w") as fh:
            fh.write(n + "\n")
        p = subprocess.run([sys.executable, SCRIPT, "--outcomes", op, "--removed-file", rp,
                            "--prior-ledger", os.path.join(d, "nope.json")],
                           capture_output=True, text=True)
        self.assertIn("does not exist", p.stdout + p.stderr)
        self.assertEqual(p.returncode, EXIT_UNSOUND)


class TestControl(Base):
    def test_nothing_removed_is_green(self):
        """The control. If this ever fails the harness itself is broken."""
        rc, out = self.run_script(outcomes=report([mutant(name_for(), "CaughtMutant")]),
                                  removed=[])
        self.assertIn("no removed survivor lines", out)
        self.assertEqual(rc, EXIT_OK)

    def test_every_classification_names_an_action(self):
        """A red check with no prescribed remedy is how a gate gets disabled."""
        for summary in ("CaughtMutant", "Timeout", "Unviable", "MissedMutant"):
            with self.subTest(summary=summary):
                n = name_for()
                _, out = self.run_script(outcomes=report([mutant(n, summary)]), removed=[n])
                self.assertIn("ACTION:", out)




# ===========================================================================
# FIXTURES WITH REAL STRUCTURE.
#
# The suite above was falsified against ten broken classifiers and caught only
# one, and the reason was the fixtures rather than the assertions: every report
# had ONE file, ONE function, and ONE replacement, so every component of both
# identity keys was degenerate and dropping any of them changed nothing.
#
# Measured on the real 213-mutant ledger, dropping a component collapses it:
#   full key                     213 distinct
#   without `replacement`        123   (90 collisions)
#   without `column`             204   ( 9 collisions)
# The `994:35` site in store.rs is the canonical shape -- ONE line:col carrying
# THREE different replacements (`>` -> `<`, `==`, `>=`) with different verdicts.
# ===========================================================================

FILE_A = "crates/a/src/x.rs"
FILE_B = "crates/b/src/y.rs"


class TestKeyComponentsAreLoadBearing(Base):
    """Each test here fails if the identity key drops the component it names."""

    def test_replacement_disambiguates_two_mutants_at_ONE_site(self):
        """The `994:35` shape: one line:col, several replacements, different verdicts.

        A mutant is also added above the target so the ORDINAL key cannot pair --
        otherwise it rescues the pairing and this says nothing about `offset_key`.
        Drop `replacement` and the offset key matches BOTH mutants at the site, so
        the pairing stops being unique. Measured on the real 213-mutant ledger,
        dropping `replacement` collapses it to 123 distinct keys.
        """
        old = name_for(FILE_A, 100, 35, "replace > with == in f")
        prior = report([mutant(old, "MissedMutant", file=FILE_A, fn="f",
                               replacement="==", line=100, column=35, fn_start=50)])
        cur = report([
            mutant(name_for(FILE_A, 70, 9, "replace + with - in f"), "CaughtMutant",
                   file=FILE_A, fn="f", replacement="-", line=70, column=9, fn_start=65),
            mutant(name_for(FILE_A, 115, 35, "replace > with == in f"), "MissedMutant",
                   file=FILE_A, fn="f", replacement="==", line=115, column=35, fn_start=65),
            mutant(name_for(FILE_A, 115, 35, "replace > with < in f"), "CaughtMutant",
                   file=FILE_A, fn="f", replacement="<", line=115, column=35, fn_start=65),
        ])
        rc, out = self.run_script(outcomes=cur, removed=[old], prior=prior)
        self.assertIn("paired via: offset key only", out)
        self.assertIn("DRIFT-STILL-MISSED", out)
        # Pairing to the CaughtMutant sibling would say "now killed, delete it"
        # about a mutant that is still alive.
        self.assertNotIn("DRIFT-NOW-CAUGHT", out)
        self.assertEqual(rc, EXIT_CLASSIFIED)

    def test_column_disambiguates_two_mutants_on_ONE_line(self):
        """Same construction: the ordinal key is deliberately broken first.

        Dropping `column` collapses the real ledger from 213 to 204 distinct keys.
        """
        old = name_for(FILE_A, 100, 12, "delete ! in f")
        prior = report([mutant(old, "MissedMutant", file=FILE_A, fn="f",
                               genre="UnaryOperator", replacement="",
                               line=100, column=12, fn_start=50)])
        cur = report([
            mutant(name_for(FILE_A, 70, 9, "replace + with - in f"), "CaughtMutant",
                   file=FILE_A, fn="f", replacement="-", line=70, column=9, fn_start=65),
            mutant(name_for(FILE_A, 115, 12, "delete ! in f"), "MissedMutant",
                   file=FILE_A, fn="f", genre="UnaryOperator", replacement="",
                   line=115, column=12, fn_start=65),
            mutant(name_for(FILE_A, 115, 40, "delete ! in f"), "CaughtMutant",
                   file=FILE_A, fn="f", genre="UnaryOperator", replacement="",
                   line=115, column=40, fn_start=65),
        ])
        rc, out = self.run_script(outcomes=cur, removed=[old], prior=prior)
        self.assertIn("paired via: offset key only", out)
        self.assertIn("DRIFT-STILL-MISSED", out)
        self.assertNotIn("DRIFT-NOW-CAUGHT", out)
        self.assertEqual(rc, EXIT_CLASSIFIED)

    def test_the_coarse_fallback_does_not_match_across_FILES(self):
        """`describe()` must keep the file component.

        Same description in two files; the committed line names FILE_A, whose
        expression is gone. Dropping the file from the coarse key would offer
        FILE_B's mutant as a candidate for FILE_A's line.

        FILE_A must keep a mutant of its OWN here -- one with a different
        description, so it is not a coarse candidate. Without it the file
        contributes nothing to the report and the FILE-NOT-IN-SCOPE branch
        answers first, which would leave this test passing on a classifier whose
        coarse key had lost its file component entirely.
        """
        old = name_for(FILE_A, 100, 9, "replace > with == in f")
        cur = report([mutant(name_for(FILE_B, 100, 9, "replace > with == in f"),
                             "CaughtMutant", file=FILE_B, fn="f"),
                      mutant(name_for(FILE_A, 500, 9, "replace - with + in unrelated"),
                             "CaughtMutant", file=FILE_A, fn="unrelated")])
        rc, out = self.run_script(outcomes=cur, removed=[old], prior=report([]))
        self.assertIn("NO CANDIDATE", out)
        self.assertNotIn(FILE_B, out.split("NO CANDIDATE")[0].split(old)[-1])
        self.assertEqual(rc, EXIT_CLASSIFIED)

    def test_ordinal_is_scoped_to_the_FUNCTION_not_the_file(self):
        """Two functions in one file, and an edit INSIDE the second one.

        The offset key cannot pair (the target moved within its own function), so
        only the ordinal key can -- which makes this test actually about the
        ordinal. `g` keeps its single mutant, so its within-function ordinal is
        still 0; but `f` GAINS one, so a file-scoped ordinal shifts from 1 to 2
        and the pairing is lost.
        """
        old = name_for(FILE_A, 100, 9, "replace > with == in g")
        prior = report([
            mutant(name_for(FILE_A, 20, 9, "replace + with - in f"), "CaughtMutant",
                   file=FILE_A, fn="f", replacement="-", line=20, column=9, fn_start=10),
            mutant(old, "MissedMutant", file=FILE_A, fn="g", line=100, column=9, fn_start=50),
        ])
        cur = report([
            mutant(name_for(FILE_A, 20, 9, "replace + with - in f"), "CaughtMutant",
                   file=FILE_A, fn="f", replacement="-", line=20, column=9, fn_start=10),
            mutant(name_for(FILE_A, 30, 9, "replace * with / in f"), "CaughtMutant",
                   file=FILE_A, fn="f", replacement="/", line=30, column=9, fn_start=10),
            mutant(name_for(FILE_A, 130, 9, "replace > with == in g"), "MissedMutant",
                   file=FILE_A, fn="g", line=130, column=9, fn_start=50),
        ])
        rc, out = self.run_script(outcomes=cur, removed=[old], prior=prior)
        self.assertIn("paired via: ordinal key only", out)
        self.assertIn("DRIFT-STILL-MISSED", out)
        self.assertIn(name_for(FILE_A, 130, 9, "replace > with == in g"), out)
        self.assertEqual(rc, EXIT_CLASSIFIED)

    def test_a_non_integer_total_mutants_says_so_plainly(self):
        """Without the type check this still fails, but with a misleading
        "truncated" message that sends the reader after the wrong problem."""
        n = name_for()
        rc, out = self.run_script(outcomes=report([mutant(n, "CaughtMutant")], total="213"),
                                  removed=[n])
        self.assertIn("no integer 'total_mutants'", out)
        self.assertEqual(rc, EXIT_UNSOUND)


    def test_ordinal_sort_breaks_ties_on_COLUMN_not_list_order(self):
        """Two mutants on ONE line, listed with the HIGHER column first.

        Ordering the function's mutants by line alone leaves a stable sort holding
        list order, so the ordinals swap. Because the sibling is CaughtMutant, the
        mispairing does not merely fail -- it reports DRIFT-NOW-CAUGHT and tells
        the reader to DELETE a survivor that is still alive. That is the precise
        failure this whole phase exists to prevent, reached through the sort.

        The edit is INSIDE the function, so the offset key cannot pair and the
        ordinal key is genuinely the one under test.
        """
        old = name_for(FILE_A, 100, 12, "delete ! in f")
        prior = report([
            mutant(old, "MissedMutant", file=FILE_A, fn="f", genre="UnaryOperator",
                   replacement="", line=100, column=12, fn_start=50),
            mutant(name_for(FILE_A, 100, 40, "delete ! in f"), "CaughtMutant",
                   file=FILE_A, fn="f", genre="UnaryOperator", replacement="",
                   line=100, column=40, fn_start=50),
        ])
        cur = report([
            # Higher column FIRST in the list, on purpose.
            mutant(name_for(FILE_A, 130, 40, "delete ! in f"), "CaughtMutant",
                   file=FILE_A, fn="f", genre="UnaryOperator", replacement="",
                   line=130, column=40, fn_start=50),
            mutant(name_for(FILE_A, 130, 12, "delete ! in f"), "MissedMutant",
                   file=FILE_A, fn="f", genre="UnaryOperator", replacement="",
                   line=130, column=12, fn_start=50),
        ])
        rc, out = self.run_script(outcomes=cur, removed=[old], prior=prior)
        self.assertIn("DRIFT-STILL-MISSED", out)
        self.assertIn(name_for(FILE_A, 130, 12, "delete ! in f"), out)
        self.assertNotIn("DRIFT-NOW-CAUGHT", out)
        self.assertNotIn("DELETE the line", out)
        self.assertEqual(rc, EXIT_CLASSIFIED)


class TestSingleKeyPairing(Base):
    """Exactly ONE key surviving is not a disagreement -- and must say so."""

    def test_offset_survives_an_edit_that_shifts_the_ordinal(self):
        """A mutant is ADDED to the function above the target.

        The ordinal shifts (so the ordinal key finds nothing at the old ordinal)
        while the fn-relative offset is unchanged. Requiring BOTH keys would
        wrongly dump this into the coarse fallback.
        """
        old = name_for(FILE_A, 100, 9, "replace > with == in f")
        prior = report([mutant(old, "MissedMutant", file=FILE_A, fn="f",
                               line=100, column=9, fn_start=50)])
        cur = report([
            mutant(name_for(FILE_A, 80, 9, "replace + with - in f"), "CaughtMutant",
                   file=FILE_A, fn="f", replacement="-", line=80, column=9, fn_start=50),
            mutant(name_for(FILE_A, 100, 9, "replace > with == in f2"), "MissedMutant",
                   file=FILE_A, fn="f", line=100, column=9, fn_start=50),
        ])
        rc, out = self.run_script(outcomes=cur, removed=[old], prior=prior)
        self.assertIn("paired via: offset key only", out)
        self.assertIn("DRIFT-STILL-MISSED", out)
        # It must NOT claim the keys disagreed -- they did not; one found nothing.
        self.assertNotIn("resolved to DIFFERENT mutants", out)
        self.assertEqual(rc, EXIT_CLASSIFIED)

    def test_ordinal_survives_an_edit_INSIDE_the_function(self):
        """Lines inserted inside the function shift the offset; the ordinal holds."""
        old = name_for(FILE_A, 100, 9, "replace > with == in f")
        prior = report([mutant(old, "MissedMutant", file=FILE_A, fn="f",
                               line=100, column=9, fn_start=50)])
        cur = report([mutant(name_for(FILE_A, 130, 9, "replace > with == in f"),
                             "MissedMutant", file=FILE_A, fn="f",
                             line=130, column=9, fn_start=50)])
        rc, out = self.run_script(outcomes=cur, removed=[old], prior=prior)
        self.assertIn("paired via: ordinal key only", out)
        self.assertIn("DRIFT-STILL-MISSED", out)
        self.assertNotIn("resolved to DIFFERENT mutants", out)
        self.assertEqual(rc, EXIT_CLASSIFIED)


class TestMalformedAndShards(Base):
    def test_a_malformed_line_is_never_told_to_delete(self):
        """The committed TEXT is broken -- that is not evidence a mutant died."""
        rc, out = self.run_script(outcomes=report([mutant(name_for(), "CaughtMutant")]),
                                  removed=["garbage line with no colons"])
        self.assertIn("MALFORMED", out)
        self.assertIn("Do NOT delete", out)
        self.assertNotIn("NO CANDIDATE", out)
        self.assertEqual(rc, EXIT_CLASSIFIED)

    def test_a_sharded_report_is_refused(self):
        """Per-shard totals pass the truncation check, so only the scope pin
        can tell a shard from a whole run. Classifying a shard would emit
        `delete the line` for every survivor belonging to another shard."""
        n = name_for()
        rc, out = self.run_script(outcomes=report([mutant(n, "CaughtMutant")]),
                                  removed=[n], extra=("--expected-scope", "213"))
        self.assertIn("SHARDED", out)
        self.assertEqual(rc, EXIT_UNSOUND)

    def test_a_merged_report_with_TOO_MANY_records_is_refused(self):
        """The other direction of the truncation check: two reports concatenated."""
        n = name_for()
        rc, out = self.run_script(
            outcomes=report([mutant(n, "CaughtMutant"), mutant(name_for(line=200), "CaughtMutant")],
                            total=1),
            removed=[n])
        self.assertIn("truncated or internally inconsistent", out)
        self.assertEqual(rc, EXIT_UNSOUND)

    def test_an_unknown_verdict_still_refuses_to_say_delete(self):
        n = name_for()
        rc, out = self.run_script(outcomes=report([mutant(n, "SomeNewVariant")]), removed=[n])
        self.assertIn("UNKNOWN VERDICT", out)
        self.assertIn("will not tell you to delete", out)
        self.assertEqual(rc, EXIT_CLASSIFIED)

    def test_a_line_is_classified_exactly_once(self):
        """A missing `continue` would print two contradictory verdicts for one line."""
        n = name_for()
        _, out = self.run_script(outcomes=report([mutant(n, "CaughtMutant")]), removed=[n])
        self.assertEqual(out.count("KILLED (proven by this run)"), 1)
        self.assertNotIn("NOT A KILL", out)


class TestLedgerFreshness(Base):
    def test_a_committed_line_missing_from_the_prior_ledger_is_refused(self):
        """Without this, the first edit to MUTANTS_SURVIVORS that forgets to
        commit a matching ledger silently degrades every future drift to a
        coarse guess, while the wiring still looks correct.

        The removed line is deliberately ABSENT from the current report, so this
        classification genuinely needs the ledger to pair against.
        """
        gone, other = name_for(line=100), name_for(line=300)
        rc, out = self.run_script(
            outcomes=report([mutant(name_for(line=900), "CaughtMutant", line=900)]),
            removed=[gone],
            prior=report([mutant(other, "MissedMutant", line=300)]),
            extra=("--committed-file", "@COMMITTED@"))
        self.assertIn("STALE", out)
        self.assertEqual(rc, EXIT_UNSOUND)

    def test_a_stale_ledger_does_NOT_suppress_a_provable_verdict(self):
        """A line whose mutant is still in scope is judged from THIS run's verdict
        and never touches the ledger. Blocking it on ledger staleness would
        withhold a provable answer and leave the ratchet unactionable -- the
        failure mode .cargo/mutants.toml warns about: a red check nobody can act
        on gets disabled."""
        n = name_for()
        rc, out = self.run_script(
            outcomes=report([mutant(n, "CaughtMutant")]),
            removed=[n],
            prior=report([mutant(name_for(line=300), "MissedMutant", line=300)]),
            extra=("--committed-file", "@COMMITTED@"))
        self.assertIn("KILLED (proven by this run)", out)
        self.assertNotIn("STALE", out)
        self.assertEqual(rc, EXIT_CLASSIFIED)

    def test_ambiguous_identity_keys_escalate_instead_of_picking_one(self):
        """TWO SAME-NAMED FUNCTIONS IN ONE FILE -- ordinary Rust, one `f` per impl.

        Both candidates sit at the same fn-relative offset AND column, so the
        offset key matches two mutants; the prior ordinal (2) does not exist in
        the current file, so the ordinal key matches none. That is the only route
        into the identity-key ambiguity branch, and it was previously untested:
        the old "ambiguous" test passed an EMPTY prior ledger, so it never entered
        the pairing block and exercised the coarse matcher instead.

        This must kill two mutations at once:
          * `len(by_off) == 1` -> `by_off`  (take the first candidate)
          * `elif by_off or by_ord:` -> `and`  (never reach the branch)
        The first is the dangerous one: candidate A is CaughtMutant and the real
        match is MissedMutant, so picking the first turns "decide by hand" into
        "DELETE the line" against a survivor that is still alive.
        """
        target = name_for(FILE_A, 100, 9, "replace > with == in f")
        prior = report([
            mutant(name_for(FILE_A, 60, 9, "replace + with - in f"), "CaughtMutant",
                   file=FILE_A, fn="f", replacement="-", line=60, column=9, fn_start=50),
            mutant(name_for(FILE_A, 80, 9, "replace * with / in f"), "CaughtMutant",
                   file=FILE_A, fn="f", replacement="/", line=80, column=9, fn_start=50),
            mutant(target, "MissedMutant", file=FILE_A, fn="f",
                   replacement="==", line=100, column=9, fn_start=50),
        ])
        cur = report([
            mutant(name_for(FILE_A, 105, 9, "replace > with == in f"), "CaughtMutant",
                   file=FILE_A, fn="f", replacement="==", line=105, column=9, fn_start=55),
            mutant(name_for(FILE_A, 250, 9, "replace > with == in f"), "MissedMutant",
                   file=FILE_A, fn="f", replacement="==", line=250, column=9, fn_start=200),
        ])
        rc, out = self.run_script(outcomes=cur, removed=[target], prior=prior)
        # Wording unique to the IDENTITY-key branch; the coarse fallback says
        # "AMBIGUOUS", so asserting that instead would pass via the wrong path.
        self.assertIn("an identity key matched MORE THAN ONE mutant", out)
        self.assertIn("by hand", out)
        self.assertNotIn("DELETE the line", out)
        self.assertNotIn("DRIFT-NOW-CAUGHT", out)
        self.assertNotIn("paired via", out)
        self.assertEqual(rc, EXIT_CLASSIFIED)

    def test_drift_to_an_UNKNOWN_verdict_never_says_delete(self):
        """The drift path's unknown-verdict fallback. Replacing it with
        CaughtMutant's action would make an unrecognised verdict mean
        "delete the line", and nothing else covers this branch."""
        old = name_for(FILE_A, 100, 9, "replace > with == in f")
        prior = report([mutant(old, "MissedMutant", file=FILE_A, fn="f",
                               line=100, column=9, fn_start=50)])
        cur = report([mutant(name_for(FILE_A, 115, 9, "replace > with == in f"),
                             "SomeFutureVariant", file=FILE_A, fn="f",
                             line=115, column=9, fn_start=65)])
        rc, out = self.run_script(outcomes=cur, removed=[old], prior=prior)
        self.assertIn("DRIFTED", out)
        self.assertIn("not a kill", out)
        self.assertIn("will not tell you to delete", out)
        self.assertNotIn("DELETE the line", out)
        self.assertEqual(rc, EXIT_CLASSIFIED)


class TestFileLeftTheScope(Base):
    """A file dropped from `examine_globs` empties its survivors from missed.txt
    without a single test being written. That is the one way to make the ratchet
    look tighter by watching less, so it must never be reported as a kill -- and
    it must not be reported as "the expression no longer exists" either, which
    would prescribe exactly the deletion that locks the narrowing in."""

    def test_a_file_with_zero_mutants_is_named_as_a_scope_change(self):
        gone = name_for(file=FILE_B, line=200, desc="replace > with == in h")
        cur = report([mutant(name_for(file=FILE_A), "CaughtMutant", file=FILE_A)])
        rc, out = self.run_script(outcomes=cur, removed=[gone])
        self.assertIn("FILE NOT IN SCOPE", out)
        self.assertIn(FILE_B, out)
        # The dangerous misreading, in all three of its spellings.
        self.assertNotIn("NO CANDIDATE", out)
        self.assertNotIn("KILLED (proven by this run)", out)
        self.assertIn("do NOT record it", out)
        self.assertEqual(rc, EXIT_CLASSIFIED)

    def test_a_file_still_in_scope_is_NOT_reported_as_a_scope_change(self):
        """The discrimination, not just the new branch: same shape, same absent
        mutant, but the file is still being examined -- so the honest reading is
        that the expression really is gone."""
        gone = name_for(file=FILE_A, line=999, desc="replace + with - in vanished")
        cur = report([mutant(name_for(file=FILE_A), "CaughtMutant", file=FILE_A)])
        rc, out = self.run_script(outcomes=cur, removed=[gone])
        self.assertIn("NO CANDIDATE", out)
        self.assertNotIn("FILE NOT IN SCOPE", out)
        self.assertEqual(rc, EXIT_CLASSIFIED)

    def test_the_expected_scope_guard_CANNOT_catch_this(self):
        """Why the file check has to exist at its own level. --expected-scope
        compares TOTALS, so narrowing examine_globs and updating
        MUTANTS_EXPECTED_SCOPE in the same commit keeps total == expected. The
        totals agree, the guard stays silent, and a whole file stops being
        watched -- this must still be caught, and as CLASSIFIED, not UNSOUND."""
        gone = name_for(file=FILE_B, line=200, desc="replace > with == in h")
        cur = report([mutant(name_for(file=FILE_A, line=i), "CaughtMutant", file=FILE_A)
                      for i in (10, 20, 30)])
        rc, out = self.run_script(outcomes=cur, removed=[gone],
                                  extra=["--expected-scope", "3"])
        self.assertIn("FILE NOT IN SCOPE", out)
        self.assertEqual(rc, EXIT_CLASSIFIED)
        self.assertNotEqual(rc, EXIT_UNSOUND)


if __name__ == "__main__":
    unittest.main(verbosity=2)
