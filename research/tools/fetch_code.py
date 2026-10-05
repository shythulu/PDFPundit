#!/usr/bin/env python3
"""Check out one pinned commit of a source-code repository into the gitignored cache.

    python research/tools/fetch_code.py SRC-0500 https://github.com/qpdf/qpdf <commit> \
        --license Apache-2.0 [--registry research/staging/<agent>/sources/registry.jsonl]

The checkout lands in research/cache/code/<repo-slug>@<commit12>/ (shallow, that commit only).
The source record (type: source-code) gets url, commit and license. Code-citation claims then
use `locator: {path, line_start, line_end}` + a verbatim `quote`, and kb_validate.py checks the
quote against that exact file at that exact commit.

Clean-room rule: copyleft (GPL/AGPL) code may be read and cited in the knowledge base (short
quotes, behaviour described in our own words), but never copied into PDFPundit, and coding
agents are never pointed at it.
"""

from __future__ import annotations

import argparse
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from kbcommon import REGISTRIES, code_checkout, load_jsonl, write_jsonl  # noqa: E402


def run(*cmd: str, cwd: Path | None = None) -> str:
    return subprocess.run(cmd, cwd=cwd, check=True, capture_output=True, text=True).stdout.strip()


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("src_id")
    ap.add_argument("url")
    ap.add_argument("commit", help="full or abbreviated commit sha; 'HEAD' resolves the default branch")
    ap.add_argument("--license", required=True, help="SPDX id, e.g. Apache-2.0, AGPL-3.0-or-later")
    ap.add_argument("--registry", type=Path, default=REGISTRIES["source"][0])
    a = ap.parse_args()

    commit = a.commit
    if commit == "HEAD":
        commit = run("git", "ls-remote", a.url, "HEAD").split()[0]
    if len(commit) != 40:
        sys.exit("pass a full 40-character commit sha (or HEAD); abbreviated shas can't be fetched shallowly")
    dest = code_checkout(a.url, commit)
    if not (dest / ".git").exists():
        dest.mkdir(parents=True, exist_ok=True)
        run("git", "init", "-q", cwd=dest)
        run("git", "remote", "add", "origin", a.url, cwd=dest)
        run("git", "fetch", "-q", "--depth", "1", "origin", commit, cwd=dest)
        run("git", "checkout", "-q", "FETCH_HEAD", cwd=dest)
    full = run("git", "rev-parse", "HEAD", cwd=dest)

    recs = load_jsonl(a.registry)
    for r in recs:
        if r.get("id") == a.src_id:
            r.update({"url": a.url, "commit": full, "license": a.license, "access": "oa"})
            break
    else:
        sys.exit(f"{a.src_id} not found in {a.registry}")
    write_jsonl(a.registry, recs)
    print(f"{a.src_id}: {a.url}@{full} -> {dest}")


if __name__ == "__main__":
    main()
