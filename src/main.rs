mod backend;
mod logging;
mod settings;
mod steam;

use std::path::{Path, PathBuf};

use backend::dxvk;
use backend::t7patch::{
    check_t7_patch_status, install_t7_patch, uninstall_t7_patch, update_t7patch_conf,
};
use iced::executor;
use iced::theme::{self, Palette};
use iced::widget::{Space, button, column, container, row, scrollable, text, text_input, toggler};
use iced::{
    Alignment, Application, Color, Command, Element, Length, Settings, Subscription, Theme, window,
};
use logging::{LogCategory, LogEntry, init_global_logger, log};
use settings::{AppSettings, default_application_dir};

fn main() -> iced::Result {
    let mut settings = Settings::default();
    settings.window = window::Settings {
        size: (1180, 720),
        min_size: Some((1024, 600)),
        ..Default::default()
    };
    PatchOpsApp::run(settings)
}

struct PatchOpsApp {
    app_dir: PathBuf,
    mod_dir: PathBuf,
    settings: AppSettings,
    game_dir_input: String,
    gamertag_input: String,
    password_input: String,
    friends_only: bool,
    log_entries: Vec<LogEntry>,
    log_receiver: flume::Receiver<LogEntry>,
    busy: bool,
    t7_status: String,
    dxvk_installed: bool,
}

#[derive(Debug, Clone)]
enum Message {
    GameDirChanged(String),
    BrowseGameDir,
    GameDirSelected(Option<PathBuf>),
    SaveGameDir,
    GamertagChanged(String),
    PasswordChanged(String),
    FriendsOnlyToggled(bool),
    InstallPatch,
    PatchInstalled(Result<(), String>),
    UninstallPatch,
    PatchUninstalled(Result<(), String>),
    UpdateGamertag,
    GamertagUpdated(Result<(), String>),
    UpdatePassword,
    PasswordUpdated(Result<(), String>),
    FriendsOnlyUpdated(Result<(), String>),
    InstallDxvk,
    DxvkInstalled(Result<(), String>),
    UninstallDxvk,
    DxvkUninstalled(Result<(), String>),
    LogReceived(LogEntry),
}

impl Application for PatchOpsApp {
    type Executor = executor::Default;
    type Message = Message;
    type Theme = Theme;
    type Flags = ();

    fn new(_flags: Self::Flags) -> (Self, Command<Self::Message>) {
        let app_dir = default_application_dir().expect("Unable to determine application directory");
        let mod_dir = app_dir.join("BO3 Mod Files");
        std::fs::create_dir_all(&mod_dir).ok();

        init_global_logger(app_dir.join("PatchOpsIII.log")).expect("Failed to initialise logger");
        let (sender, receiver) = flume::unbounded();
        logging::set_channel(sender);

        let settings = AppSettings::load(&app_dir).unwrap_or_default();
        let game_dir_input = settings
            .game_directory
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(default_game_path);

        let mut app = PatchOpsApp {
            app_dir,
            mod_dir,
            settings,
            game_dir_input,
            gamertag_input: String::new(),
            password_input: String::new(),
            friends_only: false,
            log_entries: Vec::new(),
            log_receiver: receiver,
            busy: false,
            t7_status: String::new(),
            dxvk_installed: false,
        };
        app.refresh_state();
        (app, Command::none())
    }

    fn title(&self) -> String {
        "PatchOpsIII (Rust Edition)".into()
    }

    fn update(&mut self, message: Self::Message) -> Command<Self::Message> {
        match message {
            Message::GameDirChanged(value) => {
                self.game_dir_input = value;
                Command::none()
            }
            Message::BrowseGameDir => {
                Command::perform(select_directory(), Message::GameDirSelected)
            }
            Message::GameDirSelected(selection) => {
                if let Some(path) = selection {
                    self.game_dir_input = path.to_string_lossy().to_string();
                }
                Command::none()
            }
            Message::SaveGameDir => {
                self.settings.game_directory = Some(PathBuf::from(&self.game_dir_input));
                if let Err(err) = self.settings.save(&self.app_dir) {
                    log(
                        LogCategory::Error,
                        format!("Failed to save game directory: {err}"),
                    );
                } else {
                    log(LogCategory::Success, "Saved game directory");
                }
                self.refresh_state();
                Command::none()
            }
            Message::GamertagChanged(value) => {
                self.gamertag_input = value;
                Command::none()
            }
            Message::PasswordChanged(value) => {
                self.password_input = value;
                Command::none()
            }
            Message::FriendsOnlyToggled(value) => {
                self.friends_only = value;
                if self.busy {
                    Command::none()
                } else {
                    self.busy = true;
                    let game_dir = PathBuf::from(self.game_dir_input.clone());
                    Command::perform(
                        async move {
                            update_t7patch_conf(&game_dir, None, None, Some(value))
                                .map_err(|e| e.to_string())
                        },
                        Message::FriendsOnlyUpdated,
                    )
                }
            }
            Message::InstallPatch => {
                if self.busy {
                    return Command::none();
                }
                self.busy = true;
                let game_dir = PathBuf::from(self.game_dir_input.clone());
                let mod_dir = self.mod_dir.clone();
                Command::perform(
                    async move { install_t7_patch(&game_dir, &mod_dir).map_err(|e| e.to_string()) },
                    Message::PatchInstalled,
                )
            }
            Message::PatchInstalled(result) => {
                self.busy = false;
                report_result(result, "Installed T7 Patch");
                self.refresh_state();
                Command::none()
            }
            Message::UninstallPatch => {
                if self.busy {
                    return Command::none();
                }
                self.busy = true;
                let game_dir = PathBuf::from(self.game_dir_input.clone());
                let mod_dir = self.mod_dir.clone();
                Command::perform(
                    async move { uninstall_t7_patch(&game_dir, &mod_dir).map_err(|e| e.to_string()) },
                    Message::PatchUninstalled,
                )
            }
            Message::PatchUninstalled(result) => {
                self.busy = false;
                report_result(result, "Uninstalled T7 Patch");
                self.refresh_state();
                Command::none()
            }
            Message::UpdateGamertag => {
                if self.busy {
                    return Command::none();
                }
                self.busy = true;
                let game_dir = PathBuf::from(self.game_dir_input.clone());
                let name = self.gamertag_input.clone();
                Command::perform(
                    async move {
                        update_t7patch_conf(&game_dir, Some(&name), None, None)
                            .map_err(|e| e.to_string())
                    },
                    Message::GamertagUpdated,
                )
            }
            Message::GamertagUpdated(result) => {
                self.busy = false;
                report_result(result, "Updated gamertag");
                self.refresh_state();
                Command::none()
            }
            Message::UpdatePassword => {
                if self.busy {
                    return Command::none();
                }
                self.busy = true;
                let game_dir = PathBuf::from(self.game_dir_input.clone());
                let password = self.password_input.clone();
                Command::perform(
                    async move {
                        update_t7patch_conf(&game_dir, None, Some(&password), None)
                            .map_err(|e| e.to_string())
                    },
                    Message::PasswordUpdated,
                )
            }
            Message::PasswordUpdated(result) => {
                self.busy = false;
                report_result(result, "Updated network password");
                self.refresh_state();
                Command::none()
            }
            Message::FriendsOnlyUpdated(result) => {
                self.busy = false;
                report_result(result, "Updated friends-only mode");
                self.refresh_state();
                Command::none()
            }
            Message::InstallDxvk => {
                if self.busy {
                    return Command::none();
                }
                self.busy = true;
                let game_dir = PathBuf::from(self.game_dir_input.clone());
                let mod_dir = self.mod_dir.clone();
                Command::perform(
                    async move { dxvk::install(&game_dir, &mod_dir).map_err(|e| e.to_string()) },
                    Message::DxvkInstalled,
                )
            }
            Message::DxvkInstalled(result) => {
                self.busy = false;
                report_result(result, "Installed DXVK-GPLAsync");
                self.refresh_state();
                Command::none()
            }
            Message::UninstallDxvk => {
                if self.busy {
                    return Command::none();
                }
                self.busy = true;
                let game_dir = PathBuf::from(self.game_dir_input.clone());
                Command::perform(
                    async move { dxvk::uninstall(&game_dir).map_err(|e| e.to_string()) },
                    Message::DxvkUninstalled,
                )
            }
            Message::DxvkUninstalled(result) => {
                self.busy = false;
                report_result(result, "Uninstalled DXVK-GPLAsync");
                self.refresh_state();
                Command::none()
            }
            Message::LogReceived(entry) => {
                self.log_entries.push(entry);
                if self.log_entries.len() > 500 {
                    self.log_entries.drain(0..self.log_entries.len() - 500);
                }
                Command::none()
            }
        }
    }

    fn view(&self) -> Element<Self::Message> {
        let header = row![
            text("PatchOpsIII").size(32),
            iced::widget::Space::with_width(Length::Fill),
            button("Save Game Directory").on_press(Message::SaveGameDir)
        ]
        .align_items(Alignment::Center);

        let game_dir_section = column![
            text("Game Directory").size(24),
            row![
                text_input("Path to Call of Duty Black Ops III", &self.game_dir_input)
                    .on_input(Message::GameDirChanged)
                    .padding(12)
                    .width(Length::Fill),
                button("Browse...").on_press(Message::BrowseGameDir)
            ]
            .spacing(12)
        ]
        .spacing(8)
        .padding(16)
        .style(card_style());

        let t7_section = column![
            text("T7 Patch Management").size(24),
            row![
                button("Install / Update")
                    .on_press(Message::InstallPatch)
                    .style(theme::Button::Primary),
                button("Uninstall")
                    .on_press(Message::UninstallPatch)
                    .style(theme::Button::Destructive)
            ]
            .spacing(12),
            text(format!("Status: {}", self.t7_status)).size(16),
            row![
                text_input("Gamertag", &self.gamertag_input)
                    .on_input(Message::GamertagChanged)
                    .padding(10)
                    .width(Length::Fill),
                button("Update").on_press(Message::UpdateGamertag)
            ]
            .spacing(12),
            row![
                text_input("Network Password", &self.password_input)
                    .on_input(Message::PasswordChanged)
                    .padding(10)
                    .width(Length::Fill),
                button("Update").on_press(Message::UpdatePassword)
            ]
            .spacing(12),
            toggler(
                "Friends Only",
                self.friends_only,
                Message::FriendsOnlyToggled
            )
        ]
        .spacing(12)
        .padding(16)
        .style(card_style());

        let dxvk_section = column![
            text("DXVK-GPLAsync").size(24),
            text(if self.dxvk_installed {
                "DXVK-GPLAsync is installed"
            } else {
                "DXVK-GPLAsync is not installed"
            })
            .size(16),
            if self.dxvk_installed {
                button("Uninstall DXVK-GPLAsync").on_press(Message::UninstallDxvk)
            } else {
                button("Install DXVK-GPLAsync")
                    .on_press(Message::InstallDxvk)
                    .style(theme::Button::Primary)
            }
        ]
        .spacing(12)
        .padding(16)
        .style(card_style());

        let log_entries = self.log_entries.iter().rev().map(|entry| {
            let color = match entry.category {
                LogCategory::Info => Color::from_rgb8(210, 210, 214),
                LogCategory::Success => Color::from_rgb8(102, 187, 106),
                LogCategory::Warning => Color::from_rgb8(255, 193, 7),
                LogCategory::Error => Color::from_rgb8(231, 76, 60),
            };
            text(format!("{} - {}", entry.timestamp, entry.message))
                .style(color)
                .size(16)
        });

        let log_panel = column![
            text("Activity Log").size(24),
            scrollable(column(log_entries).spacing(8))
        ]
        .spacing(12)
        .padding(16)
        .style(card_style());

        let layout = column![
            header,
            row![
                column![game_dir_section, t7_section, dxvk_section]
                    .spacing(16)
                    .width(Length::FillPortion(2)),
                log_panel.width(Length::FillPortion(1))
            ]
            .spacing(16)
        ]
        .spacing(24)
        .padding(24);

        container(layout)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(background_style())
            .into()
    }

    fn theme(&self) -> Theme {
        Theme::custom(theme::Custom::new(Palette {
            background: Color::from_rgb8(16, 17, 21),
            text: Color::from_rgb8(233, 233, 238),
            primary: Color::from_rgb8(231, 76, 60),
            success: Color::from_rgb8(76, 175, 80),
            danger: Color::from_rgb8(192, 57, 43),
        }))
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        iced::subscription::unfold(
            "log-stream",
            self.log_receiver.clone(),
            |receiver| async move {
                match receiver.recv_async().await {
                    Ok(entry) => (Some(Message::LogReceived(entry)), receiver),
                    Err(_) => (None, receiver),
                }
            },
        )
    }
}

impl PatchOpsApp {
    fn refresh_state(&mut self) {
        let path = PathBuf::from(&self.game_dir_input);
        match check_t7_patch_status(&path) {
            Ok(status) => {
                self.t7_status = status
                    .gamertag
                    .clone()
                    .unwrap_or_else(|| "T7 Patch not detected".into());
                if let Some(name) = status.plain_name {
                    self.gamertag_input = name;
                }
                if let Some(password) = status.password {
                    self.password_input = password;
                }
                if let Some(flag) = status.friends_only {
                    self.friends_only = flag;
                }
            }
            Err(_) => {
                self.t7_status = "Unable to read t7patch.conf".into();
            }
        }
        self.dxvk_installed = dxvk::is_installed(&path);
    }
}

fn report_result(result: Result<(), String>, success_message: &str) {
    match result {
        Ok(()) => log(LogCategory::Success, success_message),
        Err(err) => log(LogCategory::Error, err),
    }
}

fn background_style() -> impl Fn(&Theme) -> iced::Background {
    |_theme: &Theme| iced::Background::Color(Color::from_rgb8(18, 18, 22))
}

fn card_style() -> impl iced::widget::container::StyleSheet {
    struct Card;
    impl iced::widget::container::StyleSheet for Card {
        type Style = Theme;
        fn appearance(&self, _style: &Self::Style) -> iced::widget::container::Appearance {
            iced::widget::container::Appearance {
                background: Some(iced::Background::Color(Color::from_rgb8(34, 34, 42))),
                border_radius: 12.0,
                border_width: 0.0,
                text_color: Some(Color::from_rgb8(230, 230, 236)),
                ..Default::default()
            }
        }
    }
    Card
}

fn default_game_path() -> String {
    if cfg!(target_os = "linux") {
        "~/.local/share/Steam/steamapps/common/Call of Duty Black Ops III".into()
    } else {
        "C:/Program Files (x86)/Steam/steamapps/common/Call of Duty Black Ops III".into()
    }
}

fn select_directory() -> impl std::future::Future<Output = Option<PathBuf>> {
    async move { rfd::FileDialog::new().pick_folder() }
}
