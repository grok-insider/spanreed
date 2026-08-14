#!/usr/bin/env python3
"""Resize the white spanreed tray master from assets/tray/spanreed-light.png.

Requires ImageMagick (`magick`). The light mark's alpha is reused; RGB is
forced white so the tray can tint by severity.
"""

import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SRC = ROOT / "assets" / "tray" / "spanreed-light.png"


def main() -> None:
    if not SRC.is_file():
        sys.exit(f"missing {SRC}")
    if shutil.which("magick") is None:
        sys.exit("need ImageMagick `magick` on PATH")
    dests = [ROOT / "assets" / "tray", ROOT / "src" / "assets" / "tray"]
    for d in dests:
        d.mkdir(parents=True, exist_ok=True)
    for size in (16, 32, 64):
        out = dests[0] / f"spanreed-{size}.png"
        subprocess.check_call(
            [
                "magick",
                str(SRC),
                "-alpha",
                "extract",
                "-resize",
                f"{size}x{size}",
                "(",
                "-size",
                f"{size}x{size}",
                "xc:white",
                ")",
                "+swap",
                "-compose",
                "CopyOpacity",
                "-composite",
                f"PNG32:{out}",
            ]
        )
        for d in dests[1:]:
            shutil.copyfile(out, d / out.name)
        print("wrote", out)


if __name__ == "__main__":
    main()
