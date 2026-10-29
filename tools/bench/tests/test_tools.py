import importlib.util
from pathlib import Path
from types import SimpleNamespace
import tempfile
import tomllib
import unittest

BENCH = Path(__file__).resolve().parents[1]


def load(name):
    spec = importlib.util.spec_from_file_location(name, BENCH / f'{name}.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class ToolsTests(unittest.TestCase):
    def test_reference_paths_are_explicit_and_toml_quoted(self):
        reference = load('compare_reference')
        with tempfile.TemporaryDirectory() as tmp:
            old = Path(tmp) / 'old source 🦀 "quoted"'
            old.mkdir()
            (old / 'Cargo.toml').write_text('[package]\nname="legacy"\n')
            project = reference.prepare(old, Path(tmp) / 'generated')
            manifest = tomllib.loads((project / 'Cargo.toml').read_text())
            self.assertEqual(manifest['dependencies']['old']['path'], str(old))
            self.assertEqual(manifest['dependencies']['new']['path'],
                             str(reference.REPO / 'deft-reversi-engine'))
            self.assertTrue((project / 'src/main.rs').is_file())
            with self.assertRaises(ValueError):
                reference.prepare(Path(tmp) / 'missing', project)
            with self.assertRaises(ValueError):
                reference.prepare(old, old)

    def test_real_engine_output_parsers(self):
        import json
        compare = load('compare_engines')
        logs = BENCH.parents[1] / 'docs/benchmarks/2026-10-27-three-engines/run/process-output.jsonl'
        seen = set()
        with logs.open() as source:
            for line in source:
                item = json.loads(line)
                name = item['engine']
                if name in seen:
                    continue
                score, seconds, nodes, action = compare.parse_result(
                    name, SimpleNamespace(stdout=item['stdout']), 20)
                self.assertEqual(score, 38)
                self.assertGreater(seconds, 0)
                self.assertGreater(nodes, 0)
                self.assertTrue(action)
                seen.add(name)
                if len(seen) == 4:
                    break
        self.assertEqual(seen, set(compare.NAMES))

    def test_selective_or_incomplete_deft_output_is_rejected(self):
        compare = load('compare_engines')
        for mode, stop, depth in [('SelectiveEndgame', 'Completed', 20),
                                  ('Exact', 'NodeLimit', 20), ('Exact', 'Completed', 19)]:
            text = f'stop\tmode\tdepth\tscore\tseconds\tnodes\taction\n{stop}\t{mode}\t{depth}\t38\t1\t5\ta1\n'
            with self.assertRaises(RuntimeError):
                compare.parse_result('deft', SimpleNamespace(stdout=text), 20)

    def test_orders_balance_slots_and_predecessors(self):
        compare = load('compare_engines')
        orders = compare.ORDERS
        for slot in range(4):
            self.assertEqual({o[slot] for o in orders}, set(range(4)))
        pairs = [(a, b) for o in orders for a, b in zip(o, o[1:])]
        self.assertEqual(len(set(pairs)), 12)


if __name__ == '__main__':
    unittest.main()
