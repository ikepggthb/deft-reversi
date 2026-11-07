#!/usr/bin/env python3
"""Verify and compress the published PEN model for the web application."""
import gzip
import hashlib
import os
from pathlib import Path
import shutil

root = Path(__file__).resolve().parents[1]
source = Path(os.environ.get('DEFT_PEN_MODEL', root / 'deft-eval-v3-pen.dpr2'))
expected = 'aa2d7cd9e42e8f8f7df4f547124683fc86d85cb83b83b332637f07dc1d1f05a8'
hash_value = hashlib.sha256()
with source.open('rb') as data:
    for chunk in iter(lambda: data.read(1024 * 1024), b''):
        hash_value.update(chunk)
if hash_value.hexdigest() != expected:
    raise SystemExit(f'Invalid PEN model SHA-256: {source}')
output = root / 'app/assets/eval-pen.dpr2.gz'
output.parent.mkdir(parents=True, exist_ok=True)
with source.open('rb') as data, output.open('wb') as file:
    with gzip.GzipFile(filename='', fileobj=file, mode='wb', compresslevel=9, mtime=0) as target:
        shutil.copyfileobj(data, target, length=1024 * 1024)
print(f'{output}: {output.stat().st_size} bytes')
