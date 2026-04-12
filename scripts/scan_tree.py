#!/usr/bin/env python3
"""
Shallow decision tree on IV scan data — diagnostic ceiling test.

Trains best-vs-worst (excludes middle), prints tree rules and feature importances.
Also runs a 50/50 temporal split (first half train, second half test) to check stability.
"""

import pandas as pd
import numpy as np
from sklearn.tree import DecisionTreeClassifier, export_text
from sklearn.metrics import classification_report, confusion_matrix
import sys, os

CSV = os.path.join(os.path.dirname(__file__), "..", "data", "results", "scan_data.csv")

def load(path):
    df = pd.read_csv(path)
    df = df[df["bucket"].isin(["Best", "Worst"])].copy()
    df["label"] = (df["bucket"] == "Best").astype(int)
    return df

ENTRY_FEATURES = [
    "bars_since_spike", "atr_pct", "slow_atr_pct", "tsi", "adx",
    "atr_regime_ratio", "gamma_pos", "net_gex", "gex_abs_ema",
    "pw_vs_spw_atr", "cw_vs_scw_atr", "wall_spread_atr",
    "iv_now", "iv_spike_level", "iv_compression_ratio",
    "iv_base_ratio", "iv_spike_ratio",
    "atr_at_spike", "atr_spike_ratio",
    "cum_iv_drop", "cum_return_atr", "spike_mfe_atr", "spike_mae_atr",
    "pw_dist_atr", "cw_dist_atr",
]

SPIKE_FEATURES = [
    "sp_atr_pct", "sp_slow_atr_pct", "sp_tsi", "sp_adx",
    "sp_atr_regime_ratio", "sp_gamma_pos", "sp_net_gex", "sp_gex_abs_ema",
    "sp_pw_vs_spw_atr", "sp_cw_vs_scw_atr", "sp_wall_spread_atr",
    "sp_pw_dist_atr", "sp_cw_dist_atr",
    "iv_spike_level", "iv_spike_ratio", "atr_at_spike",
]

FEATURES = ENTRY_FEATURES

def train_tree(X, y, max_depth=3):
    clf = DecisionTreeClassifier(max_depth=max_depth, min_samples_leaf=20, class_weight="balanced")
    clf.fit(X, y)
    return clf

def show(clf, X, y, label=""):
    pred = clf.predict(X)
    print(f"\n{'='*60}")
    print(f"  {label}  (n={len(y)}, best={y.sum()}, worst={len(y)-y.sum()})")
    print(f"{'='*60}")
    print(classification_report(y, pred, target_names=["worst", "best"], zero_division=0))
    cm = confusion_matrix(y, pred)
    print(f"  Confusion: TN={cm[0,0]} FP={cm[0,1]} FN={cm[1,0]} TP={cm[1,1]}")
    b_w = cm[1,1] / max(cm[0,1], 1)
    print(f"  Predicted-best b/w: {cm[1,1]}/{cm[0,1]} = {b_w:.2f}")

def main():
    path = sys.argv[1] if len(sys.argv) > 1 else CSV
    df = load(path)
    print(f"Loaded {len(df)} rows: {df['label'].sum()} best, {len(df)-df['label'].sum()} worst")

    X = df[FEATURES].fillna(0).values
    y = df["label"].values
    feat_names = FEATURES

    # Full-period overfit
    clf = train_tree(X, y, max_depth=3)
    print("\n--- Full-period tree (depth=3) ---")
    print(export_text(clf, feature_names=feat_names, decimals=3))
    show(clf, X, y, "Full period (train=test, overfit ceiling)")

    imp = sorted(zip(feat_names, clf.feature_importances_), key=lambda x: -x[1])
    print("\nFeature importances:")
    for name, val in imp:
        if val > 0.01:
            print(f"  {name:<25} {val:.3f}")

    # Temporal split: first half train, second half test
    n = len(df)
    mid = n // 2
    X_tr, y_tr = X[:mid], y[:mid]
    X_te, y_te = X[mid:], y[mid:]

    clf2 = train_tree(X_tr, y_tr, max_depth=3)
    print("\n--- Temporal split tree (depth=3) ---")
    print(export_text(clf2, feature_names=feat_names, decimals=3))
    show(clf2, X_tr, y_tr, "Train (first half)")
    show(clf2, X_te, y_te, "Test (second half)")

    # Depth=4 for more expressiveness
    clf4 = train_tree(X, y, max_depth=4)
    print("\n--- Full-period tree (depth=4) ---")
    print(export_text(clf4, feature_names=feat_names, decimals=3))
    show(clf4, X, y, "Full period depth=4")

    clf4t = train_tree(X_tr, y_tr, max_depth=4)
    show(clf4t, X_te, y_te, "Temporal test depth=4")

    # ── Spike-time features only ──
    sp_cols = [c for c in SPIKE_FEATURES if c in df.columns]
    if sp_cols:
        print("\n" + "="*70)
        print("  SPIKE-TIME FEATURES ONLY")
        print("="*70)
        Xsp = df[sp_cols].fillna(0).values
        clf_sp = train_tree(Xsp, y, max_depth=3)
        print("\n--- Spike-time tree (depth=3) ---")
        print(export_text(clf_sp, feature_names=sp_cols, decimals=3))
        show(clf_sp, Xsp, y, "Spike-time full period")

        imp_sp = sorted(zip(sp_cols, clf_sp.feature_importances_), key=lambda x: -x[1])
        print("\nSpike-time feature importances:")
        for name, val in imp_sp:
            if val > 0.01:
                print(f"  {name:<25} {val:.3f}")

        Xsp_tr, Xsp_te = Xsp[:mid], Xsp[mid:]
        clf_sp2 = train_tree(Xsp_tr, y_tr, max_depth=3)
        print("\n--- Spike-time temporal split tree (depth=3) ---")
        print(export_text(clf_sp2, feature_names=sp_cols, decimals=3))
        show(clf_sp2, Xsp_tr, y_tr, "Spike-time train (first half)")
        show(clf_sp2, Xsp_te, y_te, "Spike-time test (second half)")

if __name__ == "__main__":
    main()
