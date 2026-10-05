#!/usr/bin/env python3
"""Crypto-critical cargo-vet exemption guard (#405).

WHY THIS EXISTS. `cargo vet` passes a crate covered by an exemption in
supply-chain/config.toml exactly like one covered by an audit, so on its own it
cannot tell "audited" from "exempted". The usual way to turn a red vet job
green, `cargo vet regenerate exemptions`, would therefore quietly exempt a bump
of ed25519-dalek (or any other crypto crate) to a version nobody has read.

This guard closes that gap for the crates in supply-chain/crypto-critical.txt.
It fails when:

  (a) config.toml exempts a crypto-critical (crate, version) that is not on
      supply-chain/crypto-exemptions-allowed.txt -- the bump must be audited,
      or allow-listed in review with a reason and a tracking issue;
  (b) an allow-list entry is no longer exempted -- the entry is stale and must
      be deleted, so the allow-list can only shrink as audits land.

It also fails (exit 2) on anything it cannot read with certainty: a malformed
list line, a crypto-critical name absent from Cargo.lock (a typo would guard
nothing), an allow-list entry for a crate not on the crypto-critical list, or
an exemption in config.toml written in a shape this parser does not know.

acdp-rs runs the same idea as scripts/check-crypto-vet.sh, which reads
`cargo vet --output-format=json`. This one reads config.toml directly instead:
the question is "is it exempted", which config.toml answers without a cargo-vet
run, so the self-test needs no cargo-vet and no network. `cargo vet --locked`
runs in the same job first and answers "is everything covered at all".

config.toml is read with the stdlib `tomllib` when it exists (Python 3.11+, so
on CI), which reads every TOML spelling of an exemption. On Python 3.9/3.10 a
line reader is the fallback: cargo-vet writes config.toml in a fixed shape
(`[[exemptions.<crate>]]` followed by `version = "<v>"`), and the line reader
refuses (exit 2) any other mention of `exemptions` -- quoted, dotted, inline or
spaced -- rather than guess. The self-test checks that both readers return the
same set on the real config.toml.

Usage: check_crypto_exemptions.py [--root DIR] [--parser auto|toml|lines]
Exit: 0 pass, 1 violation(s), 2 unreadable input.
"""

from __future__ import annotations

import argparse
import os
import re
import sys

try:  # Python 3.11+
    import tomllib
except ImportError:  # pragma: no cover - exercised on 3.9/3.10
    tomllib = None

EXIT_OK, EXIT_VIOLATION, EXIT_UNREADABLE = 0, 1, 2

CRITICAL = "supply-chain/crypto-critical.txt"
ALLOWED = "supply-chain/crypto-exemptions-allowed.txt"
CONFIG = "supply-chain/config.toml"
LOCK = "Cargo.lock"

CRATE_RE = re.compile(r"^[A-Za-z0-9_-]+$")
VERSION_RE = re.compile(r"^[0-9]+\.[0-9]+\.[0-9]+(?:[-+][0-9A-Za-z.+-]+)?$")
TRACKING_RE = re.compile(r"^(?:[A-Za-z0-9_.-]+(?:/[A-Za-z0-9_.-]+)?)?#[0-9]+$")
EXEMPTION_HEADER_RE = re.compile(r'^\[\[exemptions\.(?:"([^"]+)"|([A-Za-z0-9_-]+))\]\]$')
VERSION_LINE_RE = re.compile(r'^version\s*=\s*"([^"]*)"\s*(?:#.*)?$')
LOCK_NAME_RE = re.compile(r'^name\s*=\s*"([^"]+)"\s*$')


class Unreadable(Exception):
    """An input could not be read with certainty."""


def _content_lines(path):
    """(lineno, stripped line) for each non-blank, non-comment line."""
    try:
        with open(path, encoding="utf-8") as f:
            lines = f.read().splitlines()
    except OSError as e:
        raise Unreadable("cannot read %s: %s" % (path, e))
    for i, raw in enumerate(lines, 1):
        line = raw.strip()
        if line and not line.startswith("#"):
            yield i, line


def read_critical(path):
    names = []
    for i, line in _content_lines(path):
        if not CRATE_RE.match(line):
            raise Unreadable("%s:%d: expected one crate name, got %r" % (path, i, line))
        if line in names:
            raise Unreadable("%s:%d: duplicate crate %r" % (path, i, line))
        names.append(line)
    if not names:
        raise Unreadable("%s names no crates" % path)
    return names


def read_allowed(path):
    """{(crate, version): (lineno, tracking)}."""
    entries = {}
    for i, line in _content_lines(path):
        parts = line.split(None, 3)
        if len(parts) < 4:
            raise Unreadable(
                "%s:%d: expected '<crate> <version> <tracking> <reason>', got %r" % (path, i, line)
            )
        crate, version, tracking, _reason = parts
        if not CRATE_RE.match(crate):
            raise Unreadable("%s:%d: bad crate name %r" % (path, i, crate))
        if not VERSION_RE.match(version):
            raise Unreadable("%s:%d: %r is not an exact version" % (path, i, version))
        if not TRACKING_RE.match(tracking):
            raise Unreadable(
                "%s:%d: tracking %r must be '<repo>#<n>' or '#<n>'" % (path, i, tracking)
            )
        key = (crate, version)
        if key in entries:
            raise Unreadable("%s:%d: duplicate entry %s %s" % (path, i, crate, version))
        entries[key] = (i, tracking)
    return entries


def read_exemptions_toml(path):
    """{(crate, version)} via tomllib: every TOML spelling of an exemption."""
    try:
        with open(path, "rb") as f:
            data = tomllib.load(f)
    except OSError as e:
        raise Unreadable("cannot read %s: %s" % (path, e))
    except tomllib.TOMLDecodeError as e:
        raise Unreadable("%s is not valid TOML: %s" % (path, e))
    table = data.get("exemptions", {})
    if not isinstance(table, dict):
        raise Unreadable("%s: `exemptions` is not a table" % path)
    exempted = set()
    for crate, entries in table.items():
        if not isinstance(entries, list):
            raise Unreadable("%s: exemptions.%s is not an array of tables" % (path, crate))
        for entry in entries:
            version = entry.get("version") if isinstance(entry, dict) else None
            if not isinstance(version, str):
                raise Unreadable("%s: [[exemptions.%s]] has no version line" % (path, crate))
            exempted.add((crate, version))
    return exempted


def read_exemptions(path, parser="auto"):
    """{(crate, version)} exempted in config.toml, by the chosen reader."""
    if parser == "toml" or (parser == "auto" and tomllib is not None):
        if tomllib is None:
            raise Unreadable("--parser toml needs Python 3.11+ (tomllib)")
        return read_exemptions_toml(path)
    return read_exemptions_lines(path)


# A line that may assign or open `exemptions` in any spelling the line reader
# does not parse: bare, "quoted" or 'quoted', followed by `.`, `=` or space.
OTHER_EXEMPTIONS_KEY_RE = re.compile(r"""^(?:exemptions|"exemptions"|'exemptions')\s*[.=]""")


def read_exemptions_lines(path):
    """{(crate, version)} from cargo-vet's `[[exemptions.<crate>]]` tables.

    The 3.9 fallback. Refuses any other mention of exemptions."""
    exempted = set()
    current = None  # crate of the open exemption table
    seen_version = True
    try:
        with open(path, encoding="utf-8") as f:
            lines = f.read().splitlines()
    except OSError as e:
        raise Unreadable("cannot read %s: %s" % (path, e))

    def close(lineno):
        if current is not None and not seen_version:
            raise Unreadable(
                "%s:%d: [[exemptions.%s]] has no version line" % (path, lineno, current)
            )

    for i, raw in enumerate(lines, 1):
        line = raw.strip()
        if line.startswith("["):
            close(i)
            m = EXEMPTION_HEADER_RE.match(line)
            if m:
                current, seen_version = m.group(1) or m.group(2), False
                continue
            if "exemptions" in line:
                raise Unreadable(
                    "%s:%d: unrecognised exemptions table %r; this guard reads only "
                    "cargo-vet's [[exemptions.<crate>]] form" % (path, i, line)
                )
            current, seen_version = None, True
            continue
        if OTHER_EXEMPTIONS_KEY_RE.match(line):
            raise Unreadable(
                "%s:%d: exemptions written as a dotted or inline key (%r); this guard "
                "reads only cargo-vet's [[exemptions.<crate>]] form" % (path, i, line)
            )
        if current is None or not line.startswith("version"):
            continue
        m = VERSION_LINE_RE.match(line)
        if not m or seen_version:
            raise Unreadable("%s:%d: unreadable exemption version %r" % (path, i, line))
        exempted.add((current, m.group(1)))
        seen_version = True
    close(len(lines) + 1)
    return exempted


def read_lock_names(path):
    names = set()
    for _i, line in _content_lines(path):
        m = LOCK_NAME_RE.match(line)
        if m:
            names.add(m.group(1))
    if not names:
        raise Unreadable("%s lists no packages" % path)
    return names


def check(root, parser="auto"):
    """(violations, notes). Raises Unreadable."""
    critical = read_critical(os.path.join(root, CRITICAL))
    allowed = read_allowed(os.path.join(root, ALLOWED))
    exempted = read_exemptions(os.path.join(root, CONFIG), parser)
    locked = read_lock_names(os.path.join(root, LOCK))

    missing = [n for n in critical if n not in locked]
    if missing:
        raise Unreadable(
            "%s names crates absent from Cargo.lock (typo, or remove them): %s"
            % (CRITICAL, ", ".join(missing))
        )
    critical_set = set(critical)
    stray = sorted(k for k in allowed if k[0] not in critical_set)
    if stray:
        raise Unreadable(
            "%s allow-lists crates that are not in %s: %s"
            % (ALLOWED, CRITICAL, ", ".join("%s %s" % k for k in stray))
        )

    violations, notes = [], []
    for crate, version in sorted(exempted):
        if crate not in critical_set:
            continue
        if (crate, version) in allowed:
            notes.append(
                "allowed exempt: %s %s (%s)" % (crate, version, allowed[(crate, version)][1])
            )
        else:
            violations.append(
                "%s %s is crypto-critical but EXEMPTED in %s, not audited, and not on %s. "
                "Do not re-exempt a crypto-critical bump: get it audited (an acdp-rs audit "
                "imported via `cargo vet regenerate imports`, or `cargo vet certify %s %s` "
                "/ a delta from an audited version), or, as a reviewed decision, add "
                "'%s %s <tracking> <reason>' to %s."
                % (crate, version, CONFIG, ALLOWED, crate, version, crate, version, ALLOWED)
            )
    for (crate, version), (lineno, _tracking) in sorted(allowed.items()):
        if (crate, version) not in exempted:
            violations.append(
                "%s:%d: stale entry '%s %s': %s no longer exempts it. Delete the line "
                "(the allow-list only shrinks)." % (ALLOWED, lineno, crate, version, CONFIG)
            )
    return violations, notes


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument(
        "--root",
        default=os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..")),
        help="repository root (default: this script's repo)",
    )
    ap.add_argument(
        "--parser",
        choices=("auto", "toml", "lines"),
        default="auto",
        help="config.toml reader: auto (tomllib if available), toml, or lines (3.9 fallback)",
    )
    args = ap.parse_args(argv)
    try:
        violations, notes = check(args.root, args.parser)
    except Unreadable as e:
        print("check-crypto-exemptions: ERROR: %s" % e, file=sys.stderr)
        return EXIT_UNREADABLE
    for n in notes:
        print("check-crypto-exemptions: %s" % n)
    for v in violations:
        print("check-crypto-exemptions: FAIL: %s" % v, file=sys.stderr)
    if violations:
        print(
            "check-crypto-exemptions: %d violation(s); see docs/MAINTAINING.md "
            "'Supply-chain audits (cargo vet)'." % len(violations),
            file=sys.stderr,
        )
        return EXIT_VIOLATION
    print("check-crypto-exemptions: ok (%d allowed exemption(s))." % len(notes))
    return EXIT_OK


if __name__ == "__main__":
    sys.exit(main())
