"""Checks of the bench helpers that need no engine: python3 -m unittest discover -s tools/bench/tests"""
import contextlib
import io
import sys
import tempfile
import unittest
from unittest.mock import patch
from types import SimpleNamespace
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import POSITIONS, Deft, DEFAULTS, empties, engine, paths, read_answers  # noqa: E402


class Common(unittest.TestCase):
    def test_answers_are_the_best_scores(self):
        positions = read_answers(POSITIONS / "ffo40-59.obf")
        self.assertEqual(len(positions), 20)
        self.assertEqual(positions[0][1], 38)
        self.assertEqual(positions[1][1], 0)
        self.assertEqual(empties(positions[0][0]), 20)

    def test_answers_without_scores(self):
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "a.obf"
            p.write_text("# comment\n" + "-" * 64 + " X\n")
            self.assertEqual(read_answers(p), [("-" * 64 + " X", None)])

    def test_deft_settings(self):
        tools = {k: Path("/x") / k for k in DEFAULTS}
        d = Deft("new:solve=/tmp/s,model=/tmp/m,tt=64,DEFT_X=1", tools)
        self.assertEqual((d.name, d.solve, d.model, d.tt, d.env), ("new", Path("/tmp/s"), Path("/tmp/m"), 64, {"DEFT_X": "1"}))
        self.assertEqual(Deft("base", tools).solve, tools["solve"])
        with self.assertRaises(SystemExit):
            Deft("bad:depth=3", tools)
        self.assertEqual(engine("edax", tools).binary, tools["edax"])

    def test_searches_use_the_cli_and_keep_machine_results(self):
        tools = {k: Path("/x") / k for k in DEFAULTS}
        cli = Deft("cli", tools)
        output = "position\tscore\taction\tmode\tdepth\tnodes\tseconds\tstop\n1\t38\tA2\tExact\t20\t42\t0.123456789\tCompleted\n"
        versions = "Versions: CLI 1.0.0-dev.1 | Engine 2.0.0-dev.1 | Book library 1.0.0-dev.1"
        diagnostics = io.StringIO()
        for goal, flags in [("exact", ["--exact"]), ("level18", ["--level", "18"]), ("12", ["--depth", "12"])]:
            with self.subTest(goal=goal), patch(
                "common.subprocess.run", return_value=SimpleNamespace(stdout=output, stderr=versions+"\n")
            ) as run, contextlib.redirect_stderr(diagnostics):
                rows = cli.solve_file(Path("case.obf"), goal, 4, "0,2,4,6")
                cmd = run.call_args.args[0]
                self.assertEqual(cmd[:5], ["taskset", "-c", "0,2,4,6", "/x/solve", "solve"])
                self.assertTrue(Path(cmd[5]).is_absolute())
                self.assertEqual(cmd[6:], ["--model", "/x/model", *flags, "--threads", "4", "--tt-mib", "256", "--tsv"])
                self.assertEqual(rows[0]["seconds"], "0.123456789")
                self.assertEqual(rows[0]["stop"], "Completed")
        self.assertEqual(diagnostics.getvalue(), f"cli: {versions}\n")

    def test_paths_are_absolute(self):
        self.assertTrue(all(p.is_absolute() for p in paths().values()))


if __name__ == "__main__":
    unittest.main()
