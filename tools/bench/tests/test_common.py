"""外部エンジンを必要としない bench 共通ヘルパーの単体テスト。

実行方法:
  python3 -m unittest discover -s tools/bench/tests
"""
import contextlib
import io
import sys
import tempfile
import unittest
from unittest.mock import patch
from types import SimpleNamespace
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import common  # noqa: E402
from common import POSITIONS, Deft, DEFAULTS, empties, engine, fetch_xot, paths, read_answers  # noqa: E402


class Common(unittest.TestCase):
    """common.py のパス解決・局面読み込み・エンジン設定・CLI 呼び出しを検証するテスト群。"""

    def test_answers_are_the_best_scores(self):
        """正解付き .obf から最善手スコアと空きマス数を正しく読み取れることを確認する。"""
        positions = read_answers(POSITIONS / "ffo40-59.obf")
        self.assertEqual(len(positions), 20)
        self.assertEqual(positions[0][1], 38)
        self.assertEqual(positions[1][1], 0)
        self.assertEqual(empties(positions[0][0]), 20)

    def test_answers_without_scores(self):
        """スコア記載のない .obf 行ではスコアが None になることを確認する。"""
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "a.obf"
            p.write_text("# comment\n" + "-" * 64 + " X\n")
            self.assertEqual(read_answers(p), [("-" * 64 + " X", None)])

    def test_deft_settings(self):
        """Deft 構成文字列の解析、未知キーの拒否、および engine() の振り分けを確認する。"""
        tools = {k: Path("/x") / k for k in DEFAULTS}
        d = Deft("new:solve=/tmp/s,model=/tmp/m,tt=64,DEFT_X=1", tools)
        self.assertEqual((d.name, d.solve, d.model, d.tt, d.env), ("new", Path("/tmp/s"), Path("/tmp/m"), 64, {"DEFT_X": "1"}))
        self.assertEqual(Deft("base", tools).solve, tools["solve"])
        with self.assertRaises(SystemExit):
            Deft("bad:depth=3", tools)
        self.assertEqual(engine("edax", tools).binary, tools["edax"])

    def test_searches_use_the_cli_and_keep_machine_results(self):
        """solve_file が正しい CLI 引数を組み立て、TSV の高精度な値とバージョン表示を保持することを確認する。"""
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
        """paths() が返す全パスが絶対パスであることを確認する。"""
        self.assertTrue(all(p.is_absolute() for p in paths().values()))

    def test_xot_is_fetched_once_and_checked(self):
        """XOT の開局リストは手元になければ取得し、SHA-256 が違えば保存しないことを確認する。"""
        body = b"f5d6c4d3c2b3b4b5\n"

        class Response(io.BytesIO):
            def __enter__(self):
                return self

            def __exit__(self, *args):
                return False

        with tempfile.TemporaryDirectory() as d:
            path = Path(d) / "xot" / "openingslarge.txt"
            with patch.object(common, "XOT_SHA256", common.hashlib.sha256(body).hexdigest()), \
                    patch.object(common.urllib.request, "urlopen", return_value=Response(body)) as get, \
                    contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(fetch_xot(path), path)
                self.assertEqual(path.read_bytes(), body)
                fetch_xot(path)
                self.assertEqual(get.call_count, 1)
            other = Path(d) / "other.txt"
            with patch.object(common.urllib.request, "urlopen", return_value=Response(body)), \
                    contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
                fetch_xot(other)
            self.assertFalse(other.exists())


if __name__ == "__main__":
    unittest.main()
