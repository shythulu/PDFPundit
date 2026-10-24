"""ocr-recall: REPDF-style OCR word recall over the corpus `ocr` mode's rasters
(T-38b). Dev tooling, never part of the shipped build.

    python3 -I tools/ocr-recall/ocr_recall.py \\
        --manifest target/corpus/ocr_inputs/manifest.csv \\
        --out target/corpus/ocr_results.csv [--tier tiny|medium] \\
        [--engine paddle|tesseract|documentai] [--dry-run]

OCR runs once per PNG in manifest order; each original's OCR text is the
ground truth for its repaired variants. The results CSV and every log table
start with the D-063 label. See README.md for the metric and its limits.
"""

import argparse
import csv
import hashlib
import os
import sys
import time
from pathlib import Path

if sys.version_info < (3, 11):
    sys.exit("ocr-recall: needs Python 3.11 or later (3.12 or 3.13 for PaddleOCR)")

# `python3 -I` keeps the script's own directory off sys.path.
sys.path.insert(0, str(Path(__file__).resolve().parent))

import engines  # noqa: E402
import manifest  # noqa: E402
import models  # noqa: E402
import recall  # noqa: E402

COLUMNS = (
    "engine",
    "engine_version",
    "tier",
    "file",
    "class",
    "producer",
    "base_doc",
    "page",
    "lang",
    "script",
    "models",
    "open_ok",
    "ocr_pages",
    "words_orig",
    "words_rep",
    "ocr_word_recall",
    "ocr_word_recall_nopunct",
    "ocr_char_recall",
    "lcs_f1",
    "text_visual_gap",
)


def label(engine: str, version: str) -> str:
    """The D-063 label: the first line of the CSV and of every table. The
    nightly log is public; these numbers are regression baselines only."""
    return (
        "regression baseline; 44-file REPDF smoke subset; "
        f"REPDF-style OCR word recall, engine {engine} {version}; "
        "not REPDF's metric; not comparable to the paper; not a published recovery rate"
    )


def parse(argv: list[str] | None) -> argparse.Namespace:
    p = argparse.ArgumentParser(prog="ocr-recall", description=__doc__.splitlines()[0])
    p.add_argument("--manifest", required=True, type=Path, help="T-38a's ocr_inputs/manifest.csv")
    p.add_argument("--out", required=True, type=Path, help="the per-page results CSV")
    p.add_argument("--tier", choices=("tiny", "medium"), default="tiny")
    p.add_argument("--engine", choices=engines.NAMES, default="paddle")
    p.add_argument(
        "--dry-run",
        action="store_true",
        help="check the engine is installed and read the manifest; fetch nothing, OCR nothing",
    )
    return p.parse_args(argv)


def main(argv: list[str] | None = None, engine=None) -> int:
    """`engine` replaces the module `--engine` names (tests pass fixed line lists)."""
    args = parse(argv)
    try:
        eng = engine if engine is not None else engines.load(args.engine)
        eng.check_installed()
        rows = manifest.read(args.manifest)
        todo = [r for r in rows if r.png_path is not None]
        routes = {recall.route_of(recall.script_of(r.lang)) for r in todo}
        if args.dry_run:
            print(
                f"ocr-recall: dry run: engine {eng.NAME}, tier {args.tier}: {len(rows)} manifest rows, "
                f"{len(todo)} PNGs to OCR, routes {', '.join(sorted(routes)) or 'none'}"
            )
            return 0
        apparatus = eng.prepare(args.tier, routes, os.cpu_count() or 1)
    except (engines.EngineBlocked, engines.EngineMissing, manifest.ManifestError, models.ModelError) as e:
        print(f"ocr-recall: {e}", file=sys.stderr)
        return 2

    head = [label(apparatus.name, apparatus.version), f"apparatus: engine {apparatus.name} {apparatus.version}; {apparatus.detail}"]
    print("\n".join(head), flush=True)

    texts: dict[str, list[str]] = {}
    for n, r in enumerate(todo, start=1):
        script = recall.script_of(r.lang)
        start = time.perf_counter()
        lines = apparatus.recognisers[recall.route_of(script)].ocr(r.png_path)
        seconds = time.perf_counter() - start
        texts[r.png] = lines
        digest = hashlib.sha256("\n".join(lines).encode("utf-8")).hexdigest()
        print(
            f"ocr {n}/{len(todo)} {r.png} {script} {len(lines)} lines {seconds:.2f} s sha256 {digest}",
            flush=True,
        )

    try:
        results = recall.pair(rows, texts)
    except manifest.ManifestError as e:
        print(f"ocr-recall: {e}", file=sys.stderr)
        return 2
    model_label = {route: rec.models for route, rec in apparatus.recognisers.items()}
    write_csv(args.out, head[0], apparatus, args.tier, results, model_label)
    for by_script in (True, False):
        print()
        print(table(head, results, by_script))
    print(f"\nocr-recall: {len(results)} scored pages -> {args.out}")
    return 0


def _cell(x) -> str:
    return "" if x is None else recall.fixed(x, 6)


def write_csv(out: Path, first_line: str, apparatus, tier: str, results, model_label) -> None:
    out.parent.mkdir(parents=True, exist_ok=True)
    with out.open("w", newline="", encoding="utf-8") as f:
        f.write(first_line + "\n")
        w = csv.writer(f, lineterminator="\n")
        w.writerow(COLUMNS)
        for r in results:
            w.writerow(
                [
                    apparatus.name,
                    apparatus.version,
                    tier,
                    r.file,
                    r.cls,
                    r.producer,
                    r.base_doc,
                    r.page,
                    r.lang,
                    r.script,
                    model_label.get(recall.route_of(r.script), ""),
                    int(r.open_ok),
                    r.ocr_pages,
                    r.words_orig,
                    r.words_rep,
                    _cell(r.word),
                    _cell(r.word_nopunct),
                    _cell(r.char),
                    _cell(r.lcs_f1),
                    _cell(r.gap),
                ]
            )


def table(head: list[str], results, by_script: bool) -> str:
    """A per-class (and per-script) table of per-page means, headed by the
    D-063 label and the apparatus line."""
    keys = ("class", "script") if by_script else ("class",)
    cols = (*keys, "pages", "ocr_word_recall", "ocr_word_recall_nopunct", "ocr_char_recall", "text_visual_gap")
    body = [
        [
            *key,
            str(m.pages),
            *("-" if v is None else recall.fixed(v, 3) for v in (m.word, m.word_nopunct, m.char, m.gap)),
        ]
        for key, m in recall.aggregate(results, by_script)
    ]
    widths = [max(len(c), *(len(row[i]) for row in body)) if body else len(c) for i, c in enumerate(cols)]
    fmt = lambda row: "  ".join(cell.ljust(w) if i < len(keys) else cell.rjust(w) for i, (cell, w) in enumerate(zip(row, widths)))
    title = "per class and script" if by_script else "per class"
    lines = [*head, f"mean per page, {title}; pages = pages whose original OCR has words", fmt(cols)]
    lines += [fmt(row) for row in body]
    return "\n".join(line.rstrip() for line in lines)


if __name__ == "__main__":
    sys.exit(main())
