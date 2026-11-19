#!/usr/bin/env python3
"""One-shot `agent-os` timings, recorded on Linux (CLI-15; X6 in docs/implementation-plan.md).

Usage: agent_os_timings.py <agent-os executable> [--runs N] [--json FILE] [--markdown FILE]

Runs each one-shot command that answers today without an office N times (after one
unmeasured run) and prints the median and the 95th percentile of its wall time, in
milliseconds, as a Markdown table. `--json` also writes the samples and the method;
`--markdown` appends the table to a file (CI passes `$GITHUB_STEP_SUMMARY`).

Method: each run starts the executable directly (Python's `subprocess`), with no shell, an empty
environment apart from `PATH` and a fresh temporary `HOME` (so nothing reads or writes
a real home or organization), standard input closed and its output captured; the time
is `time.perf_counter_ns()` from before the spawn to after the process has been reaped.
So it includes the spawn and the wait, which a caller in a shell pays too. A run that
exits with another code than 0, or prints something other than what the command
prints, fails the script: a broken command is never recorded as fast.

Nothing here is a bound. The design holds the target of ADR-0014 on the reference Mac
and records Linux (docs/design/agent-os-command-line.md, section 5, row X6); the guard
of CLI-15 comes with the commands it measures. The commands an office answers are
listed with the increment that makes them measurable, never timed against something
that only refuses them.
"""

from __future__ import annotations

import argparse
import json
import math
import os
import platform
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path

# (label, arguments, what standard output must start with)
MEASURED = [
    ("process start (`agent-os --version`)", ["--version"], "agent-os "),
    ("`agent-os whoami --as nova`", ["whoami", "--as", "nova"], "nova\n"),
]

# Commands of the design that need an office to answer. None is timed until then.
WAITING = [
    ("`agent-os who`", "an office the test can start (B1b, then B2)"),
    ("`agent-os msg send`, `msg inbox`, `msg read`, `msg wait`", "messaging in the office (B6)"),
    ("`agent-os task ...`", "tasks (B5)"),
    ("each command inside `session attach`", "sessions (B6)"),
]


def percentile(samples: list[float], fraction: float) -> float:
    """The nearest-rank percentile: the smallest sample with at least `fraction` of the
    samples at or below it."""
    ordered = sorted(samples)
    rank = max(1, math.ceil(fraction * len(ordered)))
    return ordered[rank - 1]


def run_once(executable: str, args: list[str], env: dict[str, str], expect: str) -> float:
    started = time.perf_counter_ns()
    done = subprocess.run(
        [executable, *args],
        env=env,
        stdin=subprocess.DEVNULL,
        capture_output=True,
        check=False,
    )
    elapsed = time.perf_counter_ns() - started
    out = done.stdout.decode(errors="replace")
    if done.returncode != 0 or not out.startswith(expect):
        sys.exit(
            f"agent-os {' '.join(args)} exited with {done.returncode} and printed "
            f"{out[:200]!r} (expected output starting with {expect!r}); stderr: "
            f"{done.stderr.decode(errors='replace')[:200]!r}"
        )
    return elapsed / 1e6


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("executable")
    parser.add_argument("--runs", type=int, default=50)
    parser.add_argument("--json", dest="json_path")
    parser.add_argument("--markdown", dest="markdown_path")
    options = parser.parse_args()
    if options.runs < 1:
        parser.error("--runs is at least 1")
    executable = os.path.abspath(options.executable)
    if not os.access(executable, os.X_OK):
        parser.error(f"{executable} is not an executable file")

    with tempfile.TemporaryDirectory(prefix="agent-os-timings.") as home:
        env = {"PATH": "/usr/bin:/bin", "HOME": home}
        version = subprocess.run(
            [executable, "--version"], capture_output=True, check=True, env=env
        ).stdout.decode().strip()
        results = []
        for label, args, expect in MEASURED:
            run_once(executable, args, env, expect)  # unmeasured: page cache, first exec
            samples = [run_once(executable, args, env, expect) for _ in range(options.runs)]
            results.append(
                {
                    "command": label,
                    "args": args,
                    "runs": len(samples),
                    "median_ms": statistics.median(samples),
                    "p95_ms": percentile(samples, 0.95),
                    "samples_ms": samples,
                }
            )

    machine = f"{platform.system()} {platform.release()} {platform.machine()}"
    lines = [
        f"### `agent-os` one-shot timings ({machine})",
        "",
        f"{version}, {executable}; {options.runs} runs each after one unmeasured run; "
        "wall time from spawn to reap, in ms (method: `.github/scripts/agent_os_timings.py`). "
        "Recorded only: no bound is checked here.",
        "",
        "| Command | Median (ms) | p95 (ms) |",
        "| --- | ---: | ---: |",
    ]
    for result in results:
        lines.append(f"| {result['command']} | {result['median_ms']:.2f} | {result['p95_ms']:.2f} |")
    lines += ["", "Not yet measurable (nothing answers them, or only with a refusal):", ""]
    lines += [f"- {command}: waits for {why}" for command, why in WAITING]
    table = "\n".join(lines) + "\n"
    print(table, end="")
    if options.markdown_path:
        with open(options.markdown_path, "a", encoding="utf-8") as summary:
            summary.write(table)
    if options.json_path:
        record = {
            "agent_os": version,
            "executable": executable,
            "machine": machine,
            "method": "one subprocess per run, no shell, empty environment but PATH and a "
            "temporary HOME, stdin closed, output captured; perf_counter_ns from before "
            "spawn to after reap; one unmeasured run first; nearest-rank p95",
            "results": results,
            "waiting": [{"command": c, "waits_for": w} for c, w in WAITING],
        }
        Path(options.json_path).write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    sys.exit(main())
