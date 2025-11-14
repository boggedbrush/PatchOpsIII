"""Central location for the PatchOpsIII application version."""
from __future__ import annotations

import os

APP_VERSION: str = os.environ.get("PATCHOPSIII_VERSION", "1.1.0")
