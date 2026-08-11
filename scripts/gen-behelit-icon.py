#!/usr/bin/env python3
"""Generate original simplified Behelit-inspired tray icons (mono silhouette)."""

from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parents[1]


def draw_behelit(size: int) -> Image.Image:
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    m = max(1, size // 16)
    left, top = m + size // 8, m
    right, bottom = size - m - size // 10, size - m
    fill = (255, 255, 255, 255)
    hole = (0, 0, 0, 0)
    d.ellipse([left, top, right, bottom], fill=fill)

    def cut_ellipse(cx: int, cy: int, rx: int, ry: int) -> None:
        d.ellipse([cx - rx, cy - ry, cx + rx, cy + ry], fill=hole)

    s = size
    cut_ellipse(int(s * 0.38), int(s * 0.38), max(1, s // 10), max(1, s // 12))
    cut_ellipse(int(s * 0.62), int(s * 0.48), max(1, s // 9), max(1, s // 11))
    cut_ellipse(int(s * 0.52), int(s * 0.28), max(1, s // 14), max(1, s // 18))
    cut_ellipse(int(s * 0.50), int(s * 0.68), max(1, s // 7), max(1, s // 14))
    if s >= 24:
        d.line(
            [(int(s * 0.55), int(s * 0.22)), (int(s * 0.70), int(s * 0.40))],
            fill=hole,
            width=max(1, s // 20),
        )
    return img


def main() -> None:
    dirs = [ROOT / "assets" / "tray", ROOT / "src" / "assets" / "tray"]
    for d in dirs:
        d.mkdir(parents=True, exist_ok=True)
    for s in (16, 32, 64):
        im = draw_behelit(s)
        for d in dirs:
            path = d / f"behelit-{s}.png"
            im.save(path, "PNG")
            print("wrote", path)

    svg = """<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32" width="32" height="32">
  <!-- Original simplified Behelit-inspired amulet outline (not a copy of Berserk art). -->
  <ellipse cx="16" cy="16" rx="11" ry="14" fill="#fff"/>
  <ellipse cx="12" cy="12" rx="2.2" ry="2.6" fill="#000"/>
  <ellipse cx="20" cy="15" rx="2.5" ry="2.8" fill="#000"/>
  <ellipse cx="16.5" cy="9" rx="1.5" ry="1.2" fill="#000"/>
  <ellipse cx="16" cy="22" rx="3.2" ry="1.8" fill="#000"/>
  <path d="M18 7 L23 13" stroke="#000" stroke-width="1.5" fill="none"/>
</svg>
"""
    (ROOT / "assets" / "tray" / "behelit.svg").write_text(svg, encoding="utf-8")
    print("wrote svg")


if __name__ == "__main__":
    main()
