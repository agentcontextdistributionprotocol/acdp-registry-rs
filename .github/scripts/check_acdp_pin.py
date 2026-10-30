#!/usr/bin/env python3
"""Exact-pin guard for the acdp SDK.

WHY THIS EXISTS. This repository pins `acdp` EXACTLY (`version = "=X.Y.Z"`):
the registry implements the protocol through the SDK's types and verifiers, so
a new SDK version is a new protocol surface to adopt deliberately, together
with `tools/live-smoke` (its own workspace and lockfile) and the cargo-vet
exemptions for every `acdp*` crate. The shared bump automation
(acdp-ci's bump-consume.yml) rewrites the version string in Cargo.toml only --
dropping the `=` -- and moves neither live-smoke nor the exemptions. Because
`cargo-vet` is advisory here on purpose (docs/MAINTAINING.md), such a bump PR
went green on every REQUIRED check and auto-merged with vet red, three times
(#476, #485, #497). This guard runs in the required `lint` job, so a bump that
leaves any of the coupled places behind cannot auto-merge.

It fails (exit 1) unless ALL of these hold for one version X:

  (a) Cargo.toml declares `acdp` with version requirement `=X`;
  (b) tools/live-smoke/Cargo.toml declares `acdp` with `=X`;
  (c) every crates.io `acdp` / `acdp-*` package in Cargo.lock and in
      tools/live-smoke/Cargo.lock is locked at X (the workspace's own
      `acdp-registry-*` crates have no `source` and are not SDK crates);
  (d) every such package has a `[[exemptions.<name>]]` in
      supply-chain/config.toml at version X, and no `acdp*` exemption names
      another version (a stale one would mean the bump skipped the move).

It exits 2 on input it cannot read with certainty (a missing file, or no
`acdp` declaration found), rather than pass a tree it did not inspect.

Usage: check_acdp_pin.py [--root DIR]
"""

import argparse
import os
import re
import sys

DECL = re.compile(r'^\s*acdp\s*=\s*(.+?)\s*(?:#.*)?$')
VERSION_IN_TABLE = re.compile(r'version\s*=\s*"([^"]+)"')
BARE_STRING = re.compile(r'^"([^"]+)"$')
EXEMPTION_HEADER = re.compile(r'^\[\[exemptions\.("?)([A-Za-z0-9_-]+)\1\]\]\s*$')
KEY_VERSION = re.compile(r'^version\s*=\s*"([^"]+)"\s*$')


class Unreadable(Exception):
    pass


def read(path):
    try:
        with open(path, encoding="utf-8") as f:
            return f.read()
    except OSError as e:
        raise Unreadable(f"cannot read {path}: {e}")


def acdp_requirement(manifest_path):
    """The version requirement string of the `acdp` dependency in a manifest."""
    found = []
    for line in read(manifest_path).splitlines():
        m = DECL.match(line)
        if not m:
            continue
        rhs = m.group(1)
        v = VERSION_IN_TABLE.search(rhs) if rhs.startswith("{") else BARE_STRING.match(rhs)
        if not v:
            raise Unreadable(f"{manifest_path}: unrecognised acdp declaration: {line.strip()}")
        found.append(v.group(1))
    if len(found) != 1:
        raise Unreadable(f"{manifest_path}: expected exactly one acdp declaration, found {len(found)}")
    return found[0]


def sdk_packages(lock_path):
    """{name: [versions]} for crates.io acdp / acdp-* packages in a lockfile."""
    out = {}
    for block in read(lock_path).split("[[package]]")[1:]:
        name = re.search(r'^name = "([^"]+)"', block, re.M)
        version = re.search(r'^version = "([^"]+)"', block, re.M)
        source = re.search(r'^source = "([^"]+)"', block, re.M)
        if not (name and version):
            raise Unreadable(f"{lock_path}: a [[package]] without name/version")
        n = name.group(1)
        if (n == "acdp" or n.startswith("acdp-")) and source and source.group(1).startswith("registry+"):
            out.setdefault(n, []).append(version.group(1))
    return out


def sdk_exemptions(config_path):
    """{name: [versions]} for acdp / acdp-* entries under [[exemptions.*]]."""
    out = {}
    current = None
    for line in read(config_path).splitlines():
        h = EXEMPTION_HEADER.match(line)
        if h:
            n = h.group(2)
            current = n if (n == "acdp" or n.startswith("acdp-")) else None
            continue
        if line.startswith("["):
            current = None
            continue
        if current:
            v = KEY_VERSION.match(line.strip())
            if v:
                out.setdefault(current, []).append(v.group(1))
    return out


def violations(root):
    errs = []
    ws = acdp_requirement(os.path.join(root, "Cargo.toml"))
    smoke = acdp_requirement(os.path.join(root, "tools/live-smoke/Cargo.toml"))
    if not ws.startswith("="):
        errs.append(f'Cargo.toml: acdp requirement "{ws}" is not an exact pin (want "={ws.lstrip("^~= ")}")')
    pin = ws.lstrip("=").strip()
    if smoke != f"={pin}":
        errs.append(f'tools/live-smoke/Cargo.toml: acdp requirement "{smoke}" != "={pin}"')

    locked = {}
    for lock in ("Cargo.lock", "tools/live-smoke/Cargo.lock"):
        pkgs = sdk_packages(os.path.join(root, lock))
        if "acdp" not in pkgs:
            raise Unreadable(f"{lock}: no crates.io acdp package")
        for n, vs in sorted(pkgs.items()):
            for v in vs:
                if v != pin:
                    errs.append(f"{lock}: {n} locked at {v}, pin is {pin}")
            locked.setdefault(n, set()).update(vs)

    exempt = sdk_exemptions(os.path.join(root, "supply-chain/config.toml"))
    for n in sorted(locked):
        if pin not in exempt.get(n, []):
            errs.append(f"supply-chain/config.toml: no [[exemptions.{n}]] at {pin}")
    for n, vs in sorted(exempt.items()):
        for v in vs:
            if v != pin:
                errs.append(f"supply-chain/config.toml: [[exemptions.{n}]] at {v}, pin is {pin}")
    return errs


def main(argv):
    ap = argparse.ArgumentParser()
    ap.add_argument("--root", default=".")
    args = ap.parse_args(argv)
    try:
        errs = violations(args.root)
    except Unreadable as e:
        print(f"check-acdp-pin: UNREADABLE: {e}", file=sys.stderr)
        return 2
    if errs:
        for e in errs:
            print(f"check-acdp-pin: {e}", file=sys.stderr)
        print(
            "check-acdp-pin: an acdp bump must move all coupled places together: "
            '`acdp = "=X"` in Cargo.toml and tools/live-smoke/Cargo.toml, '
            "`cargo update -p acdp --precise X` in both workspaces, and every "
            "[[exemptions.acdp*]] in supply-chain/config.toml to X.",
            file=sys.stderr,
        )
        return 1
    print("check-acdp-pin: ok")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
