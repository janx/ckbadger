#!/usr/bin/env python3
"""Compare a temporary DAO statistics fixture in two isolated worktrees.

Use dao_perf_fixture.py to add the same test to each revision first. This
script runs already-built test binaries, so compilation is outside timing.
"""

import argparse
import json
import os
from pathlib import Path
import re
import statistics
import subprocess
import time


PERF_LINE = re.compile(
    r"DAO_PERF entries=(\d+) mixed=(\w+) runs=(\d+) capacity=(\d+) "
    r"free=(\d+) unclaimed=(\d+) claimed=(\d+) refresh_ms=\[([^]]+)\] "
    r"snapshot_ms=\[([^]]+)\](?: batch_ms=\[([^]]+)\])?"
)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--before-worktree", type=Path, required=True)
    parser.add_argument("--after-worktree", type=Path, required=True)
    parser.add_argument("--before-binary", type=Path, required=True)
    parser.add_argument("--after-binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--mixed", action="store_true")
    parser.add_argument("--iterations", type=int, default=12)
    parser.add_argument("--entries", nargs=2, type=int, default=[512, 8192])
    args = parser.parse_args()
    if args.iterations < 1 or any(count < 1 for count in args.entries):
        parser.error("iterations and entry counts must be positive")

    first, second = args.entries
    order = [
        ("before", first), ("after", first), ("after", second), ("before", second),
        ("after", first), ("before", first), ("before", second), ("after", second),
    ]
    worktrees = {"before": args.before_worktree, "after": args.after_worktree}
    binaries = {"before": args.before_binary, "after": args.after_binary}
    expected = {}
    with args.output.open("w") as sink:
        for label, count in order:
            env = dict(
                os.environ,
                DAO_PERF_ENTRIES=str(count),
                DAO_PERF_ITERS=str(args.iterations),
                DAO_PERF_MIXED="1" if args.mixed else "0",
                RUST_TEST_THREADS="1",
            )
            started = time.time()
            load = os.getloadavg()
            result = subprocess.run(
                [str(binaries[label].resolve()), "--nocapture", "test_dao_perf_nonempty_fixture"],
                cwd=worktrees[label],
                env=env,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                check=False,
            )
            match = PERF_LINE.search(result.stdout)
            if result.returncode or "test result: ok. 1 passed" not in result.stdout or not match:
                raise RuntimeError(f"{label} {count} fixture failed:\n{result.stdout}")
            assert int(match.group(1)) == count
            assert (match.group(2) == "true") == args.mixed
            assert int(match.group(3)) == args.iterations
            values = tuple(map(int, match.group(4, 5, 6, 7)))
            if count in expected and values != expected[count]:
                raise AssertionError(
                    f"DAO numeric output differs at {count} entries: {values} != {expected[count]}"
                )
            expected[count] = values
            timings = {
                "refresh_ms": [float(v) for v in match.group(8).split(",")],
                "snapshot_ms": [float(v) for v in match.group(9).split(",")],
            }
            if match.group(10):
                timings["batch_ms"] = [float(v) for v in match.group(10).split(",")]
            record = {
                "revision": label,
                "entries": count,
                "mixed": args.mixed,
                "load": load,
                "start_unix": started,
                "elapsed_s": time.time() - started,
                "capacity": values[0],
                "free": values[1],
                "unclaimed": values[2],
                "claimed": values[3],
                **timings,
                **{f"{key}_median": statistics.median(value) for key, value in timings.items()},
                "test_output": result.stdout,
            }
            sink.write(json.dumps(record) + "\n")
            sink.flush()
            print(
                label, count,
                f"load={load[0]:.1f}",
                *(f"{key}={record[key + '_median']:.2f}" for key in timings),
                flush=True,
            )


if __name__ == "__main__":
    main()
