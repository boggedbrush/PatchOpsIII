#!/usr/bin/env python3
"""Create a self-contained PatchOpsIII native desktop archive."""
from __future__ import annotations

import argparse
from pathlib import Path, PurePosixPath
from zipfile import ZIP_DEFLATED, ZipFile

ROOT = Path(__file__).resolve().parents[1]
ARCHIVE_ROOT = PurePosixPath("PatchOpsIII")


def archive_manifest(executable: Path) -> list[tuple[Path, PurePosixPath]]:
    required = [
        ROOT / "LICENSE",
        ROOT / "README.md",
        ROOT / "package.json",
        ROOT / "presets.json",
        ROOT / "PatchOpsIII.ico",
        ROOT / "website" / "assets" / "img" / "icon-512.png",
        ROOT / "native" / "gpui" / "assets" / "icons" / "LICENSE-lucide.txt",
        ROOT / "native" / "gpui" / "README.md",
        ROOT / "docs" / "gpui-architecture.md",
    ]
    missing = [path for path in [executable, *required] if not path.is_file()]
    if missing:
        names = ", ".join(str(path) for path in missing)
        raise FileNotFoundError(f"Required packaging input missing: {names}")

    return [
        (executable, ARCHIVE_ROOT / executable.name),
        (ROOT / "presets.json", ARCHIVE_ROOT / "resources" / "presets.json"),
        (ROOT / "package.json", ARCHIVE_ROOT / "resources" / "package.json"),
        (ROOT / "PatchOpsIII.ico", ARCHIVE_ROOT / "resources" / "PatchOpsIII.ico"),
        (
            ROOT / "website" / "assets" / "img" / "icon-512.png",
            ARCHIVE_ROOT / "resources" / "icon-512.png",
        ),
        (ROOT / "LICENSE", ARCHIVE_ROOT / "LICENSE"),
        (
            ROOT / "native" / "gpui" / "assets" / "icons" / "LICENSE-lucide.txt",
            ARCHIVE_ROOT / "THIRD-PARTY-NOTICES" / "Lucide.txt",
        ),
        (ROOT / "README.md", ARCHIVE_ROOT / "README.md"),
        (ROOT / "native" / "gpui" / "README.md", ARCHIVE_ROOT / "NATIVE-README.md"),
        (
            ROOT / "docs" / "gpui-architecture.md",
            ARCHIVE_ROOT / "docs" / "native-desktop.md",
        ),
    ]


def create_archive(executable: Path, output: Path) -> Path:
    output.parent.mkdir(parents=True, exist_ok=True)
    with ZipFile(output, "w", ZIP_DEFLATED) as archive:
        for source, destination in archive_manifest(executable):
            archive.write(source, destination.as_posix())
    return output


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--executable", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        output = create_archive(args.executable, args.output)
    except FileNotFoundError as error:
        parser.error(str(error))
    print(output)


if __name__ == "__main__":
    main()
