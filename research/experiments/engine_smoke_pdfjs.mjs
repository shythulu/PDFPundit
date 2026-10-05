// pdf.js probe for engine_smoke.py (OBS-0700). Usage:
//   PDFJS_MJS=/path/to/node_modules/pdfjs-dist/legacy/build/pdf.mjs node engine_smoke_pdfjs.mjs <in.pdf>
// Prints one JSON line: {"ok":bool,"pages":n,"chars":non-whitespace text characters,"error":msg}.
// Options: stopAtErrors:false (pdf.js default; keep going past recoverable errors), no font loading,
// no eval. Text = concatenated TextItem.str of every page.
import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";
const pdfjs = await import(pathToFileURL(process.env.PDFJS_MJS).href);
const data = new Uint8Array(readFileSync(process.argv[2]));
const out = { ok: false, pages: 0, chars: 0, error: null };
try {
  const task = pdfjs.getDocument({ data, stopAtErrors: false, disableFontFace: true,
    isEvalSupported: false, useSystemFonts: false, verbosity: 0 });
  const doc = await task.promise;
  out.pages = doc.numPages;
  let text = "";
  for (let i = 1; i <= doc.numPages; i++) {
    try {
      const page = await doc.getPage(i);
      const tc = await page.getTextContent();
      text += tc.items.map((it) => it.str || "").join("");
    } catch (e) { out.error = `page ${i}: ${e.message}`; }
  }
  out.chars = [...text].filter((c) => !/\s/u.test(c)).length;
  out.ok = true;
  await task.destroy();
} catch (e) { out.error = String(e && e.message ? e.message : e); }
console.log(JSON.stringify(out));
