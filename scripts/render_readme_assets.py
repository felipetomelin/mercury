#!/usr/bin/env python3
"""Render public README assets from Mercury's real UI frames and original SVG.

Optional developer dependency: Pillow. No account or vault access.
See assets/mercury/README.md for the opt-in Rust frame exporter command.
"""
from __future__ import annotations

import argparse
import json
import math
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont


def font_path(explicit: str | None) -> str:
    for name in [explicit, "/System/Library/Fonts/Menlo.ttc",
                 "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"]:
        if name and Path(name).is_file():
            return name
    raise SystemExit("Provide a monospace font with --font")


def color(value: int, default: str) -> tuple[int, int, int] | str:
    if value >> 24 == 2:
        return ((value >> 16) & 255, (value >> 8) & 255, value & 255)
    palette = ["#10151d", "#10151d", "#f7768e", "#9ece6a", "#e0af68",
               "#7aa2f7", "#bb9af7", "#7dcfff", "#c0caf5", "#565f89",
               "#f7768e", "#9ece6a", "#e0af68", "#7aa2f7", "#bb9af7",
               "#7dcfff", "#ffffff"]
    if value >> 24 == 0:
        return default if value == 0 else palette[min(value & 255, 16)]
    index = value & 255
    if index < 16:
        return palette[index + 1]
    if index >= 232:
        shade = 8 + (index - 232) * 10
        return shade, shade, shade
    n = index - 16
    levels = [0, 95, 135, 175, 215, 255]
    return levels[n // 36], levels[(n // 6) % 6], levels[n % 6]


def screenshot(frame_path: Path, output: Path, font_file: str) -> None:
    frame = json.loads(frame_path.read_text())
    font = ImageFont.truetype(font_file, 24)
    try:
        bold = ImageFont.truetype(font_file, 24, index=1)
    except OSError:
        bold = font
    small = ImageFont.truetype(font_file, 19)
    cw, ch = font.getlength("M"), 34
    width = math.ceil(frame["width"] * cw)
    height = frame["height"] * ch
    canvas = Image.new("RGB", (width + 64, height + 154), "#0b1018")
    draw = ImageDraw.Draw(canvas)
    draw.rounded_rectangle((15, 15, width + 49, height + 115), radius=18,
                           fill="#121923", outline="#283242", width=2)
    for x, fill in [(40, "#fa7773"), (68, "#eac16d"), (96, "#78d6ad")]:
        draw.ellipse((x, 39, x + 13, 52), fill=fill)
    draw.text((132, 31), "Mercury / " + frame_path.stem.capitalize(),
              font=small, fill="#c6d1df")
    for i, cell in enumerate(frame["cells"]):
        col, row = i % frame["width"], i // frame["width"]
        x, y = 32 + col * cw, 78 + row * ch
        draw.rectangle((int(x), y, math.ceil(x + cw), y + ch),
                       fill=color(cell["bg"], "#10151d"))
        # Draw terminal box lines as cell geometry, independent of font coverage.
        if cell["symbol"] in ("─", "━"):
            draw.line((int(x), y + ch // 2, math.ceil(x + cw), y + ch // 2),
                      fill=color(cell["fg"], "#d8e1ed"), width=3 if cell["symbol"] == "━" else 1)
            continue
        if cell["symbol"] in ("│", "┃"):
            draw.line((int(x + cw / 2), y, int(x + cw / 2), y + ch),
                      fill=color(cell["fg"], "#d8e1ed"), width=3 if cell["symbol"] == "┃" else 1)
            continue
        if cell["symbol"].strip() and not cell.get("skip", False):
            face = bold if cell["modifier"] & 1 else font
            draw.text((x, y + 26), cell["symbol"], font=face,
                      fill=color(cell["fg"], "#d8e1ed"), anchor="ls")
    draw.text((32, height + 126), "REAL MERCURY RENDERER  /  FICTIONAL DEMO DATA",
              font=small, fill="#8b9bb0")
    canvas.save(output, optimize=True)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--frames", type=Path, required=True)
    parser.add_argument("--output", type=Path, default=Path("assets/mercury"))
    parser.add_argument("--font")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    font = font_path(args.font)
    for name in ["usage", "crons", "knowledge"]:
        screenshot(args.frames / (name + ".json"), args.output / (name + ".png"), font)
    social = Image.new("RGB", (1280, 640))
    draw = ImageDraw.Draw(social)
    for y in range(640):
        t = y / 639
        shade = tuple(round(a + (b - a) * t) for a, b in zip((12, 17, 27), (29, 43, 67)))
        draw.line((0, y, 1280, y), fill=shade)
    sans = next((p for p in ["/System/Library/Fonts/Supplemental/Arial.ttf",
                             "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"] if Path(p).exists()), font)
    bold = next((p for p in ["/System/Library/Fonts/Supplemental/Arial Bold.ttf",
                             "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"] if Path(p).exists()), sans)
    for y in [84, 460]:
        draw.line((80, y, 1200, y), fill="#344054", width=2)
    for x, y, text, face, size, fill in [
        (80, 124, "CODEX + CLAUDE CODE", font, 24, "#a5f3d4"),
        (73, 180, "Mercury", bold, 148, "#f0f5ff"),
        (80, 353, "Your coding agents. One terminal.", sans, 40, "#c3cfdf"),
        (80, 490, "Persistent sessions · Usage · Scheduling · GraphRAG", sans, 26, "#94a8c5"),
        (80, 548, "github.com/felipetomelin/mercury", font, 23, "#a5f3d4"),
    ]:
        draw.text((x, y), text, font=ImageFont.truetype(face, size), fill=fill)
    social.save(args.output / "social-preview.png", optimize=True)
    print("Rendered Usage, Crons, Knowledge and social preview assets.")


if __name__ == "__main__":
    main()
