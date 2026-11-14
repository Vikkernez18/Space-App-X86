#!/usr/bin/env python3
# AI-assisted
"""Canonical catalog builder for Space-App-X86 (`AGENTS.md` §3.1).

Reads `pipeline/weights.json` and site `metadata.json` files under `data/processed/`,
computes canonical weighted scores over pre-normalized `[0.0, 1.0]` feature scores,
and writes `data/processed/catalog.json`.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path


def normalize_weights(raw_weights: dict[str, float]) -> dict[str, float]:
    clean: dict[str, float] = {}
    total = 0.0
    for k, v in raw_weights.items():
        val = float(v)
        if math_is_positive_finite(val):
            clean[k] = val
            total += val
        else:
            clean[k] = 0.0
    if total > 0.0:
        for k in clean:
            clean[k] = round(clean[k] / total, 8)
    return clean


def math_is_positive_finite(val: float) -> bool:
    return val > 0.0 and val != float("inf") and val == val


def compute_canonical_score(
    feature_scores: dict[str, float], normalized_weights: dict[str, float]
) -> float:
    score = 0.0
    for feature, weight in normalized_weights.items():
        if weight <= 0.0:
            continue
        if feature in feature_scores:
            raw = float(feature_scores[feature])
            clamped = max(0.0, min(1.0, raw))
            score += clamped * weight
    return round(max(0.0, min(1.0, score)), 6)


def discover_site_metadata_files(processed_dir: Path) -> list[Path]:
    candidates: list[Path] = []
    direct = sorted(processed_dir.glob("*/metadata.json"))
    nested = sorted((processed_dir / "sites").glob("*/metadata.json"))
    seen: set[Path] = set()
    for path in direct + nested:
        resolved = path.resolve()
        if resolved not in seen:
            seen.add(resolved)
            candidates.append(path)
    return candidates


def main() -> int:
    repo_root = Path(__file__).resolve().parent.parent
    weights_path = repo_root / "pipeline" / "weights.json"
    processed_dir = repo_root / "data" / "processed"
    catalog_path = processed_dir / "catalog.json"

    if not weights_path.is_file():
        raise FileNotFoundError(f"Missing weights configuration: {weights_path}")

    with weights_path.open("r", encoding="utf-8") as f:
        weights_cfg = json.load(f)

    raw_weights = weights_cfg.get("weights", {})
    normalized_weights = normalize_weights(raw_weights)

    sites: list[dict] = []
    for meta_path in discover_site_metadata_files(processed_dir):
        with meta_path.open("r", encoding="utf-8") as f:
            site_meta = json.load(f)

        feature_scores = site_meta.get("feature_scores", {})
        canonical = compute_canonical_score(feature_scores, normalized_weights)
        site_meta["canonical_score"] = canonical

        with meta_path.open("w", encoding="utf-8") as f:
            json.dump(site_meta, f, indent=2)
            f.write("\n")

        sites.append(site_meta)

    sites.sort(key=lambda s: (-float(s.get("canonical_score", 0.0)), str(s.get("id", ""))))

    catalog = {
        "version": "1.0.0",
        "default_weights": normalized_weights,
        "sites": sites,
        "generated_by": "AI-assisted (pipeline/build_catalog.py)",
    }

    processed_dir.mkdir(parents=True, exist_ok=True)
    with catalog_path.open("w", encoding="utf-8") as f:
        json.dump(catalog, f, indent=2)
        f.write("\n")

    print(f"[OK] Wrote {catalog_path} with {len(sites)} site(s).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
