#!/usr/bin/env python3
"""Unit tests for check_acdp_pin.py: a passing fixture tree, one negative
control per coupled place the guard exists to keep in step (each a shape a
real bot bump produced), the unreadable-input exits, and the real tree."""

import os
import shutil
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
SCRIPT = os.path.join(HERE, "check_acdp_pin.py")
REPO = os.path.dirname(os.path.dirname(HERE))
CRATES_IO = "registry+https://github.com/rust-lang/crates.io-index"

WS_MANIFEST = """[workspace.dependencies]
acdp = {{ version = "{req}", features = ["server", "client"] }}
axum = {{ version = "0.8" }}
"""
SMOKE_MANIFEST = """[dependencies]
acdp       = {{ version = "{req}", features = ["client"] }}
"""


def lock(packages):
    out = ["version = 4", ""]
    for name, version, source in packages:
        out += ["[[package]]", f'name = "{name}"', f'version = "{version}"']
        if source:
            out.append(f'source = "{source}"')
        out.append("")
    return "\n".join(out)


def exemptions(entries):
    out = ["[cargo-vet]", 'version = "0.10"', ""]
    for name, version in entries:
        out += [f"[[exemptions.{name}]]", f'version = "{version}"', 'criteria = "safe-to-deploy"', ""]
    return "\n".join(out)


class Tree:
    """A minimal repo tree in a temp dir, good by default at 1.2.3."""

    def __init__(self):
        self.root = tempfile.mkdtemp(prefix="acdp-pin-")
        os.makedirs(os.path.join(self.root, "tools/live-smoke"))
        os.makedirs(os.path.join(self.root, "supply-chain"))
        self.ws_req = "=1.2.3"
        self.smoke_req = "=1.2.3"
        self.ws_lock = [
            ("acdp", "1.2.3", CRATES_IO),
            ("acdp-types", "1.2.3", CRATES_IO),
            ("acdp-registry-core", "0.7.0", None),
        ]
        self.smoke_lock = [("acdp", "1.2.3", CRATES_IO), ("acdp-types", "1.2.3", CRATES_IO)]
        self.exempt = [("acdp", "1.2.3"), ("acdp-types", "1.2.3"), ("axum", "0.8.0")]

    def write(self):
        files = {
            "Cargo.toml": WS_MANIFEST.format(req=self.ws_req),
            "tools/live-smoke/Cargo.toml": SMOKE_MANIFEST.format(req=self.smoke_req),
            "Cargo.lock": lock(self.ws_lock),
            "tools/live-smoke/Cargo.lock": lock(self.smoke_lock),
            "supply-chain/config.toml": exemptions(self.exempt),
        }
        for rel, body in files.items():
            with open(os.path.join(self.root, rel), "w", encoding="utf-8") as f:
                f.write(body)
        return self

    def run(self):
        p = subprocess.run(
            [sys.executable, SCRIPT, "--root", self.root], capture_output=True, text=True
        )
        return p.returncode, p.stderr

    def close(self):
        shutil.rmtree(self.root, ignore_errors=True)


class Fixtures(unittest.TestCase):
    def setUp(self):
        self.t = Tree()

    def tearDown(self):
        self.t.close()

    def assertFails(self, needle):
        rc, err = self.t.write().run()
        self.assertEqual(rc, 1, err)
        self.assertIn(needle, err)

    def test_consistent_tree_passes(self):
        rc, err = self.t.write().run()
        self.assertEqual(rc, 0, err)

    def test_caret_pin_in_workspace_fails(self):
        # The exact shape bump-consume.yml writes: the `=` dropped.
        self.t.ws_req = "1.2.3"
        self.assertFails("is not an exact pin")

    def test_live_smoke_left_behind_fails(self):
        self.t.smoke_req = "=1.2.2"
        self.assertFails("tools/live-smoke/Cargo.toml")

    def test_live_smoke_lock_left_behind_fails(self):
        self.t.smoke_lock = [("acdp", "1.2.2", CRATES_IO), ("acdp-types", "1.2.2", CRATES_IO)]
        self.assertFails("tools/live-smoke/Cargo.lock: acdp locked at 1.2.2")

    def test_workspace_lock_disagreeing_with_pin_fails(self):
        self.t.ws_lock[1] = ("acdp-types", "1.2.4", CRATES_IO)
        self.assertFails("Cargo.lock: acdp-types locked at 1.2.4")

    def test_stale_vet_exemption_fails(self):
        self.t.exempt = [("acdp", "1.2.2"), ("acdp-types", "1.2.2")]
        self.assertFails("[[exemptions.acdp]] at 1.2.2")

    def test_missing_vet_exemption_fails(self):
        self.t.exempt = [("acdp", "1.2.3")]
        self.assertFails("no [[exemptions.acdp-types]] at 1.2.3")

    def test_workspace_crates_without_source_are_not_sdk_crates(self):
        # acdp-registry-core has no source line and no exemption: still passes.
        rc, err = self.t.write().run()
        self.assertEqual(rc, 0, err)

    def test_missing_manifest_is_unreadable(self):
        self.t.write()
        os.remove(os.path.join(self.t.root, "tools/live-smoke/Cargo.toml"))
        rc, err = self.t.run()
        self.assertEqual(rc, 2, err)

    def test_manifest_without_acdp_is_unreadable(self):
        self.t.write()
        with open(os.path.join(self.t.root, "Cargo.toml"), "w") as f:
            f.write("[workspace.dependencies]\naxum = \"0.8\"\n")
        rc, err = self.t.run()
        self.assertEqual(rc, 2, err)


@unittest.skipIf(os.environ.get("ACDP_PIN_GUARD_SKIP_REAL_TREE") == "1", "real tree skipped")
class RealTree(unittest.TestCase):
    def test_this_repository_is_consistent(self):
        p = subprocess.run([sys.executable, SCRIPT, "--root", REPO], capture_output=True, text=True)
        self.assertEqual(p.returncode, 0, p.stderr)


if __name__ == "__main__":
    unittest.main()
