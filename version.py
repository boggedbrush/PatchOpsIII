"""Central location for the PatchOpsIII application version."""
from __future__ import annotations

import os

# Baked build version; update this when cutting a release so packaged binaries
# report the correct version even when no environment variables are present.
BUILT_APP_VERSION = "1.2.2"

# Optional override for local testing without changing the baked version.
APP_VERSION: str = os.environ.get("PATCHOPSIII_VERSION_OVERRIDE", BUILT_APP_VERSION)
