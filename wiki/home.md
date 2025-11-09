# PatchOpsIII Wiki  

## Overview  
PatchOpsIII is a Python-based application developed by [boggedbrush](https://github.com/boggedbrush/PatchOpsIII). The project is designed to streamline and optimize operations through a robust and versatile framework. The application features a tabbed interface (`Mods`, `Graphics`, & `Advanced`) with both dark and light mode support. The application is packaged using Nuitka to support both Linux and Windows environments.

![Program Screenshot](https://github.com/user-attachments/assets/a79e7273-4274-4a43-8d4d-e81a12cbd1ff)

---

## Features  

### 1. Mods Tab

#### 1.1 Game Directory  
Allows users to specify their game directory. The default behavior is to search for:  

C:\Program Files (x86)\Steam\steamapps\common\Call of Duty Black Ops III

If the directory isn't found, users can manually select the game's installation folder using the "Browse" button at the top right.  

#### 1.2 T7Patch Management  
The **T7 Patch** is a community-developed modification for *Call of Duty: Black Ops III* that addresses various security vulnerabilities and performance issues within the game.  

**Key Benefits:**  
- Fixes remote code execution (RCE) vulnerabilities  
- Prevents potential exploits  
- Resolves FPS-related problems  

PatchOpsIII enables users to:  
- Install and update the T7 Patch  
- Update their gamertag  
- Set their gamertag color
- Configure network password
- Toggle Friends Only Mode
- Install LPC to resolve A.B.C errors
- Uninstall T7Patch when needed

This management only needs to run once and does not require `t7patch.exe` to remain open in the background. Implementing the T7 Patch is crucial for maintaining game security and performance, as it safeguards against known exploits and enhances overall stability.  

You can learn more about the T7 Patch [here](https://github.com/shiversoftdev/t7patch).  

#### 1.3 DXVK-GPLAsync Management  
**Shader compilation stuttering** is a common issue in PC gaming, causing noticeable delays when new shaders are compiled during gameplay. DXVK-GPLAsync offers a solution by converting **DirectX** calls to **Vulkan** with asynchronous shader compilation, reducing stutters and enhancing overall performance.  

**Feature Highlights:**  
- Install/uninstall `dxvk-gplasync`  
- Minimize in-game stuttering caused by real-time shader compilation  
- Enhance performance and reduce latency, especially for stutter-prone games  

Learn more about [DXVK](https://www.pcgamingwiki.com/wiki/DXVK), [DXVK-GPLAsync](https://gitlab.com/Ph42oN/dxvk-gplasync), and [shader stutter](https://youtu.be/f7yml1y3fDE?si=NpwybZNqIRVhxmL7).  

#### 1.4 Quality of Life Features
- **Skip All Intro Videos:** Bypass all game intro videos
- **Launch Options:** Support for various mod configurations:
  - Play Offline
  - All-around Enhancement Lite
  - Ultimate Experience Mod
- **Clickable Help:** Access detailed information and download links for Steam Workshop mods

### 2. Graphics Tab

#### 2.1 Graphic Presets  
Allows users to apply graphics presets from pre-configured JSON files for quick and easy configuration.  

#### 2.2 Basic Settings  
- **Skip Intro Video:** Renames `BO3_Global_Logo_LogoSequence.mkv` to `.bak`, skipping the intro cutscene. [Skip intro videos](https://www.pcgamingwiki.com/wiki/Call_of_Duty:_Black_Ops_III#Skip_intro_videos)  
- **FPS Limiter:** Set the FPS limiter from **0-1000** (previously **24-1000**). Setting to **0** can improve loading speed. [Increased loading speed](https://www.pcgamingwiki.com/wiki/Call_of_Duty:_Black_Ops_III#Increased_loading_speed_levels)  
- **Convenience Settings:** Adjust:  
  - Field of View (FOV)  
  - Display Mode  
  - Resolution  
  - Refresh Rate  
  - Render Resolution %  
  - Enable V-Sync  
  - Show FPS Counter  

### 3. Advanced Tab

#### 3.1 Advanced Settings  
- **Smooth Framerate:** Changes `SmoothFramerate` from **0** to **1** in `config.ini`. [Frame rate isn't smooth](https://www.pcgamingwiki.com/wiki/Call_of_Duty:_Black_Ops_III#Frame_rate_isn.27t_smooth)  
- **Use Full VRAM:** Sets `VideoMemory` to **1** and `StreamMinResident` to **0** in `config.ini`. [Use full VRAM](https://www.pcgamingwiki.com/wiki/Call_of_Duty:_Black_Ops_III#Game_does_not_take_advantage_of_the_entire_VRAM_amount_available)  
- **Lower Latency:** Modifies `MaxFrameLatency` in `config.ini` to allow between **0 (System Level)** and **4** queued frames. [Improve responsiveness](https://www.pcgamingwiki.com/wiki/Call_of_Duty:_Black_Ops_III#Game_isn.27t_responsive_enough)  
- **Reduce CPU Usage:** Toggles `SerializeRender` from **0** to **2**, recommended for older/weaker CPUs. [High CPU usage](https://www.pcgamingwiki.com/wiki/Call_of_Duty:_Black_Ops_III#CPU_usage_sometimes_goes_too_high_on_some_configurations)  
- **Reduce Stuttering:** Renames `d3dcompiler_46.dll` to `.bak` to enforce the latest DirectX11 version. [Stuttering issues](https://www.pcgamingwiki.com/wiki/Call_of_Duty:_Black_Ops_III#Stuttering)  
- **Unlock All Graphics Options:** Sets `RestrictGraphicsOptions` from **1** to **0** in `config.ini`. [Unlock settings](https://www.pcgamingwiki.com/wiki/Call_of_Duty:_Black_Ops_III#Make_all_settings_available)  
- **Lock `config.ini` (read-only):** Prevents unintended changes by setting `config.ini` to **read-only**.  

### 4. Terminal/Log Window  
Displays logs in a terminal view, providing transparency about what operations succeeded or failed.  

- **Log Creation:** On startup, a `PatchOpsIII.log` file is generated with full session logs.  
- **Troubleshooting:** Users can easily identify issues through detailed log messages.  

### Known Issues
- All-around Enhancement Mod (full version) may have compatibility issues with launch options
- Launch options stability may vary between users
- Some features are still under testing