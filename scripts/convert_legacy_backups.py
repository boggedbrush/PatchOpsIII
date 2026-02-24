#!/usr/bin/env python3
import argparse
import os

PATCHOPS_SUFFIX = ".patchops.bak"
LEGACY_SUFFIX = ".bak"


def iter_legacy_backups(root_dir):
    for current_root, _, files in os.walk(root_dir):
        for name in files:
            if not name.endswith(LEGACY_SUFFIX):
                continue
            if name.endswith(PATCHOPS_SUFFIX):
                continue
            source = os.path.join(current_root, name)
            target = source[:-len(LEGACY_SUFFIX)] + PATCHOPS_SUFFIX
            yield source, target


def convert_backups(root_dir, apply_changes):
    renamed = 0
    skipped = 0

    for source, target in iter_legacy_backups(root_dir):
        if os.path.exists(target):
            print(f"SKIP (target exists): {source} -> {target}")
            skipped += 1
            continue

        if apply_changes:
            os.rename(source, target)
            print(f"RENAMED: {source} -> {target}")
        else:
            print(f"DRY RUN: {source} -> {target}")
        renamed += 1

    return renamed, skipped


def main():
    parser = argparse.ArgumentParser(
        description="Convert legacy *.bak backups to *.patchops.bak backups."
    )
    parser.add_argument(
        "root_dir",
        help="Root directory to scan recursively (typically your BO3 game directory).",
    )
    parser.add_argument(
        "--apply",
        action="store_true",
        help="Apply changes. Without this flag, the script only prints a dry run.",
    )
    args = parser.parse_args()

    root_dir = os.path.abspath(args.root_dir)
    if not os.path.isdir(root_dir):
        raise SystemExit(f"Directory not found: {root_dir}")

    renamed, skipped = convert_backups(root_dir, apply_changes=args.apply)
    mode = "Applied" if args.apply else "Planned"
    print(f"{mode} conversions: {renamed}; skipped: {skipped}")


if __name__ == "__main__":
    main()
