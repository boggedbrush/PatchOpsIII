const fs = require("fs");
const path = require("path");

function xmlAttr(value) {
  return String(value)
    .replace(/&/g, "&amp;")
    .replace(/"/g, "&quot;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}

exports.default = async function brandMsi(projectFile) {
  const repoRoot = path.resolve(__dirname, "..");
  const bannerPath = path.join(repoRoot, "installer", "patchops-msi-banner.bmp");
  const dialogPath = path.join(repoRoot, "installer", "patchops-msi-dialog.bmp");

  for (const assetPath of [bannerPath, dialogPath]) {
    if (!fs.existsSync(assetPath)) {
      throw new Error(`Missing MSI branding asset: ${assetPath}`);
    }
  }

  const branding = [
    `      <WixVariable Id="WixUIBannerBmp" Value="${xmlAttr(bannerPath)}"/>`,
    `      <WixVariable Id="WixUIDialogBmp" Value="${xmlAttr(dialogPath)}"/>`,
  ].join("\n");

  let project = fs.readFileSync(projectFile, "utf8");

  const updated = project.replace("      <UIRef Id=\"WixUI_InstallDir\"/>", `${branding}\n\n      <UIRef Id="WixUI_InstallDir"/>`);
  if (updated === project && !project.includes('Id="WixUIBannerBmp"')) {
    throw new Error("Could not find WiX UI reference while branding MSI project.");
  }
  project = updated;
  project = forceProgramFilesDefault(project);

  fs.writeFileSync(projectFile, project, "utf8");
};

function forceProgramFilesDefault(project) {
  let updated = project
    .replace('<Property Id="WixAppFolder" Value="WixPerUserFolder"/>', '<Property Id="WixAppFolder" Value="WixPerMachineFolder"/>')
    .replace(/\s*<Property Id="MSIINSTALLPERUSER" Secure="yes" Value="1"\/>\r?\n/, "\n");

  if (!updated.includes('<Property Id="ALLUSERS" Secure="yes" Value="1"/>')) {
    updated = updated.replace(
      '<Property Id="WIXUI_INSTALLDIR" Value="APPLICATIONFOLDER"/>',
      '<Property Id="WIXUI_INSTALLDIR" Value="APPLICATIONFOLDER"/>\n    <Property Id="ALLUSERS" Secure="yes" Value="1"/>'
    );
  }

  return updated;
}
