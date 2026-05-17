import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import path from "node:path";
import process from "node:process";

const root = process.cwd();
const appExe = path.join(root, "target", "release", "patchopsiii-tauri.exe");
const wixSource = path.join(root, "target", "release", "wix", "x64", "main.wxs");
const msiDir = path.join(root, "target", "release", "bundle", "msi");
const banner = path.join(root, "installer", "patchops-msi-banner.bmp");
const dialog = path.join(root, "installer", "patchops-msi-dialog.bmp");

function fail(message: string): never {
  throw new Error(message);
}

function relative(filePath: string) {
  return path.relative(root, filePath) || ".";
}

function requireFile(filePath: string) {
  if (!existsSync(filePath)) {
    fail(`Missing ${relative(filePath)}`);
  }
  const stats = statSync(filePath);
  if (!stats.isFile() || stats.size <= 0) {
    fail(`Invalid file ${relative(filePath)}`);
  }
}

function verifyWindowsGuiSubsystem() {
  requireFile(appExe);
  const file = readFileSync(appExe);
  if (file.readUInt16LE(0) !== 0x5a4d) {
    fail(`${relative(appExe)} is not a PE executable`);
  }

  const peOffset = file.readUInt32LE(0x3c);
  if (file.toString("ascii", peOffset, peOffset + 4) !== "PE\u0000\u0000") {
    fail(`${relative(appExe)} has an invalid PE header`);
  }

  const optionalHeaderOffset = peOffset + 24;
  const subsystem = file.readUInt16LE(optionalHeaderOffset + 0x44);
  if (subsystem !== 2) {
    fail(`${relative(appExe)} uses PE subsystem ${subsystem}; expected 2 (Windows GUI)`);
  }

  console.log(`Verified ${relative(appExe)} uses Windows GUI subsystem`);
}

function verifyWixBranding() {
  requireFile(banner);
  requireFile(dialog);
  requireFile(wixSource);

  const wix = readFileSync(wixSource, "utf-8");
  const expectations = [
    ["WixUIBannerBmp", path.basename(banner)],
    ["WixUIDialogBmp", path.basename(dialog)],
    ['InstallScope="perMachine"', "per-machine install scope"]
  ];

  for (const [needle, label] of expectations) {
    if (!wix.includes(needle)) {
      fail(`${relative(wixSource)} does not include ${label}`);
    }
  }

  console.log(`Verified MSI WiX customizations in ${relative(wixSource)}`);
}

function verifyMsiArtifact() {
  if (!existsSync(msiDir)) {
    fail(`Missing ${relative(msiDir)}`);
  }
  const msis = readdirSync(msiDir).filter((entry) => entry.toLowerCase().endsWith(".msi"));
  if (msis.length !== 1) {
    fail(`Expected exactly one MSI in ${relative(msiDir)}, found ${msis.length}`);
  }
  requireFile(path.join(msiDir, msis[0]));

  console.log(`Verified MSI artifact ${path.join(relative(msiDir), msis[0])}`);
}

verifyWindowsGuiSubsystem();
verifyWixBranding();
verifyMsiArtifact();
