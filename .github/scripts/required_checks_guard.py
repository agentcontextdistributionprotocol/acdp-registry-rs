#!/usr/bin/env python3
"""PR-time guard for .github/required-checks.json.

WHY THIS EXISTS. A required status check is matched by NAME. Rename a job whose
name is required (or make it skip, or filter it off some PRs) and branch
protection waits forever on a context that never reports -- every PR blocks,
and nothing in the PR that caused it goes red. `msrv (1.88)` was one toolchain
bump away from exactly that. This guard makes the rename itself go red, in the
PR that does it, by resolving every name in the committed baseline to a real
job and rejecting the job shapes that can leave a required context unreported
or vacuously green.

For every name in `required` and `advisory_pending` it asserts that the name
resolves to EXACTLY ONE job (the job's `name:`, else its id; `caller / callee`
for a local reusable workflow) and that the job:

  * lives in a workflow triggered by `pull_request`, whose trigger has no
    `paths` / `paths-ignore` / `branches-ignore` filter, whose `branches`
    filter (if any) matches `main`, and whose `types` filter (if any) keeps
    opened/synchronize/reopened -- otherwise some PRs never get the check;
  * has no job-level `if:` and no `needs:` -- a SKIPPED job reports success,
    so a required check that can skip is a check that can pass without running;
  * has no `continue-on-error` -- a failure would report green;
  * has no `${{ }}` in its `name:` and no matrix -- the reported check name
    would not be the literal one listed.

A name it cannot resolve FAILS LOUD; it is never skipped. It also validates the
baseline's own shape: every `required` entry must pin `app_id` (an unpinned
context can be satisfied by any app with statuses:write) to GitHub Actions,
which is the only app that runs these workflows.

This guard reads the COMMITTED baseline, not the live setting -- the live
setting is compared to the same file by branch-protection-drift.yml.

Modes:
  --check      (default) validate the repo this script lives in.
  --self-test  NEGATIVE CONTROLS: build fixture repos in a temp dir and assert
               each rejection rule fires, plus a positive control that passes.
               A gate that cannot fail is not a gate.

python3 + PyYAML only. PyYAML is preinstalled on ubuntu-latest but NOT on a stock
macOS python3: run `pip3 install --user pyyaml` once to use it locally -- the same
requirement as lint.yml's existing composite-action step. Python 3.9 compatible.
Note PyYAML reads a bare `on:` key as boolean True.
"""

import fnmatch
import glob
import io
import json
import os
import shutil
import sys
import tempfile

import yaml

# GitHub Actions' app id. Every workflow job's check run is created by this
# app, so it is the only correct pin for a context that resolves to a job here.
ACTIONS_APP_ID = 15368

BASELINE_REL = os.path.join(".github", "required-checks.json")
WORKFLOWS_REL = os.path.join(".github", "workflows")

TOP_LEVEL_REQUIRED = ("strict", "required", "advisory_pending",
                      "enforce_admins", "pending_settings", "tag_ruleset")
TOP_LEVEL_ALLOWED = set(TOP_LEVEL_REQUIRED) | {"_comment"}
# `tag_ruleset` is shaped like the rulesets API body (rules as bare type names), so
# protection-drift.sh can print a ready-to-run POST/PUT derived from this file.
TAG_RULESET_KEYS = {"name", "target", "enforcement", "conditions", "rules", "bypass_actors"}
RULESET_ENFORCEMENTS = ("active", "evaluate", "disabled")
PR_TYPES_NEEDED = {"opened", "synchronize", "reopened"}
FORBIDDEN_PR_FILTERS = ("paths", "paths-ignore", "branches-ignore")


def _triggers(doc):
    """Normalise a workflow's `on:` into {event: config-or-None}."""
    on = doc.get(True, doc.get("on"))
    if on is None:
        return {}
    if isinstance(on, str):
        return {on: None}
    if isinstance(on, list):
        return {str(e): None for e in on}
    if isinstance(on, dict):
        return {str(k): v for k, v in on.items()}
    return {}


def _branches_include(branch, patterns):
    """GitHub's `branches:` filter semantics: patterns are evaluated in order and
    the LAST one that matches wins -- a `!pattern` that matches excludes the
    branch, a later positive pattern that matches re-includes it. So
    ['*', '!main'] excludes main and ['!main', 'main'] includes it. A branch no
    pattern matches is excluded. (fnmatch's `*` also crosses `/`, unlike
    GitHub's; that only matters for branch names with a slash, and `main` has
    none.)
    """
    included = False
    for pat in patterns:
        pat = str(pat)
        if pat.startswith("!"):
            if fnmatch.fnmatchcase(branch, pat[1:]):
                included = False
        elif fnmatch.fnmatchcase(branch, pat):
            included = True
    return included


def _pr_trigger_problems(triggers):
    """Problems that keep a pull_request-triggered check off some PRs to main.

    Returns None if the workflow is not triggered by pull_request at all.
    """
    if "pull_request" not in triggers:
        return None
    cfg = triggers["pull_request"] or {}
    if not isinstance(cfg, dict):
        return ["`on.pull_request` has an unrecognised shape"]
    problems = []
    for key in FORBIDDEN_PR_FILTERS:
        if key in cfg:
            problems.append(
                "`on.pull_request.%s` is set: PRs outside the filter never get this check, "
                "and a required check that never reports blocks them forever" % key
            )
    if "branches" in cfg:
        branches = cfg["branches"] or []
        if isinstance(branches, str):
            branches = [branches]
        if not _branches_include("main", branches):
            problems.append("`on.pull_request.branches` does not match `main`")
    if "types" in cfg:
        types = cfg["types"] or []
        if isinstance(types, str):
            types = [types]
        missing = sorted(PR_TYPES_NEEDED - set(types))
        if missing:
            problems.append(
                "`on.pull_request.types` omits %s: the check would not re-run on those events"
                % ", ".join(missing)
            )
    return problems


def _job_problems(job):
    problems = []
    if not isinstance(job, dict):
        return ["job is not a mapping"]
    if "if" in job:
        problems.append("has a job-level `if:` -- a skipped job reports success")
    if "needs" in job:
        problems.append("has `needs:` -- if a dependency fails or skips, this job skips, and a skipped job reports success")
    if "continue-on-error" in job:
        problems.append("has `continue-on-error` -- a failure would not fail the check")
    name = job.get("name")
    if isinstance(name, str) and "${{" in name:
        problems.append("its `name:` contains `${{ }}` -- the reported check name is not the literal one")
    strategy = job.get("strategy") or {}
    if isinstance(strategy, dict) and strategy.get("matrix") is not None:
        problems.append("has a matrix -- check names become `<name> (<leg>)`, not the literal name")
    return problems


def _load_yaml(path):
    with io.open(path, encoding="utf-8") as fh:
        return yaml.safe_load(fh) or {}


def _index_jobs(root):
    """Map every check name a workflow job can report -> list of candidates.

    A candidate is a dict: where (str), pr (bool: triggered by pull_request),
    problems (list of str). Jobs in workflows that are NOT pr-triggered are
    indexed too, so a listed name that also belongs to some other job is
    reported as ambiguous rather than silently resolved to one of them.
    """
    index = {}
    wf_dir = os.path.join(root, WORKFLOWS_REL)
    paths = sorted(glob.glob(os.path.join(wf_dir, "*.yml")) + glob.glob(os.path.join(wf_dir, "*.yaml")))
    for path in paths:
        rel = os.path.relpath(path, root)
        doc = _load_yaml(path)
        triggers = _triggers(doc)
        if set(triggers) == {"workflow_call"}:
            # Only reachable through a caller; indexed under `caller / callee` below.
            continue
        trig = _pr_trigger_problems(triggers)
        pr = trig is not None
        wf_problems = trig or []
        for job_id, job in (doc.get("jobs") or {}).items():
            job = job or {}
            caller_name = job.get("name", job_id) if isinstance(job, dict) else job_id
            problems = wf_problems + _job_problems(job)
            uses = job.get("uses") if isinstance(job, dict) else None
            if isinstance(uses, str):
                if uses.startswith("./"):
                    callee_path = os.path.join(root, uses[2:].split("@")[0])
                    if not os.path.isfile(callee_path):
                        index.setdefault("%s / *" % caller_name, []).append(
                            {"where": "%s: job `%s`" % (rel, job_id), "pr": pr,
                             "problems": problems + ["calls %s, which does not exist" % uses]})
                        continue
                    callee = _load_yaml(callee_path)
                    for cid, cjob in (callee.get("jobs") or {}).items():
                        cjob = cjob or {}
                        cname = cjob.get("name", cid) if isinstance(cjob, dict) else cid
                        index.setdefault("%s / %s" % (caller_name, cname), []).append(
                            {"where": "%s: job `%s` -> %s: job `%s`" % (rel, job_id, uses, cid),
                             "pr": pr, "problems": problems + _job_problems(cjob)})
                else:
                    # Remote reusable workflow: its job names are not in this repo.
                    index.setdefault("%s / *" % caller_name, []).append(
                        {"where": "%s: job `%s` (calls remote %s)" % (rel, job_id, uses), "pr": pr,
                         "problems": problems + ["calls a remote reusable workflow whose job names cannot be resolved from this repo"]})
                continue
            index.setdefault(str(caller_name), []).append(
                {"where": "%s: job `%s`" % (rel, job_id), "pr": pr, "problems": problems})
    return index


def _validate_baseline(data):
    """Shape errors in the baseline file itself. Returns (errors, names)."""
    errors = []
    if not isinstance(data, dict):
        return ["%s must be a JSON object" % BASELINE_REL], []
    for key in TOP_LEVEL_REQUIRED:
        if key not in data:
            errors.append("%s is missing key `%s`" % (BASELINE_REL, key))
    for key in sorted(set(data) - TOP_LEVEL_ALLOWED):
        errors.append("%s has unknown key `%s` (typo? the drift job would ignore it)" % (BASELINE_REL, key))
    if "strict" in data and not isinstance(data["strict"], bool):
        errors.append("`strict` must be true or false")

    names = []
    required = data.get("required")
    if "required" in data and (not isinstance(required, list) or not required):
        errors.append("`required` must be a non-empty list")
        required = []
    for i, entry in enumerate(required or []):
        if not isinstance(entry, dict):
            errors.append("`required[%d]` must be an object {context, app_id}" % i)
            continue
        extra = sorted(set(entry) - {"context", "app_id"})
        if extra:
            errors.append("`required[%d]` has unknown key(s) %s" % (i, ", ".join(extra)))
        ctx = entry.get("context")
        if not isinstance(ctx, str) or not ctx:
            errors.append("`required[%d].context` must be a non-empty string" % i)
            continue
        names.append(ctx)
        if "app_id" not in entry or entry["app_id"] is None:
            errors.append(
                "`%s` is not pinned to an app_id: an unpinned required context is satisfied by ANY "
                "app with statuses:write (e.g. the bot or Vercel). Pin it to %d (GitHub Actions)."
                % (ctx, ACTIONS_APP_ID))
        elif isinstance(entry["app_id"], bool) or not isinstance(entry["app_id"], int):
            errors.append("`%s`: app_id must be an integer" % ctx)
        elif entry["app_id"] != ACTIONS_APP_ID:
            errors.append(
                "`%s` is pinned to app_id %d, but it resolves to a workflow job, whose check run "
                "is created by GitHub Actions (%d). The wrong pin means the real check can never "
                "satisfy it." % (ctx, entry["app_id"], ACTIONS_APP_ID))

    pending = data.get("advisory_pending")
    if "advisory_pending" in data and not isinstance(pending, list):
        errors.append("`advisory_pending` must be a list of check names")
        pending = []
    for i, ctx in enumerate(pending or []):
        if not isinstance(ctx, str) or not ctx:
            errors.append("`advisory_pending[%d]` must be a non-empty string" % i)
            continue
        names.append(ctx)

    seen = set()
    for ctx in names:
        if ctx in seen:
            errors.append("`%s` is listed more than once across `required` / `advisory_pending`" % ctx)
        seen.add(ctx)

    for key in ("enforce_admins", "pending_settings"):
        if key in data and not isinstance(data[key], bool):
            errors.append("`%s` must be true or false" % key)
    if "tag_ruleset" in data:
        errors.extend(_validate_tag_ruleset(data["tag_ruleset"]))
    return errors, names


def _is_str_list(value, non_empty):
    return (isinstance(value, list) and all(isinstance(v, str) and v for v in value)
            and (bool(value) or not non_empty))


def _validate_tag_ruleset(rs):
    """Shape of the expected tag ruleset that protection-drift.sh compares against."""
    if not isinstance(rs, dict):
        return ["`tag_ruleset` must be an object"]
    errors = []
    missing = sorted(TAG_RULESET_KEYS - set(rs))
    extra = sorted(set(rs) - TAG_RULESET_KEYS)
    if missing:
        errors.append("`tag_ruleset` is missing %s" % ", ".join(missing))
    if extra:
        errors.append("`tag_ruleset` has unknown key(s) %s (it is POSTed as-is by the drift fix command)"
                      % ", ".join(extra))
    if "name" in rs and (not isinstance(rs["name"], str) or not rs["name"]):
        errors.append("`tag_ruleset.name` must be a non-empty string")
    if "target" in rs and rs["target"] != "tag":
        errors.append("`tag_ruleset.target` must be \"tag\"")
    if "enforcement" in rs and rs["enforcement"] not in RULESET_ENFORCEMENTS:
        errors.append("`tag_ruleset.enforcement` must be one of %s" % ", ".join(RULESET_ENFORCEMENTS))
    if "conditions" in rs:
        ref = (rs["conditions"] or {}).get("ref_name") if isinstance(rs["conditions"], dict) else None
        if not isinstance(ref, dict) or set(ref) != {"include", "exclude"}:
            errors.append("`tag_ruleset.conditions` must be {\"ref_name\": {\"include\": [...], \"exclude\": [...]}}")
        else:
            if not _is_str_list(ref["include"], non_empty=True):
                errors.append("`tag_ruleset.conditions.ref_name.include` must be a non-empty list of strings")
            if not _is_str_list(ref["exclude"], non_empty=False):
                errors.append("`tag_ruleset.conditions.ref_name.exclude` must be a list of strings")
    if "rules" in rs:
        rules = rs["rules"]
        if not _is_str_list(rules, non_empty=True) or len(set(rules)) != len(rules):
            errors.append("`tag_ruleset.rules` must be a non-empty list of distinct rule TYPE names "
                          "(e.g. \"creation\"), not rule objects")
    if "bypass_actors" in rs:
        actors = rs["bypass_actors"]
        ok = isinstance(actors, list) and all(
            isinstance(a, dict) and set(a) == {"actor_id", "actor_type", "bypass_mode"}
            and (a["actor_id"] is None or (isinstance(a["actor_id"], int) and not isinstance(a["actor_id"], bool)))
            and isinstance(a["actor_type"], str) and isinstance(a["bypass_mode"], str)
            for a in actors)
        if not ok:
            errors.append("`tag_ruleset.bypass_actors` must be a list of {actor_id, actor_type, bypass_mode}")
    return errors


def check(root):
    """Return (errors, resolved) for the repo at `root`."""
    path = os.path.join(root, BASELINE_REL)
    try:
        with io.open(path, encoding="utf-8") as fh:
            data = json.load(fh)
    except (IOError, OSError, ValueError) as exc:
        return ["cannot read %s: %s" % (BASELINE_REL, exc)], []

    errors, names = _validate_baseline(data)
    index = _index_jobs(root)
    resolved = []
    for name in names:
        cands = index.get(name, [])
        if not cands:
            wild = [k for k in index if k.endswith(" / *") and name.startswith(k[:-1])]
            hint = (" (it would come from %s)" % index[wild[0]][0]["where"]) if wild else ""
            errors.append(
                "`%s` does not resolve to any job's check name in %s%s. If a job was renamed, "
                "branch protection must change in the same window or every PR blocks forever."
                % (name, WORKFLOWS_REL, hint))
            continue
        if len(cands) > 1:
            errors.append("`%s` is ambiguous: %d jobs report that check name (%s)"
                          % (name, len(cands), "; ".join(c["where"] for c in cands)))
            continue
        cand = cands[0]
        problems = list(cand["problems"])
        if not cand["pr"]:
            problems.insert(0, "its workflow is not triggered by `pull_request`")
        if problems:
            errors.append("`%s` (%s) cannot be a required check: %s"
                          % (name, cand["where"], "; ".join(problems)))
            continue
        resolved.append((name, cand["where"]))
    return errors, resolved


# ── self-test ────────────────────────────────────────────────────────────────

GOOD_CI = """\
name: CI
on:
  push:
    branches: [main]
  pull_request:
    branches: [main]
jobs:
  fmt:
    name: rustfmt
    runs-on: ubuntu-latest
    steps: [{run: "true"}]
  msrv:
    name: msrv
    runs-on: ubuntu-latest
    steps: [{run: "true"}]
  pre:
    runs-on: ubuntu-latest
    if: github.event_name == 'push'
    steps: [{run: "true"}]
"""

GOOD_LINT = """\
on: pull_request
jobs:
  lint:
    runs-on: ubuntu-latest
    steps: [{run: "true"}]
"""

GOOD_BASELINE = {
    "strict": True,
    "required": [{"context": "rustfmt", "app_id": ACTIONS_APP_ID},
                 {"context": "lint", "app_id": ACTIONS_APP_ID}],
    "advisory_pending": ["msrv"],
    "enforce_admins": True,
    "pending_settings": True,
    "tag_ruleset": {
        "name": "protect-release-tags", "target": "tag", "enforcement": "active",
        "conditions": {"ref_name": {"include": ["~ALL"], "exclude": []}},
        "rules": ["creation", "update", "deletion", "non_fast_forward"],
        "bypass_actors": [{"actor_id": 4260407, "actor_type": "Integration", "bypass_mode": "always"}],
    },
}


def _with_rs(**changes):
    b = json.loads(json.dumps(GOOD_BASELINE))
    b["tag_ruleset"].update(changes)
    return b


def _fixture(tmp, files, baseline):
    root = tempfile.mkdtemp(dir=tmp)
    os.makedirs(os.path.join(root, WORKFLOWS_REL))
    for rel, body in files.items():
        with io.open(os.path.join(root, WORKFLOWS_REL, rel), "w", encoding="utf-8") as fh:
            fh.write(body)
    with io.open(os.path.join(root, BASELINE_REL), "w", encoding="utf-8") as fh:
        fh.write(baseline if isinstance(baseline, str) else json.dumps(baseline))
    return root


def _with(**changes):
    b = json.loads(json.dumps(GOOD_BASELINE))
    b.update(changes)
    return b


def _ci_job(extra, name="msrv"):
    return GOOD_CI.replace("    name: %s\n" % name, "    name: %s\n%s" % (name, extra), 1)


def self_test():
    # (label, workflow files, baseline, expected substring or None for "passes")
    base = {"ci.yml": GOOD_CI, "lint.yml": GOOD_LINT}
    reusable_caller = """\
on: {pull_request: {branches: ["ma*"]}}
jobs:
  call:
    uses: ./.github/workflows/reusable.yml
"""
    reusable = """\
on: workflow_call
jobs:
  inner:
    name: smoke
    runs-on: ubuntu-latest
    steps: [{run: "true"}]
"""
    cases = [
        ("positive control: the good fixture passes", base, GOOD_BASELINE, None),
        ("`on:` as a list resolves", dict(base, **{"lint.yml": GOOD_LINT.replace("on: pull_request", "on: [push, pull_request]")}),
         GOOD_BASELINE, None),
        ("local reusable workflow resolves as `caller / callee`",
         dict(base, **{"caller.yml": reusable_caller, "reusable.yml": reusable}),
         _with(advisory_pending=["msrv", "call / smoke"]), None),
        ("bogus name in `required`", base,
         _with(required=GOOD_BASELINE["required"] + [{"context": "no-such-job", "app_id": ACTIONS_APP_ID}]),
         "`no-such-job` does not resolve"),
        ("bogus name in `advisory_pending`", base, _with(advisory_pending=["msrv", "docker"]),
         "`docker` does not resolve"),
        ("required job renamed in the workflow", dict(base, **{"ci.yml": GOOD_CI.replace("name: rustfmt", "name: fmt")}),
         GOOD_BASELINE, "`rustfmt` does not resolve"),
        ("job deleted (falls back to id, which differs)",
         dict(base, **{"ci.yml": GOOD_CI.replace("    name: msrv\n", "")}),
         _with(advisory_pending=["msrv (1.88)"]), "`msrv (1.88)` does not resolve"),
        ("entry without app_id pin", base,
         _with(required=[{"context": "rustfmt"}, {"context": "lint", "app_id": ACTIONS_APP_ID}]),
         "`rustfmt` is not pinned to an app_id"),
        ("entry with null app_id", base,
         _with(required=[{"context": "rustfmt", "app_id": None}, {"context": "lint", "app_id": ACTIONS_APP_ID}]),
         "`rustfmt` is not pinned to an app_id"),
        ("entry pinned to a non-Actions app", base,
         _with(required=[{"context": "rustfmt", "app_id": 8329}, {"context": "lint", "app_id": ACTIONS_APP_ID}]),
         "pinned to app_id 8329"),
        ("bare string entry in `required`", base, _with(required=["rustfmt"]), "`required[0]` must be an object"),
        ("empty `required`", base, _with(required=[]), "`required` must be a non-empty list"),
        ("missing `strict`", base, {k: v for k, v in GOOD_BASELINE.items() if k != "strict"}, "missing key `strict`"),
        ("non-bool `strict`", base, _with(strict="true"), "`strict` must be true or false"),
        ("unknown top-level key", base, _with(stric=True), "unknown key `stric`"),
        ("duplicate name across lists", base, _with(advisory_pending=["msrv", "lint"]), "`lint` is listed more than once"),
        ("invalid JSON", base, "{not json", "cannot read"),
        ("missing `enforce_admins`", base, {k: v for k, v in GOOD_BASELINE.items() if k != "enforce_admins"},
         "missing key `enforce_admins`"),
        ("non-bool `pending_settings`", base, _with(pending_settings="yes"), "`pending_settings` must be true or false"),
        ("tag_ruleset targeting branches", base, _with_rs(target="branch"), "`tag_ruleset.target` must be \"tag\""),
        ("tag_ruleset unknown enforcement", base, _with_rs(enforcement="on"), "`tag_ruleset.enforcement` must be one of"),
        ("tag_ruleset rules as API objects", base, _with_rs(rules=[{"type": "creation"}]), "rule TYPE names"),
        ("tag_ruleset duplicate rule", base, _with_rs(rules=["creation", "creation"]), "rule TYPE names"),
        ("tag_ruleset empty include", base, _with_rs(conditions={"ref_name": {"include": [], "exclude": []}}),
         "include` must be a non-empty list"),
        ("tag_ruleset unknown key", base, _with_rs(id=7), "`tag_ruleset` has unknown key(s) id"),
        ("tag_ruleset malformed bypass actor", base, _with_rs(bypass_actors=[{"actor_id": "4260407"}]),
         "`tag_ruleset.bypass_actors` must be"),
        ("job-level if:", dict(base, **{"ci.yml": _ci_job("    if: github.event_name == 'pull_request'\n")}),
         GOOD_BASELINE, "job-level `if:`"),
        ("needs:", dict(base, **{"ci.yml": _ci_job("    needs: fmt\n")}), GOOD_BASELINE, "has `needs:`"),
        ("continue-on-error", dict(base, **{"ci.yml": _ci_job("    continue-on-error: true\n")}),
         GOOD_BASELINE, "`continue-on-error`"),
        ("matrix", dict(base, **{"ci.yml": _ci_job("    strategy:\n      matrix:\n        os: [a, b]\n")}),
         GOOD_BASELINE, "has a matrix"),
        ("${{ }} in name (listed literally)", dict(base, **{"ci.yml": GOOD_CI.replace("name: msrv", "name: \"msrv ${{ env.V }}\"")}),
         _with(advisory_pending=["msrv ${{ env.V }}"]), "contains `${{ }}`"),
        ("${{ }} in name (listed as rendered)", dict(base, **{"ci.yml": GOOD_CI.replace("name: msrv", "name: \"msrv ${{ env.V }}\"")}),
         GOOD_BASELINE, "`msrv` does not resolve"),
        ("paths filter", dict(base, **{"lint.yml": GOOD_LINT.replace("on: pull_request", "on: {pull_request: {paths: ['src/**']}}")}),
         GOOD_BASELINE, "`on.pull_request.paths` is set"),
        ("paths-ignore filter", dict(base, **{"lint.yml": GOOD_LINT.replace("on: pull_request", "on: {pull_request: {paths-ignore: ['docs/**']}}")}),
         GOOD_BASELINE, "`on.pull_request.paths-ignore` is set"),
        ("branches-ignore filter", dict(base, **{"lint.yml": GOOD_LINT.replace("on: pull_request", "on: {pull_request: {branches-ignore: ['x']}}")}),
         GOOD_BASELINE, "`on.pull_request.branches-ignore` is set"),
        ("branches ['*', '!main'] excludes main (last match wins)",
         dict(base, **{"lint.yml": GOOD_LINT.replace("on: pull_request", "on: {pull_request: {branches: ['*', '!main']}}")}),
         GOOD_BASELINE, "does not match `main`"),
        ("branches ['**', '!ma*'] excludes main",
         dict(base, **{"lint.yml": GOOD_LINT.replace("on: pull_request", "on: {pull_request: {branches: ['**', '!ma*']}}")}),
         GOOD_BASELINE, "does not match `main`"),
        ("branches ['main'] passes",
         dict(base, **{"lint.yml": GOOD_LINT.replace("on: pull_request", "on: {pull_request: {branches: ['main']}}")}),
         GOOD_BASELINE, None),
        ("branches ['**'] passes",
         dict(base, **{"lint.yml": GOOD_LINT.replace("on: pull_request", "on: {pull_request: {branches: ['**']}}")}),
         GOOD_BASELINE, None),
        ("branches ['!main', 'main'] passes (a later positive re-includes)",
         dict(base, **{"lint.yml": GOOD_LINT.replace("on: pull_request", "on: {pull_request: {branches: ['!main', 'main']}}")}),
         GOOD_BASELINE, None),
        ("branches filter that excludes main", dict(base, **{"lint.yml": GOOD_LINT.replace("on: pull_request", "on: {pull_request: {branches: [develop]}}")}),
         GOOD_BASELINE, "does not match `main`"),
        ("types filter without synchronize", dict(base, **{"lint.yml": GOOD_LINT.replace("on: pull_request", "on: {pull_request: {types: [opened]}}")}),
         GOOD_BASELINE, "omits reopened, synchronize"),
        ("push-only workflow", dict(base, **{"lint.yml": GOOD_LINT.replace("on: pull_request", "on: push")}),
         GOOD_BASELINE, "not triggered by `pull_request`"),
        ("same check name in two workflows", dict(base, **{"other.yml": GOOD_LINT.replace("on: pull_request", "on: push")}),
         GOOD_BASELINE, "`lint` is ambiguous"),
        ("bare callee name of a reusable workflow",
         dict(base, **{"caller.yml": reusable_caller, "reusable.yml": reusable}),
         _with(advisory_pending=["msrv", "smoke"]), "`smoke` does not resolve"),
        ("callee job with if: is rejected through the caller",
         dict(base, **{"caller.yml": reusable_caller, "reusable.yml": reusable.replace("    name: smoke\n", "    name: smoke\n    if: false\n")}),
         _with(advisory_pending=["msrv", "call / smoke"]), "job-level `if:`"),
    ]

    tmp = tempfile.mkdtemp(prefix="required-checks-selftest-")
    failures = 0
    try:
        for label, files, baseline, expect in cases:
            errors, _ = check(_fixture(tmp, files, baseline))
            if expect is None:
                ok = not errors
                detail = "; ".join(errors)
            else:
                ok = any(expect in e for e in errors)
                detail = "expected an error containing %r, got: %s" % (expect, errors or "NO ERRORS (guard passed vacuously)")
            print("%s  %s" % ("ok  " if ok else "FAIL", label))
            if not ok:
                print("      " + detail)
                failures += 1
    finally:
        shutil.rmtree(tmp, ignore_errors=True)
    print("self-test: %d case(s), %d failure(s)" % (len(cases), failures))
    return 1 if failures else 0


def main(argv):
    mode = argv[1] if len(argv) > 1 else "--check"
    if mode == "--self-test":
        return self_test()
    if mode != "--check":
        sys.stderr.write("usage: %s [--check|--self-test]\n" % argv[0])
        return 2
    root = os.path.abspath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", ".."))
    errors, resolved = check(root)
    for name, where in resolved:
        print("resolved  %-30s <- %s" % (name, where))
    for e in errors:
        print("::error file=%s::%s" % (BASELINE_REL, e))
    if errors:
        return 1
    if not resolved:
        # A guard that resolved nothing reports success identically to one that found nothing wrong.
        print("::error::resolved 0 check names from %s; the guard checked nothing" % BASELINE_REL)
        return 1
    print("%d required/advisory check name(s) resolve to PR-triggered, unskippable jobs" % len(resolved))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
