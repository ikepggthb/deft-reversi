#!/usr/bin/env python3
"""Write exact compressed and uncompressed byte counts for progress displays."""
import gzip
import hashlib
import json
from pathlib import Path

assets = Path(__file__).resolve().parents[1] / 'app/assets'
manifest = {}
for name in ('classic', 'pen'):
    path = assets / f'eval-{name}.dpr2.gz'
    with gzip.open(path, 'rb') as source:
        unpacked = sum(len(chunk) for chunk in iter(lambda: source.read(1024 * 1024), b''))
    digest = hashlib.sha256()
    with path.open('rb') as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b''):
            digest.update(chunk)
    manifest[name] = {'file': path.name, 'compressedBytes': path.stat().st_size, 'uncompressedBytes': unpacked, 'sha256': digest.hexdigest()}
(assets / 'model-manifest.json').write_text(json.dumps(manifest, ensure_ascii=False) + '\n')
print(json.dumps(manifest, ensure_ascii=False))
