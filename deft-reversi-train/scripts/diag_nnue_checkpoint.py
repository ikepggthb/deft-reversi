#!/usr/bin/env python3
"""NNUEチェックポイント診断: 重み分布とint16量子化域の逸脱を報告する。"""

from __future__ import annotations

import argparse
import sys

import numpy as np
import torch

INT16_MIN = -32768
INT16_MAX = 32767


def describe(name: str, tensor: torch.Tensor, limit_min: int, limit_max: int) -> None:
    flat = tensor.detach().float().reshape(-1)
    rounded = flat.round()
    over = ((rounded < limit_min) | (rounded > limit_max)).sum().item()
    arr = flat.numpy()
    quants = np.quantile(arr, [0.0, 0.001, 0.01, 0.5, 0.99, 0.999, 1.0]).tolist()
    print(f"{name}")
    print(f"  shape={tuple(tensor.shape)} n={flat.numel()}")
    print(
        "  min={:.1f} p0.1%={:.1f} p1%={:.1f} median={:.1f} "
        "p99%={:.1f} p99.9%={:.1f} max={:.1f}".format(*quants)
    )
    print(f"  abs_max={flat.abs().max().item():.1f} (limit ±{limit_max})")
    print(f"  clamp対象: {over} / {flat.numel()} ({100.0 * over / flat.numel():.4f}%)")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("checkpoint")
    args = parser.parse_args()

    payload = torch.load(args.checkpoint, map_location="cpu", weights_only=False)
    print(f"checkpoint keys: {sorted(payload.keys())}")
    for meta_key in ("epoch", "step", "samples", "seed"):
        if meta_key in payload:
            print(f"  {meta_key}={payload[meta_key]}")
    state = payload.get("model", payload.get("model_state_dict", payload))
    print()
    for key, tensor in state.items():
        if not isinstance(tensor, torch.Tensor):
            continue
        is_bias = "bias" in key
        limit = (INT16_MIN, INT16_MAX)
        if is_bias:
            limit = (-(2**31), 2**31 - 1)
        describe(f"{key}{' (int32枠)' if is_bias else ' (int16枠)'}", tensor, *limit)
        print()
    return 0


if __name__ == "__main__":
    sys.exit(main())
