#!/usr/bin/env python3
"""Convert the legacy model to the standalone engine's DPR2 format."""
import array
import gzip
import json
import sys
from pathlib import Path

root = Path(__file__).resolve().parents[1]
with gzip.open(root / 'app/assets/deft_eval_2024-01-27.json.gz', 'rt') as source:
    model = json.load(source)
output = root / 'app/assets/eval.dpr2.gz'
with gzip.GzipFile(filename=str(output), mode='wb', compresslevel=9, mtime=0) as target:
    target.write(b'DPR2\x01\x00\x00\x00')
    for n in range(60):
        phase = model['eval'][n % 2][n // 2]
        for values in [*phase['pattern_eval'], phase['mobility_eval'], [phase['const_eval']]]:
            data = array.array('h', values)
            if sys.byteorder != 'little':
                data.byteswap()
            target.write(data.tobytes())
print(f'{output}: {output.stat().st_size} bytes')
