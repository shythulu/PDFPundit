#!/usr/bin/env python3
"""Purity and network gates (plan §3.2, T-01; D-029, D-050, D-053).

Run with an isolated interpreter from anywhere in the repo:

    python3 -I tools/purity-gate.py                 # purity, the three CI targets
    python3 -I tools/purity-gate.py <triple> ...    # purity, the named targets
    python3 -I tools/purity-gate.py --network       # network gate over Cargo.lock
    python3 -I tools/purity-gate.py --self-test     # the classifiers on synthetic input

Purity (per target). The authority for "compiled" is
`cargo tree --locked --target <t> -e normal,build`: Cargo.lock and cargo metadata's
resolve over-approximate it (they keep optional, wasm-only and windows-gnu packages
that no CI target builds). Over that set it fails on:
  (a) a `-sys`/`_sys` package outside {linux-raw-sys, windows-sys};
  (b) any `links =` key (the allow-list is empty; M5's fontconfig adds an entry);
  (c) a compiler-driver crate: cc, cmake, bindgen, nasm-rs, cxx-build, gcc,
      clang-sys, meson; and rayon, which the determinism rules ban outright;
  (d) a shipped .c .cc .cpp .cxx .s .S .asm .a .lib .o .obj file outside tests/,
      test/, examples/, benches/ and fuzz/, except signal-hook's
      src/low_level/extract.c, which it compiles only under its
      `extended-siginfo-raw` feature. That feature needs `cc`, so check (c) is what
      makes the exemption safe: keep the two together.
Rust inline `asm!` is never grepped for: rustix, getrandom, parking_lot and zlib-rs
use rustc-compiled inline assembly, which is not "assembly in the build" (D-053).

Negative control (documented, not run in CI): enabling signal-hook's
`extended-siginfo` feature in a scratch copy makes this gate fail on
aarch64-apple-darwin with "cc: compiler-driver crate in resolved graph"
(measured during planning, eng-r1-fr3).

Network (D-050): fails if Cargo.lock names reqwest, ureq, hyper, rustls,
native-tls, attohttpc, curl, tokio, mio-http, or any package whose name contains
`http` or `tls`. The shipped binary has no network code.
"""

import json
import os
import re
import subprocess
import sys

TARGETS = ["x86_64-unknown-linux-gnu", "aarch64-apple-darwin", "x86_64-pc-windows-msvc"]

ALLOW_SYS = {"linux-raw-sys", "windows-sys"}
ALLOW_LINKS = set()
FORBID_CRATES = {
    "cc": "compiler-driver crate in resolved graph",
    "cmake": "compiler-driver crate in resolved graph",
    "bindgen": "compiler-driver crate in resolved graph",
    "nasm-rs": "compiler-driver crate in resolved graph",
    "cxx-build": "compiler-driver crate in resolved graph",
    "gcc": "compiler-driver crate in resolved graph",
    "clang-sys": "compiler-driver crate in resolved graph",
    "meson": "compiler-driver crate in resolved graph",
    "rayon": "banned by the determinism rules",
}
NATIVE_EXT = {".c", ".cc", ".cpp", ".cxx", ".s", ".S", ".asm", ".a", ".lib", ".o", ".obj"}
SKIP_DIRS = {"tests", "test", "examples", "benches", "fuzz"}
ALLOW_FILES = {"signal-hook": {"src/low_level/extract.c"}}

NETWORK_NAMES = {
    "reqwest", "ureq", "hyper", "rustls", "native-tls", "attohttpc", "curl", "tokio", "mio-http",
}
NETWORK_SUBSTRINGS = ("http", "tls")

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def package_violations(name, links, files):
    """Violations for one activated package. `files` are paths relative to the
    package root, with '/' separators."""
    bad = []
    if (name.endswith("-sys") or name.endswith("_sys")) and name not in ALLOW_SYS:
        bad.append(f"{name}: -sys crate not allow-listed")
    if links and name not in ALLOW_LINKS:
        bad.append(f"{name}: links={links} not allow-listed")
    if name in FORBID_CRATES:
        bad.append(f"{name}: {FORBID_CRATES[name]}")
    for rel in sorted(files):
        parts = rel.split("/")
        if any(p in SKIP_DIRS for p in parts[:-1]):
            continue
        if os.path.splitext(parts[-1])[1] in NATIVE_EXT and rel not in ALLOW_FILES.get(name, ()):
            bad.append(f"{name}: ships {rel}")
    return bad


def network_violations(names):
    return [
        f"{n}: network crate in Cargo.lock"
        for n in sorted(set(names))
        if n in NETWORK_NAMES or any(s in n for s in NETWORK_SUBSTRINGS)
    ]


def cargo(*args):
    return subprocess.check_output(["cargo", *args], cwd=REPO, text=True)


def package_files(root):
    out = []
    for dirpath, dirs, files in os.walk(root):
        dirs.sort()
        for f in files:
            out.append(os.path.relpath(os.path.join(dirpath, f), root).replace(os.sep, "/"))
    return out


def resolved_set(target):
    out = cargo("tree", "--locked", "--target", target, "-e", "normal,build",
                "--prefix", "none", "--no-dedupe", "-f", "{p}")
    seen = set()
    for line in out.splitlines():
        parts = line.split()
        if len(parts) >= 2 and parts[1].startswith("v"):
            seen.add((parts[0], parts[1][1:]))
    return seen


def purity(targets):
    meta = json.loads(cargo("metadata", "--locked", "--format-version", "1"))
    packages = {(p["name"], p["version"]): p for p in meta["packages"]}
    failed = False
    for target in targets:
        # Path packages (this crate, workspace members) are ours, not dependencies.
        seen = sorted(k for k in resolved_set(target) if packages[k]["source"] is not None)
        bad = []
        for key in seen:
            p = packages[key]
            root = os.path.dirname(p["manifest_path"])
            bad += package_violations(p["name"], p.get("links"), package_files(root))
        print(f"{target}: {len(seen)} packages checked, {len(bad)} violations")
        for b in bad:
            print(f"   {b}")
        failed |= bool(bad)
    return 1 if failed else 0


def network():
    with open(os.path.join(REPO, "Cargo.lock"), encoding="utf-8") as f:
        names = re.findall(r'^name = "([^"]+)"$', f.read(), re.M)
    bad = network_violations(names)
    print(f"Cargo.lock: {len(set(names))} packages checked, {len(bad)} network violations")
    for b in bad:
        print(f"   {b}")
    return 1 if bad else 0


def self_test():
    cases = [
        (("linux-raw-sys", None, ["src/lib.rs"]), 0),
        (("windows-sys", None, []), 0),
        (("dirs-sys", None, []), 1),
        (("openssl_sys", None, []), 1),
        (("ring", "ring_core", []), 1),
        (("cc", None, []), 1),
        (("rayon", None, []), 1),
        (("signal-hook", None, ["src/low_level/extract.c"]), 0),
        (("signal-hook", None, ["src/other.c"]), 1),
        (("other", None, ["src/low_level/extract.c"]), 1),
        (("zstd", None, ["tests/fixture.c", "examples/x.S", "fuzz/a.o", "benches/b.asm"]), 0),
        (("winapi-x86_64-pc-windows-gnu", None, ["lib/libkernel32.a"]), 1),
        (("x", None, ["vendor/asm/sha.S", "src/lib.rs", "build.rs"]), 1),
        (("x", None, ["src/arch.rs"]), 0),
    ]
    failures = 0
    for (args, want) in cases:
        got = len(package_violations(*args))
        if got != want:
            failures += 1
            print(f"self-test FAIL: package_violations{args} gave {got}, want {want}")
    net = network_violations(["serde", "tokio", "hyper", "rustls", "h2", "httparse",
                              "native-tls", "mio", "tokio"])
    want_net = ["httparse", "hyper", "native-tls", "rustls", "tokio"]
    if [n.split(":")[0] for n in net] != want_net:
        failures += 1
        print(f"self-test FAIL: network_violations gave {net}, want {want_net}")
    print(f"self-test: {len(cases) + 1} checks, {failures} failures")
    return 1 if failures else 0


def main(argv):
    if argv == ["--self-test"]:
        return self_test()
    if argv == ["--network"]:
        return network()
    if any(a.startswith("-") for a in argv):
        print(__doc__)
        return 2
    return purity(argv or TARGETS)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
