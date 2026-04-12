# /// script
# requires-python = ">=3.12"
# dependencies = [
#   "Pillow",
# ]
# ///

"""Split a high-contrast linocut-style PNG into two SVGs: dark regions and light regions."""

import argparse
import shutil
import subprocess
import tempfile
from pathlib import Path

from PIL import Image


def _find_potrace() -> str:
    found = shutil.which("potrace")
    if found:
        return found
    for candidate in [
        Path("/home/linuxbrew/.linuxbrew/bin/potrace"),
        Path.home() / ".linuxbrew/bin/potrace",
        Path("/opt/homebrew/bin/potrace"),
        Path("/usr/local/bin/potrace"),
    ]:
        if candidate.is_file():
            return str(candidate)
    raise FileNotFoundError(
        "potrace not found - install it (e.g. `brew install potrace`)"
    )


POTRACE = _find_potrace()


def threshold_to_pbm(img: Image.Image, threshold: int, invert: bool) -> bytes:
    """Convert a grayscale image to PBM bytes by thresholding.

    In PBM format, 1 = black (traced by potrace), 0 = white.
    - invert=False: pixels below threshold become black (dark regions)
    - invert=True: pixels at/above threshold become black (light regions)
    """
    width, height = img.size
    pixels = img.load()

    rows = []
    for y in range(height):
        row = []
        for x in range(width):
            p = pixels[x, y]
            if invert:
                row.append("1" if p >= threshold else "0")
            else:
                row.append("1" if p < threshold else "0")
        rows.append(" ".join(row))

    pbm = f"P1\n{width} {height}\n" + "\n".join(rows) + "\n"
    return pbm.encode()


def trace_to_svg(pbm_bytes: bytes, output_path: Path) -> None:
    """Run potrace on PBM data to produce an SVG."""
    with tempfile.NamedTemporaryFile(suffix=".pbm") as tmp:
        tmp.write(pbm_bytes)
        tmp.flush()
        subprocess.run(
            [POTRACE, "-s", "-o", str(output_path), tmp.name],
            check=True,
        )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input", type=Path, help="Input PNG file")
    parser.add_argument(
        "--threshold",
        type=int,
        default=128,
        help="Grayscale threshold (0-255, default: 128)",
    )
    args = parser.parse_args()

    input_path: Path = args.input
    stem = input_path.stem
    output_dir = Path(__file__).parent / "svg" / stem
    output_dir.mkdir(parents=True, exist_ok=True)

    img = Image.open(input_path).convert("L")

    dark_pbm = threshold_to_pbm(img, args.threshold, invert=False)
    light_pbm = threshold_to_pbm(img, args.threshold, invert=True)

    # Output naming convention:
    # The filename indicates which theme the SVG is rendered ON, not
    # which regions were traced. Dark regions of the source (silhouette
    # shapes) render well on a light background, so they go to
    # for-light.svg. Light regions of the source (highlight shapes)
    # render well on a dark background when tinted with a light color,
    # so they go to for-dark.svg.
    for_light_svg = output_dir / "for-light.svg"  # dark regions traced
    for_dark_svg = output_dir / "for-dark.svg"    # light regions traced

    trace_to_svg(dark_pbm, for_light_svg)
    trace_to_svg(light_pbm, for_dark_svg)

    print(f"For light theme: {for_light_svg}")
    print(f"For dark theme:  {for_dark_svg}")


if __name__ == "__main__":
    main()
