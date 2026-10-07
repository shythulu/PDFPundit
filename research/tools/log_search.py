#!/usr/bin/env python3
"""Append one search/fetch to the search log (every paid Parallel call MUST be logged).

    python research/tools/log_search.py --staging research/staging/<agent> --block 3 \
        --agent scoping-venues --tool parallel.web_search --query "..." --hits 10 --paid

Assigns the next SRCH id inside the agent's block (block n = SRCH-n00..n99; the chair is block 0,
ids SRCH-0001..0099). Prints the id so records can cite it in provenance.search_id.
"""

from __future__ import annotations

import argparse
import sys
from datetime import date
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from kbcommon import REGISTRIES, ROOT, load_jsonl, write_jsonl  # noqa: E402


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--staging", type=Path, help="agent staging dir (omit for the main log)")
    ap.add_argument("--block", type=int, required=True)
    ap.add_argument("--agent", required=True)
    ap.add_argument("--tool", required=True)
    ap.add_argument("--query", required=True)
    ap.add_argument("--hits", type=int)
    ap.add_argument("--paid", action="store_true")
    ap.add_argument("--calls", type=int, default=1)
    ap.add_argument("--notes")
    a = ap.parse_args()

    main_path = REGISTRIES["search"][0]
    path = (a.staging / main_path.relative_to(ROOT)) if a.staging else main_path
    taken = {r["id"] for r in load_jsonl(main_path) + load_jsonl(path)}
    lo = max(1, a.block * 100)
    new_id = next(f"SRCH-{n:04d}" for n in range(lo, a.block * 100 + 100) if f"SRCH-{n:04d}" not in taken)
    rec = {"id": new_id, "at": date.today().isoformat(), "agent": a.agent, "tool": a.tool,
           "query": a.query, "hits": a.hits, "paid": a.paid, "calls": a.calls, "notes": a.notes}
    write_jsonl(path, load_jsonl(path) + [rec])
    print(new_id)


if __name__ == "__main__":
    main()
