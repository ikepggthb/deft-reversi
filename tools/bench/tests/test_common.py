"""Checks of the bench helpers that need no engine: python3 -m unittest discover -s tools/bench/tests"""
import sys
import tempfile
import unittest
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

    def test_paths_are_absolute(self):
        self.assertTrue(all(p.is_absolute() for p in paths().values()))


if __name__ == "__main__":
    unittest.main()
