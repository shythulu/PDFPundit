#!/usr/bin/env python3
"""Regression tests: the knowledge-base tools must reject planted or unsupported evidence.

    python research/tools/test_kb.py

Each test builds a throwaway staging area under research/staging/_test/ (removed afterwards)
and checks that the fetcher or the validator refuses it. Needs pdftotext or PyMuPDF, rapidfuzz,
jsonschema, and the REPDF paper at its committed path.
"""

from __future__ import annotations

import hashlib
import json
import shutil
import subprocess
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from kbcommon import FULLTEXT, REPO, ROOT  # noqa: E402

TOOLS = ROOT / "tools"
STG = ROOT / "staging" / "_test"
REPDF_PDF = REPO / "nimbalyst-local/plans/REPDF-Repairing-corrupted-PDF-files-through-f_2026_Forensic-Science-Interna.pdf"
PROV = {"agent": "test", "method": "local-file", "search_id": None, "added_at": "2026-09-27"}


def run(*args: str) -> subprocess.CompletedProcess:
    return subprocess.run([sys.executable, *args], capture_output=True, text=True, cwd=REPO)


def put(rel: str, records: list[dict]) -> None:
    p = STG / rel
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text("".join(json.dumps(r) + "\n" for r in records))


def source(sid: str, title: str, **kw) -> dict:
    rec = {"id": sid, "type": "paper", "title": title, "year": 2020, "doi": None,
           "dedupe_key": "", "status": "read", "topics": ["pdf-repair"], "access": "oa", "provenance": PROV}
    rec.update(kw)
    from kbcommon import dedupe_key
    rec["dedupe_key"] = dedupe_key(rec)
    return rec


def validate() -> subprocess.CompletedProcess:
    return run(str(TOOLS / "kb_validate.py"), "--staging", str(STG), "--write")


class KBGuards(unittest.TestCase):
    def setUp(self) -> None:
        shutil.rmtree(STG, ignore_errors=True)
        self.planted: list[Path] = []

    def tearDown(self) -> None:
        shutil.rmtree(STG, ignore_errors=True)
        for p in self.planted:
            p.unlink(missing_ok=True)

    def test_refuses_local_text_as_paper(self) -> None:
        put("sources/registry.jsonl", [source("SRC-9900", "A paper that does not exist")])
        fake = STG / "fake.txt"
        fake.write_text("A paper that does not exist. Future work: everything is solved.")
        r = run(str(TOOLS / "fetch_fulltext.py"), "SRC-9900", str(fake),
                "--registry", str(STG / "sources/registry.jsonl"))
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("not a PDF", r.stdout + r.stderr)

    def test_refuses_pdf_with_wrong_title(self) -> None:
        put("sources/registry.jsonl", [source("SRC-9901", "Quantum error correction for PDF streams")])
        r = run(str(TOOLS / "fetch_fulltext.py"), "SRC-9901", str(REPDF_PDF),
                "--registry", str(STG / "sources/registry.jsonl"))
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("title", r.stdout + r.stderr)

    def _paper_with_cached_text(self, text: str, **kw) -> dict:
        sha = hashlib.sha256(("planted" + text).encode()).hexdigest()
        path = FULLTEXT / f"{sha}.txt"
        FULLTEXT.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
        self.planted.append(path)
        return source("SRC-9902", "Planted", fulltext_sha256=sha,
                      text_sha256=hashlib.sha256(path.read_bytes()).hexdigest(), **kw)

    def test_detects_edited_cache(self) -> None:
        src = self._paper_with_cached_text("one page only with some words in it for the test")
        FULLTEXT.joinpath(f"{src['fulltext_sha256']}.txt").write_text("edited afterwards to add a quote")
        put("sources/registry.jsonl", [src])
        r = validate()
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("modified after fetch", r.stdout)

    def test_number_not_in_quote_is_partial(self) -> None:
        text = "The method recovers most of the text in the damaged files we evaluated in this study."
        put("sources/registry.jsonl", [self._paper_with_cached_text(text)])
        put("sources/claims.jsonl", [{
            "id": "CLM-9900", "src": "SRC-9902", "kind": "result",
            "text": "The method recovers 97% of the text.",
            "quote": "The method recovers most of the text in the damaged files", "page": None,
            "provenance": PROV}])
        r = validate()
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("CLM-9900: numbers", r.stdout)

    def test_short_quote_rejected(self) -> None:
        put("sources/registry.jsonl", [self._paper_with_cached_text("tiny quote here and more words")])
        put("sources/claims.jsonl", [{"id": "CLM-9901", "src": "SRC-9902", "kind": "result",
                                      "text": "tiny", "quote": "tiny quote", "page": None, "provenance": PROV}])
        r = validate()
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("CLM-9901: quote is short", r.stdout)

    def test_critique_cannot_promote_gap(self) -> None:
        put("sources/registry.jsonl", [source("SRC-9903", "Critique host")])
        put("sources/claims.jsonl", [{"id": "CLM-9902", "src": "SRC-9903", "kind": "critique",
                                      "text": "Our reading: X is missing.", "quote": None, "page": None,
                                      "provenance": PROV}])
        put("gaps/gaps.jsonl", [{
            "id": "GAP-990", "title": "t", "statement": "s", "type": "method-weakness",
            "evidence": ["CLM-9902"], "scores": {"impact": 3, "feasibility": 3, "novelty": 3, "confidence": 3},
            "status": "supported", "still_open": "unknown", "provenance": PROV}])
        r = validate()
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("GAP-990: status 'supported' requires", r.stdout)

    def test_polite_get_backs_off_on_429(self) -> None:
        import http.server
        import threading
        import polite_get as pg

        hits = []

        class H(http.server.BaseHTTPRequestHandler):
            def do_GET(self):  # first request 429, then 200
                hits.append(1)
                code = 429 if len(hits) == 1 else 200
                self.send_response(code)
                self.end_headers()
                self.wfile.write(b"ok" if code == 200 else b"slow down")

            def log_message(self, *a):
                pass

        srv = http.server.HTTPServer(("127.0.0.1", 0), H)
        threading.Thread(target=srv.serve_forever, daemon=True).start()
        real_sleep, pg.time.sleep = pg.time.sleep, (lambda s: None)
        try:
            body = pg.polite_get(f"http://127.0.0.1:{srv.server_port}/x")
        finally:
            pg.time.sleep = real_sleep
            srv.shutdown()
        self.assertEqual(body, b"ok")
        self.assertEqual(len(hits), 2)

    def test_clean_main_registries(self) -> None:
        r = run(str(TOOLS / "kb_validate.py"))
        self.assertEqual(r.returncode, 0, r.stdout)


if __name__ == "__main__":
    unittest.main(verbosity=2)
