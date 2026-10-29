#!/usr/bin/env python3
"""Replay isolated diagnostic variants; never modifies the working engine."""
import argparse
import csv
import hashlib
import io
import json
import os
from pathlib import Path
import shutil
import statistics
import subprocess
import tarfile
import tempfile

FEATURES = {
    "control": "",
    "old_root": "diag-old-root",
    "mpc_tree": "diag-mpc-tree",
    "no_probe_write": "diag-mpc-tree-no-write",
    "root_tree": "diag-old-root,diag-mpc-tree",
    "previous": "diag-old-root,diag-mpc-tree,diag-narrow-mpc,diag-no-pv-table,diag-narrow-tt",
    "no_pv": "diag-no-pv",
    "no_stabilize": "diag-no-stabilize",
    "narrow_mpc": "diag-narrow-mpc",
    "window4": "diag-window4",
    "window8": "diag-window8",
    "no_pv_table": "diag-no-pv-table",
    "narrow_tt": "diag-narrow-tt",
    "tail_first": "diag-tail-first",
    "six_order": "diag-six-order",
    "tail_match": "diag-tail-match",
    "pv_root4": "diag-pv-root4",
    "pvs_stability": "diag-pvs-stability",
}
SCORES = dict(zip(range(40, 50), (38, 0, 6, -12, -14, 6, -8, 4, 28, 16)))


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    here = Path(__file__).resolve().parent
    repo = here.parents[2]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--model", type=Path, default=repo.parent / "deft-reversi-feature/data/eval/eval-v4.bin")
    parser.add_argument("--output", type=Path, default=here / "replay-output")
    parser.add_argument("--case", type=int, choices=range(40, 50), default=49)
    parser.add_argument("--rounds", type=int, default=5)
    parser.add_argument("--cpu", type=int, default=1)
    parser.add_argument("--variants", default="control,old_root,mpc_tree,no_probe_write,root_tree,previous")
    args = parser.parse_args()
    variants = args.variants.split(",")
    if args.rounds < 1 or any(name not in FEATURES for name in variants):
        parser.error("invalid rounds or variant names")
    args.output.mkdir(parents=True, exist_ok=False)
    cases = [line for line in (repo / "docs/benchmarks/2026-10-24-ffo40-49/ffo40-49.obf").read_text().splitlines()
             if line.strip() and not line.startswith("#")]
    position = args.output / "position.obf"
    position.write_text(cases[args.case - 40] + "\n")
    env = dict(os.environ, RUSTFLAGS="-Ctarget-cpu=native")
    env.pop("DEFT_INVESTIGATE_TRACE", None)
    records = []
    binaries = {}
    metadata = {"case": args.case, "cpu": args.cpu, "threads": 1, "rounds": args.rounds,
                "model_sha256": digest(args.model), "archive_sha256": digest(here / "baseline-source.tar.gz"),
                "patch_sha256": digest(here / "diagnostic.patch"), "variants": {}}
    with tempfile.TemporaryDirectory(prefix="deft-regression-replay-") as temporary:
        work = Path(temporary)
        engine = work / "engine"
        engine.mkdir()
        with tarfile.open(here / "baseline-source.tar.gz") as archive:
            archive.extractall(engine, filter="data")
        subprocess.run(["patch", "--batch", "-s", "-p1", "-d", str(engine), "-i", str(here / "diagnostic.patch")], check=True)
        shutil.copyfile(here / "diagnostic.Cargo.lock", engine / "Cargo.lock")
        for name in variants:
            command = ["cargo", "build", "--offline", "--release", "--manifest-path", str(engine / "Cargo.toml"),
                       "--example", "investigate"]
            if FEATURES[name]:
                command += ["--features", FEATURES[name]]
            result = subprocess.run(command, env=env, capture_output=True, text=True)
            (args.output / f"build-{name}.log").write_text(result.stdout + result.stderr)
            result.check_returncode()
            binary = work / name
            shutil.copyfile(engine / "target/release/examples/investigate", binary)
            binary.chmod(0o755)
            binaries[name] = binary
            metadata["variants"][name] = {"features": FEATURES[name], "binary_sha256": digest(binary)}
        (args.output / "environment.json").write_text(json.dumps(metadata, indent=2) + "\n")
        for round_number in range(1, args.rounds + 1):
            order = variants if round_number % 2 else list(reversed(variants))
            for name in order:
                result = subprocess.run(["taskset", "-c", str(args.cpu), str(binaries[name]), str(args.model.resolve()),
                                         str(position.resolve()), "exact", "1", "256"], env=env,
                                        capture_output=True, text=True, check=True, timeout=120)
                row = list(csv.DictReader(io.StringIO(result.stdout), delimiter="\t"))[0]
                if int(row["score"]) != SCORES[args.case] or row["mode"] != "Exact" or row["stop"] != "Completed":
                    raise RuntimeError(f"incomplete or incorrect search: {name}: {row}")
                records.append({"round": round_number, "variant": name, **row})
                with (args.output / "results.tsv").open("w") as output:
                    writer = csv.DictWriter(output, fieldnames=list(records[0]), delimiter="\t", lineterminator="\n")
                    writer.writeheader()
                    writer.writerows(records)
            print(f"round {round_number} completed", flush=True)
    summary = {name: {"median_seconds": statistics.median(float(row["seconds"]) for row in records if row["variant"] == name),
                      "nodes": sorted({int(row["nodes"]) for row in records if row["variant"] == name})} for name in variants}
    (args.output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    main()
