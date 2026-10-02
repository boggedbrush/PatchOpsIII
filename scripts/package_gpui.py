#!/usr/bin/env python3
"""Create an evaluation archive; Python and API dependencies remain prerequisites."""
import argparse
from pathlib import Path
from zipfile import ZIP_DEFLATED, ZipFile

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--executable", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if not args.executable.is_file():
        parser.error("Build the executable first")
    files = ["LICENSE", "package.json", "requirements.txt", "presets.json", "utils.py", "t7_patch.py", "dxvk_manager.py", "bo3_enhanced.py", "scripts/gpui.py", "native/gpui/README.md", "docs/gpui-evaluation.md"]
    files += [str(path.relative_to(ROOT)) for path in (ROOT / "backend").glob("*.py")]
    files += [str(path.relative_to(ROOT)) for path in (ROOT / "docs" / "images").glob("gpui-*.png")]
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with ZipFile(args.output, "w", ZIP_DEFLATED) as archive:
        archive.write(args.executable, f"bin/{args.executable.name}")
        for name in files:
            archive.write(ROOT / name, name)
    print(args.output)


if __name__ == "__main__":
    main()
