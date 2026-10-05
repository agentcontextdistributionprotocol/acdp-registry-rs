#!/usr/bin/env python3
"""Self-test for check_crypto_exemptions.py (#405).

Runs in the advisory `cargo-vet` job, before the guard itself, so a broken
reader cannot pass silently. Every fixture is a scratch repo with the four
files the guard reads, in their real shapes (config.toml as cargo-vet writes
it, Cargo.lock as cargo writes it); the real supply-chain/ is never modified.
The last test runs the guard against the real tree.
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
SCRIPT = os.path.join(HERE, "check_crypto_exemptions.py")
REPO = os.path.abspath(os.path.join(HERE, "..", ".."))

EXIT_OK, EXIT_VIOLATION, EXIT_UNREADABLE = 0, 1, 2

CRITICAL = """\
# comment
ed25519-dalek
sha2
digest
"""

ALLOWED = """\
# comment
digest 0.10.7 acdp-rs#339 older-line: via jsonwebtoken
"""

CONFIG = """\

# cargo-vet config file

[cargo-vet]
version = "0.10"

[imports.acdp-rs]
url = "https://example.invalid/audits.toml"

[policy.acdp-registry-core]
audit-as-crates-io = false

[[exemptions.axum]]
version = "0.8.4"
criteria = "safe-to-deploy"

[[exemptions.digest]]
version = "0.10.7"
criteria = "safe-to-deploy"
"""

LOCK = """\
version = 4

[[package]]
name = "axum"
version = "0.8.4"

[[package]]
name = "digest"
version = "0.10.7"

[[package]]
name = "ed25519-dalek"
version = "2.2.0"

[[package]]
name = "sha2"
version = "0.10.9"
"""


class Fixture(unittest.TestCase):
    def setUp(self):
        self.root = tempfile.mkdtemp(prefix="crypto-exemptions-")
        self.addCleanup(shutil.rmtree, self.root)
        os.mkdir(os.path.join(self.root, "supply-chain"))
        self.write(critical=CRITICAL, allowed=ALLOWED, config=CONFIG, lock=LOCK)

    def write(self, critical=None, allowed=None, config=None, lock=None):
        for rel, text in (
            ("supply-chain/crypto-critical.txt", critical),
            ("supply-chain/crypto-exemptions-allowed.txt", allowed),
            ("supply-chain/config.toml", config),
            ("Cargo.lock", lock),
        ):
            if text is not None:
                with open(os.path.join(self.root, rel), "w", encoding="utf-8") as f:
                    f.write(text)

    def run_guard(self, root=None):
        p = subprocess.run(
            [sys.executable, SCRIPT, "--root", root or self.root],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            universal_newlines=True,
        )
        return p.returncode, p.stdout + p.stderr

    def assertGuard(self, code, *needles):
        rc, out = self.run_guard()
        self.assertEqual(rc, code, out)
        for n in needles:
            self.assertIn(n, out)
        return out


class Passes(Fixture):
    def test_allow_listed_exemption_and_non_crypto_exemption_pass(self):
        out = self.assertGuard(EXIT_OK, "allowed exempt: digest 0.10.7 (acdp-rs#339)")
        self.assertNotIn("axum", out)

    def test_quoted_exemption_header_is_read(self):
        self.write(config=CONFIG.replace("[[exemptions.digest]]", '[[exemptions."digest"]]'))
        self.assertGuard(EXIT_OK, "allowed exempt: digest 0.10.7")


class FailureModeA_ExemptedNotAllowListed(Fixture):
    """(a) a crypto-critical crate exempted but not on the allow-list."""

    def test_audited_crypto_crate_becoming_exempted_fails(self):
        # The issue's acceptance case: ed25519-dalek is audited (no exemption),
        # then a bump gets exempted by `cargo vet regenerate exemptions`.
        self.write(
            config=CONFIG
            + '\n[[exemptions.ed25519-dalek]]\nversion = "2.2.1"\ncriteria = "safe-to-deploy"\n'
        )
        self.assertGuard(
            EXIT_VIOLATION,
            "ed25519-dalek 2.2.1 is crypto-critical but EXEMPTED",
            "1 violation(s)",
        )

    def test_moving_an_allowed_exemption_to_a_new_version_fails(self):
        # Allow-list pins the exact version: re-exempting a bump is not covered,
        # and the old entry goes stale at the same time.
        self.write(config=CONFIG.replace('version = "0.10.7"', 'version = "0.10.8"'))
        self.assertGuard(
            EXIT_VIOLATION,
            "digest 0.10.8 is crypto-critical but EXEMPTED",
            "stale entry 'digest 0.10.7'",
            "2 violation(s)",
        )


class FailureModeB_StaleAllowListEntry(Fixture):
    """(b) an allow-list entry that config.toml no longer exempts."""

    def test_entry_whose_exemption_was_removed_fails(self):
        self.write(
            config=CONFIG.replace(
                '[[exemptions.digest]]\nversion = "0.10.7"\ncriteria = "safe-to-deploy"\n', ""
            )
        )
        out = self.assertGuard(
            EXIT_VIOLATION,
            "crypto-exemptions-allowed.txt:2: stale entry 'digest 0.10.7'",
            "1 violation(s)",
        )
        self.assertNotIn("EXEMPTED", out)


class Unreadable(Fixture):
    def test_unknown_exemption_shape_fails_loud(self):
        self.write(config=CONFIG + '\n[exemptions]\nsha2 = [{ version = "0.10.9" }]\n')
        self.assertGuard(EXIT_UNREADABLE, "unrecognised exemptions table")

    def test_dotted_exemption_key_fails_loud(self):
        self.write(config=CONFIG + '\nexemptions.sha2 = [{ version = "0.10.9" }]\n')
        self.assertGuard(EXIT_UNREADABLE, "dotted or inline key")

    def test_exemption_without_version_fails_loud(self):
        self.write(config=CONFIG + '\n[[exemptions.sha2]]\ncriteria = "safe-to-deploy"\n')
        self.assertGuard(EXIT_UNREADABLE, "[[exemptions.sha2]] has no version line")

    def test_critical_name_absent_from_lock_fails(self):
        self.write(critical=CRITICAL + "ed25519-dalekk\n")
        self.assertGuard(EXIT_UNREADABLE, "absent from Cargo.lock", "ed25519-dalekk")

    def test_allow_list_entry_without_reason_fails(self):
        self.write(allowed="digest 0.10.7 acdp-rs#339\n")
        self.assertGuard(EXIT_UNREADABLE, "<tracking> <reason>")

    def test_allow_list_entry_with_bad_tracking_fails(self):
        self.write(allowed="digest 0.10.7 soon because reasons\n")
        self.assertGuard(EXIT_UNREADABLE, "tracking 'soon'")

    def test_allow_list_version_range_fails(self):
        self.write(allowed="digest 0.10.* acdp-rs#339 older-line\n")
        self.assertGuard(EXIT_UNREADABLE, "is not an exact version")

    def test_allow_list_entry_for_non_critical_crate_fails(self):
        self.write(allowed=ALLOWED + "axum 0.8.4 #1 not crypto\n")
        self.assertGuard(EXIT_UNREADABLE, "not in supply-chain/crypto-critical.txt", "axum 0.8.4")

    def test_duplicate_allow_list_entry_fails(self):
        self.write(allowed=ALLOWED + "digest 0.10.7 acdp-rs#339 again\n")
        self.assertGuard(EXIT_UNREADABLE, "duplicate entry digest 0.10.7")

    def test_empty_critical_list_fails(self):
        self.write(critical="# nothing\n")
        self.assertGuard(EXIT_UNREADABLE, "names no crates")


class RealTree(Fixture):
    def test_guard_passes_on_the_real_tree(self):
        rc, out = self.run_guard(root=REPO)
        self.assertEqual(rc, EXIT_OK, out)
        # The real allow-list is not empty today, so a reader that silently
        # found no exemptions would show up here as a stale-entry failure, and
        # one that found no allow-list as violations; pin a known entry too.
        self.assertIn("allowed exempt: digest 0.10.7", out)


if __name__ == "__main__":
    unittest.main()
