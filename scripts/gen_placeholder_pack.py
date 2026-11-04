#!/usr/bin/env python3
"""Generate the bundled placeholder character pack: "Pip", an original seedling.

Pip is a round moss-green creature with a two-leaf sprout, dot eyes and a small
blush. It is original work made for this project: every shape below is written
by hand as path data, nothing is traced or downloaded. The output is dedicated
to the public domain (CC0-1.0, see packs/placeholder/LICENSE.md).

Output (deterministic, no network, standard library only):
  packs/placeholder/<animation>.svg   one horizontal strip per animation,
                                      96x96 frames, left to right (SPEC §7)
  packs/placeholder/pack.json         the pack manifest

Usage:
  python3 scripts/gen_placeholder_pack.py           # write, then validate
  python3 scripts/gen_placeholder_pack.py --check   # validate only, write nothing
  python3 scripts/gen_placeholder_pack.py --self-test  # unit-test the generator, then validate

Validation checks the manifest against SPEC §7 (and the unlock table in
data/unlocks.json, so every animation/idleVariant unlock has art), that every
sheet exists, is well-formed SVG without scripts or external references, and
that sheet width == frames * frameWidth, and that every generated file is up to
date with this script. It checks that the blink strip changes only the eyes of
the states it replaces (idle and focus, SPEC §7.1), so a blink never makes the
body jump. It also checks the palettes: the base "forest" palette below and
every palette-kind unlock in data/unlocks.json (hex colors only, whitelisted
filters, the CONTRAST_RULES ratios).

scripts/gen_icons.py imports the geometry from this module so the app icon and
the sprites share one design.
"""

from __future__ import annotations

import argparse
import json
import math
import re
import sys
import xml.etree.ElementTree as ET
from dataclasses import dataclass, field, fields, replace
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PACK_DIR = ROOT / "packs" / "placeholder"
UNLOCKS_PATH = ROOT / "data" / "unlocks.json"
README_PATH = PACK_DIR / "README.md"

FRAME = 96  # frame width and height in px
GROUND_Y = 84.0  # where Pip's local origin (bottom centre of the body) sits
CENTER_X = 48.0

# ---------------------------------------------------------------------------
# Colors
# ---------------------------------------------------------------------------

OUTLINE = "#3b5b32"
BODY = "#8dbb66"
BODY_HIGHLIGHT = "#b4d78f"
BODY_DARK = "#6f9d4f"
LEAF = "#a9d46b"
LEAF_VEIN = "#6c9c43"
STEM = "#5f8d3f"
EYE = "#2b2c26"
BLUSH = "#f59c9c"
MITT = "#9cc775"
MOUTH_FILL = "#7a3b33"
SHADOW = "#24331f"

OUTLINE_W = 2.2

# ---------------------------------------------------------------------------
# Geometry. Paths are lists of commands: ("M", x, y), ("L", x, y),
# ("Q", x1, y1, x, y), ("C", x1, y1, x2, y2, x, y), ("Z",).
# Body-local coordinates: origin at the bottom centre of the body, y up is
# negative. The body is 54 px wide and 46 px tall.
# ---------------------------------------------------------------------------

BODY_PATH = [
    ("M", 0, 0),
    ("C", 16, 0, 27, -3, 27, -16),
    ("C", 27, -34, 15, -46, 0, -46),
    ("C", -15, -46, -27, -34, -27, -16),
    ("C", -27, -3, -16, 0, 0, 0),
    ("Z",),
]
BODY_TOP = -46.0

# A leaf pointing right from its base at (0, 0).
LEAF_PATH = [
    ("M", 0, 0),
    ("C", 3, -5.5, 10, -8.5, 16, -6.5),
    ("C", 12.5, 0, 5.5, 2.5, 0, 0),
    ("Z",),
]
LEAF_VEIN_PATH = [("M", 1.8, -0.6), ("Q", 8, -4.2, 13.5, -5.6)]

# The stem grows from inside the top of the head; its tip is where the leaves meet.
STEM_PATH = [("M", 0, 4), ("C", 0.6, -2, -0.6, -6, 0.5, -11)]
STEM_TIP = (0.5, -11.0)

# (x, y, rotation, scale, mirrored) of the two leaves relative to the stem tip.
LEAVES = [(0.0, 0.0, -24.0, 1.0, False), (0.0, 0.6, -28.0, 0.82, True)]

FOOT_POSITIONS = [(-11.0, -1.4), (11.0, -1.4)]
FOOT_RX, FOOT_RY = 6.6, 3.7

SHOULDERS = [(-23.5, -19.0), (23.5, -19.0)]
ARM_W, ARM_LEN = 7.4, 12.0

EYE_POSITIONS = [(-9.0, -26.5), (9.0, -26.5)]
EYE_RX, EYE_RY = 2.8, 3.4
BLUSH_POSITIONS = [(-15.5, -19.8), (15.5, -19.8)]
MOUTH_Y = -20.6

HIGHLIGHT = (-10.5, -35.0, 7.5, 4.2, -28.0)  # cx, cy, rx, ry, rotation
SPECKLES = [(15.0, -11.0, 1.5), (20.0, -16.5, 1.1), (-18.0, -9.0, 1.3), (-13.5, -6.0, 0.9)]


# ---------------------------------------------------------------------------
# SVG helpers
# ---------------------------------------------------------------------------


def num(v: float) -> str:
    """Compact, deterministic number formatting (2 decimals max)."""
    s = f"{v:.2f}".rstrip("0").rstrip(".")
    return "0" if s in ("-0", "", "-") else s


def path_d(cmds) -> str:
    parts = []
    for c in cmds:
        op, args = c[0], c[1:]
        parts.append(op + " ".join(num(a) for a in args) if args else op)
    return " ".join(parts)


def attrs(**kw) -> str:
    out = []
    for k, v in kw.items():
        if v is None:
            continue
        name = k.rstrip("_").replace("_", "-")
        out.append(f'{name}="{num(v) if isinstance(v, (int, float)) else v}"')
    return " ".join(out)


def el(tag: str, **kw) -> str:
    return f"<{tag} {attrs(**kw)}/>"


def group(content: str, transform: str | None = None, **kw) -> str:
    if not content:
        return ""
    a = attrs(transform=transform, **kw)
    return f"<g {a}>{content}</g>" if a else f"<g>{content}</g>"


def tf(x: float = 0, y: float = 0, rot: float = 0, sx: float = 1, sy: float | None = None) -> str:
    sy = sx if sy is None else sy
    parts = []
    if x or y:
        parts.append(f"translate({num(x)} {num(y)})")
    if rot:
        parts.append(f"rotate({num(rot)})")
    if sx != 1 or sy != 1:
        parts.append(f"scale({num(sx)} {num(sy)})" if sx != sy else f"scale({num(sx)})")
    return " ".join(parts) or None


def stroke_kw(color: str = OUTLINE, width: float = OUTLINE_W) -> dict:
    return {"stroke": color, "stroke_width": width, "stroke_linejoin": "round", "stroke_linecap": "round"}


def lerp(a: float, b: float, t: float) -> float:
    return a + (b - a) * t


# ---------------------------------------------------------------------------
# Small props and effects (all original, simple geometric shapes)
# ---------------------------------------------------------------------------


def sparkle(
    x: float, y: float, size: float, fill: str = "#f6d25e", outline: str = "#c3922b", opacity: float = 1.0
) -> str:
    """A four-pointed twinkle."""
    if size <= 0.2 or opacity <= 0.01:
        return ""
    s, k = size, size * 0.22
    cmds = [("M", 0, -s), ("Q", k, -k, s, 0), ("Q", k, k, 0, s), ("Q", -k, k, -s, 0), ("Q", -k, -k, 0, -s), ("Z",)]
    return el(
        "path",
        d=path_d(cmds),
        transform=tf(x, y),
        fill=fill,
        stroke=outline,
        stroke_width=max(0.5, size * 0.12),
        stroke_linejoin="round",
        opacity=opacity if opacity < 1 else None,
    )


def zee(x: float, y: float, size: float, opacity: float) -> str:
    if opacity <= 0.01:
        return ""
    s = size
    cmds = [("M", 0, 0), ("L", s, 0), ("L", 0, s), ("L", s, s)]
    return el(
        "path",
        d=path_d(cmds),
        transform=tf(x - s / 2, y - s / 2),
        fill="none",
        opacity=opacity if opacity < 1 else None,
        **stroke_kw("#7b8bab", max(1.2, s * 0.24)),
    )


def note(x: float, y: float, scale: float, opacity: float, double: bool = False) -> str:
    """A small music note (eighth note, or two beamed notes)."""
    if opacity <= 0.01:
        return ""
    c = "#8a78c2"
    parts = [el("ellipse", cx=0, cy=0, rx=2.4, ry=1.8, transform="rotate(-22)", fill=c)]
    if double:
        parts.append(el("ellipse", cx=6, cy=-1.2, rx=2.4, ry=1.8, transform="rotate(-22 6 -1.2)", fill=c))
        parts.append(
            el(
                "path",
                d=path_d([("M", 2, -0.8), ("L", 2, -8.5), ("L", 8, -9.7), ("L", 8, -2)]),
                fill="none",
                **stroke_kw(c, 1.3),
            )
        )
        parts.append(el("path", d=path_d([("M", 2, -8.5), ("L", 8, -9.7)]), **stroke_kw(c, 2.2)))
    else:
        parts.append(el("path", d=path_d([("M", 2, -0.8), ("L", 2, -9)]), **stroke_kw(c, 1.3)))
        parts.append(el("path", d=path_d([("M", 2, -9), ("Q", 2.6, -6, 5.6, -5.2)]), fill="none", **stroke_kw(c, 1.3)))
    return group("".join(parts), tf(x, y, sx=scale), opacity=opacity if opacity < 1 else None)


def raindrop(x: float, y: float, opacity: float = 0.9) -> str:
    if opacity <= 0.01:
        return ""
    return el(
        "path",
        d=path_d([("M", 0, 0), ("L", -1.2, 4.6)]),
        transform=tf(x, y),
        opacity=opacity if opacity < 1 else None,
        **stroke_kw("#6fa3cf", 1.6),
    )


def splash(x: float, y: float, t: float) -> str:
    """A tiny ground splash; t in [0, 1)."""
    if t <= 0 or t >= 1:
        return ""
    w = 2 + 3 * t
    op = 0.85 * (1 - t)
    return el(
        "path",
        d=path_d(
            [
                ("M", -w, 0),
                ("Q", -w * 0.6, -2.2 * (1 - t) - 0.6, -w * 0.25, -0.4),
                ("M", w * 0.25, -0.4),
                ("Q", w * 0.6, -2.2 * (1 - t) - 0.6, w, 0),
            ]
        ),
        transform=tf(x, y),
        fill="none",
        opacity=op,
        **stroke_kw("#6fa3cf", 1.2),
    )


def firefly(x: float, y: float, glow: float) -> str:
    return (
        el("circle", cx=x, cy=y, r=8.5, fill="url(#glow-firefly)", opacity=glow)
        + el("circle", cx=x - 1.8, cy=y - 1.2, r=1.3, fill="#4d4a33")
        + el("circle", cx=x + 0.3, cy=y + 0.2, r=2.2, fill="#f4f27c", stroke="#8f8c2f", stroke_width=0.7)
    )


def acorn(scale: float = 1.0, glint: float = 0.0) -> str:
    """An acorn centred near (0, 0)."""
    brown, dark, cap = "#c98b4e", "#6b4426", "#8a5a33"
    parts = [
        el(
            "path",
            d=path_d([("M", -6, -1), ("C", -6, 5, -3, 9, 0, 10.5), ("C", 3, 9, 6, 5, 6, -1), ("Z",)]),
            fill=brown,
            **stroke_kw(dark, 1.4),
        ),
        el(
            "path",
            d=path_d([("M", -3.4, 1.2), ("C", -3.4, 4, -2.2, 6, -0.8, 7.2)]),
            fill="none",
            **stroke_kw("#e6b37c", 1.3),
        ),
        el(
            "path",
            d=path_d(
                [
                    ("M", -7.6, -0.4),
                    ("C", -7.6, -5.6, -4, -7.6, 0, -7.6),
                    ("C", 4, -7.6, 7.6, -5.6, 7.6, -0.4),
                    ("C", 5, 1, -5, 1, -7.6, -0.4),
                    ("Z",),
                ]
            ),
            fill=cap,
            **stroke_kw(dark, 1.4),
        ),
        el(
            "path",
            d=path_d(
                [
                    ("M", -4.5, -3.2),
                    ("L", -2.8, -4.6),
                    ("M", -0.9, -3.0),
                    ("L", 0.9, -4.6),
                    ("M", 2.8, -3.2),
                    ("L", 4.5, -4.6),
                ]
            ),
            fill="none",
            **stroke_kw("#5d3a20", 1.0),
        ),
        el(
            "path",
            d=path_d([("M", 0, -7.6), ("C", 0.3, -9.6, 1.5, -10.6, 2.6, -10.9)]),
            fill="none",
            **stroke_kw(dark, 1.6),
        ),
    ]
    if glint > 0:
        parts.append(sparkle(5.5, 3.5, 2.6 * glint, fill="#fff6d0", outline="#d0a64a"))
    return group("".join(parts), tf(sx=scale))


def cup(steam_phase: float | None) -> str:
    """A small tea cup centred at (0, 0); steam drifts with steam_phase."""
    dark = "#7a5a3c"
    parts = []
    if steam_phase is not None:
        for i, (dx, base_op) in enumerate(((-2.2, 0.75), (2.4, 0.6))):
            ph = steam_phase + i * 0.5
            sway = 1.6 * math.sin(2 * math.pi * ph)
            rise = 1.5 * ((ph * 2) % 1.0)
            parts.append(
                el(
                    "path",
                    d=path_d(
                        [
                            ("M", dx, -8.5 - rise),
                            (
                                "C",
                                dx + 2 + sway,
                                -11 - rise,
                                dx - 2 + sway,
                                -13.5 - rise,
                                dx + sway * 0.6,
                                -16.5 - rise,
                            ),
                        ]
                    ),
                    fill="none",
                    opacity=base_op,
                    **stroke_kw("#a9a291", 1.4),
                )
            )
    parts.append(
        el("path", d=path_d([("M", 6, -3.5), ("C", 10, -3.5, 10, 2.2, 6, 2.2)]), fill="none", **stroke_kw(dark, 1.6))
    )
    parts.append(
        el(
            "path",
            d=path_d(
                [
                    ("M", -6.5, -6),
                    ("L", 6.5, -6),
                    ("L", 6.5, 0.5),
                    ("C", 6.5, 4.4, 3.8, 6, 0, 6),
                    ("C", -3.8, 6, -6.5, 4.4, -6.5, 0.5),
                    ("Z",),
                ]
            ),
            fill="#f3e6cf",
            **stroke_kw(dark, 1.6),
        )
    )
    parts.append(el("ellipse", cx=0, cy=-6, rx=6.5, ry=1.6, fill="#b98756", **stroke_kw(dark, 1.2)))
    parts.append(group(el("path", d=path_d(LEAF_PATH), fill="#8dbb66"), tf(-2.6, 2.2, -30, 0.36)))
    return "".join(parts)


def leaf_umbrella() -> str:
    """A broad leaf held as an umbrella, body-local coordinates (canopy over the head)."""
    green, vein, dark = "#5f9f68", "#3f7a4b", "#2f5a39"
    rim = [("M", -33, -63), ("C", -29, -76, -14, -81, 2, -81), ("C", 18, -81, 32, -75, 36, -62)]
    under = [
        ("C", 30, -66.5, 25, -64.5, 19, -67.5),
        ("C", 13, -70.5, 8, -69, 2, -70.5),
        ("C", -5, -69, -10, -71, -17, -67.5),
        ("C", -23, -64.5, -28, -66.5, -33, -63),
        ("Z",),
    ]
    canopy = el("path", d=path_d(rim + under), fill=green, **stroke_kw(dark, 2.0))
    veins = el(
        "path",
        d=path_d(
            [
                ("M", 2, -70),
                ("Q", 1, -76, 2, -80),
                ("M", 2, -71),
                ("Q", -10, -73, -20, -71.5),
                ("M", 2, -71),
                ("Q", 14, -73.5, 24, -70.5),
                ("M", 2, -70.5),
                ("Q", -6, -77, -14, -78),
                ("M", 2, -70.5),
                ("Q", 10, -77.5, 19, -78),
            ]
        ),
        fill="none",
        **stroke_kw(vein, 1.2),
    )
    sheen = el(
        "path",
        d=path_d([("M", -24, -70), ("Q", -16, -78, -4, -79.4)]),
        fill="none",
        opacity=0.55,
        **stroke_kw("#a6d39e", 1.5),
    )
    return canopy + veins + sheen


def umbrella_stem() -> str:
    return el(
        "path",
        d=path_d([("M", 29.5, -27), ("C", 27, -42, 14, -55, 2, -66.5)]),
        fill="none",
        **stroke_kw("#4b6f37", 2.4),
    )


def lantern(swing: float, glow_r: float, glow_op: float) -> str:
    """A small paper lantern hanging from the right hand (body-local coords)."""
    hx, hy = 34.5, -25.0  # hand
    dark, paper, rib = "#6b4a2e", "#f4c869", "#d58f3e"
    body = [
        el("path", d=path_d([("M", 0, 0), ("L", 0, 4)]), **stroke_kw(dark, 1.4)),
        el("rect", x=-3.6, y=4, width=7.2, height=2.4, rx=0.8, fill=dark),
        el(
            "path",
            d=path_d(
                [
                    ("M", -3.6, 6.4),
                    ("C", -8.2, 7.5, -8.2, 18.5, -3.6, 19.6),
                    ("L", 3.6, 19.6),
                    ("C", 8.2, 18.5, 8.2, 7.5, 3.6, 6.4),
                    ("Z",),
                ]
            ),
            fill=paper,
            **stroke_kw(dark, 1.4),
        ),
        el(
            "path",
            d=path_d([("M", -1.8, 6.8), ("Q", -3.6, 13, -1.8, 19.2), ("M", 1.8, 6.8), ("Q", 3.6, 13, 1.8, 19.2)]),
            fill="none",
            **stroke_kw(rib, 1.0),
        ),
        el("ellipse", cx=0, cy=13, rx=2.2, ry=3.2, fill="#fff3c4", opacity=0.85),
        el("rect", x=-3.6, y=19.6, width=7.2, height=2.2, rx=0.8, fill=dark),
        el("path", d=path_d([("M", 0, 21.8), ("L", 0, 24.2)]), **stroke_kw(dark, 1.2)),
    ]
    glow = el("circle", cx=0, cy=13, r=glow_r, fill="url(#glow-lantern)", opacity=glow_op)
    return group(glow + "".join(body), f"translate({num(hx)} {num(hy)}) rotate({num(swing)})")


def pause_bubble(dy: float, opacity: float) -> str:
    """A small thought bubble with a pause sign: a calm 'waiting' cue."""
    bx, by = 79.5, 19.0 + dy
    parts = [
        el("circle", cx=bx - 12.5, cy=by + 12.5, r=1.6, fill="#fffdf4", **stroke_kw(OUTLINE, 1.1)),
        el("circle", cx=bx - 9, cy=by + 8.8, r=2.4, fill="#fffdf4", **stroke_kw(OUTLINE, 1.2)),
        el("rect", x=bx - 9, y=by - 7.5, width=18, height=15, rx=7.5, fill="#fffdf4", **stroke_kw(OUTLINE, 1.4)),
        el("rect", x=bx - 3.9, y=by - 3.8, width=2.6, height=7.6, rx=1.2, fill="#6d8a5a"),
        el("rect", x=bx + 1.3, y=by - 3.8, width=2.6, height=7.6, rx=1.2, fill="#6d8a5a"),
    ]
    return group("".join(parts), opacity=opacity if opacity < 1 else None)


def flower(stage: int) -> str:
    """long_session: a bud that opens at the stem tip (stage 0..3)."""
    if stage <= 0:
        return ""
    if stage == 1:
        return el("ellipse", cx=0, cy=-2.4, rx=1.8, ry=2.6, fill="#f2b6c4", **stroke_kw("#b86f81", 1.0))
    if stage == 2:
        return el("ellipse", cx=0, cy=-3.2, rx=2.6, ry=3.6, fill="#f4bfcb", **stroke_kw("#b86f81", 1.0)) + el(
            "path", d=path_d([("M", -2.4, -1.2), ("Q", 0, 0.6, 2.4, -1.2)]), fill="none", **stroke_kw("#5f8d3f", 1.2)
        )
    petals = "".join(
        el(
            "ellipse",
            cx=0,
            cy=-3.3,
            rx=2.3,
            ry=3.3,
            transform=f"rotate({num(a)})",
            fill="#f7c8d2",
            **stroke_kw("#b86f81", 0.9),
        )
        for a in (0, 72, 144, 216, 288)
    )
    return group(
        petals + el("circle", cx=0, cy=0, r=2.1, fill="#f3c24f", **stroke_kw("#b98a2a", 0.8)), "translate(0 -4.5)"
    )


# ---------------------------------------------------------------------------
# The character
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class Pose:
    x: float = 0.0  # horizontal shift
    lift: float = 0.0  # height above the ground (hops)
    sx: float = 1.0  # squash and stretch around the ground point
    sy: float = 1.0
    rot: float = 0.0  # tilt in degrees around the ground point (clockwise)
    eyes: str = "open"  # open | soft | half | shut | closed | happy
    look: tuple[float, float] = (0.0, 0.0)  # pupil offset for open and soft eyes
    mouth: str = "smile"  # smile | small | grin | o | hum | none
    blush: float = 0.55
    sprout: float = 0.0  # sprout sway in degrees (clockwise)
    arm_l: float | None = 40.0  # degrees from straight down, positive = outward; None hides it
    arm_r: float | None = 40.0
    arms_over: str = ""  # "l", "r" or "lr": draw those side arms over the body
    mitts: tuple[tuple[float, float], ...] = ()  # little hands in front (body-local)
    bloom: int = 0
    held_back: str = ""  # body-local SVG behind the body
    held: str = ""  # body-local SVG in front of the body (props in hands)
    back: str = ""  # frame SVG behind the character
    front: str = ""  # frame SVG in front of the character


def draw_sprout(p: Pose) -> str:
    stem = el("path", d=path_d(STEM_PATH), fill="none", **stroke_kw(STEM, 2.6))
    leaves = []
    for lx, ly, rot, scale, mirrored in LEAVES:
        sx = -scale if mirrored else scale
        r = -rot if mirrored else rot
        leaf = el("path", d=path_d(LEAF_PATH), fill=LEAF, **stroke_kw(OUTLINE, OUTLINE_W / scale * 0.85))
        vein = el("path", d=path_d(LEAF_VEIN_PATH), fill="none", **stroke_kw(LEAF_VEIN, 1.1 / scale))
        leaves.append(
            group(
                leaf + vein,
                f"translate({num(STEM_TIP[0] + lx)} {num(STEM_TIP[1] + ly)}) rotate({num(r)}) scale({num(sx)} {num(scale)})",
            )
        )
    bloom = group(flower(p.bloom), tf(*STEM_TIP)) if p.bloom else ""
    return group(stem + "".join(leaves) + bloom, f"translate(0 {num(BODY_TOP + 1)}) rotate({num(p.sprout)})")


def draw_arm(side: int, angle: float) -> str:
    sx, sy = SHOULDERS[side]
    rot = angle if side == 0 else -angle
    return el(
        "rect",
        x=-ARM_W / 2,
        y=-ARM_W / 2,
        width=ARM_W,
        height=ARM_LEN + ARM_W / 2,
        rx=ARM_W / 2,
        transform=f"translate({num(sx)} {num(sy)}) rotate({num(rot)})",
        fill=BODY,
        **stroke_kw(),
    )


def draw_eyes(p: Pose) -> str:
    out = []
    lx, ly = p.look
    for ex, ey in EYE_POSITIONS:
        if p.eyes == "open":
            out.append(el("ellipse", cx=ex + lx, cy=ey + ly, rx=EYE_RX, ry=EYE_RY, fill=EYE))
            out.append(el("circle", cx=ex + lx - 0.9, cy=ey + ly - 1.25, r=1.0, fill="#ffffff"))
        elif p.eyes == "soft":  # focus: open, relaxed rounded lids, a calm, slightly lowered gaze
            cx, cy = ex + lx, ey + ly
            out.append(
                el(
                    "path",
                    d=path_d(
                        [
                            ("M", cx - 2.95, cy - 0.2),
                            ("C", cx - 2.6, cy - 3.0, cx + 2.6, cy - 3.0, cx + 2.95, cy - 0.2),
                            ("C", cx + 3.0, cy + 4.1, cx - 3.0, cy + 4.1, cx - 2.95, cy - 0.2),
                            ("Z",),
                        ]
                    ),
                    fill=EYE,
                    **stroke_kw(EYE, 0.9),
                )
            )
            out.append(el("circle", cx=cx - 0.9, cy=cy + 0.3, r=0.85, fill="#ffffff"))
        elif p.eyes == "half":
            out.append(
                el(
                    "path",
                    d=path_d(
                        [
                            ("M", ex - 3, ey + 0.2),
                            ("C", ex - 2.4, ey + 3.2, ex + 2.4, ey + 3.2, ex + 3, ey + 0.2),
                            ("Z",),
                        ]
                    ),
                    fill=EYE,
                    **stroke_kw(EYE, 1.0),
                )
            )
        elif p.eyes == "shut":
            out.append(
                el(
                    "path",
                    d=path_d([("M", ex - 3.1, ey + 0.8), ("Q", ex, ey + 1.8, ex + 3.1, ey + 0.8)]),
                    fill="none",
                    **stroke_kw(EYE, 1.7),
                )
            )
        elif p.eyes == "closed":  # calm, content: a soft downward curve
            out.append(
                el(
                    "path",
                    d=path_d([("M", ex - 3.2, ey - 0.2), ("Q", ex, ey + 2.9, ex + 3.2, ey - 0.2)]),
                    fill="none",
                    **stroke_kw(EYE, 1.7),
                )
            )
        elif p.eyes == "happy":  # an upward curve
            out.append(
                el(
                    "path",
                    d=path_d([("M", ex - 3.2, ey + 1.2), ("Q", ex, ey - 2.8, ex + 3.2, ey + 1.2)]),
                    fill="none",
                    **stroke_kw(EYE, 1.8),
                )
            )
        else:
            raise ValueError(f"unknown eyes {p.eyes!r}")
    return "".join(out)


def draw_mouth(p: Pose) -> str:
    y = MOUTH_Y
    if p.mouth == "smile":
        return el(
            "path",
            d=path_d([("M", -2.9, y - 0.4), ("Q", 0, y + 2.3, 2.9, y - 0.4)]),
            fill="none",
            **stroke_kw(EYE, 1.5),
        )
    if p.mouth == "small":
        return el("path", d=path_d([("M", -1.9, y), ("Q", 0, y + 1.4, 1.9, y)]), fill="none", **stroke_kw(EYE, 1.4))
    if p.mouth == "grin":
        return el(
            "path",
            d=path_d(
                [
                    ("M", -3.6, y - 0.8),
                    ("Q", 0, y - 0.1, 3.6, y - 0.8),
                    ("Q", 3.2, y + 4.2, 0, y + 4.2),
                    ("Q", -3.2, y + 4.2, -3.6, y - 0.8),
                    ("Z",),
                ]
            ),
            fill=MOUTH_FILL,
            **stroke_kw(EYE, 1.2),
        )
    if p.mouth == "o":
        return el("ellipse", cx=0, cy=y + 1, rx=1.9, ry=2.3, fill=MOUTH_FILL, **stroke_kw(EYE, 1.1))
    if p.mouth == "hum":
        return el("ellipse", cx=0, cy=y + 0.8, rx=1.3, ry=1.5, fill=MOUTH_FILL, **stroke_kw(EYE, 1.0))
    if p.mouth == "none":
        return ""
    raise ValueError(f"unknown mouth {p.mouth!r}")


def draw_frame(p: Pose) -> str:
    out = [p.back]
    k = max(0.45, 1.0 - p.lift / 28.0)
    out.append(
        el(
            "ellipse",
            cx=CENTER_X + p.x,
            cy=GROUND_Y + 2.2,
            rx=21 * k * p.sx,
            ry=3.3 * k,
            fill=SHADOW,
            fill_opacity=0.16 * k,
        )
    )

    body = [p.held_back, draw_sprout(p)]
    arms = [(i, a) for i, a in enumerate((p.arm_l, p.arm_r)) if a is not None]
    body += [draw_arm(i, a) for i, a in arms if "lr"[i] not in p.arms_over]
    body += [
        el("ellipse", cx=fx, cy=fy, rx=FOOT_RX, ry=FOOT_RY, fill=BODY_DARK, **stroke_kw())
        for (fx, fy) in FOOT_POSITIONS
    ]
    body.append(el("path", d=path_d(BODY_PATH), fill=BODY, **stroke_kw()))
    hx, hy, hrx, hry, hrot = HIGHLIGHT
    body.append(
        el(
            "ellipse",
            cx=hx,
            cy=hy,
            rx=hrx,
            ry=hry,
            transform=f"rotate({num(hrot)} {num(hx)} {num(hy)})",
            fill=BODY_HIGHLIGHT,
            opacity=0.8,
        )
    )
    body += [el("circle", cx=x, cy=y, r=r, fill=BODY_DARK, opacity=0.75) for (x, y, r) in SPECKLES]
    if p.blush > 0:
        op = round(min(1.0, 0.4 + 0.55 * p.blush), 2)
        body += [el("ellipse", cx=x, cy=y, rx=4.1, ry=2.3, fill=BLUSH, opacity=op) for (x, y) in BLUSH_POSITIONS]
    body.append(draw_eyes(p))
    body.append(draw_mouth(p))
    body += [draw_arm(i, a) for i, a in arms if "lr"[i] in p.arms_over]
    body.append(p.held)
    body += [el("ellipse", cx=x, cy=y, rx=4.5, ry=4.0, fill=MITT, **stroke_kw(OUTLINE, 1.7)) for (x, y) in p.mitts]

    out.append(group("".join(body), tf(CENTER_X + p.x, GROUND_Y - p.lift, p.rot, p.sx, p.sy)))
    out.append(p.front)
    return "".join(out)


# ---------------------------------------------------------------------------
# Animations
# ---------------------------------------------------------------------------


def breath(k: int, frames: int, amp: float, base_sy: float = 1.0) -> dict:
    s = math.sin(2 * math.pi * k / frames)
    return {"sx": 1 - amp * 0.45 * s, "sy": base_sy + amp * s}


def anim_idle() -> list[Pose]:
    out = []
    for k in range(6):
        s = math.sin(2 * math.pi * k / 6)
        out.append(
            Pose(
                **breath(k, 6, 0.026),
                sprout=2.6 * math.sin(2 * math.pi * k / 6 - 0.9) + 1.5,
                arm_l=40 + 3 * s,
                arm_r=40 + 3 * s,
            )
        )
    return out


def rest_pose() -> Pose:
    """Pip's rest pose: frame 0 of `idle`. `focus` starts from it and `blink` is drawn on it."""
    return anim_idle()[0]


def anim_blink() -> list[Pose]:
    # Only the eyes change, so the blink fits every state in BLINK_REPLACES (validated
    # by check_blink_fit): those states start from this same body pose.
    return [replace(rest_pose(), eyes=e) for e in ("half", "shut", "half")]


def anim_focus() -> list[Pose]:
    # Settled and still: the rest pose with relaxed, slightly lowered eyes (open, so the
    # occasional blink over focus reads as a blink), very slow shallow breathing and an
    # almost still sprout. Frame 0 is the rest pose itself apart from the eyes.
    rest = rest_pose()
    return [
        replace(
            rest,
            **breath(k, 4, 0.014),
            eyes="soft",
            look=(0.0, 0.5),
            sprout=rest.sprout + 1.0 * math.sin(2 * math.pi * k / 4),
        )
        for k in range(4)
    ]


def anim_paused() -> list[Pose]:
    out = []
    for k in range(4):
        dy = -1.0 * math.sin(math.pi * k / 4) ** 2
        out.append(
            Pose(
                **breath(k, 4, 0.014, 0.975),
                eyes="open",
                look=(0.7, -0.5),
                mouth="small",
                blush=0.5,
                sprout=4,
                arm_l=28,
                arm_r=28,
                front=pause_bubble(dy, 1.0 if k % 2 == 0 else 0.92),
            )
        )
    return out


def anim_completed() -> list[Pose]:
    return [
        Pose(sx=1.07, sy=0.91, eyes="happy", mouth="smile", blush=0.7, arm_l=30, arm_r=30, sprout=-4),
        Pose(
            lift=5,
            sx=0.96,
            sy=1.06,
            eyes="happy",
            mouth="grin",
            blush=0.75,
            arm_l=125,
            arm_r=125,
            arms_over="lr",
            sprout=6,
        ),
        Pose(
            lift=8,
            sx=1.0,
            sy=1.02,
            eyes="happy",
            mouth="grin",
            blush=0.8,
            arm_l=155,
            arm_r=155,
            arms_over="lr",
            sprout=-3,
        ),
        Pose(
            lift=4,
            sx=0.98,
            sy=1.03,
            eyes="happy",
            mouth="grin",
            blush=0.8,
            arm_l=130,
            arm_r=130,
            arms_over="lr",
            sprout=-7,
        ),
        Pose(sx=1.06, sy=0.93, eyes="happy", mouth="grin", blush=0.8, arm_l=70, arm_r=70, sprout=6),
        Pose(eyes="happy", mouth="grin", blush=0.75, arm_l=62, arm_r=62, sprout=1.5),
    ]


def anim_break() -> list[Pose]:
    def beside(ph: float) -> tuple[str, tuple[tuple[float, float], ...]]:
        return (group(cup(ph), "translate(31 -12)"), ((26.5, -10.5),))

    at_mouth = (group(cup(None), "translate(9 -16.5) rotate(-20)"), ((14.5, -11),))
    frames = []
    spec = [("hold", 0.0), ("hold", 0.25), ("lift", 0.5), ("sip", None), ("lift", 0.75), ("hold", 0.5)]
    for k, (kind, ph) in enumerate(spec):
        b = breath(k, 6, 0.012, 0.97)
        if kind == "sip":
            held, mitts = at_mouth
            frames.append(
                Pose(
                    **b, eyes="closed", mouth="small", blush=0.7, sprout=2, arm_l=30, arm_r=None, held=held, mitts=mitts
                )
            )
        elif kind == "lift":
            held = group(cup(ph), "translate(22 -16) rotate(-6)")
            frames.append(
                Pose(
                    **b,
                    eyes="open",
                    look=(0.8, 0.2),
                    mouth="small",
                    blush=0.6,
                    sprout=1,
                    arm_l=30,
                    arm_r=None,
                    held=held,
                    mitts=((17.5, -14),),
                )
            )
        else:
            held, mitts = beside(ph)
            frames.append(
                Pose(
                    **b,
                    eyes="open",
                    look=(0.9, 0.5),
                    mouth="smile",
                    blush=0.6,
                    sprout=0,
                    arm_l=30,
                    arm_r=None,
                    held=held,
                    mitts=mitts,
                )
            )
    return frames


def anim_react() -> list[Pose]:
    rots = [-7, 7, -4.5, 2.5, 0]
    lifts = [1.5, 2.5, 1, 0, 0]
    return [
        Pose(
            rot=r,
            lift=lf,
            eyes="happy",
            mouth="grin" if i < 4 else "smile",
            blush=0.85,
            sprout=-r * 1.6,
            arm_l=52,
            arm_r=52,
        )
        for i, (r, lf) in enumerate(zip(rots, lifts, strict=True))
    ]


def anim_wave() -> list[Pose]:
    arms = [75, 145, 118, 148, 118, 145, 75]
    out = []
    for i, a in enumerate(arms):
        happy = 0 < i < 6
        out.append(
            Pose(
                rot=1.5 if happy else 0,
                eyes="happy" if happy else "open",
                mouth="grin" if happy else "smile",
                blush=0.7,
                sprout=-2 if i % 2 else 2,
                arm_l=38,
                arm_r=a,
                arms_over="r" if a > 90 else "",
            )
        )
    return out


def anim_stretch() -> list[Pose]:
    spec = [
        {"sx": 1.03, "sy": 0.96, "arm_l": 40, "arm_r": 40, "eyes": "open", "mouth": "smile"},
        {"sx": 0.98, "sy": 1.05, "arm_l": 125, "arm_r": 125, "eyes": "closed", "mouth": "o", "arms_over": "lr"},
        {
            "sx": 0.93,
            "sy": 1.12,
            "arm_l": 170,
            "arm_r": 170,
            "eyes": "closed",
            "mouth": "o",
            "arms_over": "lr",
            "sprout": 0,
        },
        {
            "sx": 0.93,
            "sy": 1.12,
            "arm_l": 172,
            "arm_r": 165,
            "eyes": "closed",
            "mouth": "o",
            "arms_over": "lr",
            "rot": -3,
            "sprout": 4,
        },
        {
            "sx": 0.93,
            "sy": 1.12,
            "arm_l": 165,
            "arm_r": 172,
            "eyes": "closed",
            "mouth": "o",
            "arms_over": "lr",
            "rot": 3,
            "sprout": -4,
        },
        {"sx": 1.05, "sy": 0.94, "arm_l": 70, "arm_r": 70, "eyes": "happy", "mouth": "smile"},
        {"arm_l": 42, "arm_r": 42, "eyes": "open", "mouth": "smile"},
    ]
    return [Pose(blush=0.6, **s) for s in spec]


RAIN_COLUMNS = [(8.0, 0.0), (14.0, 0.55), (84.0, 0.3), (90.0, 0.8), (19.0, 0.15), (78.0, 0.65)]


def anim_umbrella() -> list[Pose]:
    out = []
    for k in range(6):
        t = k / 6
        back = []
        for i, (x, ph) in enumerate(RAIN_COLUMNS):
            y0, y1 = (3.0, 78.0) if i < 4 else (54.0, 80.0)
            y = y0 + ((t + ph) % 1.0) * (y1 - y0)
            back.append(raindrop(x + (y - y0) * -0.04, y, 0.9))
        front = "".join(splash(x, 85.5, ((t + ph) * 2) % 1.0) for x, ph in ((10.0, 0.1), (86.0, 0.6)))
        out.append(
            Pose(
                **breath(k, 6, 0.02),
                eyes="open",
                look=(-0.3, -0.7),
                mouth="smile",
                blush=0.55,
                sprout=-8 + 1.5 * math.sin(2 * math.pi * t),
                arm_l=36,
                arm_r=152,
                arms_over="r",
                held_back=umbrella_stem(),
                held=group(leaf_umbrella(), "translate(0 4)"),
                back="".join(back),
                front=front,
            )
        )
    return out


def anim_sleepy() -> list[Pose]:
    out = []
    for k in range(4):
        zs = []
        for i in range(3):
            t = (i + k / 4) / 3
            x, y = lerp(60, 82, t), lerp(40, 9, t)
            op = min(1.0, t * 4, (1 - t) * 3.2)
            zs.append(zee(x, y, lerp(3.6, 7.6, t), round(max(0.0, op), 2)))
        out.append(
            Pose(
                **breath(k, 4, 0.02, 0.955),
                rot=3,
                eyes="closed",
                mouth="hum",
                blush=0.45,
                sprout=16 + 2 * math.sin(2 * math.pi * k / 4),
                arm_l=24,
                arm_r=24,
                front="".join(zs),
            )
        )
    return out


def anim_celebrate() -> list[Pose]:
    jumps = [
        {"sx": 1.08, "sy": 0.9, "arm_l": 35, "arm_r": 35, "sprout": -4},
        {"lift": 5, "sx": 0.95, "sy": 1.06, "arm_l": 140, "arm_r": 140, "arms_over": "lr", "sprout": 6},
        {"lift": 7, "sy": 1.03, "arm_l": 165, "arm_r": 165, "arms_over": "lr", "sprout": -2},
        {"lift": 8, "sy": 1.01, "arm_l": 168, "arm_r": 168, "arms_over": "lr", "sprout": -6},
        {"lift": 5, "sy": 1.03, "arm_l": 140, "arm_r": 140, "arms_over": "lr", "sprout": -8},
        {"sx": 1.07, "sy": 0.92, "arm_l": 80, "arm_r": 80, "sprout": 5},
        {"lift": 3, "sy": 1.02, "arm_l": 120, "arm_r": 120, "arms_over": "lr", "sprout": -2},
        {"arm_l": 65, "arm_r": 65, "sprout": 1},
    ]
    spots = [
        (12.0, 22.0, 0.0),
        (85.0, 28.0, 0.15),
        (16.0, 58.0, 0.3),
        (82.0, 62.0, 0.1),
        (28.0, 11.0, 0.25),
        (70.0, 12.0, 0.05),
    ]
    out = []
    for k, j in enumerate(jumps):
        sparks = []
        if k >= 1:
            for x, y, delay in spots:
                t = (k - 1) / 6 - delay
                if t <= 0:
                    continue
                size = 4.8 * math.sin(math.pi * min(1.0, t * 1.4)) + 1.0
                sparks.append(sparkle(x, y, size, opacity=1.0 if t < 0.75 else 0.7))
        out.append(Pose(eyes="happy", mouth="grin", blush=0.85, front="".join(sparks), **j))
    return out


def anim_acorn_gift() -> list[Pose]:
    return [
        Pose(eyes="open", look=(0, 0.6), mouth="smile", blush=0.6, arm_l=20, arm_r=20),
        Pose(
            sx=1.02,
            sy=0.98,
            eyes="open",
            look=(0, 1.0),
            mouth="smile",
            blush=0.7,
            arm_l=None,
            arm_r=None,
            held=group(acorn(0.95), "translate(0 -1.5)"),
            mitts=((-7, -0.8), (7, -0.8)),
        ),
        Pose(
            eyes="happy",
            mouth="grin",
            blush=0.8,
            arm_l=None,
            arm_r=None,
            held=group(acorn(1.14), "translate(0 -3.5)"),
            mitts=((-8.4, -2.6), (8.4, -2.6)),
        ),
        Pose(
            lift=2.5,
            sy=1.02,
            eyes="happy",
            mouth="grin",
            blush=0.85,
            arm_l=None,
            arm_r=None,
            held=group(acorn(1.14), "translate(0 -3.5)"),
            mitts=((-8.4, -2.6), (8.4, -2.6)),
        ),
        Pose(
            eyes="open",
            look=(0, 0.2),
            mouth="smile",
            blush=0.8,
            arm_l=None,
            arm_r=None,
            held=group(acorn(1.14, glint=1.0), "translate(0 -3.5)"),
            mitts=((-8.4, -2.6), (8.4, -2.6)),
        ),
        Pose(
            eyes="happy",
            mouth="smile",
            blush=0.8,
            arm_l=None,
            arm_r=None,
            held=group(acorn(1.14, glint=0.5), "translate(0 -3.5)"),
            mitts=((-8.4, -2.6), (8.4, -2.6)),
        ),
    ]


def anim_long_session() -> list[Pose]:
    return [
        Pose(eyes="open", mouth="smile", blush=0.6, arm_l=40, arm_r=40),
        Pose(sx=0.99, sy=1.03, eyes="closed", mouth="small", blush=0.6, arm_l=55, arm_r=55, sprout=-2),
        Pose(sx=1.04, sy=1.06, eyes="happy", mouth="smile", blush=0.75, arm_l=68, arm_r=68, bloom=1),
        Pose(sx=1.04, sy=1.06, eyes="happy", mouth="smile", blush=0.8, arm_l=68, arm_r=68, bloom=2, sprout=2),
        Pose(
            sx=1.04,
            sy=1.07,
            eyes="happy",
            mouth="grin",
            blush=0.85,
            arm_l=70,
            arm_r=70,
            bloom=3,
            front=sparkle(66, 16, 3.4) + sparkle(30, 14, 2.2),
        ),
        Pose(
            sx=1.03,
            sy=1.05,
            eyes="happy",
            mouth="grin",
            blush=0.85,
            arm_l=66,
            arm_r=66,
            bloom=3,
            front=sparkle(66, 16, 2.0),
        ),
        Pose(sx=1.02, sy=1.03, eyes="open", mouth="smile", blush=0.8, arm_l=55, arm_r=55, bloom=3),
    ]


def anim_hum() -> list[Pose]:
    out = []
    for k in range(8):
        s = math.sin(2 * math.pi * k / 8)
        notes = []
        for i, (x0, x1, double) in enumerate(((64.0, 78.0, False), (30.0, 14.0, True), (68.0, 82.0, False))):
            t = (k / 8 + i / 3) % 1.0
            op = min(1.0, t * 5, (1 - t) * 3)
            wob = 2.0 * math.sin(2 * math.pi * t * 1.5)
            notes.append(
                note(lerp(x0, x1, t) + wob, lerp(48, 15, t), lerp(0.75, 1.05, t), round(max(0.0, op), 2), double)
            )
        out.append(
            Pose(
                rot=3.5 * s,
                eyes="closed",
                mouth="hum",
                blush=0.65,
                sprout=-4.5 * s,
                arm_l=40 + 7 * s,
                arm_r=40 - 7 * s,
                front="".join(notes),
            )
        )
    return out


def anim_firefly() -> list[Pose]:
    flies = [((22.0, 30.0), (7.0, 6.0), 0.0), ((76.0, 44.0), (6.0, 8.0), 0.35), ((64.0, 16.0), (8.0, 4.0), 0.7)]
    out = []
    for k in range(6):
        t = k / 6
        parts = []
        for (cx, cy), (rx, ry), ph in flies:
            a = 2 * math.pi * (t + ph)
            glow = 0.55 + 0.4 * math.sin(2 * math.pi * (t * 2 + ph))
            parts.append(firefly(cx + rx * math.cos(a), cy + ry * math.sin(a), round(glow, 2)))
        look = (-1.2, -0.6) if k < 3 else (1.2, -0.3)
        out.append(
            Pose(
                **breath(k, 6, 0.022),
                eyes="open",
                look=look,
                mouth="smile",
                blush=0.6,
                sprout=2 * math.sin(2 * math.pi * t),
                front="".join(parts),
            )
        )
    return out


def anim_lantern() -> list[Pose]:
    spec = [(-2.0, 14.5, 0.6), (1.5, 16.0, 0.72), (2.5, 15.0, 0.62), (-1.0, 16.5, 0.75)]
    return [
        Pose(
            **breath(k, 4, 0.015),
            x=-7,
            eyes="open",
            look=(1.2, 0.4),
            mouth="smile",
            blush=0.65,
            sprout=1.5 * math.sin(2 * math.pi * k / 4),
            arm_l=36,
            arm_r=108,
            arms_over="r",
            held=lantern(sw, r, op),
        )
        for k, (sw, r, op) in enumerate(spec)
    ]


STARS = [
    (11.0, 15.0, 4.6, 0.0),
    (27.0, 7.0, 3.2, 0.4),
    (70.0, 8.0, 4.0, 0.2),
    (86.0, 22.0, 3.0, 0.65),
    (88.0, 46.0, 2.6, 0.85),
    (8.0, 42.0, 2.8, 0.5),
]


def anim_stargaze() -> list[Pose]:
    out = []
    for k in range(6):
        t = k / 6
        stars = []
        for x, y, s, ph in STARS:
            tw = 0.7 + 0.3 * math.sin(2 * math.pi * (t + ph))
            stars.append(sparkle(x, y, s * tw, fill="#f6e08a", outline="#b48f33"))
        if k in (3, 4):
            a = 0.85 if k == 3 else 0.4
            hx, hy = (36.0, 9.0) if k == 3 else (29.0, 12.5)
            stars.append(
                el(
                    "path",
                    d=path_d([("M", hx, hy), ("L", hx + 9, hy - 4.5)]),
                    opacity=a * 0.7,
                    **stroke_kw("#e9c96a", 1.2),
                )
            )
            stars.append(el("circle", cx=hx, cy=hy, r=1.4, fill="#f6e08a", opacity=a))
        out.append(
            Pose(
                **breath(k, 6, 0.016),
                eyes="open",
                look=(0.2, -1.6),
                mouth="o" if k in (3, 4) else "small",
                blush=0.55,
                sprout=-7 + math.sin(2 * math.pi * t),
                arm_l=26,
                arm_r=26,
                back="".join(stars),
            )
        )
    return out


@dataclass(frozen=True)
class Anim:
    key: str
    build: object
    fps: float = 4.0
    loop: bool = True
    still: int = 0
    durations: tuple[int, ...] | None = None
    note: str = ""
    defs: tuple[str, ...] = field(default_factory=tuple)


GLOW_FIREFLY = (
    '<radialGradient id="glow-firefly"><stop offset="0" stop-color="#f1f27a" stop-opacity="1"/>'
    '<stop offset="0.4" stop-color="#d9e45a" stop-opacity="0.6"/><stop offset="1" stop-color="#d9e45a" stop-opacity="0"/></radialGradient>'
)
GLOW_LANTERN = (
    '<radialGradient id="glow-lantern"><stop offset="0" stop-color="#ffe7a3" stop-opacity="0.9"/>'
    '<stop offset="0.5" stop-color="#ffd27a" stop-opacity="0.35"/><stop offset="1" stop-color="#ffd27a" stop-opacity="0"/></radialGradient>'
)

# Listing order = pack.json order: base states, extras, then unlock ids.
ANIMATIONS = [
    Anim("idle", anim_idle, fps=2.5, note="gentle breathing"),
    Anim("focus", anim_focus, durations=(900, 700, 900, 700), note="settled, soft lowered gaze, slow breathing"),
    Anim("paused", anim_paused, fps=2, note="sitting, eyes open, a small pause bubble"),
    Anim(
        "completed",
        anim_completed,
        loop=False,
        still=5,
        durations=(110, 90, 150, 90, 110, 240),
        note="happy hop, holds the last frame",
    ),
    Anim("break", anim_break, durations=(800, 800, 450, 1000, 450, 800), note="resting with a cup of tea, sipping"),
    Anim("react", anim_react, loop=False, still=4, durations=(70, 80, 80, 80, 120), note="short friendly wiggle"),
    Anim("blink", anim_blink, loop=False, durations=(60, 100, 60), note="one-shot blink"),
    Anim("wave", anim_wave, fps=8, loop=False, still=3, note="a small wave"),
    Anim(
        "stretch", anim_stretch, loop=False, still=2, durations=(120, 160, 420, 260, 260, 180, 160), note="big stretch"
    ),
    Anim("umbrella", anim_umbrella, fps=4, note="idle under a leaf umbrella in the rain"),
    Anim("sleepy", anim_sleepy, durations=(1000, 1000, 1000, 1000), note="dozing, tiny z's"),
    Anim("celebrate", anim_celebrate, fps=10, loop=False, still=3, note="jump with sparkles"),
    Anim(
        "acorn_gift",
        anim_acorn_gift,
        loop=False,
        still=4,
        durations=(160, 160, 220, 160, 420, 600),
        note="holds out an acorn",
    ),
    Anim(
        "long_session",
        anim_long_session,
        loop=False,
        still=5,
        durations=(150, 160, 220, 220, 280, 420, 600),
        note="proud; the sprout blooms",
    ),
    Anim("hum", anim_hum, fps=5, loop=False, note="hums a little tune"),
    Anim("firefly", anim_firefly, fps=3, defs=(GLOW_FIREFLY,), note="idle with fireflies"),
    Anim("lantern", anim_lantern, fps=3, defs=(GLOW_LANTERN,), note="holding a small lantern"),
    Anim("stargaze", anim_stargaze, fps=2, note="looking up at the stars"),
]


# ---------------------------------------------------------------------------
# Output
# ---------------------------------------------------------------------------


def render_sheet(a: Anim, frames: list[Pose]) -> str:
    w, h = FRAME * len(frames), FRAME
    out = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">',
        (
            f"<!-- Pip (placeholder pack): {a.key}, {len(frames)} frames of {FRAME}x{FRAME}, left to right. "
            "Original art generated by scripts/gen_placeholder_pack.py. CC0-1.0. -->"
        ),
        f'<defs><clipPath id="cell"><rect width="{FRAME}" height="{FRAME}"/></clipPath>{"".join(a.defs)}</defs>',
    ]
    for i, p in enumerate(frames):
        out.append(
            f'<g id="frame-{i}" transform="translate({i * FRAME} 0)"><g clip-path="url(#cell)">{draw_frame(p)}</g></g>'
        )
    out.append("</svg>")
    return "\n".join(out) + "\n"


def fps_for(a: Anim, frames: int) -> float:
    if a.durations:
        return round(frames * 1000 / sum(a.durations), 2)
    return a.fps


def manifest(built: dict[str, list[Pose]]) -> dict:
    animations = {}
    for a in ANIMATIONS:
        n = len(built[a.key])
        spec = {
            "sheet": f"{a.key}.svg",
            "frameWidth": FRAME,
            "frameHeight": FRAME,
            "frames": n,
            "fps": fps_for(a, n),
            "loop": a.loop,
            "row": 0,
            "column": 0,
            "sheetColumns": n,
            "sheetRows": 1,
            "stillFrame": a.still,
        }
        if a.durations:
            spec["frameDurationsMs"] = list(a.durations)
        animations[a.key] = spec
    return {
        "format": 1,
        "id": "placeholder",
        "name": "Pip (placeholder)",
        "author": "Project contributors (generated by scripts/gen_placeholder_pack.py)",
        "license": "CC0-1.0",
        "description": "Pip, an original moss-green seedling companion. Programmatically generated vector art, dedicated to the public domain.",
        "rendering": "smooth",
        "animations": animations,
        "fallbacks": {},
        "palettes": {},
    }


def build_all() -> dict[str, list[Pose]]:
    return {a.key: a.build() for a in ANIMATIONS}


def expected_files(built: dict[str, list[Pose]]) -> dict[str, str]:
    """File name -> exact content the generator writes for the pack."""
    out = {f"{a.key}.svg": render_sheet(a, built[a.key]) for a in ANIMATIONS}
    out["pack.json"] = json.dumps(manifest(built), indent=2) + "\n"
    return out


def generate() -> list[Path]:
    PACK_DIR.mkdir(parents=True, exist_ok=True)
    written = []
    for name, text in expected_files(build_all()).items():
        path = PACK_DIR / name
        path.write_text(text, encoding="utf-8", newline="\n")
        written.append(path)
    # Remove stale sheets from earlier versions of this script.
    keep = {p.name for p in written}
    for p in PACK_DIR.glob("*.svg"):
        if p.name not in keep:
            p.unlink()
    return written


# ---------------------------------------------------------------------------
# Validation
# ---------------------------------------------------------------------------

PACK_ID_RE = re.compile(r"^[a-z0-9][a-z0-9_-]{0,63}$")
HEX_RE = re.compile(r"^#[0-9a-f]{6}$")
# Stricter than the app's sanitizer on purpose: non-negative numbers only, so the
# shipped filters are accepted by any reasonable implementation of SPEC §5.
FILTER_FN_RE = re.compile(r"(brightness|contrast|saturate|sepia|hue-rotate|grayscale)\(\s*(\d+(?:\.\d+)?)(deg|%)?\s*\)")
BASE_STATES = ["idle", "focus", "paused", "completed", "break"]
EXTRA_STATES = ["react", "blink"]
PALETTE_FIELDS = ["label", "background", "surface", "text", "muted", "accent", "ring", "ringTrack", "characterFilter"]
FORBIDDEN_TAGS = {"script", "foreignObject", "image", "iframe", "audio", "video", "use", "style", "a"}
MAX_SHEET_BYTES = 256 * 1024

# Base palette ("forest"). The renderer defines this as a constant
# (src/theme/palettes.ts); packs/placeholder/README.md documents the same values.
FOREST = {
    "label": "Forest",
    "background": "#f3f0e4",
    "surface": "#e3e9d6",
    "text": "#26331f",
    "muted": "#55634a",
    "accent": "#477033",
    "ring": "#5b8c43",
    "ringTrack": "#d2dac3",
}


def relative_luminance(hex_color: str) -> float:
    def ch(c: int) -> float:
        v = c / 255
        return v / 12.92 if v <= 0.04045 else ((v + 0.055) / 1.055) ** 2.4

    r, g, b = (int(hex_color[i : i + 2], 16) for i in (1, 3, 5))
    return 0.2126 * ch(r) + 0.7152 * ch(g) + 0.0722 * ch(b)


def contrast(a: str, b: str) -> float:
    la, lb = sorted((relative_luminance(a), relative_luminance(b)), reverse=True)
    return (la + 0.05) / (lb + 0.05)


def is_safe_filter(v: str) -> bool:
    rest = v.strip()
    if not rest:
        return False
    pos = 0
    while pos < len(rest):
        m = FILTER_FN_RE.match(rest, pos)
        if not m:
            return False
        pos = m.end()
        while pos < len(rest) and rest[pos] == " ":
            pos += 1
    return True


# Minimum contrast ratios checked for every palette.
CONTRAST_RULES = [
    ("text", "surface", 4.5),
    ("text", "background", 4.5),
    ("muted", "surface", 4.5),
    ("muted", "background", 4.5),
    ("accent", "background", 4.5),
    ("accent", "surface", 4.5),  # accent text on chips and buttons (e.g. the selected preset)
    ("ring", "background", 3.0),
    ("ring", "ringTrack", 2.0),
]


def check_palette(name: str, pal: dict, require_filter: bool) -> tuple[list[str], list[str]]:
    errors, report = [], []
    for f in PALETTE_FIELDS:
        if f == "characterFilter" and not require_filter and f not in pal:
            continue
        if f not in pal:
            errors.append(f"palette {name}: missing {f}")
    for f, v in pal.items():
        if f not in PALETTE_FIELDS:
            errors.append(f"palette {name}: unknown field {f}")
        elif f == "label":
            if not isinstance(v, str) or not v.strip():
                errors.append(f"palette {name}: empty label")
        elif f == "characterFilter":
            if not isinstance(v, str) or not is_safe_filter(v):
                errors.append(f"palette {name}: unsafe characterFilter {v!r}")
        elif not isinstance(v, str) or not HEX_RE.match(v):
            errors.append(f"palette {name}: {f} must be lowercase #rrggbb, got {v!r}")
    if errors:
        return errors, report
    ratios = []
    for a, b, minimum in CONTRAST_RULES:
        r = contrast(pal[a], pal[b])
        ratios.append(f"{a}/{b} {r:.2f}")
        if r < minimum:
            errors.append(f"palette {name}: contrast {a}/{b} = {r:.2f} < {minimum}")
    report.append(f"  {name:<12} " + ", ".join(ratios))
    return errors, report


def check_svg(path: Path, frames: int, fw: int, fh: int) -> list[str]:
    errors = []
    raw = path.read_bytes()
    if len(raw) > MAX_SHEET_BYTES:
        errors.append(f"{path.name}: {len(raw)} bytes > {MAX_SHEET_BYTES}")
    try:
        root = ET.fromstring(raw)
    except ET.ParseError as e:
        return [f"{path.name}: not well-formed XML: {e}"]
    ns = "{http://www.w3.org/2000/svg}"
    if root.tag != f"{ns}svg":
        errors.append(f"{path.name}: root element is {root.tag}, expected svg")
    w, h = root.get("width"), root.get("height")
    if w != str(frames * fw) or h != str(fh):
        errors.append(f"{path.name}: size {w}x{h}, expected {frames * fw}x{fh} (frames * frameWidth x frameHeight)")
    if root.get("viewBox") != f"0 0 {frames * fw} {fh}":
        errors.append(f"{path.name}: viewBox {root.get('viewBox')!r} does not match the size")
    cells = [g for g in root.iter(f"{ns}g") if (g.get("id") or "").startswith("frame-")]
    if len(cells) != frames:
        errors.append(f"{path.name}: {len(cells)} frame groups, manifest says {frames}")
    for i, g in enumerate(cells):
        if g.get("id") != f"frame-{i}" or g.get("transform") != f"translate({i * fw} 0)":
            errors.append(f"{path.name}: frame {i} is not at x={i * fw}")
    for e in root.iter():
        tag = e.tag.replace(ns, "")
        if tag in FORBIDDEN_TAGS:
            errors.append(f"{path.name}: forbidden element <{tag}>")
        for k, v in e.attrib.items():
            if k.lower().startswith("on"):
                errors.append(f"{path.name}: event attribute {k}")
            if "href" in k.lower() or re.search(r"url\((?!#)", v) or "://" in v:
                errors.append(f"{path.name}: external reference in {k}={v!r}")
    return errors


def safe_rel(p: str) -> bool:
    if not p or p.startswith("/") or "\\" in p or "\0" in p or re.match(r"^[A-Za-z]:", p):
        return False
    return all(seg and seg != ".." and not seg.startswith(".") for seg in p.split("/"))


# SPEC §7.1: `blink` is a one-shot that replaces `idle` or `focus` for one cycle (the
# renderer then restarts the base loop at frame 0). So the blink may change only the
# eyes of the pose those states start from, or the body visibly jumps twice per blink.
BLINK_REPLACES = ("idle", "focus")
EYE_FIELDS = frozenset({"eyes", "look"})
# A blink can also start mid-loop. Any frame of a replaced loop may differ from the
# blink pose by at most this much breathing and sway; every other body field must
# match exactly.
BLINK_DRIFT = {"sx": 0.04, "sy": 0.04, "arm_l": 4.0, "arm_r": 4.0, "sprout": 5.0}


def _short(v: object) -> str:
    s = repr(v)
    return s if len(s) <= 24 else s[:20] + "...'"


def pose_diffs(a: Pose, b: Pose, tolerance: dict[str, float] | None = None) -> list[str]:
    """The body fields (every field except the eyes) in which two poses differ beyond `tolerance`."""
    out = []
    for f in fields(Pose):
        if f.name in EYE_FIELDS:
            continue
        va, vb = getattr(a, f.name), getattr(b, f.name)
        numeric = all(isinstance(v, (int, float)) and not isinstance(v, bool) for v in (va, vb))
        if numeric:
            if abs(va - vb) > (tolerance or {}).get(f.name, 0.0) + 1e-9:
                out.append(f"{f.name} {num(va)} vs {num(vb)}")
        elif va != vb:
            out.append(f"{f.name} {_short(va)} vs {_short(vb)}")
    return out


def eyes_neutral(p: Pose) -> Pose:
    return replace(p, eyes="open", look=(0.0, 0.0))


def check_blink_fit(built: dict[str, list[Pose]]) -> list[str]:
    """The blink strip changes only the eyes of every state in BLINK_REPLACES."""
    blink = built.get("blink")
    if not blink:
        return ["blink: no frames"]
    errors = []
    base = blink[0]
    for i, p in enumerate(blink[1:], 1):
        d = pose_diffs(base, p)
        if d:
            errors.append(
                f"blink frame {i}: body differs from blink frame 0 ({', '.join(d)}); a blink moves only the eyes"
            )
    for key in BLINK_REPLACES:
        frames = built.get(key)
        if not frames:
            errors.append(f"blink replaces {key}, which has no frames")
            continue
        d = pose_diffs(base, frames[0])
        if d:
            errors.append(f"blink does not fit {key}: body differs from {key} frame 0 ({', '.join(d)})")
        elif draw_frame(eyes_neutral(base)) != draw_frame(eyes_neutral(frames[0])):
            errors.append(f"blink does not fit {key}: frame 0 renders differently outside the eyes")
        for k, p in enumerate(frames[1:], 1):
            d = pose_diffs(base, p, BLINK_DRIFT)
            if d:
                errors.append(f"blink does not fit {key}: frame {k} is too far from the blink pose ({', '.join(d)})")
    return errors


def check_up_to_date(pack_dir: Path, built: dict[str, list[Pose]]) -> list[str]:
    """Every generated file on disk is exactly what this script writes (no hand edits, no stale output)."""
    errors = []
    for name, text in expected_files(built).items():
        path = pack_dir / name
        if not path.is_file():
            errors.append(f"{name}: missing; run python3 scripts/gen_placeholder_pack.py")
        elif path.read_text(encoding="utf-8") != text:
            errors.append(f"{name}: differs from the generator output; run python3 scripts/gen_placeholder_pack.py")
    return errors


def validate(verbose: bool = True) -> list[str]:
    errors: list[str] = []
    report: list[str] = []
    mpath = PACK_DIR / "pack.json"
    try:
        m = json.loads(mpath.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as e:
        return [f"pack.json: {e}"]
    table = json.loads(UNLOCKS_PATH.read_text(encoding="utf-8"))

    if m.get("format") != 1:
        errors.append("pack.json: format must be 1")
    if m.get("id") != "placeholder" or not PACK_ID_RE.match(str(m.get("id"))):
        errors.append("pack.json: id must be 'placeholder'")
    if m.get("name") != "Pip (placeholder)":
        errors.append("pack.json: name must be 'Pip (placeholder)'")
    if m.get("license") != "CC0-1.0":
        errors.append("pack.json: license must be CC0-1.0")
    if m.get("rendering") not in ("smooth", "pixelated"):
        errors.append("pack.json: rendering must be smooth or pixelated")

    anims = m.get("animations") or {}
    unlock_keys = [u["id"] for u in table["unlocks"] if u["kind"] in ("animation", "idleVariant")]
    for key in BASE_STATES + EXTRA_STATES + unlock_keys:
        if key not in anims:
            errors.append(f"pack.json: missing animation {key!r}")
    for key, a in anims.items():
        where = f"animations.{key}"
        frames, fw, fh = a.get("frames", 0), a.get("frameWidth", 0), a.get("frameHeight", 0)
        if not (isinstance(frames, int) and 1 <= frames <= 64):
            errors.append(f"{where}: frames {frames!r} out of range")
            continue
        if fw != FRAME or fh != FRAME:
            errors.append(f"{where}: frame size {fw}x{fh}, expected {FRAME}x{FRAME}")
        fps = a.get("fps")
        if not isinstance(fps, (int, float)) or not (0 < fps <= 60):
            errors.append(f"{where}: fps {fps!r} outside (0, 60]")
        if a.get("row", 0) != 0 or a.get("column", 0) != 0:
            errors.append(f"{where}: expected row 0, column 0 (one strip per sheet)")
        if a.get("sheetColumns", frames) != frames or a.get("sheetRows", 1) != 1:
            errors.append(f"{where}: sheetColumns/sheetRows must be {frames}/1")
        still = a.get("stillFrame", 0)
        if not (isinstance(still, int) and 0 <= still < frames):
            errors.append(f"{where}: stillFrame {still!r} outside 0..{frames - 1}")
        durs = a.get("frameDurationsMs")
        if durs is not None and (len(durs) != frames or any((not isinstance(d, int)) or d <= 0 for d in durs)):
            errors.append(f"{where}: frameDurationsMs must hold {frames} positive integers")
        if not isinstance(a.get("loop", True), bool):
            errors.append(f"{where}: loop must be a boolean")
        sheet = a.get("sheet", "")
        if not safe_rel(sheet) or not sheet.endswith(".svg"):
            errors.append(f"{where}: unsafe sheet path {sheet!r}")
            continue
        spath = PACK_DIR / sheet
        if not spath.is_file():
            errors.append(f"{where}: sheet {sheet} does not exist")
            continue
        errors += check_svg(spath, frames, fw, fh)
        total_ms = sum(durs) if durs else frames * 1000 / fps
        report.append(
            f"  {key:<13} {frames} frames  {spath.stat().st_size:>6} B  {total_ms / 1000:.2f} s  loop={a.get('loop', True)}  still={still}"
        )
    for src, dst in (m.get("fallbacks") or {}).items():
        if dst not in anims:
            errors.append(f"fallbacks.{src}: target {dst!r} is not an animation")
    for pid, pal in (m.get("palettes") or {}).items():
        e, _ = check_palette(f"pack:{pid}", pal, require_filter=False)
        errors += e

    built = build_all()
    errors += check_up_to_date(PACK_DIR, built)
    fit = check_blink_fit(built)
    errors += fit
    if not fit:
        report.append(f"  blink changes only the eyes of: {', '.join(BLINK_REPLACES)} (SPEC §7.1)")

    report.append("palettes (contrast ratios):")
    e, r = check_palette("forest", FOREST, require_filter=False)
    errors += e
    report += r
    for u in table["unlocks"]:
        if u["kind"] != "palette":
            continue
        pal = u.get("palette")
        if not isinstance(pal, dict):
            errors.append(f"unlocks.json: palette unlock {u['id']} has no palette object")
            continue
        e, r = check_palette(u["id"], pal, require_filter=True)
        errors += e
        report += r
    if README_PATH.is_file():
        readme = README_PATH.read_text(encoding="utf-8")
        for f, v in FOREST.items():
            if f != "label" and v not in readme:
                errors.append(f"README.md: forest {f} {v} is not documented")
    else:
        errors.append("packs/placeholder/README.md is missing")
    for name in ("LICENSE.md",):
        if not (PACK_DIR / name).is_file():
            errors.append(f"packs/placeholder/{name} is missing")

    if verbose:
        print("placeholder pack:")
        print("\n".join(report))
    return errors


def self_test() -> list[str]:
    """Unit tests for the geometry output and the validator (no files are written to the pack)."""
    import tempfile

    failures: list[str] = []

    def expect(cond: bool, what: str) -> None:
        if not cond:
            failures.append(what)

    # Number formatting is compact and never prints "-0".
    expect(
        num(0.0) == "0" and num(-0.001) == "0" and num(1.50) == "1.5" and num(-2.345) == "-2.35" and num(96) == "96",
        "num() formatting",
    )

    # Contrast math (WCAG 2.x reference values).
    expect(abs(contrast("#000000", "#ffffff") - 21.0) < 1e-6, "contrast black/white == 21")
    expect(abs(contrast("#777777", "#777777") - 1.0) < 1e-9, "contrast of a color with itself == 1")
    expect(abs(contrast("#767676", "#ffffff") - 4.54) < 0.01, "contrast #767676 on white ~ 4.54")

    # Filter whitelist.
    for ok in (
        "brightness(0.9)",
        "brightness(0.88) saturate(0.85) sepia(0.15)",
        "hue-rotate(350deg)",
        "sepia(20%)",
        "grayscale(1)",
    ):
        expect(is_safe_filter(ok), f"filter accepted: {ok}")
    for bad in (
        "",
        "none",
        "blur(2px)",
        "url(#x)",
        "brightness(0.9);color:red",
        "drop-shadow(0 0 2px red)",
        "brightness(-1)",
        "brightness()",
        "saturate(1)x",
    ):
        expect(not is_safe_filter(bad), f"filter rejected: {bad!r}")

    # Sheet path safety.
    for ok in ("idle.svg", "sheets/idle.svg"):
        expect(safe_rel(ok), f"path accepted: {ok}")
    for bad in (
        "",
        "/idle.svg",
        "../idle.svg",
        "a/../b.svg",
        "a\\b.svg",
        ".hidden.svg",
        "a/.x/b.svg",
        "C:idle.svg",
        "a//b.svg",
        "nul\0.svg",
    ):
        expect(not safe_rel(bad), f"path rejected: {bad!r}")

    # Palette checks.
    good = dict(FOREST, characterFilter="brightness(0.95)")
    expect(check_palette("t", good, True)[0] == [], "valid palette passes")
    expect(
        any("missing ring" in e for e in check_palette("t", {k: v for k, v in good.items() if k != "ring"}, True)[0]),
        "missing field fails",
    )
    expect(
        any(
            "characterFilter" in e
            for e in check_palette("t", {k: v for k, v in good.items() if k != "characterFilter"}, True)[0]
        ),
        "missing filter fails when required",
    )
    expect(
        any("lowercase" in e for e in check_palette("t", dict(good, text="#26331F"), True)[0]), "uppercase hex fails"
    )
    expect(
        any("lowercase" in e for e in check_palette("t", dict(good, text="rgb(0,0,0)"), True)[0]),
        "rgb() fails (hex only)",
    )
    expect(
        any("contrast text/surface" in e for e in check_palette("t", dict(good, text="#c9cfbf"), True)[0]),
        "low text contrast fails",
    )
    expect(
        any(
            "unsafe characterFilter" in e for e in check_palette("t", dict(good, characterFilter="blur(1px)"), True)[0]
        ),
        "unsafe filter fails",
    )
    expect(
        any("unknown field" in e for e in check_palette("t", dict(good, glow="#ffffff"), True)[0]),
        "unknown field fails",
    )
    # Accent is used as text on surface-colored chips: the old forest accent passed on
    # the background (4.73) but not on the surface (4.35), and must now fail.
    errs = check_palette("t", dict(good, accent="#4a7535"), True)[0]
    expect(len(errs) == 1 and "contrast accent/surface" in errs[0], f"accent/surface rule rejects #4a7535 (got {errs})")

    # Blink fits every state it may replace (SPEC §7.1): it changes only the eyes.
    built = build_all()
    fit = check_blink_fit(built)
    expect(fit == [], f"blink fits idle and focus (got {fit[:2]})")
    expect([p.eyes for p in built["blink"]] == ["half", "shut", "half"], "blink is half, shut, half")
    expect(
        all(p.eyes in ("open", "soft") for k in BLINK_REPLACES for p in built[k]),
        "idle and focus keep their eyes open, so a blink over them reads as a blink",
    )
    expect(set(BLINK_REPLACES) == {"idle", "focus"}, "BLINK_REPLACES matches SPEC §7.1 (idle, focus)")
    for key in BLINK_REPLACES:
        expect(
            draw_frame(eyes_neutral(built["blink"][0])) == draw_frame(eyes_neutral(built[key][0])),
            f"blink frame 0 and {key} frame 0 render identically apart from the eyes",
        )
    expect(draw_frame(built["blink"][0]) != draw_frame(built["focus"][0]), "focus and blink still differ in the eyes")
    expect(pose_diffs(Pose(eyes="open", look=(1.0, 1.0)), Pose(eyes="shut")) == [], "pose_diffs ignores the eyes")
    expect(pose_diffs(Pose(arm_l=40), Pose(arm_l=43), {"arm_l": 4.0}) == [], "pose_diffs honours a tolerance")
    expect(pose_diffs(Pose(arm_l=40), Pose(arm_l=None)) != [], "a hidden arm is a body difference")
    expect(pose_diffs(Pose(held="<circle/>"), Pose()) != [], "a held prop is a body difference")
    # The original closed-eyed focus (arms 26, small mouth, less blush) is rejected.
    old_focus = [
        Pose(**breath(k, 4, 0.016), eyes="closed", mouth="small", blush=0.45, arm_l=26, arm_r=26) for k in range(4)
    ]
    errs = check_blink_fit(dict(built, focus=old_focus))
    expect(
        any("focus frame 0" in e and all(f in e for f in ("arm_l", "arm_r", "mouth", "blush")) for e in errs),
        f"blink over the old focus pose is rejected (got {errs[:2]})",
    )
    # A blink frame that moves the body is rejected.
    moved = [built["blink"][0], replace(built["blink"][1], arm_r=60.0), built["blink"][2]]
    expect(
        any(e.startswith("blink frame 1:") for e in check_blink_fit(dict(built, blink=moved))),
        "a blink that moves an arm is rejected",
    )
    # Frame 0 must match exactly (the base loop restarts there after a blink).
    idle = list(built["idle"])
    idle[0] = replace(idle[0], sprout=idle[0].sprout + 0.5)
    expect(
        any("idle frame 0" in e and "sprout" in e for e in check_blink_fit(dict(built, idle=idle))),
        "a sprout offset on idle frame 0 is rejected",
    )
    # A loop may breathe and sway, but not drift far from the blink pose mid-loop.
    focus = list(built["focus"])
    focus[2] = replace(focus[2], arm_l=55.0)
    expect(
        any("focus: frame 2" in e and "arm_l" in e for e in check_blink_fit(dict(built, focus=focus))),
        "a focus frame that raises an arm is rejected",
    )
    focus[2] = replace(built["focus"][2], mouth="o")
    expect(
        any("focus: frame 2" in e and "mouth" in e for e in check_blink_fit(dict(built, focus=focus))),
        "a focus frame with another mouth is rejected",
    )
    expect(
        any("has no frames" in e for e in check_blink_fit({k: v for k, v in built.items() if k != "focus"})),
        "a missing replaced state is reported",
    )
    expect(check_blink_fit({k: v for k, v in built.items() if k != "blink"}) == ["blink: no frames"], "no blink")
    expect(draw_eyes(Pose(eyes="soft")).count("<path") == 2, "soft eyes draw one lid shape per eye")

    # Rendering is deterministic and every sheet has one clipped cell per frame.
    for a in ANIMATIONS:
        f1, f2 = a.build(), a.build()
        s1, s2 = render_sheet(a, f1), render_sheet(a, f2)
        expect(s1 == s2, f"{a.key}: rendering is deterministic")
        expect(s1.count('clip-path="url(#cell)"') == len(f1), f"{a.key}: every frame is clipped to its cell")
        if a.durations:
            expect(len(a.durations) == len(f1), f"{a.key}: frameDurationsMs length == frames")
        expect(0 <= a.still < len(f1), f"{a.key}: stillFrame in range")

    # check_svg catches bad sheets.
    good_sheet = render_sheet(ANIMATIONS[0], ANIMATIONS[0].build())
    frames = len(ANIMATIONS[0].build())
    cases = {
        "good": (good_sheet, frames, False),
        "wrong width": (
            good_sheet.replace(f'width="{frames * FRAME}"', f'width="{frames * FRAME + 1}"', 1),
            frames,
            True,
        ),
        "frame count": (good_sheet, frames + 1, True),
        "script": (good_sheet.replace("</svg>", "<script>alert(1)</script></svg>"), frames, True),
        "external href": (
            good_sheet.replace("</svg>", '<image href="https://example.com/x.png"/></svg>'),
            frames,
            True,
        ),
        "css url": (good_sheet.replace('fill="#8dbb66"', 'fill="url(https://example.com/a)"', 1), frames, True),
        "event handler": (good_sheet.replace("<svg ", '<svg onload="x()" ', 1), frames, True),
        "not xml": (good_sheet[:-20], frames, True),
    }
    with tempfile.TemporaryDirectory() as tmp:
        for name, (text, n, should_fail) in cases.items():
            path = Path(tmp) / "sheet.svg"
            path.write_text(text, encoding="utf-8")
            errs = check_svg(path, n, FRAME, FRAME)
            expect(
                bool(errs) == should_fail,
                f"check_svg {name}: expected {'errors' if should_fail else 'none'}, got {errs[:2]}",
            )

    # check_up_to_date catches hand edits and missing output.
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        files = expected_files(built)
        for name, text in files.items():
            (d / name).write_text(text, encoding="utf-8", newline="\n")
        expect(check_up_to_date(d, built) == [], "fresh generator output is up to date")
        (d / "blink.svg").write_text(files["blink.svg"].replace(f'fill="{EYE}"', 'fill="#000000"', 1), encoding="utf-8")
        (d / "focus.svg").unlink()
        errs = check_up_to_date(d, built)
        expect(
            any(e.startswith("blink.svg: differs") for e in errs)
            and any(e.startswith("focus.svg: missing") for e in errs),
            f"check_up_to_date reports an edited and a missing sheet (got {errs})",
        )
    return failures


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--check", action="store_true", help="validate only; do not write files")
    ap.add_argument("--self-test", action="store_true", help="run the generator's own unit tests, then validate")
    args = ap.parse_args(argv)
    if args.self_test:
        failures = self_test()
        for f in failures:
            print(f"  FAIL {f}", file=sys.stderr)
        if failures:
            return 1
        print("self-test ok")
        args.check = True
    if not args.check:
        for p in generate():
            print(f"wrote {p.relative_to(ROOT)}")
    errors = validate()
    if errors:
        print("\nvalidation FAILED:", file=sys.stderr)
        for e in errors:
            print(f"  - {e}", file=sys.stderr)
        return 1
    print("validation ok")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
