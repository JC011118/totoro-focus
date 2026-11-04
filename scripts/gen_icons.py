#!/usr/bin/env python3
"""Generate the app icon set and the macOS menu-bar template icon (original art).

  1. Renders a 1024x1024 app icon: Pip (the placeholder seedling, geometry shared
     with scripts/gen_placeholder_pack.py) on a soft green rounded square laid
     out on Apple's macOS icon grid (824 px body, 100 px margins, soft shadow).
  2. Runs the pinned Tauri CLI (`npx --no-install tauri icon <png> -o src-tauri/icons`)
     and deletes the outputs this macOS app does not use (android/, ios/,
     Square*Logo.png, StoreLogo.png). Kept: 32x32.png, 64x64.png, 128x128.png,
     128x128@2x.png, icon.icns, icon.ico, icon.png.
  3. Writes src-tauri/icons/tray-template.png: a 36x36 px (18 pt @2x) macOS
     template image, pure black shapes with alpha on transparent: a two-leaf
     sprout rising from a small round mound.

Deterministic, offline. Needs Pillow (`pip install pillow`) and, for step 2,
`npm ci` (the Tauri CLI from package-lock.json).

Usage:
  python3 scripts/gen_icons.py                       # all three steps
  python3 scripts/gen_icons.py --skip-tauri          # only the tray icon (and the source PNG if --source-out)
  python3 scripts/gen_icons.py --source-out icon.png # also keep the 1024 px source
"""

from __future__ import annotations

import argparse
import math
import shutil
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter

sys.dont_write_bytecode = True  # importing the sibling script must not leave scripts/__pycache__/
sys.path.insert(0, str(Path(__file__).resolve().parent))
import gen_placeholder_pack as pip

ROOT = Path(__file__).resolve().parent.parent
ICONS = ROOT / "src-tauri" / "icons"
KEEP = {
    "32x32.png",
    "64x64.png",
    "128x128.png",
    "128x128@2x.png",
    "icon.icns",
    "icon.ico",
    "icon.png",
    "tray-template.png",
}

# ---------------------------------------------------------------------------
# A tiny supersampled vector rasterizer (affine transforms, bezier flattening)
# ---------------------------------------------------------------------------


class Affine:
    def __init__(self, a=1.0, b=0.0, c=0.0, d=1.0, e=0.0, f=0.0):
        self.m = (a, b, c, d, e, f)  # x' = a x + c y + e ; y' = b x + d y + f

    def __matmul__(self, o: Affine) -> Affine:
        a, b, c, d, e, f = self.m
        A, B, C, D, E, F = o.m
        return Affine(a * A + c * B, b * A + d * B, a * C + c * D, b * C + d * D, a * E + c * F + e, b * E + d * F + f)

    def apply(self, x: float, y: float) -> tuple[float, float]:
        a, b, c, d, e, f = self.m
        return (a * x + c * y + e, b * x + d * y + f)

    @property
    def scale(self) -> float:
        a, b, c, d, _, _ = self.m
        return math.sqrt(abs(a * d - b * c))


def T(x: float, y: float) -> Affine:
    return Affine(e=x, f=y)


def S(sx: float, sy: float | None = None) -> Affine:
    return Affine(a=sx, d=sx if sy is None else sy)


def R(deg: float) -> Affine:
    r = math.radians(deg)
    return Affine(math.cos(r), math.sin(r), -math.sin(r), math.cos(r))


def flatten(cmds, steps: int = 32) -> list[tuple[list[tuple[float, float]], bool]]:
    """Path commands -> list of (points, closed)."""
    subpaths, pts, cur, start = [], [], (0.0, 0.0), (0.0, 0.0)
    for c in cmds:
        op = c[0]
        if op == "M":
            if pts:
                subpaths.append((pts, False))
            cur = start = (c[1], c[2])
            pts = [cur]
        elif op == "L":
            cur = (c[1], c[2])
            pts.append(cur)
        elif op == "Q":
            (x0, y0), (x1, y1), (x2, y2) = cur, (c[1], c[2]), (c[3], c[4])
            for i in range(1, steps + 1):
                t = i / steps
                u = 1 - t
                pts.append((u * u * x0 + 2 * u * t * x1 + t * t * x2, u * u * y0 + 2 * u * t * y1 + t * t * y2))
            cur = (x2, y2)
        elif op == "C":
            (x0, y0), (x1, y1), (x2, y2), (x3, y3) = cur, (c[1], c[2]), (c[3], c[4]), (c[5], c[6])
            for i in range(1, steps + 1):
                t = i / steps
                u = 1 - t
                pts.append(
                    (
                        u**3 * x0 + 3 * u * u * t * x1 + 3 * u * t * t * x2 + t**3 * x3,
                        u**3 * y0 + 3 * u * u * t * y1 + 3 * u * t * t * y2 + t**3 * y3,
                    )
                )
            cur = (x3, y3)
        elif op == "Z":
            subpaths.append((pts, True))
            pts, cur = [], start
    if pts:
        subpaths.append((pts, False))
    return subpaths


def ellipse_path(cx: float, cy: float, rx: float, ry: float, n: int = 96):
    cmds = [("M", cx + rx, cy)]
    cmds += [
        ("L", cx + rx * math.cos(2 * math.pi * i / n), cy + ry * math.sin(2 * math.pi * i / n)) for i in range(1, n)
    ]
    return cmds + [("Z",)]


def rect_path(x: float, y: float, w: float, h: float, r: float):
    """Rounded rectangle (r <= min(w, h) / 2)."""
    k = 0.5523 * r
    return [
        ("M", x + r, y),
        ("L", x + w - r, y),
        ("C", x + w - r + k, y, x + w, y + r - k, x + w, y + r),
        ("L", x + w, y + h - r),
        ("C", x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h),
        ("L", x + r, y + h),
        ("C", x + r - k, y + h, x, y + h - r + k, x, y + h - r),
        ("L", x, y + r),
        ("C", x, y + r - k, x + r - k, y, x + r, y),
        ("Z",),
    ]


class Canvas:
    def __init__(self, size: int, ss: int):
        self.size, self.ss = size, ss
        self.img = Image.new("RGBA", (size * ss, size * ss), (0, 0, 0, 0))
        self.draw = ImageDraw.Draw(self.img, "RGBA")
        self.base = S(ss)

    def _pts(self, cmds, m: Affine):
        full = self.base @ m
        return [([full.apply(x, y) for (x, y) in pts], closed) for pts, closed in flatten(cmds)], full.scale

    def fill(self, cmds, m: Affine, color, opacity: float = 1.0):
        rgba = rgba_of(color, opacity)
        for pts, _closed in self._pts(cmds, m)[0]:
            if len(pts) >= 3:
                self.draw.polygon(pts, fill=rgba)

    def stroke(self, cmds, m: Affine, color, width: float, opacity: float = 1.0):
        subs, scale = self._pts(cmds, m)
        w = max(1, round(width * scale))
        rgba = rgba_of(color, opacity)
        # Thick polylines leave wedge gaps at joints; stamping a round dot on every
        # vertex gives clean round joins and caps. Opacity < 1 would double up at
        # the overlaps, so translucent strokes are drawn on a layer first.
        target = self.draw
        layer = None
        if rgba[3] < 255:
            layer = Image.new("L", self.img.size, 0)
            target = ImageDraw.Draw(layer)
            ink = 255
        else:
            ink = rgba
        for pts, closed in subs:
            line = pts + [pts[0]] if closed else pts
            target.line(line, fill=ink, width=w)
            for x, y in line:
                target.ellipse([x - w / 2, y - w / 2, x + w / 2, y + w / 2], fill=ink)
        if layer is not None:
            solid = Image.new("RGBA", self.img.size, rgba[:3] + (255,))
            solid.putalpha(layer.point(lambda v: v * rgba[3] // 255))
            self.img.alpha_composite(solid)

    def shape(self, cmds, m: Affine, fill=None, stroke=None, width: float = 0.0, opacity: float = 1.0):
        if fill:
            self.fill(cmds, m, fill, opacity)
        if stroke and width > 0:
            self.stroke(cmds, m, stroke, width, opacity)

    def result(self) -> Image.Image:
        return self.img.resize((self.size, self.size), Image.LANCZOS)


def rgba_of(color: str, opacity: float = 1.0) -> tuple[int, int, int, int]:
    c = color.lstrip("#")
    return (int(c[0:2], 16), int(c[2:4], 16), int(c[4:6], 16), round(255 * opacity))


# ---------------------------------------------------------------------------
# App icon
# ---------------------------------------------------------------------------

ICON_SIZE = 1024
GRID_BODY = 824  # Apple macOS icon grid: 824 px rounded square centred in 1024
SQUIRCLE_N = 5.0


def squircle(cx: float, cy: float, half: float, n: float = SQUIRCLE_N, pts: int = 720):
    cmds = []
    for i in range(pts):
        t = 2 * math.pi * i / pts
        c, s = math.cos(t), math.sin(t)
        x = cx + half * math.copysign(abs(c) ** (2 / n), c)
        y = cy + half * math.copysign(abs(s) ** (2 / n), s)
        cmds.append(("M" if i == 0 else "L", x, y))
    return cmds + [("Z",)]


def draw_pip(cv: Canvas, origin: Affine) -> None:
    """Pip in the idle pose, matching gen_placeholder_pack.draw_frame()."""
    ow = pip.OUTLINE_W
    # sprout
    sprout = origin @ T(0, pip.BODY_TOP + 1) @ R(1.5)
    cv.stroke(pip.STEM_PATH, sprout, pip.STEM, 2.6)
    for lx, ly, rot, scale, mirrored in pip.LEAVES:
        m = (
            sprout
            @ T(pip.STEM_TIP[0] + lx, pip.STEM_TIP[1] + ly)
            @ R(-rot if mirrored else rot)
            @ S(-scale if mirrored else scale, scale)
        )
        cv.shape(pip.LEAF_PATH, m, fill=pip.LEAF, stroke=pip.OUTLINE, width=ow / scale * 0.85)
        cv.stroke(pip.LEAF_VEIN_PATH, m, pip.LEAF_VEIN, 1.1 / scale)
    # arms (behind the body)
    for side, (sx, sy) in enumerate(pip.SHOULDERS):
        m = origin @ T(sx, sy) @ R(40 if side == 0 else -40)
        cv.shape(
            rect_path(-pip.ARM_W / 2, -pip.ARM_W / 2, pip.ARM_W, pip.ARM_LEN + pip.ARM_W / 2, pip.ARM_W / 2),
            m,
            fill=pip.BODY,
            stroke=pip.OUTLINE,
            width=ow,
        )
    for fx, fy in pip.FOOT_POSITIONS:
        cv.shape(
            ellipse_path(fx, fy, pip.FOOT_RX, pip.FOOT_RY), origin, fill=pip.BODY_DARK, stroke=pip.OUTLINE, width=ow
        )
    cv.shape(pip.BODY_PATH, origin, fill=pip.BODY, stroke=pip.OUTLINE, width=ow)
    hx, hy, hrx, hry, hrot = pip.HIGHLIGHT
    cv.fill(ellipse_path(0, 0, hrx, hry), origin @ T(hx, hy) @ R(hrot), pip.BODY_HIGHLIGHT, 0.8)
    for x, y, r in pip.SPECKLES:
        cv.fill(ellipse_path(x, y, r, r, 32), origin, pip.BODY_DARK, 0.75)
    for x, y in pip.BLUSH_POSITIONS:
        cv.fill(ellipse_path(x, y, 4.1, 2.3), origin, pip.BLUSH, 0.72)
    # Slightly larger eyes than the sprite so the face survives small icon sizes.
    for ex, ey in pip.EYE_POSITIONS:
        cv.fill(ellipse_path(ex, ey, pip.EYE_RX * 1.18, pip.EYE_RY * 1.18), origin, pip.EYE)
        cv.fill(ellipse_path(ex - 1.0, ey - 1.45, 1.15, 1.15, 32), origin, "#ffffff")
    y = pip.MOUTH_Y
    cv.stroke([("M", -2.9, y - 0.4), ("Q", 0, y + 2.3, 2.9, y - 0.4)], origin, pip.EYE, 1.6)


def gradient(size: int, top: str, bottom: str) -> Image.Image:
    t, b = rgba_of(top), rgba_of(bottom)
    col = Image.new("RGBA", (1, size))
    for y in range(size):
        k = y / (size - 1)
        col.putpixel((0, y), tuple(round(t[i] + (b[i] - t[i]) * k) for i in range(4)))
    return col.resize((size, size), Image.NEAREST)


def render_app_icon(ss: int = 4) -> Image.Image:
    size = ICON_SIZE * ss
    half = GRID_BODY / 2
    cx, cy = ICON_SIZE / 2, ICON_SIZE / 2

    mask_cv = Canvas(ICON_SIZE, ss)
    mask_cv.fill(squircle(cx, cy, half), Affine(), "#ffffff")
    mask = mask_cv.img.getchannel("A")

    # Soft drop shadow below the rounded square.
    shadow = Image.new("RGBA", (size, size), (20, 40, 20, 0))
    shadow.putalpha(mask.point(lambda v: round(v * 0.30)))
    shadow = shadow.transform(shadow.size, Image.AFFINE, (1, 0, 0, 0, 1, -10 * ss)).filter(
        ImageFilter.GaussianBlur(14 * ss)
    )

    # Rounded square: vertical gradient plus a gentle hill for Pip to sit on.
    bg = gradient(size, "#eef5dd", "#c5e0a5")
    hill_cv = Canvas(ICON_SIZE, ss)
    hill_cv.fill(ellipse_path(cx, 1010, 560, 250, 180), Affine(), "#a9cf86")
    bg.alpha_composite(hill_cv.img)
    # A soft light halo behind Pip (blur only the mask, so no dark fringes).
    halo_cv = Canvas(ICON_SIZE, ss)
    halo_cv.fill(ellipse_path(cx, 470, 300, 300, 180), Affine(), "#ffffff")
    halo_mask = halo_cv.img.getchannel("A").filter(ImageFilter.GaussianBlur(40 * ss)).point(lambda v: v * 40 // 100)
    halo = Image.new("RGBA", (size, size), (255, 255, 255, 255))
    halo.putalpha(halo_mask)
    bg.alpha_composite(halo)
    tile = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    tile.paste(bg, (0, 0), mask)

    # Pip on the hill.
    pip_cv = Canvas(ICON_SIZE, ss)
    scale = 8.6
    ground = 778.0
    pip_cv.fill(ellipse_path(cx, ground + 2.2 * scale, 21 * scale, 3.3 * scale, 120), Affine(), pip.SHADOW, 0.18)
    draw_pip(pip_cv, T(cx, ground) @ S(scale))
    pip_layer = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    pip_layer.paste(pip_cv.img, (0, 0), mask)  # keep everything inside the tile

    out = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    out.alpha_composite(shadow)
    out.alpha_composite(tile)
    out.alpha_composite(pip_layer)
    return out.resize((ICON_SIZE, ICON_SIZE), Image.LANCZOS)


# ---------------------------------------------------------------------------
# Menu-bar template icon
# ---------------------------------------------------------------------------

TRAY_SIZE = 36  # 18 pt @2x


def render_tray(ss: int = 16) -> Image.Image:
    cv = Canvas(TRAY_SIZE, ss)
    black = "#000000"
    # A small round mound (Pip's head, without a face) ...
    mound = [
        ("M", 9.0, 32.5),
        ("C", 9.0, 27.6, 13.0, 25.0, 18, 25.0),
        ("C", 23.0, 25.0, 27.0, 27.6, 27.0, 32.5),
        ("Z",),
    ]
    cv.fill(mound, Affine(), black)
    # ... with a stem and two leaves (the same leaf shape as the character).
    cv.stroke([("M", 18, 26), ("C", 18.4, 22.5, 17.7, 19.2, 18.3, 15.8)], Affine(), black, 2.6)
    cv.fill(pip.LEAF_PATH, T(18.3, 16.4) @ R(-32) @ S(0.95), black)
    cv.fill(pip.LEAF_PATH, T(18.0, 17.4) @ R(30) @ S(-0.8, 0.8), black)
    img = cv.img.resize((TRAY_SIZE, TRAY_SIZE), Image.BOX)
    # Template images must be pure black: keep only the coverage in alpha.
    alpha = img.getchannel("A")
    out = Image.new("RGBA", (TRAY_SIZE, TRAY_SIZE), (0, 0, 0, 0))
    out.putalpha(alpha)
    return out


# ---------------------------------------------------------------------------


def save_png(img: Image.Image, path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    img.save(path, format="PNG", optimize=True)


# Legacy entries first, then the PNG-based ones from small to large.
ICNS_ORDER = ["is32", "s8mk", "il32", "l8mk", "ic11", "ic12", "ic07", "ic13", "ic08", "ic14", "ic09", "ic10"]


def canonicalize_icns(path: Path) -> None:
    """Rewrite an .icns with its entries in a fixed order.

    `tauri icon` (2.12.x) writes the same entries in a random order on each run.
    macOS looks entries up by type, so the order carries no meaning; sorting it
    makes the output byte-for-byte reproducible.
    """
    data = path.read_bytes()
    if data[:4] != b"icns":
        raise SystemExit(f"{path} is not an icns file")
    total = struct.unpack(">I", data[4:8])[0]
    pos, entries = 8, []
    while pos < total:
        kind = data[pos : pos + 4]
        n = struct.unpack(">I", data[pos + 4 : pos + 8])[0]
        if n < 8 or pos + n > total:
            raise SystemExit(f"{path}: corrupt icns entry at {pos}")
        entries.append((kind.decode("latin-1"), data[pos : pos + n]))
        pos += n
    rank = {k: i for i, k in enumerate(ICNS_ORDER)}
    entries.sort(key=lambda e: (rank.get(e[0], len(rank)), e[0], e[1]))
    body = b"".join(chunk for _, chunk in entries)
    path.write_bytes(b"icns" + struct.pack(">I", 8 + len(body)) + body)


def run_tauri_icon(source: Path) -> None:
    npx = shutil.which("npx")
    if not npx:
        raise SystemExit("npx not found; install Node and run `npm ci`, or pass --skip-tauri")
    cmd = [npx, "--no-install", "tauri", "icon", str(source), "-o", str(ICONS)]
    print("running:", " ".join(cmd[1:]))
    subprocess.run(cmd, cwd=ROOT, check=True)
    canonicalize_icns(ICONS / "icon.icns")
    for p in sorted(ICONS.iterdir()):
        if p.name in KEEP:
            continue
        if p.is_dir() and p.name in ("android", "ios"):
            shutil.rmtree(p)
            print(f"removed {p.relative_to(ROOT)}/")
        elif p.is_file() and (p.name.startswith("Square") or p.name == "StoreLogo.png"):
            p.unlink()
            print(f"removed {p.relative_to(ROOT)}")


def check_outputs() -> list[str]:
    errors = []
    expected = {"32x32.png": 32, "64x64.png": 64, "128x128.png": 128, "128x128@2x.png": 256, "icon.png": 512}
    for name, px in expected.items():
        p = ICONS / name
        if not p.is_file():
            errors.append(f"missing {name}")
            continue
        with Image.open(p) as im:
            if im.size != (px, px):
                errors.append(f"{name} is {im.size}, expected {px}x{px}")
    for name in ("icon.icns", "icon.ico"):
        if not (ICONS / name).is_file():
            errors.append(f"missing {name}")
    extra = sorted(p.name for p in ICONS.iterdir() if p.name not in KEEP)
    if extra:
        errors.append(f"unexpected files in src-tauri/icons: {extra}")
    tray = ICONS / "tray-template.png"
    with Image.open(tray) as im:
        im = im.convert("RGBA")
        if im.size != (TRAY_SIZE, TRAY_SIZE):
            errors.append(f"tray-template.png is {im.size}, expected 36x36")
        raw = im.tobytes()
        px = [tuple(raw[i : i + 4]) for i in range(0, len(raw), 4)]
        if any(p[:3] != (0, 0, 0) for p in px):
            errors.append("tray-template.png must be pure black (RGB 0,0,0) with alpha")
        if not any(p[3] == 255 for p in px) or px[0][3] != 0:
            errors.append("tray-template.png needs opaque shapes on a transparent background")
    return errors


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser(description="Generate the app icon set and tray template icon.")
    ap.add_argument(
        "--skip-tauri", action="store_true", help="do not run `tauri icon` (only the tray icon is rewritten)"
    )
    ap.add_argument("--source-out", type=Path, help="also save the 1024 px source PNG here")
    args = ap.parse_args(argv)

    if args.source_out or not args.skip_tauri:
        icon = render_app_icon()
        with tempfile.TemporaryDirectory() as tmp:
            src = args.source_out or Path(tmp) / "app-icon-1024.png"
            save_png(icon, src)
            print(f"rendered {src}")
            if not args.skip_tauri:
                run_tauri_icon(src)
    save_png(render_tray(), ICONS / "tray-template.png")
    print(f"wrote {(ICONS / 'tray-template.png').relative_to(ROOT)}")
    errors = check_outputs()
    if errors:
        for e in errors:
            print(f"  - {e}", file=sys.stderr)
        return 1
    print("icons ok")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
