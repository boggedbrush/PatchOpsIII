pub mod commands;
pub mod config_reader;
pub mod error;
pub mod file_integrity;
pub mod game_scan;
pub mod steam_detect;

pub use commands::handle_command;
