#!/usr/bin/env python
import importlib.util
import os
import re
import shutil
import sys
import tarfile
import zipfile

import requests
from urllib.parse import urlsplit

# ---------- DXVK Helper Functions (unchanged) ----------

DXVK_ASYNC_FILES = ["dxgi.dll", "d3d11.dll"]


def _supports_gpl_async_cache(release):
    """dxvk.gplAsyncCache was removed in gplasync 2.7+."""
    tag = (release or {}).get("tag_name") or (release or {}).get("name") or ""
    match = re.search(r"(\d+)\.(\d+)", tag)
    if not match:
        return True
    major = int(match.group(1))
    minor = int(match.group(2))
    return (major, minor) < (2, 7)


def _preset_settings(preset):
    if preset == "none":
        return {
            "enable_async": True,
            "gpl_async_cache": False,
            "num_compiler_threads": 0,
            "max_frame_rate": 0,
            "max_frame_latency": 0,
            "tear_free": "Auto",
            "hud_enabled": False,
        }
    return {
        "enable_async": True,
        "gpl_async_cache": True,
        "num_compiler_threads": 0,
        "max_frame_rate": 0,
        "max_frame_latency": 1,
        "tear_free": "True",
        "hud_enabled": False,
    }


def _build_dxvk_conf(settings, include_gpl_async_cache=True):
    lines = []
    lines.append(f"dxvk.enableAsync={'true' if settings.get('enable_async', True) else 'false'}")
    if include_gpl_async_cache and settings.get("gpl_async_cache", False):
        lines.append("dxvk.gplAsyncCache=true")
    lines.append(f"dxvk.numCompilerThreads={settings.get('num_compiler_threads', 0)}")
    lines.append(f"dxgi.maxFrameRate={settings.get('max_frame_rate', 0)}")
    lines.append(f"dxgi.maxFrameLatency={settings.get('max_frame_latency', 0)}")
    lines.append(f"dxvk.tearFree={settings.get('tear_free', 'Auto')}")
    if settings.get("hud_enabled", False):
        lines.append("dxvk.hud=fps,frametimes,gpuload")
    return "\n".join(lines) + "\n"


def get_latest_release():
    api_url = "https://gitlab.com/api/v4/projects/Ph42oN%2Fdxvk-gplasync/releases"
    r = requests.get(api_url)
    r.raise_for_status()
    releases = r.json()
    if not releases:
        raise RuntimeError("No releases returned from DXVK-GPLAsync API")
    return releases[0]  # Assumes releases are sorted latest first

def get_download_url(release):
    assets = release.get("assets", {})
    links = assets.get("links", [])
    if links:
        # Prefer archives we can extract natively before falling back to anything else
        preferred_order = (".zip", ".tar.xz", ".tar.gz", ".tar.bz2", ".tar.zst", ".tzst")
        for suffix in preferred_order:
            for link in links:
                url = link.get("url", "")
                if url.lower().endswith(suffix):
                    return url
        return links[0]["url"]
    sources = assets.get("sources", [])
    if sources:
        for source in sources:
            if source.get("format") == "zip":
                return source.get("url")
        return sources[0].get("url")
    raise RuntimeError("No downloadable asset found in DXVK-GPLAsync release metadata")


def _load_zstandard():
    if "zstandard" in sys.modules:
        return sys.modules["zstandard"]

    spec = importlib.util.find_spec("zstandard")
    if spec is None:
        raise ModuleNotFoundError(
            "The 'zstandard' package is required to unpack .tar.zst archives."
        )

    module = importlib.util.module_from_spec(spec)
    loader = spec.loader
    if loader is None:
        raise ImportError("Unable to load the 'zstandard' module")
    loader.exec_module(module)
    sys.modules["zstandard"] = module
    return module


def extract_archive(archive_path, extract_dir):
    lower_name = archive_path.lower()
    if lower_name.endswith(".zip"):
        with zipfile.ZipFile(archive_path, "r") as zip_ref:
            zip_ref.extractall(extract_dir)
        return

    if lower_name.endswith((".tar", ".tar.gz", ".tgz", ".tar.bz2", ".tbz2", ".tar.xz", ".txz")):
        with tarfile.open(archive_path, "r:*") as tar:
            tar.extractall(path=extract_dir)
        return

    if lower_name.endswith((".tar.zst", ".tzst")):
        zstandard = _load_zstandard()
        with open(archive_path, "rb") as compressed:
            dctx = zstandard.ZstdDecompressor()
            with dctx.stream_reader(compressed) as reader:
                with tarfile.open(fileobj=reader, mode="r|") as tar:
                    tar.extractall(path=extract_dir)
        return

    # Let shutil attempt to handle any other known formats
    shutil.unpack_archive(archive_path, extract_dir)

def download_file(url, filename):
    print(f"Downloading from {url}")
    with requests.get(url, stream=True) as r:
        r.raise_for_status()
        # Extract filename from URL
        parsed_url = urlsplit(url)
        original_filename = os.path.basename(parsed_url.path)
        
        # Use the original filename if available, otherwise use the provided filename
        if original_filename:
            final_filename = os.path.join(os.path.dirname(filename), original_filename)
        else:
            final_filename = filename

        with open(final_filename, "wb") as f:
            for chunk in r.iter_content(chunk_size=8192):
                if chunk:
                    f.write(chunk)
    print(f"Downloaded file saved as: {final_filename}")
    return final_filename  # Return the modified filename

def is_dxvk_async_installed(game_dir):
    return all(os.path.exists(os.path.join(game_dir, f)) for f in DXVK_ASYNC_FILES)
