#!/usr/bin/env python3
"""Unit tests for merge_mutants_shards.py.

These run on EVERY PULL REQUEST (ci.yml's `tests` job), which is the point:
`mutants.yml` has no `pull_request` trigger, so a broken merge would otherwise
surface only on the following Monday's cron. Every refusal branch has a fixture,
because a merge that quietly drops a shard reads as a smaller, greener report.
"""

from __future__ import annotations

import json
import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import merge_mutants_shards as m  # noqa: E402


def mutant(name: str, summary: str, log: str) -> dict:
    return {"scenario": {"Mutant": {"name": name}}, "summary": summary,
            "log_path": "log/" + log, "diff_path": "diff/" + log.replace(".log", ".diff")}


def write_shard(root: str, k: int, outcomes: list, missed=(), caught=(),
                end_time="2026-11-01T10:00:00Z", version="27.1.0", logs=None) -> str:
    d = os.path.join(root, "shard-%d" % k)
    os.makedirs(os.path.join(d, "log"))
    os.makedirs(os.path.join(d, "diff"))
    counts = {"missed": 0, "caught": 0, "timeout": 0, "unviable": 0, "success": 0}
    for o in outcomes:
        key = {"MissedMutant": "missed", "CaughtMutant": "caught", "Timeout": "timeout",
               "Unviable": "unviable"}.get(o["summary"])
        if key:
            counts[key] += 1
    report = dict(counts, total_mutants=sum(1 for o in outcomes if m._mutant_name(o)),
                  start_time="2026-11-01T0%d:00:00Z" % k, end_time=end_time,
                  cargo_mutants_version=version,
                  outcomes=[{"scenario": "Baseline", "summary": "Success",
                             "log_path": "log/baseline.log"}] + outcomes)
    with open(os.path.join(d, "outcomes.json"), "w") as fh:
        json.dump(report, fh)
    for name, lines in (("missed.txt", missed), ("caught.txt", caught)):
        with open(os.path.join(d, name), "w") as fh:
            fh.write("".join(x + "\n" for x in lines))
    for name, body in (logs or {}).items():
        with open(os.path.join(d, "log", name), "w") as fh:
            fh.write(body)
    return d


class MergeTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = self.tmp.name
        self.out = os.path.join(self.root, "merged")

    def tearDown(self):
        self.tmp.cleanup()

    def two_good_shards(self):
        a = write_shard(self.root, 0, [mutant("a.rs:1:1: x", "CaughtMutant", "a_001.log"),
                                       mutant("a.rs:2:1: y", "MissedMutant", "a_002.log")],
                        missed=["a.rs:2:1: y"], caught=["a.rs:1:1: x"],
                        logs={"baseline.log": "ok", "a_001.log": "test t ... FAILED\n"})
        b = write_shard(self.root, 1, [mutant("b.rs:1:1: z", "Unviable", "a_001.log")],
                        end_time="2026-11-01T11:00:00Z",
                        logs={"baseline.log": "ok", "a_001.log": "unviable"})
        return a, b

    def test_sums_counts_and_concatenates_lists(self):
        merged = m.merge(list(self.two_good_shards()), 2, self.out)
        self.assertEqual((merged["total_mutants"], merged["caught"], merged["missed"],
                          merged["unviable"]), (3, 1, 1, 1))
        self.assertEqual(merged["end_time"], "2026-11-01T11:00:00Z")
        with open(os.path.join(self.out, "missed.txt")) as fh:
            self.assertEqual(fh.read(), "a.rs:2:1: y\n")
        with open(os.path.join(self.out, "outcomes.json")) as fh:
            self.assertEqual(json.load(fh)["total_mutants"], 3)

    def test_colliding_log_names_stay_distinct_and_paths_follow(self):
        m.merge(list(self.two_good_shards()), 2, self.out)
        logs = sorted(os.listdir(os.path.join(self.out, "log")))
        self.assertEqual(logs, ["s0__a_001.log", "s0__baseline.log",
                                "s1__a_001.log", "s1__baseline.log"])
        with open(os.path.join(self.out, "outcomes.json")) as fh:
            paths = [o["log_path"] for o in json.load(fh)["outcomes"]
                     if m._mutant_name(o)]
        self.assertEqual(paths, ["log/s0__a_001.log", "log/s0__a_002.log", "log/s1__a_001.log"])
        for p in paths[:1] + paths[2:]:
            self.assertTrue(os.path.isfile(os.path.join(self.out, p)), p)

    def test_refuses_a_missing_shard(self):
        a, _ = self.two_good_shards()
        with self.assertRaisesRegex(m.Refused, "expected 2 shard directories"):
            m.merge([a], 2, self.out)

    def test_refuses_a_shard_without_a_report(self):
        a, b = self.two_good_shards()
        os.remove(os.path.join(b, "outcomes.json"))
        with self.assertRaisesRegex(m.Refused, "produced no report"):
            m.merge([a, b], 2, self.out)

    def test_refuses_an_unparseable_report(self):
        a, b = self.two_good_shards()
        with open(os.path.join(b, "outcomes.json"), "w") as fh:
            fh.write("{")
        with self.assertRaisesRegex(m.Refused, "not valid JSON"):
            m.merge([a, b], 2, self.out)

    def test_refuses_an_unfinished_shard(self):
        a = write_shard(self.root, 0, [mutant("a.rs:1:1: x", "CaughtMutant", "a.log")])
        b = write_shard(self.root, 1, [mutant("b.rs:1:1: z", "CaughtMutant", "b.log")],
                        end_time=None)
        with self.assertRaisesRegex(m.Refused, "did not finish"):
            m.merge([a, b], 2, self.out)

    def test_refuses_mixed_tool_versions(self):
        a = write_shard(self.root, 0, [mutant("a.rs:1:1: x", "CaughtMutant", "a.log")])
        b = write_shard(self.root, 1, [mutant("b.rs:1:1: z", "CaughtMutant", "b.log")],
                        version="27.2.0")
        with self.assertRaisesRegex(m.Refused, "different cargo-mutants versions"):
            m.merge([a, b], 2, self.out)

    def test_refuses_a_mutant_in_two_shards(self):
        a = write_shard(self.root, 0, [mutant("a.rs:1:1: x", "CaughtMutant", "a.log")])
        b = write_shard(self.root, 1, [mutant("a.rs:1:1: x", "CaughtMutant", "a.log")])
        with self.assertRaisesRegex(m.Refused, "appears in shard 0 and shard 1"):
            m.merge([a, b], 2, self.out)

    def test_cli_exit_codes(self):
        a, b = self.two_good_shards()
        self.assertEqual(m.main(["--shards", "2", "--out", self.out, a, b]), 0)
        self.assertEqual(m.main(["--shards", "3", "--out", self.out + "2", a, b]), 1)


if __name__ == "__main__":
    unittest.main()
