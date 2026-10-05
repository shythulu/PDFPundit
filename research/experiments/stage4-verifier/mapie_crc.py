#!/usr/bin/env python3
"""Stage 4 verifier (2026-10-05): hands-on check of TOOL-470 (MAPIE) at the pinned 1.5.0.

One conformal risk-control run with mapie.risk_control.BinaryClassificationController on
(a) a seeded synthetic set (sklearn make_classification, seed 20260927) and
(b) an OBS-derived label set: rows of the engine smoke (OBS-0700 re-run, recorded versions)
    where a text engine produced text; label = text_ratio_vs_original >= 0.9; features =
    one-hot damage class + one-hot engine. The point is to show the API works at the pin and
    that the guarantee holds or abstains, not to build a useful model.

usage: python mapie_crc.py <engine_smoke.csv> <out.json>
"""
import csv, json, sys
import numpy as np
import mapie, sklearn
from mapie.risk_control import BinaryClassificationController, precision
from sklearn.datasets import make_classification
from sklearn.linear_model import LogisticRegression
from sklearn.model_selection import train_test_split

SEED = 20260927


def run(name, X, y, target, conf=0.9):
    X_tr, X_rest, y_tr, y_rest = train_test_split(X, y, test_size=0.6, random_state=SEED, stratify=y)
    X_cal, X_te, y_cal, y_te = train_test_split(X_rest, y_rest, test_size=0.5, random_state=SEED, stratify=y_rest)
    clf = LogisticRegression(max_iter=1000).fit(X_tr, y_tr)
    ctl = BinaryClassificationController(predict_function=clf.predict_proba, risk=precision,
                                         target_level=target, confidence_level=conf)
    ctl.calibrate(X_cal, y_cal)
    out = {"name": name, "n_train": int(len(y_tr)), "n_calib": int(len(y_cal)), "n_test": int(len(y_te)),
           "positive_rate": round(float(np.mean(y)), 4), "target_precision": target,
           "confidence_level": conf, "n_valid_params": int(len(ctl.valid_predict_params))}
    if ctl.best_predict_param is None:
        out["best_predict_param"] = None
        out["verdict"] = "abstained: no threshold certified at this calibration size"
        return out
    pred = np.asarray(ctl.predict(X_te)).astype(int)
    tp = int(((pred == 1) & (y_te == 1)).sum()); fp = int(((pred == 1) & (y_te == 0)).sum())
    out.update({"best_predict_param": round(float(ctl.best_predict_param), 4),
                "valid_param_range": [round(float(min(ctl.valid_predict_params)), 4),
                                      round(float(max(ctl.valid_predict_params)), 4)],
                "test_predicted_positive": tp + fp,
                "test_precision": round(tp / (tp + fp), 4) if tp + fp else None,
                "test_recall": round(tp / int((y_te == 1).sum()), 4)})
    out["verdict"] = ("target met on held-out data" if out["test_precision"] is not None
                      and out["test_precision"] >= target else "target missed on held-out data")
    return out


def main(smoke_csv, out_json):
    res = {"mapie": mapie.__version__, "sklearn": sklearn.__version__, "numpy": np.__version__,
           "seed": SEED, "runs": []}
    X, y = make_classification(n_samples=3000, n_features=6, n_informative=4, n_redundant=0,
                               class_sep=1.0, flip_y=0.05, random_state=SEED)
    res["runs"].append(run("synthetic make_classification n=3000", X, y, target=0.9))
    rows = [r for r in csv.DictReader(open(smoke_csv)) if r["text_ratio_vs_original"] not in ("", None)]
    classes = sorted({r["class"] for r in rows}); engines = sorted({r["engine"] for r in rows})
    X = np.array([[r["class"] == c for c in classes] + [r["engine"] == e for e in engines] for r in rows], float)
    y = np.array([float(r["text_ratio_vs_original"]) >= 0.9 for r in rows], int)
    res["obs_derived_input"] = {"rows": len(rows), "classes": classes, "engines": engines}
    res["runs"].append(run("engine smoke: text_ratio>=0.9 from class+engine", X, y, target=0.8))
    json.dump(res, open(out_json, "w"), indent=1)
    print(json.dumps(res, indent=1))


if __name__ == "__main__":
    main(*sys.argv[1:3])
