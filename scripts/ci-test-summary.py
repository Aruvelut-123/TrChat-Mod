#!/usr/bin/env python3
"""Write Gradle JUnit results to the GitHub Actions job summary."""

from __future__ import annotations

import glob
import os
import xml.etree.ElementTree as element_tree


def main() -> None:
    report_files = sorted(glob.glob("build/test-results/test/TEST-*.xml"))
    summary_path = os.environ.get("GITHUB_STEP_SUMMARY")
    if not summary_path:
        raise RuntimeError("GITHUB_STEP_SUMMARY is not available")

    totals = {"tests": 0, "failures": 0, "errors": 0, "skipped": 0, "time": 0.0}
    failed_cases: list[str] = []
    suites: list[tuple[str, int, int, int, int, float]] = []

    for report_file in report_files:
        root = element_tree.parse(report_file).getroot()
        tests = int(root.attrib.get("tests", 0))
        failures = int(root.attrib.get("failures", 0))
        errors = int(root.attrib.get("errors", 0))
        skipped = int(root.attrib.get("skipped", 0))
        elapsed = float(root.attrib.get("time", 0.0))
        name = root.attrib.get("name", os.path.basename(report_file))
        suites.append((name, tests, failures, errors, skipped, elapsed))
        totals["tests"] += tests
        totals["failures"] += failures
        totals["errors"] += errors
        totals["skipped"] += skipped
        totals["time"] += elapsed

        for case in root.findall(".//testcase"):
            if case.find("failure") is not None or case.find("error") is not None:
                failed_cases.append(
                    f"{case.attrib.get('classname', name)}.{case.attrib.get('name', 'unknown')}"
                )

    with open(summary_path, "a", encoding="utf-8") as summary:
        summary.write("## NeoForge 测试报告 / Test report\n\n")
        if not report_files:
            summary.write("未生成 JUnit XML；构建可能在测试阶段之前失败。\n")
            return

        passed = totals["tests"] - totals["failures"] - totals["errors"] - totals["skipped"]
        status = "✅ 通过" if not failed_cases else "❌ 失败"
        summary.write(
            f"**{status}** — {totals['tests']} tests, {passed} passed, "
            f"{totals['failures'] + totals['errors']} failed, "
            f"{totals['skipped']} skipped, {totals['time']:.3f}s\n\n"
        )
        summary.write("| Test suite | Tests | Failed | Skipped | Time |\n")
        summary.write("| --- | ---: | ---: | ---: | ---: |\n")
        for name, tests, failures, errors, skipped, elapsed in suites:
            summary.write(
                f"| `{name}` | {tests} | {failures + errors} | {skipped} | {elapsed:.3f}s |\n"
            )

        if failed_cases:
            summary.write("\n### Failed tests\n\n")
            for failed_case in failed_cases:
                summary.write(f"- `{failed_case}`\n")


if __name__ == "__main__":
    main()
