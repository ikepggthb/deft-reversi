#!/usr/bin/env python3
"""Convert feature engine pattern JSON/bincode v3/v4 into DPR2. Never trains.

Usage: python3 tools/convert_pattern_v2.py INPUT OUTPUT
Writes OUTPUT.mpc.tsv when regression data is present. This compatibility converter is optional; the engine reads v3/v4 models directly.
"""
import argparse
import hashlib
import io
import json
import math
import struct
from pathlib import Path

SIZES = [3**n for n in (10,10,10,8,8,9,10,9,6,7,8)]
MAGIC = b'DPR2\x01\x00\x00\x00'

class Binary:
    def __init__(self, stream): self.stream = stream
    def read(self, n):
        value = self.stream.read(n)
        if len(value) != n: raise ValueError('truncated input')
        return value
    def number(self, fmt): return struct.unpack('<'+fmt, self.read(struct.calcsize('<'+fmt)))[0]
    def length(self, maximum):
        n = self.number('Q')
        if n > maximum: raise ValueError('invalid vector/string length')
        return n
    def string(self): return self.read(self.length(1024*1024)).decode('utf-8')
    def weights(self, n):
        if self.length(n) != n: raise ValueError('invalid weight dimensions')
        return self.read(n*2)

def binary_model(stream):
    source=Binary(stream)
    if source.number('I') not in (3,4): raise ValueError('only feature bincode versions 3/4 are supported')
    for _ in range(5): source.string()
    source.number('Q'); source.number('Q')
    if (source.number('i'),source.number('Q'),source.number('Q'),source.number('I')) != (128,60,11,1):
        raise ValueError('incompatible metadata/schema')
    if source.number('I') != 0: raise ValueError('NNUE is outside this release')
    if source.length(60) != 60: raise ValueError('requires 60 phases')
    phases=[]
    for _ in range(60):
        if source.length(11) != 11: raise ValueError('requires 11 patterns')
        phases.append(b''.join(source.weights(n) for n in SIZES) + source.weights(128) + source.read(2))
    mpc={}
    keys=('constant','empties','depth','mpc_depth','parity','parity_times_empties','parity_times_mpc_depth')
    for mode,table in [('eval_search','search_lv_by_depth'),('final_search','search_lv_by_empties')]:
        n=source.length(61)
        if n!=61: raise ValueError('invalid MPC table')
        mpc[mode]={table:[source.number('i') for _ in range(n)]}
        for name in ('a','b','e_std'): mpc[mode][name]={key:source.number('d') for key in keys}
    if stream.read(1): raise ValueError('trailing binary data')
    return phases,mpc

def json_model(stream):
    document=json.load(stream)
    if 'format_version' in document and document['format_version'] not in (3,4): raise ValueError('unsupported JSON version')
    metadata=document.get('metadata',{})
    for key,value in [('score_scale',128),('n_phases',60),('n_patterns',11),('pattern_schema_version',1)]:
        if metadata.get(key,value)!=value: raise ValueError('incompatible '+key)
    evaluator=document.get('evaluator',document)
    if 'kind' in evaluator:
        if evaluator['kind']!='pattern': raise ValueError('NNUE is outside this release')
        evaluator=evaluator['data']
    phases=evaluator['phases']
    if len(phases)!=60: raise ValueError('requires 60 phases')
    result=[]
    for phase in phases:
        patterns=phase['pattern_weights']
        if len(patterns)!=11 or any(len(p)!=n for p,n in zip(patterns,SIZES)): raise ValueError('invalid pattern dimensions')
        mobility=phase['mobility_weights']
        if len(mobility)!=128: raise ValueError('invalid mobility dimensions')
        weights=[v for pattern in patterns for v in pattern]+mobility+[phase['bias']]
        if any(type(v) is not int or not -32768<=v<=32767 for v in weights): raise ValueError('weights must be i16')
        result.append(struct.pack('<'+'h'*len(weights),*weights))
    return result,document.get('mpc')

def regress(coefficients, empties, depth, shallow, parity):
    features=(1,empties,depth,shallow,parity,parity*empties,parity*shallow)
    keys=('constant','empties','depth','mpc_depth','parity','parity_times_empties','parity_times_mpc_depth')
    return sum(float(coefficients[key])*value for key,value in zip(keys,features))

def rules(mpc):
    for exact,name,table in [(False,'eval_search','search_lv_by_depth'),(True,'final_search','search_lv_by_empties')]:
        config=mpc[name]
        for depth in range(12 if exact else 4,61):
            shallow=config[table][depth]
            if type(shallow) is not int or not 0<=shallow<depth: continue
            # Midgame regression depends on empties: preserve it in the sidecar,
            # rather than silently freezing a coefficient at one stage.
            for empties in ([depth] if exact else range(depth,61)):
                a,b,std=(regress(config[key],empties,0 if exact else depth,shallow,depth%2 if exact else 0) for key in ('a','b','e_std'))
                if all(math.isfinite(v) for v in (a,b,std)) and a>0 and std>=0:
                    yield f'{int(exact)}\t{depth}\t{empties}\t{shallow}\t{a:.17g}\t{b:.17g}\t{std:.17g}\n'

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('input',type=Path);parser.add_argument('output',type=Path)
    args=parser.parse_args()
    if args.input.resolve()==args.output.resolve(): parser.error('input and output must differ')
    if args.output.exists() or Path(str(args.output)+'.mpc.tsv').exists(): parser.error('output already exists')
    with args.input.open('rb') as source:
        prefix=source.read(64).lstrip();source.seek(0)
        phases,mpc=json_model(source) if prefix.startswith(b'{') else binary_model(source)
    payload=MAGIC+b''.join(phases)
    sidecar=''
    if mpc:
        sidecar='# model-sha256='+hashlib.sha256(payload).hexdigest()+'\n# exact depth empties shallow slope intercept deviation\n'+''.join(rules(mpc))
    with args.output.open('xb') as output: output.write(payload)
    if sidecar:
        with Path(str(args.output)+'.mpc.tsv').open('x') as output: output.write(sidecar)
    print(f'converted {args.input} -> {args.output}; sha256={hashlib.sha256(payload).hexdigest()}')

if __name__=='__main__': main()
