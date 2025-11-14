"""Central location for the PatchOpsIII application version."""
from __future__ import annotations

import os

APP_VERSION: str = os.environ.get("PATCHOPSIII_VERSION", "0.0.0")
"""Current application version string.

The value defaults to ``"0.0.0"`` when running from source, but release builds
should inject the real version using the ``PATCHOPSIII_VERSION`` environment
variable during packaging.
"""
