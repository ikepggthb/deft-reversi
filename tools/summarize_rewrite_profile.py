#!/usr/bin/env python3
"""Resolve sampler instruction offsets against an unstripped ELF executable."""
import argparse
import bisect
import collections
import hashlib
import json
import re
import subprocess
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('binary', type=Path)
    parser.add_argument('samples', type=Path)
    parser.add_argument('out', type=Path)
    args = parser.parse_args()
    output = subprocess.check_output(['nm', '-n', '-S', '--defined-only', '--demangle',
                                      str(args.binary)], text=True)
    symbols = []
    for line in output.splitlines():
        fields = line.split(None, 3)
        if len(fields) == 4 and fields[2] in ('t', 'T') and int(fields[1], 16):
            symbols.append((int(fields[0], 16), int(fields[1], 16), fields[3]))
    symbols.sort()
    starts = [row[0] for row in symbols]
    functions = collections.Counter()
    instructions = collections.Counter()
    text = args.samples.read_text()
    header = re.search(r'total=(\d+) outside_executable=(\d+)', text)
    if header is None:
        parser.error('missing sampler header')
    for line in text.splitlines():
        if line.startswith('#'):
            continue
        pc, count = line.split()
        pc, count = int(pc, 16), int(count)
        index = bisect.bisect_right(starts, pc) - 1
        if index >= 0:
            start, size, name = symbols[index]
            if pc < start + size:
                functions[name] += count
                instructions[(name, pc)] += count
    mapped = sum(functions.values())
    report = {'binary_sha256': hashlib.sha256(args.binary.read_bytes()).hexdigest(),
              'total_samples': int(header[1]), 'outside_executable': int(header[2]),
              'mapped_samples': mapped, 'method': 'ITIMER_PROF 1ms, executable self-time',
              'functions': [{'name': name, 'samples': count, 'fraction_mapped': count / mapped}
                            for name, count in functions.most_common()],
              'instructions': [{'name': name, 'offset': hex(pc), 'samples': count}
                               for (name, pc), count in instructions.most_common(20)]}
    args.out.write_text(json.dumps(report, indent=2) + '\n')
    for row in report['functions'][:8]:
        print(f'{row["fraction_mapped"]:.1%}\t{row["name"]}')


if __name__ == '__main__':
    main()
