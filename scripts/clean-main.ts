import { existsSync, rmSync } from "node:fs";
import path from "node:path";

const outDir = path.resolve("dist", "main");

if (existsSync(outDir)) {
  rmSync(outDir, { recursive: true, force: true });
}
