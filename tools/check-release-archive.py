#!/usr/bin/env python3
"""Checks a release archive before it is published (T-35).

Run with an isolated interpreter:

    python3 -I tools/check-release-archive.py <archive> ...   # check built archives
    python3 -I tools/check-release-archive.py --self-test     # on synthetic archives

For each `pdfpundit-<target>.tar.xz` or `.zip` that `dist build` wrote, it fails on:
  (a) a missing or wrong `<archive>.sha256` beside it;
  (b) archive contents other than exactly the binary (`pdfpundit`, or
      `pdfpundit.exe` in a `.zip`), `LICENSE`, `README.md`,
      `THIRD_PARTY_NOTICES.md` and `THIRD_PARTY_CRATES.md`;
  (c) a dev-only switch in the binary: the strings `PDFPUNDIT_ASSETS` or
      `PDFPUNDIT_PANIC`, which exist only under cfg(debug_assertions) (D-051);
  (d) the terminal guard (D-043): run with stdin the null device and stdout a
      file, in an empty directory, the binary must exit 2, print exactly the
      refusal line to stderr, write nothing to stdout and create nothing.

The archive must be built for the machine running the check: (d) runs it. The
release workflow calls this on each target's own runner
(.github/workflows/release-guard.yml).
"""

import hashlib
import io
import os
import subprocess
import sys
import tarfile
import tempfile
import zipfile

# The refusal line, as tests/guard.rs asserts it.
REFUSAL = "PDFPundit runs in a terminal; drop PDFs on the cat.\n"
DEV_SWITCHES = (b"PDFPUNDIT_ASSETS", b"PDFPUNDIT_PANIC")
DOCS = ("LICENSE", "README.md", "THIRD_PARTY_CRATES.md", "THIRD_PARTY_NOTICES.md")
GUARD_TIMEOUT_SECONDS = 60


def binary_name(archive):
    return "pdfpundit.exe" if archive.endswith(".zip") else "pdfpundit"


def checksum_problems(archive):
    sidecar = archive + ".sha256"
    if not os.path.isfile(sidecar):
        return [f"{os.path.basename(sidecar)}: missing"]
    with open(sidecar, encoding="utf-8") as f:
        fields = f.read().split()
    with open(archive, "rb") as f:
        digest = hashlib.sha256(f.read()).hexdigest()
    if not fields or fields[0].lower() != digest:
        return [f"{os.path.basename(sidecar)}: does not match the archive's sha256 {digest}"]
    return []


def unpack(archive, dest):
    """Extracts `archive` into `dest`; returns the directory holding its files
    (the single top-level directory if there is one)."""
    if archive.endswith(".zip"):
        with zipfile.ZipFile(archive) as z:
            z.extractall(dest)
    elif archive.endswith(".tar.xz"):
        with tarfile.open(archive, "r:xz") as t:
            if hasattr(tarfile, "data_filter"):
                t.extractall(dest, filter="data")
            else:
                t.extractall(dest)
    else:
        raise SystemExit(f"{archive}: not a .tar.xz or .zip")
    entries = os.listdir(dest)
    if len(entries) == 1 and os.path.isdir(os.path.join(dest, entries[0])):
        return os.path.join(dest, entries[0])
    return dest


def listing(root):
    out = []
    for dirpath, dirs, files in os.walk(root):
        dirs.sort()
        for f in files:
            out.append(os.path.relpath(os.path.join(dirpath, f), root).replace(os.sep, "/"))
    return sorted(out)


def content_problems(archive, root):
    want = sorted((binary_name(archive),) + DOCS)
    got = listing(root)
    if got != want:
        return [f"contents {got}, want {want}"]
    return []


def strings_problems(binary):
    with open(binary, "rb") as f:
        data = f.read()
    return [f"binary contains the dev-only switch {s.decode()}" for s in DEV_SWITCHES if s in data]


def guard_problems(binary, scratch):
    work = os.path.join(scratch, "cwd")
    os.makedirs(work)
    out_path = os.path.join(scratch, "stdout.txt")
    with open(out_path, "wb") as out:
        try:
            run = subprocess.run([binary], stdin=subprocess.DEVNULL, stdout=out,
                                 stderr=subprocess.PIPE, cwd=work,
                                 timeout=GUARD_TIMEOUT_SECONDS)
        except subprocess.TimeoutExpired:
            return [f"guard: still running after {GUARD_TIMEOUT_SECONDS} s without a terminal"]
    problems = []
    if run.returncode != 2:
        problems.append(f"guard: exit code {run.returncode}, want 2")
    stderr = run.stderr.decode("utf-8", errors="replace")
    if stderr != REFUSAL:
        problems.append(f"guard: stderr {stderr!r}, want {REFUSAL!r}")
    if os.path.getsize(out_path):
        problems.append("guard: wrote to stdout")
    if os.listdir(work):
        problems.append(f"guard: created {sorted(os.listdir(work))}")
    return problems


def check(archive, run_guard=True):
    problems = checksum_problems(archive)
    with tempfile.TemporaryDirectory() as scratch:
        root = unpack(archive, os.path.join(scratch, "unpacked"))
        problems += content_problems(archive, root)
        binary = os.path.join(root, binary_name(archive))
        if os.path.isfile(binary):
            problems += strings_problems(binary)
            if run_guard:
                problems += guard_problems(binary, scratch)
    return problems


def main_check(archives):
    failed = False
    for archive in archives:
        problems = check(archive)
        print(f"{os.path.basename(archive)}: {len(problems)} problems")
        for p in problems:
            print(f"   {p}")
        failed |= bool(problems)
    return 1 if failed else 0


def self_test():
    """The archive checks on synthetic archives; the guard run needs a real
    binary and is exercised by the real check."""
    failures = []

    def make(dirpath, name, files, checksum=True):
        path = os.path.join(dirpath, name)
        top = name.split(".")[0]
        if name.endswith(".zip"):
            with zipfile.ZipFile(path, "w") as z:
                for fname, data in files.items():
                    z.writestr(fname, data)
        else:
            with tarfile.open(path, "w:xz") as t:
                for fname, data in files.items():
                    info = tarfile.TarInfo(f"{top}/{fname}")
                    info.size = len(data)
                    t.addfile(info, io.BytesIO(data))
        if checksum is not None:
            with open(path, "rb") as f:
                digest = hashlib.sha256(f.read()).hexdigest() if checksum else "0" * 64
            with open(path + ".sha256", "w", encoding="utf-8") as f:
                f.write(f"{digest}  {name}\n")
        return path

    docs = {d: b"text" for d in DOCS}
    cases = [
        ("pdfpundit-x.tar.xz", {"pdfpundit": b"\x7fELF clean", **docs}, True, 0),
        ("pdfpundit-x.zip", {"pdfpundit.exe": b"MZ clean", **docs}, True, 0),
        ("pdfpundit-x.tar.xz", {"pdfpundit": b"\x7fELF clean", **docs}, False, 1),
        ("pdfpundit-x.tar.xz", {"pdfpundit": b"\x7fELF clean", **docs}, None, 1),
        ("pdfpundit-x.tar.xz", {"pdfpundit": b"x PDFPUNDIT_PANIC x", **docs}, True, 1),
        ("pdfpundit-x.zip", {"pdfpundit.exe": b"PDFPUNDIT_ASSETS PDFPUNDIT_PANIC", **docs},
         True, 2),
        ("pdfpundit-x.tar.xz", {"pdfpundit": b"clean",
                                **{d: b"t" for d in DOCS if d != "THIRD_PARTY_CRATES.md"}},
         True, 1),
        ("pdfpundit-x.tar.xz", {"pdfpundit": b"clean", "install.sh": b"curl", **docs}, True, 1),
        ("pdfpundit-x.zip", {"pdfpundit": b"no .exe in a zip", **docs}, True, 1),
    ]
    for i, (name, files, checksum, want) in enumerate(cases):
        with tempfile.TemporaryDirectory() as d:
            got = check(make(d, name, files, checksum), run_guard=False)
        if len(got) != want:
            failures.append(f"case {i} ({name}): {got}, want {want} problems")
    for f in failures:
        print(f"self-test FAIL: {f}")
    print(f"self-test: {len(cases)} checks, {len(failures)} failures")
    return 1 if failures else 0


def main(argv):
    if argv == ["--self-test"]:
        return self_test()
    if not argv or any(a.startswith("-") for a in argv):
        print(__doc__)
        return 2
    return main_check(argv)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
