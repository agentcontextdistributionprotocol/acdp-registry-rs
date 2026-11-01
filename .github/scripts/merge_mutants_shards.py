#!/usr/bin/env python3
"""Merge the per-shard `mutants.out/` directories into one report.

WHY THIS EXISTS. At scope 399 a single `cargo mutants -j1` run took 307 of the
job's 330 minutes, and the first run at 405 was CANCELLED at the cap after 371
mutants (run 41281572799). 330 already sits just under GitHub's 360-minute
hosted-job limit, so the cap cannot rise again. `mutants.yml` therefore runs the
scope as `--shard k/N --sharding round-robin` jobs, and this script turns their
reports back into the single `mutants.out/` that the harness check, the ratchet
and `classify_removed_survivors.py` already read. Those checks are unchanged:
they see one report, exactly as before sharding.

A MERGE MUST NOT BE ABLE TO MANUFACTURE A GREEN REPORT. Every way a shard can be
missing or partial is a hard failure here, never a smaller total:

  * fewer (or more) shard directories than `--shards`;
  * a shard with no `outcomes.json`, or one that is not valid JSON;
  * a shard whose `end_time` is null -- cargo-mutants writes the report as it
    goes, so a shard cancelled at the job cap still leaves a parseable file
    with plausible counts (41281572799's said total_mutants=371);
  * shards measured by different cargo-mutants versions;
  * the same mutant name in two shards (a sharding change would double-count).

A dropped mutant that none of these catch still fails the ratchet, because the
summed `total_mutants` is checked as an EQUALITY against MUTANTS_EXPECTED_SCOPE.

Exit codes: 0 merged, 1 refused (message on stderr).
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import sys

COUNTS = ("total_mutants", "missed", "caught", "timeout", "unviable", "success")
LISTS = ("caught.txt", "missed.txt", "timeout.txt", "unviable.txt")


class Refused(Exception):
    pass


def _load(shard_dir: str) -> dict:
    path = os.path.join(shard_dir, "outcomes.json")
    if not os.path.isfile(path):
        raise Refused("%s does not exist: that shard produced no report" % path)
    try:
        with open(path) as fh:
            data = json.load(fh)
    except ValueError as e:
        raise Refused("%s is not valid JSON: %s" % (path, e))
    for key in COUNTS:
        if not isinstance(data.get(key), int):
            raise Refused("%s lacks the integer count %r" % (path, key))
    if not data.get("end_time"):
        raise Refused(
            "%s has no end_time: that shard did not finish (cancelled at the job "
            "cap, or crashed), so its counts cover only part of its mutants" % path
        )
    if not isinstance(data.get("outcomes"), list):
        raise Refused("%s has no outcomes list" % path)
    return data


def _mutant_name(outcome: dict):
    scenario = outcome.get("scenario")
    if isinstance(scenario, dict) and "Mutant" in scenario:
        return scenario["Mutant"]["name"]
    return None


def _copy_prefixed(src_dir: str, dst_dir: str, prefix: str) -> None:
    """Copy every file in src_dir to dst_dir as `<prefix><name>`."""
    if not os.path.isdir(src_dir):
        return
    os.makedirs(dst_dir, exist_ok=True)
    for name in sorted(os.listdir(src_dir)):
        src = os.path.join(src_dir, name)
        if not os.path.isfile(src):
            continue
        shutil.copyfile(src, os.path.join(dst_dir, prefix + name))


def merge(shard_dirs: list, shards: int, out: str) -> dict:
    if len(shard_dirs) != shards:
        raise Refused(
            "expected %d shard directories, got %d (%s): a missing shard would "
            "otherwise read as a smaller scope" % (shards, len(shard_dirs), shard_dirs)
        )
    reports = [_load(d) for d in shard_dirs]

    versions = {r.get("cargo_mutants_version") for r in reports}
    if len(versions) != 1:
        raise Refused("shards were measured by different cargo-mutants versions: %s"
                      % sorted(map(str, versions)))

    os.makedirs(out, exist_ok=True)
    merged = {key: sum(r[key] for r in reports) for key in COUNTS}
    merged["start_time"] = min(r["start_time"] for r in reports)
    merged["end_time"] = max(r["end_time"] for r in reports)
    merged["cargo_mutants_version"] = versions.pop()
    merged["shards"] = shards
    merged["outcomes"] = []

    seen = {}
    mutants_list = []
    for k, (shard_dir, report) in enumerate(zip(shard_dirs, reports)):
        # Log and diff file names are only unique WITHIN a run: two different
        # mutants on one line:col get `_001`/`_002` by order, so two shards can
        # both emit `..._001.log`. Prefixing by shard makes every name unique,
        # and the report's own paths are rewritten to match.
        prefix = "s%d__" % k
        # Every path is rewritten, copied file or not, so no merged path can
        # silently resolve to another shard's file of the same name.
        _copy_prefixed(os.path.join(shard_dir, "log"), os.path.join(out, "log"), prefix)
        _copy_prefixed(os.path.join(shard_dir, "diff"), os.path.join(out, "diff"), prefix)
        for outcome in report["outcomes"]:
            name = _mutant_name(outcome)
            if name is not None:
                if name in seen:
                    raise Refused("mutant %r appears in shard %d and shard %d"
                                  % (name, seen[name], k))
                seen[name] = k
            outcome = dict(outcome)
            for key, sub in (("log_path", "log/"), ("diff_path", "diff/")):
                path = outcome.get(key)
                if isinstance(path, str) and path.startswith(sub):
                    outcome[key] = sub + prefix + path[len(sub):]
            merged["outcomes"].append(outcome)
        listing = os.path.join(shard_dir, "mutants.json")
        if os.path.isfile(listing):
            with open(listing) as fh:
                mutants_list.extend(json.load(fh))

    for name in LISTS:
        lines = []
        for shard_dir in shard_dirs:
            path = os.path.join(shard_dir, name)
            if os.path.isfile(path):
                with open(path) as fh:
                    lines.extend(line for line in fh.read().splitlines() if line.strip())
        with open(os.path.join(out, name), "w") as fh:
            fh.write("".join(line + "\n" for line in lines))

    with open(os.path.join(out, "outcomes.json"), "w") as fh:
        json.dump(merged, fh, indent=2)
    if mutants_list:
        with open(os.path.join(out, "mutants.json"), "w") as fh:
            json.dump(mutants_list, fh, indent=2)
    return merged


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--shards", type=int, required=True, help="expected shard count")
    ap.add_argument("--out", required=True, help="merged report directory")
    ap.add_argument("shard_dirs", nargs="*", help="one mutants.out per shard")
    args = ap.parse_args(argv)
    try:
        merged = merge(args.shard_dirs, args.shards, args.out)
    except Refused as e:
        print("merge refused: %s" % e, file=sys.stderr)
        return 1
    print("merged %d shards: %s" % (args.shards, " ".join(
        "%s=%d" % (key, merged[key]) for key in COUNTS)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
