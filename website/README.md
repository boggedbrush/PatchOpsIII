# PatchOpsIII Website

This directory contains the static website for `https://patchopsiii.com`.

## Local preview

From the repo root:

```bash
python -m http.server -d website 5173
```

Then open `http://localhost:5173`.

## Editing content

- Pages are plain HTML in `website/` (and subfolders like `website/features/`).
- Shared styling: `website/assets/css/site.css`
- Shared JS: `website/assets/js/site.js`
- Release/version data is loaded at runtime from GitHub Releases (stable only) via `website/assets/js/site.js`.
