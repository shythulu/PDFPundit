# pr16-ingest report (agent E, ID block 10, 2026-10-05)

## What I did

Ingested PR #16, merged as c7a6ad4, under `nimbalyst-local/plans/research/` into the evidence-checked KB.

**Records written**

| Type | IDs | Count | Notes |
|---|---|---|---|
| Sources | SRC-1000..1005 | 6 | The report and its five notes. |
| Claims | CLM-1000..1079 | 79 | No CLM-1012. All quotes are `exact`. |
| Observations | OBS-1000..1015 | 16 | Our own re-runs on REPDF at commit e547d4d. |
| Gaps | GAP-300, GAP-301 | 2 | New. |
| Hypotheses | HYP-300..302 | 3 | New. |

The tags of 31 claims now say what reproduced them (`replicated-by`, `-approx`, `-in-part`, `contradicted-in-part`). The other 48 stay `unreplicated`. The SRC `evidence_grade.replicated` field stays false.

**Scripts.** All are in `experiments/`.
- PR #16's six experiments, copied. Four needed path-only patches, listed in `experiments/README.md`.
- Seven new scripts:
  - `build_cases.py` rebuilds the unsaved cases builder;
  - `c9_summarize.py`, `run_mzbench.sh`, `szcheck_versions.sh`;
  - `crosscheck_obs0300.py`, `small_adler_audit.py`, `headline_c9.py`.

**How the runs were done**
- Every run used `nice -n 10`, with each run under 20 minutes.
- cargo used `-j2 --locked` (0.9.1 was built `--offline`), with `CARGO_TARGET_DIR` in the scratchpad.
- The large intermediates (`partA.json`, `partB.json`, the 145 MB `cases/`) stay in the scratchpad. The observations record their sha256 values or listing hashes.

`reconciliation.md` holds the dedupe table, the proposed field updates (no main registry was edited) and the changes the 5k plan needs.

## What reproduced, and what did not

**Exact**
- E1–E3 damage profile and inflate outcomes: CLM-1000..1010 (OBS-1000..1002).
- Keep-all fractions, CLM-1013 and CLM-1014 (OBS-1003). This came after fixing our own summariser, which had dropped the output of the chunk where zlib raised.
- Block census, CLM-1011 (OBS-1004).
- Checkpoint sizes on miniz_oxide 0.8.9 and 0.9.1, CLM-1015 (OBS-1005).
- Inflate-error search counts, CLM-1016: 218/221 byte-exact (OBS-1006). With 0.9.1 it fixed 219/221 (OBS-1015).
- Grammar localizer and search, CLM-1020..1023: 343 flagged, 324/343 fixed (OBS-1008, OBS-1009).
- Widths, CLM-1039 and CLM-1040 (OBS-1011).
- Headline arithmetic: 562/1,445 = 38.9% (OBS-1014).

**Approximate**
- Search timings, CLM-1017.
- Timeout vs outside-window split, CLM-1024: ours 4/15, PR #16's 3/16.
- Mid-size unlocalized search, CLM-1019: 28/47 = 59.6% on a stride-10 sample, against PR #16's 66%, which lies inside our 95% CI (OBS-1010).

**Not reproduced as stated**
- **CLM-1018.** The committed mzbench accepts 243 of 245 small streams, 242 exact, not 245/245 with 243 exact. The committed code excludes the Adler-32 trailer from the search.
- **CLM-1028 has its detail swapped.** The genuine collision is on 21,134 bytes of output, not 2.7 KB. The 2,707-byte stream is a one-byte trailer match (OBS-1013).

**Errors in the KB.** OBS-0300..0302 undercount damaged Flate streams by 3. The cause is `flate_c9_probe.py`'s CR LF trim, which drops a final 0x0D zlib byte (OBS-1012).

**Could not run.** Each of these has no committed script; all stay claim-only.
- E6, the TrueType localizer (CLM-1026);
- E8, the C10 census (CLM-1033/1034);
- the ObjStm/BaseFont corpus scan (CLM-1036..1038).

## Not checked

- PR #16's engine rules (CLM-1041..1056): no code was read.
- CLM-1035's REPDF table numbers, against the paper.
- Papers cited second-hand by PR #16.
- Uniqueness of the passing candidates, and the trailer-inclusive mzbench variant.
- The one-byte residual between OBS-0302 (1,443 Flate-body bytes) and OBS-0300 (1,442 streams).
- Gap `still_open` against the citing literature: left `unknown`.

## Budget

0 Parallel calls used. No other web searches were made.

## Next steps

1. Fix `research/experiments/flate_c9_probe.py` and re-run OBS-0300..0302.
2. Apply the field updates in `reconciliation.md` §2, chiefly GAP-150's "thousands of bytes" wording and GAP-151's position-wise caveat.
3. Rebuild E6 with a committed script and run HYP-300.
4. Read lopdf, hayro and qpdf code for CLM-1041..1053 in 5k WP-1.4.
