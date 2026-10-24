#!/usr/bin/env python3
"""Builds the release guard's job matrix from the dist plan (T-35, G-08).

Run with an isolated interpreter:

    PLAN='<dist plan json>' python3 -I tools/release-guard-matrix.py   # print matrix=<json>
    python3 -I tools/release-guard-matrix.py plan.json                  # same, from a file
    python3 -I tools/release-guard-matrix.py --self-test                # on synthetic plans

.github/workflows/release-guard.yml runs this on the `plan` input that
v-release.yml passes it, and appends the output line to $GITHUB_OUTPUT. The guard
leg for each archive comes from the plan, so a target added to Cargo.toml's
`[workspace.metadata.dist]` cannot ship without a guard leg: every executable
archive in the plan gets one, and the run fails (so the host job publishes
nothing) when the plan has:
  (a) an archive for a target with no entry in GUARD_RUNNERS below;
  (b) an archive built for more or fewer than one target;
  (c) an archive name that is not a plain file name;
  (d) no executable archive at all.

GUARD_RUNNERS names a runner that can execute each target's binary natively,
because the guard check runs it. That is not always the runner dist builds on
(a cross-compiled target builds elsewhere), so it is kept here, not read from
the plan's build matrix.
"""

import json
import os
import re
import sys

GUARD_RUNNERS = {
    "aarch64-apple-darwin": "macos-14",
    "x86_64-pc-windows-msvc": "windows-2022",
    "x86_64-unknown-linux-gnu": "ubuntu-22.04",
}
ARCHIVE_KIND = "executable-zip"
PLAIN_NAME = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]*$")


def build_matrix(plan):
    """(matrix, problems) for a parsed dist plan; matrix is None on any problem.
    The legs are sorted by target, so one plan gives one matrix."""
    artifacts = plan.get("artifacts") if isinstance(plan, dict) else None
    if not isinstance(artifacts, dict):
        return None, ["the plan has no artifacts table"]
    legs, problems = [], []
    for name in sorted(artifacts):
        art = artifacts[name]
        if not isinstance(art, dict) or art.get("kind") != ARCHIVE_KIND:
            continue
        targets = art.get("target_triples") or []
        if not PLAIN_NAME.match(name):
            problems.append(f"{name!r}: not a plain archive file name")
        elif len(targets) != 1:
            problems.append(f"{name}: built for {len(targets)} targets {targets}; "
                            "the guard checks one target per archive")
        elif targets[0] not in GUARD_RUNNERS:
            problems.append(f"{name}: no guard runner for target {targets[0]}; "
                            "add it to GUARD_RUNNERS in tools/release-guard-matrix.py")
        else:
            legs.append((targets[0], {"runner": GUARD_RUNNERS[targets[0]], "archive": name}))
    if not legs and not problems:
        problems.append("the plan has no executable archive to guard")
    if problems:
        return None, problems
    return {"include": [leg for _, leg in sorted(legs, key=lambda t: t[0])]}, []


def self_test():
    """build_matrix on synthetic plans shaped like `dist plan --output-format=json`."""

    def archive(target, ext):
        return f"pdfpundit-{target}.{ext}", {"kind": ARCHIVE_KIND, "target_triples": [target]}

    def plan_of(*entries):
        artifacts = {}
        for name, art in entries:
            artifacts[name] = art
            artifacts[name + ".sha256"] = {"kind": "checksum",
                                           "target_triples": art.get("target_triples")}
        artifacts["sha256.sum"] = {"kind": "unified-checksum"}
        return {"artifacts": artifacts}

    shipped = [archive("aarch64-apple-darwin", "tar.xz"),
               archive("x86_64-pc-windows-msvc", "zip"),
               archive("x86_64-unknown-linux-gnu", "tar.xz")]
    three_legs = {"include": [
        {"runner": "macos-14", "archive": "pdfpundit-aarch64-apple-darwin.tar.xz"},
        {"runner": "windows-2022", "archive": "pdfpundit-x86_64-pc-windows-msvc.zip"},
        {"runner": "ubuntu-22.04", "archive": "pdfpundit-x86_64-unknown-linux-gnu.tar.xz"},
    ]}
    two = {"kind": ARCHIVE_KIND,
           "target_triples": ["x86_64-apple-darwin", "aarch64-apple-darwin"]}
    cases = [
        ("the three shipped targets", plan_of(*shipped), three_legs, 0),
        ("artifact order does not matter", plan_of(*reversed(shipped)), three_legs, 0),
        ("a dropped target loses its leg", plan_of(*shipped[:2]),
         {"include": three_legs["include"][:2]}, 0),
        ("an extra target with no guard runner",
         plan_of(*shipped, archive("x86_64-apple-darwin", "tar.xz")), None, 1),
        ("two extra targets", plan_of(*shipped, archive("x86_64-apple-darwin", "tar.xz"),
                                      archive("aarch64-unknown-linux-gnu", "tar.xz")), None, 2),
        ("a universal archive", plan_of(*shipped, ("pdfpundit-universal.tar.xz", two)), None, 1),
        ("an archive with no target",
         plan_of(*shipped, ("pdfpundit.tar.xz", {"kind": ARCHIVE_KIND})), None, 1),
        ("a name that is not a plain file name",
         plan_of(("../x.tar.xz", {"kind": ARCHIVE_KIND,
                                  "target_triples": ["x86_64-unknown-linux-gnu"]})), None, 1),
        ("no executable archive", plan_of(), None, 1),
        ("no artifacts table", {}, None, 1),
    ]
    failures = []
    for label, plan, want_matrix, want_problems in cases:
        try:
            matrix, problems = build_matrix(plan)
        except Exception as e:  # a crash is a failed case, not a failed self-test run
            failures.append(f"{label}: raised {e!r}")
            continue
        if matrix != want_matrix or len(problems) != want_problems:
            failures.append(f"{label}: matrix {matrix}, problems {problems}; "
                            f"want {want_matrix}, {want_problems} problems")
    for f in failures:
        print(f"self-test FAIL: {f}")
    print(f"self-test: {len(cases)} checks, {len(failures)} failures")
    return 1 if failures else 0


def main(argv):
    if argv == ["--self-test"]:
        return self_test()
    if len(argv) > 1 or any(a.startswith("-") for a in argv):
        print(__doc__)
        return 2
    if argv:
        with open(argv[0], encoding="utf-8") as f:
            text = f.read()
    else:
        text = os.environ.get("PLAN", "")
    try:
        plan = json.loads(text)
    except ValueError as e:
        print(f"release guard: the plan is not JSON: {e}", file=sys.stderr)
        return 1
    matrix, problems = build_matrix(plan)
    for p in problems:
        print(f"release guard: {p}", file=sys.stderr)
    if matrix is None:
        return 1
    print("matrix=" + json.dumps(matrix, separators=(",", ":")))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
