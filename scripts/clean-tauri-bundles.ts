import { existsSync, rmSync } from "node:fs";
import path from "node:path";
import process from "node:process";

const root = process.cwd();
const bundleDir = path.resolve(root, "target", "release", "bundle");
const releaseTarget = path.resolve(root, "target", "release");

if (!bundleDir.startsWith(`${releaseTarget}${path.sep}`)) {
  throw new Error(`Refusing to clean unexpected Tauri bundle path: ${bundleDir}`);
}

if (existsSync(bundleDir)) {
  rmSync(bundleDir, { recursive: true, force: true });
  console.log(`Removed ${path.relative(root, bundleDir)}`);
}
