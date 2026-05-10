import { existsSync } from "node:fs";
import { spawnSync } from "node:child_process";

const target = process.argv[2];

if (target !== "win" && target !== "linux") {
  console.error("Usage: bun scripts/check-backend-build.ts <win|linux>");
  process.exit(1);
}

const python = target === "win" ? ".venv/Scripts/python.exe" : ".venv/bin/python";

function fail(message: string): never {
  console.error(message);
  process.exit(1);
}

if (!existsSync(python)) {
  fail(`Missing Python virtual environment interpreter: ${python}`);
}

const pyinstaller = spawnSync(python, ["-m", "PyInstaller", "--version"], {
  encoding: "utf-8",
  stdio: "pipe"
});

if (pyinstaller.status !== 0) {
  fail(`PyInstaller is not available in ${python}. Install it with '${python} -m pip install -U pyinstaller'.`);
}

if (target === "linux") {
  const objdump = spawnSync("objdump", ["--version"], {
    encoding: "utf-8",
    stdio: "pipe"
  });
  if (objdump.status !== 0) {
    fail("Missing objdump. Install binutils before building the Linux backend package.");
  }
}
