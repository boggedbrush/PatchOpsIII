import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  lstatSync,
  readFileSync,
  readdirSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { basename, relative, resolve } from "node:path";

const appDir = process.argv[2] ? resolve(process.argv[2]) : undefined;
const nativeBinary = process.argv[3] ? resolve(process.argv[3]) : undefined;
const appImage = process.argv[4] ? resolve(process.argv[4]) : undefined;
const outputPath = process.argv[5] ? resolve(process.argv[5]) : undefined;

if (!appDir || !nativeBinary || !appImage) {
  throw new Error(
    "usage: node benchmarks/inspect-package.mjs <AppDir> <native-binary> <AppImage>",
  );
}

function walk(root) {
  const entries = [];
  for (const name of readdirSync(root)) {
    const path = resolve(root, name);
    const metadata = lstatSync(path);
    entries.push({ path, metadata });
    if (metadata.isDirectory()) entries.push(...walk(path));
  }
  return entries;
}

function sha256(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

function isElf(path, metadata) {
  if (!metadata.isFile() || metadata.size < 4) return false;
  return readFileSync(path).subarray(0, 4).equals(Buffer.from([0x7f, 0x45, 0x4c, 0x46]));
}

function dynamicMetadata(path) {
  const result = spawnSync("readelf", ["-d", path], { encoding: "utf8" });
  if (result.status !== 0) {
    throw new Error(`readelf failed for ${path}: ${result.stderr}`);
  }
  return {
    needed: [...result.stdout.matchAll(/Shared library: \[([^\]]+)\]/g)].map(
      (match) => match[1],
    ),
    soname: result.stdout.match(/Library soname: \[([^\]]+)\]/)?.[1] ?? null,
  };
}

function duBytes(path) {
  const result = spawnSync("du", ["-sb", path], { encoding: "utf8" });
  if (result.status !== 0) throw new Error(`du failed for ${path}: ${result.stderr}`);
  return Number(result.stdout.trim().split(/\s+/, 1)[0]);
}

const entries = walk(appDir);
const files = entries.filter(({ metadata }) => metadata.isFile());
const elfFiles = files.filter(({ path, metadata }) => isElf(path, metadata));
const elf = new Map(
  elfFiles.map(({ path, metadata }) => [
    relative(appDir, path),
    { bytes: metadata.size, ...dynamicMetadata(path) },
  ]),
);

const providers = new Map();
for (const [path, metadata] of elf) {
  for (const name of new Set([basename(path), metadata.soname].filter(Boolean))) {
    const paths = providers.get(name) ?? [];
    paths.push(path);
    providers.set(name, paths);
  }
}

const requiredBy = new Map([...elf.keys()].map((path) => [path, []]));
const unresolved = new Map();
for (const [dependent, metadata] of elf) {
  for (const needed of metadata.needed) {
    const bundledProviders = providers.get(needed) ?? [];
    if (bundledProviders.length === 0) {
      const dependents = unresolved.get(needed) ?? [];
      dependents.push(dependent);
      unresolved.set(needed, dependents);
      continue;
    }
    for (const provider of bundledProviders) requiredBy.get(provider).push(dependent);
  }
}

const duplicateHashes = new Map();
for (const { path, metadata } of files) {
  const digest = sha256(path);
  const matches = duplicateHashes.get(digest) ?? [];
  matches.push({ path: relative(appDir, path), bytes: metadata.size });
  duplicateHashes.set(digest, matches);
}

const output = {
  appDir: {
    path: appDir,
    bytes: duBytes(appDir),
    usrBinBytes: duBytes(resolve(appDir, "usr/bin")),
    usrLibBytes: duBytes(resolve(appDir, "usr/lib")),
  },
  nativeBinary: {
    path: nativeBinary,
    bytes: statSync(nativeBinary).size,
    sha256: sha256(nativeBinary),
  },
  appImage: {
    path: appImage,
    bytes: statSync(appImage).size,
    sha256: sha256(appImage),
  },
  largestFiles: files
    .map(({ path, metadata }) => ({
      path: relative(appDir, path),
      bytes: metadata.size,
      links: metadata.nlink,
      deviceAndInode: `${metadata.dev}:${metadata.ino}`,
    }))
    .sort((left, right) => right.bytes - left.bytes || left.path.localeCompare(right.path))
    .slice(0, 30),
  duplicateRealFiles: [...duplicateHashes.entries()]
    .filter(([, matches]) => matches.length > 1 && matches[0].bytes > 0)
    .map(([digest, matches]) => ({
      sha256: digest,
      matches: matches.sort((left, right) => left.path.localeCompare(right.path)),
    }))
    .sort(
      (left, right) =>
        right.matches[0].bytes * (right.matches.length - 1) -
          left.matches[0].bytes * (left.matches.length - 1) ||
        left.sha256.localeCompare(right.sha256),
    ),
  elfClosure: [...elf.entries()]
    .map(([path, metadata]) => ({
      path,
      bytes: metadata.bytes,
      soname: metadata.soname,
      needed: metadata.needed,
      requiredBy: [...new Set(requiredBy.get(path))].sort(),
    }))
    .sort((left, right) => left.path.localeCompare(right.path)),
  unresolvedDynamicLibraries: [...unresolved.entries()]
    .map(([name, dependents]) => ({ name, requiredBy: [...new Set(dependents)].sort() }))
    .sort((left, right) => left.name.localeCompare(right.name)),
  limitations: [
    "DT_NEEDED proves static ELF edges only; empty requiredBy lists may be runtime-loaded plugins or helpers and are not evidence that deletion is safe.",
    "Duplicate hashes identify byte-identical regular files; replacing SONAME aliases with symlinks still requires package portability testing.",
  ],
};

const serialized = `${JSON.stringify(output, null, 2)}\n`;
if (outputPath) {
  writeFileSync(outputPath, serialized);
  console.log(`Wrote package inspection to ${outputPath}`);
} else {
  process.stdout.write(serialized);
}
