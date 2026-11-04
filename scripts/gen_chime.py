#!/usr/bin/env python3
"""Synthesize the completion / break-over chime: an original, soft two-note chime.

Two sine tones a rising perfect fifth apart (D5 then A5), each with gentle 2nd
and 3rd harmonics that fade faster than the fundamental, a soft raised-cosine
attack, an exponential decay of about 1.2 s, and a short fade to silence at the
end. Peak level -14 dBFS, 44.1 kHz, mono, 16-bit PCM.

Output: src-tauri/assets/chime.wav (deterministic, standard library only).

Usage:
  python3 scripts/gen_chime.py           # write and verify
  python3 scripts/gen_chime.py --check   # verify the existing file only
"""

from __future__ import annotations

import argparse
import math
import struct
import sys
import wave
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "src-tauri" / "assets" / "chime.wav"

RATE = 44_100
PEAK_DBFS = -14.0
DURATION_S = 1.55
FADE_OUT_S = 0.08
MAX_BYTES = 200 * 1024

# (start s, frequency Hz, gain, decay time constant s)
NOTES = [
    (0.00, 587.33, 0.80, 0.30),  # D5
    (0.20, 880.00, 1.00, 0.34),  # A5
]
# (multiple of the fundamental, relative gain, decay speed-up)
PARTIALS = [(1.0, 1.0, 1.0), (2.0, 0.22, 1.9), (3.0, 0.06, 2.8)]
ATTACK_S = 0.012


def note_sample(t: float, freq: float, gain: float, tau: float) -> float:
    if t < 0:
        return 0.0
    attack = 0.5 - 0.5 * math.cos(math.pi * t / ATTACK_S) if t < ATTACK_S else 1.0
    v = 0.0
    for mult, rel, speed in PARTIALS:
        v += rel * math.exp(-t * speed / tau) * math.sin(2 * math.pi * freq * mult * t)
    return gain * attack * v


def synthesize() -> list[int]:
    n = round(DURATION_S * RATE)
    raw = []
    for i in range(n):
        t = i / RATE
        v = sum(note_sample(t - start, f, g, tau) for start, f, g, tau in NOTES)
        remaining = DURATION_S - t
        if remaining < FADE_OUT_S:
            v *= 0.5 - 0.5 * math.cos(math.pi * remaining / FADE_OUT_S)
        raw.append(v)
    peak = max(abs(v) for v in raw)
    target = 10 ** (PEAK_DBFS / 20) * 32767
    k = target / peak
    return [max(-32768, min(32767, round(v * k))) for v in raw]


def write(samples: list[int], path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with wave.open(str(path), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(struct.pack(f"<{len(samples)}h", *samples))


def verify(path: Path) -> list[str]:
    errors = []
    size = path.stat().st_size
    with wave.open(str(path), "rb") as w:
        ch, width, rate, frames = w.getnchannels(), w.getsampwidth(), w.getframerate(), w.getnframes()
        data = w.readframes(frames)
    if (ch, width, rate) != (1, 2, RATE):
        errors.append(f"expected mono 16-bit {RATE} Hz, got {ch} ch, {8 * width}-bit, {rate} Hz")
    samples = struct.unpack(f"<{frames}h", data)
    peak = max(abs(s) for s in samples)
    dbfs = 20 * math.log10(peak / 32767)
    if abs(dbfs - PEAK_DBFS) > 0.1:
        errors.append(f"peak {dbfs:.2f} dBFS, expected {PEAK_DBFS}")
    if size > MAX_BYTES:
        errors.append(f"{size} bytes > {MAX_BYTES}")
    if samples[0] != 0 or abs(samples[-1]) > 2:
        errors.append("chime must start and end at silence (no clicks)")
    print(
        f"{path.relative_to(ROOT)}: {frames / rate:.2f} s, {rate} Hz, {8 * width}-bit, {ch} ch, peak {dbfs:.2f} dBFS, {size} bytes"
    )
    return errors


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser(description="Synthesize src-tauri/assets/chime.wav")
    ap.add_argument("--check", action="store_true", help="verify only; do not write")
    args = ap.parse_args(argv)
    if not args.check:
        write(synthesize(), OUT)
    errors = verify(OUT)
    for e in errors:
        print(f"  - {e}", file=sys.stderr)
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
