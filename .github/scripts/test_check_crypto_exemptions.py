#!/usr/bin/env python3
"""Self-test for check_crypto_exemptions.py (#405).

Runs in two places. In the advisory `cargo-vet` job, before the guard itself,
in full. In the required `tests` job's script-test loop with
ACDP_CRYPTO_GUARD_SKIP_REAL_TREE=1, so a broken guard script goes red on every
PR even when `cargo vet --locked` is red, while the policy verdict on the real
tree (the RealTree class) stays advisory.

Every fixture is a scratch repo with the four files the guard reads, in their
real shapes (config.toml as cargo-vet writes it, Cargo.lock as cargo writes
it); the real supply-chain/ is never modified. The behaviour cases run under
both config.toml readers (tomllib, and the 3.9 line-reader fallback), and
ReadersAgree checks that both return the same exemptions on the real
config.toml. TomlParser and ReadersAgree skip on Python < 3.11.
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
sys.path.insert(0, HERE)
import check_crypto_exemptions as cge  # noqa: E402

SKIP_REAL_TREE = "ACDP_CRYPTO_GUARD_SKIP_REAL_TREE"

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


# The verifier's case: a quoted dotted key at top level is a real exemption of
# sha2 0.11.0 (tomllib reads it so), which the line reader cannot parse.
QUOTED_DOTTED = '"exemptions".sha2 = [{ version = "0.11.0", criteria = "safe-to-deploy" }]\n'


class Fixture(unittest.TestCase):
    PARSER = "auto"

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

    def run_guard(self, root=None, parser=None):
        p = subprocess.run(
            [sys.executable, SCRIPT, "--root", root or self.root, "--parser", parser or self.PARSER],
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


class GuardCases:
    """Behaviour that must hold under BOTH config.toml readers."""

    # --- passes ---
    def test_allow_listed_exemption_and_non_crypto_exemption_pass(self):
        out = self.assertGuard(EXIT_OK, "allowed exempt: digest 0.10.7 (acdp-rs#339)")
        self.assertNotIn("axum", out)

    def test_quoted_exemption_header_is_read(self):
        self.write(config=CONFIG.replace("[[exemptions.digest]]", '[[exemptions."digest"]]'))
        self.assertGuard(EXIT_OK, "allowed exempt: digest 0.10.7")

    # --- failure mode (a): exempted, not allow-listed ---
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
        # The allow-list pins the exact version: re-exempting a bump is not
        # covered, and the old entry goes stale at the same time.
        self.write(config=CONFIG.replace('version = "0.10.7"', 'version = "0.10.8"'))
        self.assertGuard(
            EXIT_VIOLATION,
            "digest 0.10.8 is crypto-critical but EXEMPTED",
            "stale entry 'digest 0.10.7'",
            "2 violation(s)",
        )

    # --- failure mode (b): stale allow-list entry ---
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

    # --- unreadable input ---
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

    # --- other TOML spellings of an exemption: never a silent pass ---
    def test_quoted_dotted_key_exemption_never_passes(self):
        self.write(config=QUOTED_DOTTED + CONFIG)
        rc, out = self.run_guard()
        self.assertNotEqual(rc, EXIT_OK, out)
        self.assertIn(rc, (EXIT_VIOLATION, EXIT_UNREADABLE), out)

    def test_dotted_key_exemption_never_passes(self):
        self.write(config='exemptions.sha2 = [{ version = "0.10.9" }]\n' + CONFIG)
        rc, out = self.run_guard()
        self.assertIn(rc, (EXIT_VIOLATION, EXIT_UNREADABLE), out)

    def test_plain_exemptions_table_never_passes(self):
        self.write(config=CONFIG + '\n[exemptions]\nsha2 = [{ version = "0.10.9" }]\n')
        rc, out = self.run_guard()
        self.assertIn(rc, (EXIT_VIOLATION, EXIT_UNREADABLE), out)

    def test_spaced_header_never_passes(self):
        self.write(config=CONFIG + '\n[[ exemptions.sha2 ]]\nversion = "0.10.9"\n')
        rc, out = self.run_guard()
        self.assertIn(rc, (EXIT_VIOLATION, EXIT_UNREADABLE), out)


class LinesParser(GuardCases, Fixture):
    """The 3.9/3.10 fallback: refuses what it cannot parse."""

    PARSER = "lines"

    def test_quoted_dotted_key_is_refused(self):
        self.write(config=QUOTED_DOTTED + CONFIG)
        self.assertGuard(EXIT_UNREADABLE, "dotted or inline key")

    def test_single_quoted_dotted_key_is_refused(self):
        self.write(config=QUOTED_DOTTED.replace('"exemptions"', "'exemptions'") + CONFIG)
        self.assertGuard(EXIT_UNREADABLE, "dotted or inline key")

    def test_bare_dotted_key_is_refused(self):
        self.write(config='exemptions.sha2 = [{ version = "0.10.9" }]\n' + CONFIG)
        self.assertGuard(EXIT_UNREADABLE, "dotted or inline key")

    def test_plain_exemptions_table_is_refused(self):
        self.write(config=CONFIG + '\n[exemptions]\nsha2 = [{ version = "0.10.9" }]\n')
        self.assertGuard(EXIT_UNREADABLE, "unrecognised exemptions table")

    def test_spaced_header_is_refused(self):
        self.write(config=CONFIG + '\n[[ exemptions.sha2 ]]\nversion = "0.10.9"\n')
        self.assertGuard(EXIT_UNREADABLE, "unrecognised exemptions table")


@unittest.skipUnless(cge.tomllib, "tomllib needs Python 3.11+")
class TomlParser(GuardCases, Fixture):
    """The tomllib reader (CI): reads every spelling, so each is a violation."""

    PARSER = "toml"

    def test_quoted_dotted_key_is_read_as_an_exemption(self):
        self.write(config=QUOTED_DOTTED + CONFIG)
        self.assertGuard(EXIT_VIOLATION, "sha2 0.11.0 is crypto-critical but EXEMPTED")

    def test_bare_dotted_key_is_read_as_an_exemption(self):
        self.write(config='exemptions.sha2 = [{ version = "0.10.9" }]\n' + CONFIG)
        self.assertGuard(EXIT_VIOLATION, "sha2 0.10.9 is crypto-critical but EXEMPTED")

    def test_spaced_header_is_read_as_an_exemption(self):
        self.write(config=CONFIG + '\n[[ exemptions.sha2 ]]\nversion = "0.10.9"\n')
        self.assertGuard(EXIT_VIOLATION, "sha2 0.10.9 is crypto-critical but EXEMPTED")

    def test_invalid_toml_is_refused(self):
        self.write(config=CONFIG + "\n[[exemptions.sha2]\n")
        self.assertGuard(EXIT_UNREADABLE, "is not valid TOML")


@unittest.skipUnless(cge.tomllib, "tomllib needs Python 3.11+")
class ReadersAgree(Fixture):
    """Both readers return the same exemptions, so the verdict cannot depend
    on the runner's Python version. Reader-level only: no policy verdict."""

    def test_readers_agree_on_the_fixture(self):
        path = os.path.join(self.root, "supply-chain", "config.toml")
        self.assertEqual(cge.read_exemptions_lines(path), cge.read_exemptions_toml(path))

    def test_readers_agree_on_the_real_config(self):
        path = os.path.join(REPO, "supply-chain", "config.toml")
        lines = cge.read_exemptions_lines(path)
        self.assertEqual(lines, cge.read_exemptions_toml(path))
        self.assertGreater(len(lines), 100)  # a reader that found nothing agrees too


@unittest.skipIf(
    os.environ.get(SKIP_REAL_TREE) == "1",
    "%s=1: the policy verdict on the real tree runs only in the advisory cargo-vet job"
    % SKIP_REAL_TREE,
)
class RealTree(Fixture):
    """The guard's verdict on the real tree. Skipped in the REQUIRED `tests`
    job (which sets SKIP_REAL_TREE) so the guard stays advisory there; that
    job still runs every fixture test, so a broken script goes red."""

    def test_guard_passes_on_the_real_tree(self):
        rc, out = self.run_guard(root=REPO)
        self.assertEqual(rc, EXIT_OK, out)
        # A reader that silently found no exemptions would fail on stale
        # entries, one that found no allow-list on violations; pin one too.
        self.assertIn("allowed exempt: digest 0.10.7", out)

    def test_both_readers_give_the_same_verdict_on_the_real_tree(self):
        lines = self.run_guard(root=REPO, parser="lines")
        self.assertEqual(lines[0], EXIT_OK, lines[1])
        if cge.tomllib is not None:
            self.assertEqual(self.run_guard(root=REPO, parser="toml"), lines)


if __name__ == "__main__":
    unittest.main()
