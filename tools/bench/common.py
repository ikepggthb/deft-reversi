"""Shared paths, engine specs and position files for the bench tools.

Engines, models and data live in the ignored `local/` folder (see
local/README.md). Their default places are below; `local/bench.toml`
overrides any of them (keys as in DEFAULTS, paths relative to the repo).
"""
import io
import csv
import os
import re
import subprocess
import tomllib
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
POSITIONS = Path(__file__).resolve().parent / "positions"
DEFAULTS = {
    # Built with RUSTFLAGS="-Ctarget-cpu=native" CARGO_TARGET_DIR=target-native.
    "solve": "target-native/release/examples/solve",
    "gtp": "target-native/release/examples/gtp",
    "model": "local/release/model-v3-pen/deft-eval-v3-pen.dpr2",
    # Edax 4.5.5 as distributed; it reads data/eval.dat next to its bin/.
    "edax": "local/edax-4.5.5/bin/lEdax-x86-64-v3",
    # Egaroucid for Console 7.8.1 built with the official clang++ command.
    "egaroucid": "local/bench-engines/dl-egaroucid-7.8.1/Egaroucid-console_v7.8.1/bin/Egaroucid_for_Console_clang.out",
}


def paths():
    """The tool paths, with local/bench.toml applied, as absolute paths."""
    values = dict(DEFAULTS)
    config = REPO / "local/bench.toml"
    if config.exists():
        values.update(tomllib.loads(config.read_text()))
    return {k: Path(v) if Path(v).is_absolute() else REPO / v for k, v in values.items()}


def read_answers(path):
    """Positions of an .obf file with answers (`BOARD SIDE; A2:+38; ...`):
    a list of (position, best score)."""
    out = []
    for line in Path(path).read_text().splitlines():
        if not line.strip() or line.startswith("#"):
            continue
        pos, _, rest = line.partition(";")
        scores = [int(s) for s in re.findall(r":\s*([+-]?\d+)", rest)]
        out.append((pos.strip(), max(scores) if scores else None))
    return out


def empties(position):
    return position.split()[0].count("-")


class Deft:
    """A Deft configuration: `NAME` or `NAME:key=value,...`.

    Keys: `solve` (binary), `model`, `tt` (MiB), and any upper-case key,
    which is set in the environment (for temporary switches)."""

    kind = "deft"

    def __init__(self, spec, tools):
        self.name, _, settings = spec.partition(":")
        self.solve, self.model, self.tt, self.env = tools["solve"], tools["model"], 256, {}
        for item in filter(None, settings.split(",")):
            key, _, value = item.partition("=")
            if key == "solve":
                self.solve = Path(value)
            elif key == "model":
                self.model = Path(value)
            elif key == "tt":
                self.tt = int(value)
            elif key.isupper():
                self.env[key] = value
            else:
                raise SystemExit(f"unknown setting {key!r} in {spec!r}")

    def solve_file(self, case, goal, threads, cpus):
        cmd = ["taskset", "-c", cpus, str(self.solve), str(self.model), str(case), goal, str(threads), str(self.tt)]
        out = subprocess.run(cmd, capture_output=True, text=True, check=True, env={**os.environ, **self.env}).stdout
        return list(csv.DictReader(io.StringIO(out), delimiter="\t"))


def engine(spec, tools):
    """`edax`, `egaroucid`, or a Deft configuration."""
    if spec == "edax":
        return Edax(tools)
    if spec == "egaroucid":
        return Egaroucid(tools)
    return Deft(spec, tools)


class Edax:
    kind = name = "edax"

    def __init__(self, tools):
        self.binary = tools["edax"]

    def exact(self, case, threads, cpus, hash_bits=23):
        cmd = ["taskset", "-c", cpus, str(self.binary), "-solve", str(case), "-n", str(threads), "-l", "60",
               "-h", str(hash_bits), "-eval-file", "data/eval.dat", "-book-usage", "off"]
        out = subprocess.run(cmd, cwd=self.binary.parent, capture_output=True, text=True, check=True).stdout
        d, s, mi, se, n = re.findall(r"^\s*1\|\s*(\d+)\s+([+-]\d+)\s+(\d+):(\d+\.\d+)\s+(\d+)", out, re.M)[0]
        return int(d), int(s), int(mi) * 60 + float(se), int(n)


class Egaroucid:
    kind = name = "egaroucid"

    def __init__(self, tools):
        self.binary = tools["egaroucid"]

    def exact(self, case, threads, cpus, hash_bits=23):
        cmd = ["taskset", "-c", cpus, str(self.binary), "-t", str(threads), "-l", "60", "-nobook",
               "-hash", str(hash_bits), "-s", str(case)]
        out = subprocess.run(cmd, cwd=self.binary.parent, capture_output=True, text=True, check=True).stdout
        d, s, h, mi, se, n = re.findall(
            r"^\|\s*60\|\s*(\d+)@100%\|\s*\w+\|\s*([+-]?\d+)\|\s*(\d+):(\d+):(\d+\.\d+)\|\s*(\d+)", out, re.M)[0]
        return int(d), int(s), int(h) * 3600 + int(mi) * 60 + float(se), int(n)
