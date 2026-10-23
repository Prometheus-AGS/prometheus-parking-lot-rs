#!/usr/bin/env python3
"""Summarize a benchmark run (cr-40) as Markdown.

Reads criterion's results (`<criterion>/**/new/estimates.json`, with the
sibling `benchmark.json` and, when the run compared against a baseline,
`change/estimates.json`) and the suites' own reports (`<reports>/*.json`:
latency percentiles, simulated utilization, machine metadata).

Usage:
    bench-report.py --criterion DIR --reports DIR [--summary FILE]
                    [--regression-threshold 0.15]

Writes the Markdown to --summary (appends; CI passes $GITHUB_STEP_SUMMARY) or
to stdout. For every benchmark whose mean got slower by more than the
threshold *with the whole 95% confidence interval of the change above it*, it
prints a GitHub `::warning::` annotation to stdout. It never fails the job:
shared CI runners differ in CPU model and neighbour load from run to run, so a
cross-run difference is evidence to look at, not a verdict (see
benches/README.md, "Regression policy").
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path


def load(path: Path):
    try:
        return json.loads(path.read_text())
    except (OSError, ValueError):
        return None


def human_time(ns: float) -> str:
    for unit, scale in (("s", 1e9), ("ms", 1e6), ("µs", 1e3)):
        if ns >= scale:
            return f"{ns / scale:.3g} {unit}"
    return f"{ns:.3g} ns"


def human_rate(per_second: float) -> str:
    for unit, scale in (("M", 1e6), ("K", 1e3)):
        if per_second >= scale:
            return f"{per_second / scale:.3g} {unit}elem/s"
    return f"{per_second:.3g} elem/s"


def criterion_rows(root: Path):
    rows = []
    for estimates_path in sorted(root.glob("**/new/estimates.json")):
        bench_dir = estimates_path.parent.parent
        estimates = load(estimates_path)
        meta = load(estimates_path.parent / "benchmark.json") or {}
        if not estimates:
            continue
        mean = estimates["mean"]
        point = mean["point_estimate"]
        low = mean["confidence_interval"]["lower_bound"]
        high = mean["confidence_interval"]["upper_bound"]
        throughput = None
        elements = (meta.get("throughput") or {}).get("Elements")
        if elements:
            throughput = elements * 1e9 / point
        change = load(bench_dir / "change" / "estimates.json")
        rows.append(
            {
                "id": meta.get("full_id") or str(bench_dir.relative_to(root)),
                "mean": point,
                "low": low,
                "high": high,
                "throughput": throughput,
                "change": None
                if not change
                else (
                    change["mean"]["point_estimate"],
                    change["mean"]["confidence_interval"]["lower_bound"],
                    change["mean"]["confidence_interval"]["upper_bound"],
                ),
            }
        )
    return rows


def criterion_table(rows, threshold: float, out, warnings):
    out.append("### Criterion (mean, 95% CI)\n")
    if not rows:
        out.append("_No criterion results found._\n")
        return
    out.append("| benchmark | mean | 95% CI | throughput | change vs baseline |")
    out.append("|---|---|---|---|---|")
    for row in rows:
        change = "no baseline"
        if row["change"]:
            point, low, high = row["change"]
            change = f"{point:+.1%} [{low:+.1%}, {high:+.1%}]"
            if low > threshold:
                change += " **slower**"
                warnings.append(
                    f"::warning title=Benchmark slower than baseline::{row['id']}: "
                    f"mean {point:+.1%} (95% CI {low:+.1%} to {high:+.1%}), "
                    f"above the {threshold:.0%} report threshold. Report-only: "
                    "re-run or compare on one machine before acting."
                )
        rate = human_rate(row["throughput"]) if row["throughput"] else ""
        out.append(
            f"| `{row['id']}` | {human_time(row['mean'])} | "
            f"{human_time(row['low'])} to {human_time(row['high'])} | {rate} | {change} |"
        )
    out.append("")


def latency_table(report, out):
    out.append("### Latency percentiles (µs; median of rounds, [min, max] across rounds)\n")
    out.append(
        "| pool | workload | tasks x rounds | start p50 | start p99 | result p50 | result p99 | utilization |"
    )
    out.append("|---|---|---|---|---|---|---|---|")

    def cell(stat):
        return f"{stat['median']:.1f} [{stat['min']:.1f}, {stat['max']:.1f}]"

    for case in report.get("cases", []):
        summary = case["summary"]
        util = summary["utilization"]
        out.append(
            f"| {case['pool']} | {case['workload']} | {case['tasks']} x {len(case['rounds'])} | "
            f"{cell(summary['submit_to_start_us']['p50'])} | {cell(summary['submit_to_start_us']['p99'])} | "
            f"{cell(summary['submit_to_result_us']['p50'])} | {cell(summary['submit_to_result_us']['p99'])} | "
            f"{util['median']:.3f} [{util['min']:.3f}, {util['max']:.3f}] |"
        )
    out.append("")


def utilization_table(report, out):
    out.append("### Simulated utilization (virtual time, deterministic)\n")
    out.append(
        "| workload | skip-ahead | utilization | per kind | 100-tick windows min / p50 / max | large wait p50 / max (ticks) |"
    )
    out.append("|---|---|---|---|---|---|")
    for case in report.get("cases", []):
        per_kind = ""
        if "utilization_gpu_vram" in case:
            per_kind = f"gpu {case['utilization_gpu_vram']:.3f}, cpu {case['utilization_cpu']:.3f}"
        windows = case["window_utilization"]
        wait = case["large_wait_ticks"]
        out.append(
            f"| {case['workload']} | {case['skip_ahead']} | {case['utilization']:.3f} | {per_kind} | "
            f"{windows['min']:.3f} / {windows['p50']:.3f} / {windows['max']:.3f} | "
            f"{wait['p50']} / {wait['max']} |"
        )
    out.append("")


def metadata_block(reports, out):
    for name, report in reports.items():
        meta = report.get("metadata")
        if meta:
            out.append(f"<details><summary>Machine and toolchain ({name} report)</summary>\n")
            out.append("```json")
            out.append(json.dumps(meta, indent=2))
            out.append("```\n</details>\n")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--criterion", type=Path, required=True)
    parser.add_argument("--reports", type=Path, required=True)
    parser.add_argument("--summary", type=Path)
    parser.add_argument("--regression-threshold", type=float, default=0.25)
    args = parser.parse_args()

    out = ["## Benchmarks (cr-40)\n"]
    warnings: list[str] = []
    criterion_table(criterion_rows(args.criterion), args.regression_threshold, out, warnings)
    reports = {}
    for name in ("latency", "utilization"):
        report = load(args.reports / f"{name}.json")
        if report:
            reports[name] = report
    if "latency" in reports:
        latency_table(reports["latency"], out)
    if "utilization" in reports:
        utilization_table(reports["utilization"], out)
    metadata_block(reports, out)

    text = "\n".join(out) + "\n"
    if args.summary:
        with args.summary.open("a", encoding="utf-8") as handle:
            handle.write(text)
    else:
        sys.stdout.write(text)
    for warning in warnings:
        print(warning)
    return 0


if __name__ == "__main__":
    sys.exit(main())
