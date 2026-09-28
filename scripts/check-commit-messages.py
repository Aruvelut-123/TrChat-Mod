#!/usr/bin/env python3
"""Validate new Git commit subjects against the repository convention."""

from __future__ import annotations

import os
import re
import subprocess
import sys


CONVENTIONAL = re.compile(
    r"^(feat|fix|docs|refactor|perf|test|build|ci|chore|revert)"
    r"(\([a-z0-9][a-z0-9._/-]*\))?!?: .{1,100}$"
)
AUTOMATIC_MERGE = re.compile(r"^Merge (pull request|branch|remote-tracking branch) ")
ZERO_SHA = "0" * 40


def git(*arguments: str) -> str:
    return subprocess.check_output(
        ["git", *arguments],
        text=True,
        encoding="utf-8",
        errors="replace",
    ).strip()


def revision_range() -> str:
    if len(sys.argv) > 1:
        return sys.argv[1]

    base = os.environ.get("COMMIT_BASE_SHA", "").strip()
    head = os.environ.get("COMMIT_HEAD_SHA", "").strip() or "HEAD"
    if base and base != ZERO_SHA:
        try:
            git("cat-file", "-e", f"{base}^{{commit}}")
            return f"{base}..{head}"
        except subprocess.CalledProcessError:
            pass

    try:
        git("rev-parse", f"{head}^")
        return f"{head}^..{head}"
    except subprocess.CalledProcessError:
        return head


def main() -> int:
    selected_range = revision_range()
    output = git("log", "--format=%H%x09%s", selected_range)
    failures: list[tuple[str, str]] = []

    for line in output.splitlines():
        commit, separator, subject = line.partition("\t")
        if not separator:
            continue
        if CONVENTIONAL.fullmatch(subject) or AUTOMATIC_MERGE.match(subject):
            continue
        failures.append((commit[:7], subject))

    if failures:
        print(
            "Commit messages must follow: "
            "<type>(<scope>): <description> (description: 1-100 characters)",
            file=sys.stderr,
        )
        for commit, subject in failures:
            print(f"  {commit}: {subject}", file=sys.stderr)
        return 1

    count = 0 if not output else len(output.splitlines())
    print(f"Validated {count} commit message(s) in {selected_range}.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
