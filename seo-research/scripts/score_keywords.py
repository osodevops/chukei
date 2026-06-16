"""Score and cluster SEMrush keyword exports into a prioritised masterlist.

Reads every CSV in seo-research/semrush-data/, dedupes keywords, applies the
LLM-weighted priority formula, clusters by shared word stems, and writes
seo-research/output/keyword_masterlist.csv sorted by score.
"""

import csv
import glob
import os
import re
from collections import defaultdict

HERE = os.path.dirname(__file__)
DATA = os.path.join(HERE, "..", "semrush-data")
OUT = os.path.join(HERE, "..", "output", "keyword_masterlist.csv")

QUESTION_STARTS = ("what", "how", "why", "when", "which", "can", "does", "is", "are", "should")


def parse_semrush(path):
    rows = []
    with open(path, newline="", encoding="utf-8") as f:
        reader = csv.DictReader(f, delimiter=";")
        if not reader.fieldnames or "Keyword" not in reader.fieldnames:
            return rows
        for r in reader:
            kw = (r.get("Keyword") or "").strip().lower()
            if not kw:
                continue
            rows.append(
                {
                    "keyword": kw,
                    "volume": _num(r.get("Search Volume")),
                    "cpc": _f(r.get("CPC")),
                    "competition": _f(r.get("Competition")),
                    "kd": _f(r.get("Keyword Difficulty Index")),
                    "intent": (r.get("Intent") or "").strip(),
                    "trend": (r.get("Trends") or "").strip(),
                    "source": os.path.basename(path),
                }
            )
    return rows


def _num(v):
    try:
        return int(v)
    except (TypeError, ValueError):
        return 0


def _f(v):
    try:
        return float(v)
    except (TypeError, ValueError):
        return 0.0


def trend_momentum(trend):
    try:
        vals = [float(x) for x in trend.split(",") if x]
    except ValueError:
        return 0.5
    if len(vals) < 4:
        return 0.5
    recent, earlier = sum(vals[-3:]) / 3, sum(vals[:3]) / 3
    if recent > earlier * 1.15:
        return 1.0
    if recent < earlier * 0.85:
        return 0.0
    return 0.5


def intent_bonus(intent):
    # SEMrush intent codes: 0=commercial 1=informational 2=navigational 3=transactional
    codes = set(intent.split(","))
    if "1" in codes:
        return 1.0
    if "0" in codes or "3" in codes:
        return 0.5
    return 0.0


def cluster_key(kw):
    stop = {"the", "a", "an", "of", "for", "in", "to", "and", "snowflake", "is", "what", "how"}
    words = [w for w in re.findall(r"[a-z]+", kw) if w not in stop]
    return " ".join(sorted(words[:2])) if words else kw


def main():
    best = {}
    for path in glob.glob(os.path.join(DATA, "*.csv")):
        for row in parse_semrush(path):
            cur = best.get(row["keyword"])
            if cur is None or row["volume"] > cur["volume"]:
                best[row["keyword"]] = row

    rows = list(best.values())
    max_vol = max((r["volume"] for r in rows), default=1) or 1
    for r in rows:
        score = (
            (r["volume"] / max_vol) * 0.20
            + ((100 - r["kd"]) / 100) * 0.25
            + (1.0 if r["keyword"].split()[0] in QUESTION_STARTS else 0.0) * 0.20
            + intent_bonus(r["intent"]) * 0.15
            + trend_momentum(r["trend"]) * 0.10
            + (1.0 if r["competition"] < 0.3 else 0.5 if r["competition"] < 0.6 else 0.0) * 0.10
        )
        r["llm_score"] = round(score, 4)
        r["cluster"] = cluster_key(r["keyword"])

    clusters = defaultdict(int)
    for r in rows:
        clusters[r["cluster"]] += 1

    rows.sort(key=lambda r: -r["llm_score"])
    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    with open(OUT, "w", newline="", encoding="utf-8") as f:
        w = csv.writer(f)
        w.writerow(
            ["keyword", "volume", "kd", "competition", "cpc", "intent", "llm_score", "cluster", "cluster_size", "source"]
        )
        for r in rows:
            w.writerow(
                [r["keyword"], r["volume"], r["kd"], r["competition"], r["cpc"], r["intent"], r["llm_score"], r["cluster"], clusters[r["cluster"]], r["source"]]
            )
    print(f"{len(rows)} unique keywords -> {OUT}")
    print("top clusters:", sorted(clusters.items(), key=lambda x: -x[1])[:10])


if __name__ == "__main__":
    main()
