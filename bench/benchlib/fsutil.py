"""Filesystem helpers: du, source copy, chmod."""
from __future__ import annotations

import os
import shutil

COPY_EXCLUDE_DIRS = {".git", "node_modules", ".venv", "dist", "build", "bench", "target", "__pycache__", ".work", ".patchopsiii-dev"}


def du(path: str) -> dict:
    """Disk usage of a file/dir: apparent bytes and allocated bytes, file count (symlinks not followed)."""
    app = alloc = files = 0
    if not os.path.lexists(path):
        return {"exists": False, "apparent_bytes": 0, "disk_bytes": 0, "files": 0}
    if os.path.isfile(path) or os.path.islink(path):
        st = os.lstat(path)
        return {"exists": True, "apparent_bytes": st.st_size, "disk_bytes": st.st_blocks * 512, "files": 1}
    for dp, dns, fns in os.walk(path, followlinks=False):
        for n in fns:
            try:
                st = os.lstat(os.path.join(dp, n))
            except OSError:
                continue
            app += st.st_size
            alloc += st.st_blocks * 512
            files += 1
    return {"exists": True, "apparent_bytes": app, "disk_bytes": alloc, "files": files}


def copy_source(src: str, dst: str, extra_exclude: set[str] | None = None, keep_cargo: bool = True) -> None:
    excl = set(COPY_EXCLUDE_DIRS) | (extra_exclude or set())
    if os.path.exists(dst):
        shutil.rmtree(dst)

    def ignore(d, names):
        top = os.path.abspath(d) == os.path.abspath(src)
        return [n for n in names if n in {".git", "node_modules", "__pycache__", "target"} or (top and n in excl)]

    shutil.copytree(src, dst, ignore=ignore, symlinks=True)


def chmod_readable(path: str):
    for dp, dns, fns in os.walk(path):
        try:
            os.chmod(dp, os.stat(dp).st_mode | 0o555)
        except OSError:
            pass
        for n in fns:
            fp = os.path.join(dp, n)
            try:
                if not os.path.islink(fp):
                    m = os.stat(fp).st_mode
                    os.chmod(fp, m | 0o444 | ((m & 0o111) and 0o111))
            except OSError:
                pass
