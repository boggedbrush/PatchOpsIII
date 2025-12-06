# PatchOpsIII v1.1.1 Release Notes

PatchOpsIII v1.1.1 is a small polish release that keeps your launch options intact during T7 Patch updates, adds easier log sharing, and clears old logs automatically.

🔍 **VirusTotal Scans:**
- Windows: [Latest Windows Scan]({{WINDOWS_VT_URL}})
- Linux: [Latest Linux Scan]({{LINUX_VT_URL}})

---

## 🚀 New Features & Improvements
- **Advanced tab footer:** Shows the running PatchOpsIII version and adds a one-click “Copy Logs” button to help with support and bug reports.

---

## 🔄 Changes
- **Log handling:** Logs are cleared automatically after every three successful app launches to keep file sizes lean.
- **Clipboard export:** Copied logs now include a formatted header and fenced code block for cleaner pasting into GitHub issues.

---

## 🛠 Fixes
- **Launch option preservation:** Updating/applying T7 Patch on Linux no longer overwrites existing Steam launch options (your custom `fs_game`/mod settings remain).
- **Log copy resilience:** Added fallbacks and warnings if the log file or clipboard is unavailable when exporting logs.

---

## ⚠️ Known Issues
- **All-around Enhancement Mod:** Current version doesn’t work with launch options (`Lite` version works fine).
- **Launch Options Stability:** May not work for all Linux distributions—still under testing.

---

## 📥 Download the Latest Release
- [Download PatchOpsIII v1.1.1 for Windows]({{WINDOWS_DOWNLOAD_URL}})
- [Download PatchOpsIII v1.1.1 for Linux & Steam Deck]({{LINUX_DOWNLOAD_URL}})

---

## 🏗 Build Metadata
- Windows SHA256: `{{WINDOWS_SHA256}}`
- Linux SHA256: `{{LINUX_SHA256}}`

---

## 🧑‍💻 Acknowledgements
PatchOpsIII is built upon the work of these amazing projects:
- **t7patch:** [t7patch on GitHub](https://github.com/shiversoftdev/t7patch)
- **dxvk-gplasync:** [dxvk-gplasync on GitLab](https://gitlab.com/Ph42oN/dxvk-gplasync)
- **ValvePython/vdf:** [ValvePython/vdf on GitHub](https://github.com/ValvePython/vdf)

---

## 🔮 What's Next?
- **Bug Fixes & Optimizations:** Continuing improvements based on user feedback.
- **[BO3 Enhanced](https://github.com/shiversoftdev/BO3Enhanced) installation assistant:** Assist Windows users in automating installs from their Microsoft Store dump.
- **Community Feedback:** Your feedback is invaluable! Report issues, suggest improvements, and propose new features.

---

Thank you for supporting **PatchOpsIII**. Happy modding! 🎮
