#!/usr/bin/env python3
"""Produce a local int8 ONNX export of bge-small-en-v1.5 by dynamic
quantization of the downloaded fp32 graph. This is a *local computation*,
not a download -- BAAI does not publish a pre-quantized ONNX for this model
(unlike all-MiniLM-L6-v2, which ships one), so we quantize ourselves per the
SPIKE's "fp32 and int8 ONNX export or quantisation" wording.

Usage:
    python src/s2/quantize_int8.py
"""
from __future__ import annotations

from pathlib import Path

from onnxruntime.quantization import QuantType, quantize_dynamic

ROOT = Path(__file__).resolve().parents[2]
SRC = ROOT / "models" / "bge-small-en-v1.5" / "model.onnx"
DST = ROOT / "models" / "bge-small-en-v1.5" / "model_int8_dynamic.onnx"


def main() -> None:
    if not SRC.exists():
        raise SystemExit(f"missing {SRC}; run download_models.py first")
    quantize_dynamic(
        model_input=str(SRC),
        model_output=str(DST),
        weight_type=QuantType.QInt8,
    )
    size = DST.stat().st_size
    print(f"wrote {DST} ({size/1e6:.1f} MB, quantized locally from {SRC.name})")


if __name__ == "__main__":
    main()
