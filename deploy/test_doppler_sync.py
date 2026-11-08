#!/usr/bin/env python3
"""Offline tests for deploy/doppler-sync.sh.

Runs the real script against fake `curl` and `doppler` binaries (and a `jq`
shim that logs its argv, then runs the real jq), in a throwaway git repo. No
network, no Railway, no Doppler.

What they prove:
  * Fail closed. A bad Railway token answers HTTP 200 with `data: null` and an
    `errors` array; a script that reads that as "no variables" lists every key as
    new and --apply rewrites them all. Every failure mode below (on the project
    read, the variables read, and the Doppler read, in dry-run and --apply) must
    exit non-zero, report no key as new or changed, and write nothing.
  * Secrets stay off argv. The sentinel token and secret values must never appear
    in the argv of curl, jq or doppler, nor in the script's output. The token must
    still reach curl, as a header read from a file descriptor.
  * The happy paths still work: unchanged, changed (dry run), --apply and
    --no-redeploy.

    python3 deploy/test_doppler_sync.py -v
    DOPPLER_SYNC_SCRIPT=/path/to/other/doppler-sync.sh python3 deploy/test_doppler_sync.py
"""

import json
import os
import shutil
import subprocess
import sys
import tempfile
import textwrap
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
SCRIPT = os.environ.get("DOPPLER_SYNC_SCRIPT", os.path.join(HERE, "doppler-sync.sh"))

TOKEN = "SENTINEL-TOKEN-7f3a9c1e"
SECRET = "SENTINEL-SECRET-b42d0e88"
SECRET_NEW = "SENTINEL-SECRET-NEW-91ce55a0"
KEYS = ["APP_SECRET", "APP_LEVEL"]
DOPPLER = {"APP_SECRET": SECRET, "APP_LEVEL": "3", "DOPPLER_PROJECT": "x"}
SENTINELS = [TOKEN, SECRET, SECRET_NEW]

# argv of every faked or shimmed binary is appended here as one JSON line.
FAKE_CURL = r'''#!/usr/bin/env python3
import json, os, sys
log = os.environ["FAKE_LOG"]
def rec(**kw):
    with open(log, "a") as f:
        f.write(json.dumps(kw) + "\n")
argv = sys.argv[1:]
rec(tool="curl", argv=argv)
token_ok = False
for i, a in enumerate(argv):
    if a == "-H" and i + 1 < len(argv) and argv[i + 1].startswith("@"):
        with open(argv[i + 1][1:]) as f:
            for line in f.read().splitlines():
                if line == "Authorization: Bearer " + os.environ["EXPECT_TOKEN"]:
                    token_ok = True
rec(tool="curl-header", token_via_fd=token_ok)
# Railway itself would also accept the token inline in argv (the old, leaky form);
# authenticate it here too so the leak is reported by the argv check, not hidden
# behind an auth error.
authed = token_ok or ("Authorization: Bearer " + os.environ["EXPECT_TOKEN"]) in argv
body = json.loads(sys.stdin.read())
q = body["query"]
kind = ("ids" if "project(id" in q else "vars" if "variables(" in q else
        "upsert" if "variableUpsert" in q else "redeploy" if "serviceInstanceRedeploy" in q else "other")
calls = os.environ["FAKE_CALLS"]
with open(calls, "a") as f:
    f.write(kind + "\n")
n = sum(1 for l in open(calls) if l.strip() == kind)
cfg = json.load(open(os.environ["FAKE_CFG"]))
fail = cfg.get("fail")
hit = fail and fail["on"] == kind and n >= fail.get("nth", 1)
state_path = os.environ["FAKE_STATE"]
if hit:
    mode = fail["mode"]
    if mode == "http_401":
        sys.stderr.write("curl: (22) The requested URL returned error: 401\n"); sys.exit(22)
    if mode == "http_500":
        sys.stderr.write("curl: (22) The requested URL returned error: 500\n"); sys.exit(22)
    if mode == "network":
        sys.stderr.write("curl: (7) Failed to connect to backboard.railway.com port 443\n"); sys.exit(7)
    out = {
        "errors": '{"data":null,"errors":[{"message":"Not Authorized"}]}',
        "data_null": '{"data":null}',
        "data_missing": '{}',
        "data_null_empty_errors": '{"data":null,"errors":[]}',
        "partial_errors": '{"data":{"variables":{}},"errors":[{"message":"Problem processing request"}]}',
        "garbage": "<html>502 Bad Gateway</html>",
        "empty": "",
        "not_object": "[]",
        "variables_null": '{"data":{"variables":null}}',
        "variables_string": '{"data":{"variables":"oops"}}',
        "variables_array": '{"data":{"variables":[]}}',
    }[mode]
    sys.stdout.write(out); sys.exit(0)
if not authed:
    # What Railway does for a missing or wrong token: HTTP 200, data null, errors.
    print('{"data":null,"errors":[{"message":"Not Authorized"}]}'); sys.exit(0)
if kind == "ids":
    print(json.dumps({"data": {"project": {
        "environments": {"edges": [{"node": {"id": "env-1", "name": "production"}}]},
        "services": {"edges": [{"node": {"id": "svc-1", "name": "acdp-registry"}}]}}}}))
elif kind == "vars":
    print(json.dumps({"data": {"variables": json.load(open(state_path))}}))
elif kind == "upsert":
    i = body["variables"]["i"]
    state = json.load(open(state_path)); state[i["name"]] = i["value"]
    json.dump(state, open(state_path, "w"))
    with open(os.environ["FAKE_WRITES"], "a") as f:
        f.write("upsert " + i["name"] + "\n")
    print('{"data":{"variableUpsert":true}}')
elif kind == "redeploy":
    with open(os.environ["FAKE_WRITES"], "a") as f:
        f.write("redeploy\n")
    print('{"data":{"serviceInstanceRedeploy":true}}')
else:
    sys.exit(99)
'''

FAKE_DOPPLER = r'''#!/usr/bin/env python3
import json, os, sys
with open(os.environ["FAKE_LOG"], "a") as f:
    f.write(json.dumps({"tool": "doppler", "argv": sys.argv[1:]}) + "\n")
mode = json.load(open(os.environ["FAKE_CFG"])).get("doppler", "ok")
if mode == "fail":
    sys.stderr.write("Doppler Error: Unable to download secrets\n"); sys.exit(1)
if mode == "garbage":
    sys.stdout.write("not json"); sys.exit(0)
if mode == "empty":
    sys.exit(0)
sys.stdout.write(open(os.environ["FAKE_DOPPLER_DATA"]).read())
'''

FAKE_RAILWAY = r'''#!/usr/bin/env python3
import sys
sys.stderr.write("the railway CLI must not be called\n"); sys.exit(97)
'''

JQ_SHIM = '''#!/usr/bin/env bash
printf '%s\\0' "$@" | python3 -c 'import json,os,sys; open(os.environ["FAKE_LOG"],"a").write(json.dumps({"tool":"jq","argv":sys.stdin.buffer.read().decode().split("\\0")[:-1]})+"\\n")'
exec {real} "$@"
'''

def _read(path):
    with open(path) as f:
        return f.read()


def _write(path, text):
    with open(path, "w") as f:
        f.write(text)


RAILWAY_FAILURES = ["errors", "data_null", "data_missing", "data_null_empty_errors", "partial_errors",
                    "http_401", "http_500", "network", "garbage", "empty", "not_object"]


class Harness:
    def __init__(self, railway_state, cfg):
        self.tmp = tempfile.mkdtemp(prefix="doppler-sync-test-")
        t = self.tmp
        self.bin = os.path.join(t, "bin")
        self.repo = os.path.join(t, "repo")
        self.home = os.path.join(t, "home")
        for d in (self.bin, os.path.join(self.repo, "deploy"), self.home):
            os.makedirs(d)
        real_jq = shutil.which("jq")
        assert real_jq, "jq is required"
        for name, body in (("curl", FAKE_CURL), ("doppler", FAKE_DOPPLER), ("railway", FAKE_RAILWAY),
                           ("jq", JQ_SHIM.replace("{real}", real_jq))):
            p = os.path.join(self.bin, name)
            with open(p, "w") as f:
                f.write(body)
            os.chmod(p, 0o755)
        shutil.copy(SCRIPT, os.path.join(self.repo, "deploy", "doppler-sync.sh"))
        with open(os.path.join(self.repo, "deploy", "doppler-keys.txt"), "w") as f:
            f.write("# test allowlist\n" + "\n".join(KEYS) + "\n")
        git = ["git", "-C", self.repo, "-c", "user.name=t", "-c", "user.email=t@example.invalid"]
        subprocess.run(["git", "init", "-q", self.repo], check=True)
        subprocess.run(git + ["add", "-A"], check=True)
        subprocess.run(git + ["commit", "-qm", "init"], check=True)
        self.p = {k: os.path.join(t, k) for k in ("log", "calls", "writes", "state", "cfg", "doppler.json")}
        for k in ("log", "calls", "writes"):
            _write(self.p[k], "")
        _write(self.p["state"], json.dumps(railway_state))
        _write(self.p["cfg"], json.dumps(cfg))
        _write(self.p["doppler.json"], json.dumps(DOPPLER))

    def run(self, *args, env_override=None):
        env = {
            "PATH": self.bin + os.pathsep + os.environ["PATH"],
            "HOME": self.home,
            "RAILWAY_API_TOKEN": TOKEN,
            "EXPECT_TOKEN": TOKEN,
            "FAKE_LOG": self.p["log"], "FAKE_CALLS": self.p["calls"], "FAKE_WRITES": self.p["writes"],
            "FAKE_STATE": self.p["state"], "FAKE_CFG": self.p["cfg"], "FAKE_DOPPLER_DATA": self.p["doppler.json"],
            "LC_ALL": "C",
        }
        for k, v in (env_override or {}).items():
            if v is None:
                env.pop(k, None)
            else:
                env[k] = v
        r = subprocess.run(["bash", os.path.join(self.repo, "deploy", "doppler-sync.sh"), *args],
                           env=env, capture_output=True, text=True, timeout=60)
        self.out = r.stdout + r.stderr
        self.code = r.returncode
        self.log = [json.loads(l) for l in _read(self.p["log"]).splitlines() if l.strip()]
        self.writes = [l.strip() for l in _read(self.p["writes"]).splitlines() if l.strip()]
        self.state = json.loads(_read(self.p["state"]))
        return self

    def close(self):
        shutil.rmtree(self.tmp, ignore_errors=True)


class Base(unittest.TestCase):
    def harness(self, railway_state=None, **cfg):
        h = Harness(dict(DOPPLER) if railway_state is None else railway_state, cfg)
        self.addCleanup(h.close)
        return h

    def assert_no_leak(self, h):
        for s in SENTINELS:
            self.assertNotIn(s, h.out, "a sentinel value appeared in the script's output")
            for entry in h.log:
                for a in entry.get("argv", []):
                    self.assertNotIn(s, a, "a sentinel value appeared in %s argv" % entry["tool"])
        headers = [e for e in h.log if e["tool"] == "curl-header"]
        self.assertTrue(headers, "curl was never called")
        for e in headers:
            self.assertTrue(e["token_via_fd"], "curl did not receive the token as a header on a file descriptor")

    def assert_failed_closed(self, h):
        self.assertNotEqual(h.code, 0, "exited 0 on a failure:\n" + h.out)
        for word in ("\tnew", "\tchanged", "would change", "set APP_", "redeploy triggered"):
            self.assertNotIn(word, h.out, "reported a change on a failure:\n" + h.out)
        self.assertEqual(h.writes, [], "wrote to Railway on a failure")
        for s in SENTINELS:
            self.assertNotIn(s, h.out)
            for entry in h.log:
                for a in entry.get("argv", []):
                    self.assertNotIn(s, a)


class FailClosed(Base):
    """Railway state is empty, so a script that failed open would call every key new."""

    def check(self, **cfg):
        for args in ((), ("--apply",)):
            with self.subTest(args=args, **{k: str(v) for k, v in cfg.items()}):
                h = self.harness(railway_state={}, **cfg).run(*args)
                self.assert_failed_closed(h)

    def test_railway_project_read_failures(self):
        for mode in RAILWAY_FAILURES:
            self.check(fail={"on": "ids", "mode": mode})

    def test_railway_variables_read_failures(self):
        for mode in RAILWAY_FAILURES + ["variables_null", "variables_string", "variables_array"]:
            self.check(fail={"on": "vars", "mode": mode})

    def test_wrong_railway_token(self):
        # Railway answers a bad or expired token with HTTP 200, data null and errors.
        for args in ((), ("--apply",)):
            with self.subTest(args=args):
                h = self.harness(railway_state={}).run(*args, env_override={"RAILWAY_API_TOKEN": TOKEN + "-wrong"})
                self.assert_failed_closed(h)

    def test_no_token_refuses_before_any_request(self):
        h = self.harness(railway_state={}).run(env_override={"RAILWAY_API_TOKEN": None})
        self.assertNotEqual(h.code, 0)
        self.assertIn("no Railway token", h.out)
        self.assertFalse([e for e in h.log if e["tool"] == "curl"], "made a request without a token")

    def test_doppler_read_failures(self):
        for mode in ("fail", "garbage", "empty"):
            self.check(doppler=mode)


class ApplyFailures(Base):
    def test_upsert_failure_stops_before_redeploy(self):
        for mode in ("errors", "http_500", "network"):
            with self.subTest(mode=mode):
                h = self.harness(railway_state={"APP_LEVEL": "3", "APP_SECRET": "old"},
                                 fail={"on": "upsert", "mode": mode}).run("--apply")
                self.assertNotEqual(h.code, 0, h.out)
                self.assertNotIn("redeploy", h.writes)
                self.assertNotIn("redeploy triggered", h.out)
                self.assertEqual(h.state["APP_SECRET"], "old")
                self.assert_no_leak(h)

    def test_post_apply_read_failure_skips_redeploy(self):
        h = self.harness(railway_state={"APP_LEVEL": "3", "APP_SECRET": "old"},
                         fail={"on": "vars", "nth": 2, "mode": "errors"}).run("--apply")
        self.assertNotEqual(h.code, 0, h.out)
        self.assertNotIn("redeploy", h.writes)


class HappyPaths(Base):
    def test_dry_run_unchanged(self):
        h = self.harness().run()
        self.assertEqual(h.code, 0, h.out)
        self.assertIn("APP_SECRET\tunchanged", h.out)
        self.assertIn("APP_LEVEL\tunchanged", h.out)
        self.assertIn("(match)", h.out)
        self.assertIn("dry run: nothing to change", h.out)
        self.assertEqual(h.writes, [])
        self.assert_no_leak(h)

    def test_dry_run_changed_writes_nothing(self):
        h = self.harness(railway_state={"APP_LEVEL": "2", "APP_SECRET": SECRET, "OTHER": "x"}).run("--dry-run")
        self.assertEqual(h.code, 0, h.out)
        self.assertIn("APP_LEVEL\tchanged", h.out)
        self.assertIn("left alone (not in doppler-keys.txt): OTHER", h.out)
        self.assertIn("(differ)", h.out)
        self.assertEqual(h.writes, [])
        self.assert_no_leak(h)

    def test_apply_sets_values_by_stdin_and_redeploys(self):
        h = self.harness(railway_state={"APP_LEVEL": "3", "APP_SECRET": SECRET_NEW})
        h.run("--apply")
        self.assertEqual(h.code, 0, h.out)
        self.assertEqual(h.writes, ["upsert APP_SECRET", "redeploy"])
        self.assertEqual(h.state["APP_SECRET"], SECRET)
        self.assertIn("hash after apply", h.out)
        self.assert_no_leak(h)

    def test_apply_no_redeploy(self):
        h = self.harness(railway_state={"APP_LEVEL": "1"}).run("--apply", "--no-redeploy")
        self.assertEqual(h.code, 0, h.out)
        self.assertEqual(sorted(h.writes), ["upsert APP_LEVEL", "upsert APP_SECRET"])
        self.assert_no_leak(h)

    def test_railway_reference_refused(self):
        h = self.harness(railway_state={"APP_LEVEL": "${{shared.LEVEL}}", "APP_SECRET": SECRET}).run("--apply")
        self.assertNotEqual(h.code, 0)
        self.assertIn("railway-reference", h.out)
        self.assertEqual(h.writes, [])


if __name__ == "__main__":
    if not shutil.which("jq") or not shutil.which("shasum"):
        sys.exit("jq and shasum are required")
    unittest.main()
