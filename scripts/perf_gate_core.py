#!/usr/bin/env python3
"""Shared perf gate core for Bash and PowerShell wrappers."""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path
from typing import Any


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run shared perf gate baseline comparison.")
    parser.add_argument("--platform", choices=("windows", "unix"), required=True)
    parser.add_argument("--report-dir", required=True)
    parser.add_argument("--report-json", required=True)
    parser.add_argument("--report-csv", required=True)
    parser.add_argument(
        "--run-report-command",
        nargs=argparse.REMAINDER,
        help="Optional command to generate perf report before comparison. Prefix with --.",
    )
    return parser.parse_args()


def resolve_baseline_path(platform: str) -> str:
    explicit = os.environ.get("SPARK_PERF_BASELINE", "").strip()
    if explicit:
        return explicit

    platform_file = "perf_gate_windows.json" if platform == "windows" else "perf_gate_unix.json"
    platform_path = Path("perf") / "baselines" / platform_file
    if platform_path.exists():
        return str(platform_path)
    return "perf/baselines/perf_gate_default.json"


def run_report(command: list[str], env: dict[str, str], report_dir: str, report_json: str, report_csv: str) -> None:
    if not command:
        return

    print("[1/2] generate benchmark report")
    next_env = dict(env)
    next_env["SPARK_BENCH_REPORT_DIR"] = report_dir
    next_env["SPARK_BENCH_REPORT_JSON"] = report_json
    next_env["SPARK_BENCH_REPORT_CSV"] = report_csv

    subprocess.run(command, env=next_env, check=True)


def load_json(path: str) -> Any:
    with open(path, "r", encoding="utf-8") as handle:
        return json.load(handle)


def compare_report(baseline_path: str, report_path: str) -> list[str]:
    baseline = load_json(baseline_path)
    report = load_json(report_path)

    scenarios = {item["scenario"]: item for item in report.get("scenarios", [])}
    errors: list[str] = []

    def check(name: str, field: str, actual: float, expected: float, mode: str) -> None:
        if mode == "min" and actual < expected:
            errors.append(f"{name}: {field}={actual} < min {expected}")
        if mode == "max" and actual > expected:
            errors.append(f"{name}: {field}={actual} > max {expected}")

    for rule in baseline.get("scenarios", []):
        name = rule["scenario"]
        actual = scenarios.get(name)
        if actual is None:
            errors.append(f"missing scenario in report: {name}")
            continue
        for field, threshold in rule.get("min", {}).items():
            check(name, field, actual.get(field, 0), threshold, "min")
        for field, threshold in rule.get("max", {}).items():
            check(name, field, actual.get(field, 0), threshold, "max")

    global_rule = baseline.get("global", {})
    global_actual = report.get("global", {})
    for field, threshold in global_rule.get("max", {}).items():
        value = global_actual.get(field)
        if value is None:
            continue
        check("global", field, value, threshold, "max")

    return errors


def main() -> int:
    args = parse_args()
    baseline_path = resolve_baseline_path(args.platform)

    print(f"Using perf baseline: {baseline_path}")
    report_command = args.run_report_command or []
    if report_command[:1] == ["--"]:
        report_command = report_command[1:]
    run_report(report_command, os.environ, args.report_dir, args.report_json, args.report_csv)

    print("[2/2] compare report with baseline")
    errors = compare_report(baseline_path, args.report_json)
    if errors:
        print("Perf gate failed:")
        for err in errors:
            print(f" - {err}")
        return 1

    print("OK: perf gate passed for all scenarios.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
