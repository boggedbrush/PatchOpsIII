use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub category: String,
    pub message: String,
    pub line: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LaunchProfile {
    pub id: String,
    pub label: String,
    pub option: String,
    pub active: bool,
    pub installed: bool,
    pub subscribed: bool,
    pub state: String,
    pub path: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnhancedState {
    pub installed: bool,
    pub detected_at: Option<String>,
    pub acknowledged_at: Option<String>,
    pub launch_options_active: bool,
    pub dump_source: String,
    pub files_installed: usize,
    pub backup_status: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExeSwapState {
    pub profile: String,
    pub mode_label: String,
    pub patch_label: String,
    pub display_label: String,
    pub state: String,
    pub active_build_id: String,
    pub active_build_date: String,
    pub current_build_id: String,
    pub current_build_date: String,
    pub compatible_build_id: String,
    pub compatible_build_date: String,
    pub enhanced_build_id: String,
    pub enhanced_build_date: String,
    pub executable: String,
    pub executable_name: String,
    pub executable_hash: String,
    pub trusted_executable: bool,
    pub integrity_status: String,
    pub integrity_message: String,
    pub backup_available: bool,
    pub latest_available: bool,
    pub compatible_available: bool,
    pub enhanced_available: bool,
    pub compatible_active: bool,
    pub enhanced_exe_active: bool,
    pub enhanced_active: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct T7State {
    pub installed: bool,
    pub conf_exists: bool,
    pub gamertag: String,
    pub plain_name: String,
    pub color_code: String,
    pub network_password: String,
    pub friends_only: bool,
    pub mode: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DxvkSettings {
    pub enable_async: bool,
    pub gpl_async_cache: bool,
    pub num_compiler_threads: i32,
    pub max_frame_rate: i32,
    pub max_frame_latency: i32,
    pub tear_free: String,
    pub hud_enabled: bool,
}

impl Default for DxvkSettings {
    fn default() -> Self {
        Self {
            enable_async: true,
            gpl_async_cache: true,
            num_compiler_threads: 0,
            max_frame_rate: 0,
            max_frame_latency: 1,
            tear_free: "True".into(),
            hud_enabled: false,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DxvkState {
    pub installed: bool,
    pub conf_exists: bool,
    pub settings: DxvkSettings,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QolState {
    pub d3dcompiler: bool,
    pub intro: bool,
    pub all_intros: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GraphicsState {
    pub max_fps: i32,
    pub fov: i32,
    pub display_mode: i32,
    pub resolution: String,
    pub refresh_rate: f64,
    pub render_resolution: i32,
    pub vsync: bool,
    pub draw_fps: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AdvancedState {
    pub smooth_framerate: bool,
    pub unlock_options: bool,
    pub reduce_cpu: bool,
    pub max_frame_latency: i32,
    pub vram_limited: bool,
    pub vram_target: i32,
    pub config_readonly: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MaintenanceState {
    pub mod_files_dir: String,
    pub log_payload: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModsState {
    pub t7_patch: bool,
    pub dxvk: bool,
    pub enhanced: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PatchOpsState {
    pub app_version: String,
    pub platform: String,
    pub game_dir: Option<String>,
    pub game_detected: bool,
    pub config_exists: bool,
    pub steam_user_id: Option<String>,
    pub log_path: String,
    pub presets: Vec<String>,
    pub current_launch_options: Option<String>,
    pub active_launch_profile: String,
    pub release_channel: String,
    pub launch_profiles: Vec<LaunchProfile>,
    pub enhanced: EnhancedState,
    pub exe_swap: ExeSwapState,
    pub t7: T7State,
    pub dxvk: DxvkState,
    pub qol: QolState,
    pub graphics: GraphicsState,
    pub advanced: AdvancedState,
    pub maintenance: MaintenanceState,
    pub mods: ModsState,
    pub logs: Vec<LogEntry>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompatibleExeResult {
    pub state: PatchOpsState,
    pub depot_command: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DepotStatus {
    pub available: bool,
    pub state: PatchOpsState,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnhancedValidation {
    pub valid: bool,
    pub message: String,
    pub state: PatchOpsState,
}
