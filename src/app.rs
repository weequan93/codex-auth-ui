use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

use crossbeam_channel::{unbounded, Receiver};
use eframe::egui::{
    self, Align, Align2, Color32, FontId, Frame, Layout, Margin, RichText, Sense, Stroke, Vec2,
    ViewportCommand,
};
use sysinfo::Pid;
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
};

use crate::{
    countdown::{last_checked_label, reset_display, ResetDisplay},
    icon,
    model::{AccountRecord, RateLimitWindow, Registry},
    prefs::{self, Preferences},
    storage,
    worker::{RefreshFailureKind, WorkerCommand, WorkerEvent, WorkerHandle},
};

mod navigation;
#[cfg(any(test, feature = "visual-qa"))]
mod preview;
use navigation::AccountFilter;

pub const WINDOW_SIZE: [f32; 2] = [400.0, 600.0];
#[cfg(any(test, not(target_os = "macos")))]
const WINDOW_WIDTH: f32 = WINDOW_SIZE[0];
#[cfg(any(test, not(target_os = "macos")))]
const WINDOW_HEIGHT: f32 = WINDOW_SIZE[1];
const CANVAS: Color32 = Color32::from_rgb(246, 247, 251);
const SURFACE: Color32 = Color32::from_rgb(255, 255, 255);
const INK: Color32 = Color32::from_rgb(27, 31, 42);
const MUTED: Color32 = Color32::from_rgb(105, 113, 132);
const SUBTLE: Color32 = Color32::from_rgb(113, 121, 139);
const BORDER: Color32 = Color32::from_rgb(225, 228, 236);
const ACCENT: Color32 = Color32::from_rgb(76, 92, 230);
const ACCENT_SOFT: Color32 = Color32::from_rgb(239, 241, 255);
const SUCCESS: Color32 = Color32::from_rgb(24, 145, 102);
const SUCCESS_SOFT: Color32 = Color32::from_rgb(232, 248, 240);
const WARNING: Color32 = Color32::from_rgb(190, 111, 29);
const WARNING_SOFT: Color32 = Color32::from_rgb(255, 247, 232);
const DANGER: Color32 = Color32::from_rgb(190, 63, 63);
const DANGER_SOFT: Color32 = Color32::from_rgb(255, 239, 239);
#[cfg(not(target_os = "macos"))]
pub const PARKED_POSITION: [f32; 2] = [-10_000.0, -10_000.0];

#[cfg(target_os = "macos")]
extern "C" {
    #[cfg(not(test))]
    fn cah_hide_main_window();
    #[cfg(not(test))]
    fn cah_show_main_window();
    fn cah_toggle_main_window() -> bool;
}

#[cfg(target_os = "macos")]
fn hide_native_window() {
    #[cfg(not(test))]
    unsafe {
        cah_hide_main_window();
    }
}

#[cfg(target_os = "macos")]
fn show_native_window() {
    #[cfg(not(test))]
    unsafe {
        cah_show_main_window();
    }
}

#[derive(Debug, Clone)]
enum AppEvent {
    #[cfg(not(target_os = "macos"))]
    Toggle {
        x: f64,
        y: f64,
    },
    #[cfg(target_os = "macos")]
    VisibilityChanged(bool),
    Menu(String),
}

// On macOS an ordered-out window may not redraw. Handle native visibility inside
// the tray callback, then report the result instead of waiting for egui to toggle it.
// Share these handlers with the synthetic native tray preview.
fn install_tray_handlers(context: &egui::Context, events: crossbeam_channel::Sender<AppEvent>) {
    let repaint = context.clone();
    let clicks = events.clone();
    TrayIconEvent::set_event_handler(Some(move |event| {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            position,
            ..
        } = event
        {
            #[cfg(target_os = "macos")]
            let event = {
                let _ = position;
                AppEvent::VisibilityChanged(unsafe { cah_toggle_main_window() })
            };
            #[cfg(not(target_os = "macos"))]
            let event = AppEvent::Toggle {
                x: position.x,
                y: position.y,
            };
            let _ = clicks.send(event);
            repaint.request_repaint();
        }
    }));
    let repaint = context.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        #[cfg(target_os = "macos")]
        if matches!(event.id.0.as_str(), "show" | "add" | "device") {
            show_native_window();
        }
        let _ = events.send(AppEvent::Menu(event.id.0));
        repaint.request_repaint();
    }));
}

#[derive(Debug, Clone)]
struct RefreshFailure {
    kind: RefreshFailureKind,
    message: String,
}

#[derive(Debug, Clone)]
struct SwitchGuard {
    account_key: String,
    pids: Vec<Pid>,
}

#[derive(Debug, Clone, PartialEq)]
enum SessionNotice {
    Selected,
    Restarting,
    RelaunchRequested,
    RestartFailed(String),
}

impl SessionNotice {
    fn presentation(&self) -> (&str, &str, Color32, Color32) {
        match self {
            Self::Selected => (
                "Credentials selected",
                "If Codex was already open, reopen it to apply this selection. Already restarted? You can dismiss this notice.",
                ACCENT_SOFT, ACCENT,
            ),
            Self::Restarting => (
                "Restart in progress",
                "Waiting for the desktop app to quit normally. Finish any prompts in that app. CLI sessions are not restarted.",
                ACCENT_SOFT, ACCENT,
            ),
            Self::RelaunchRequested => (
                "Desktop relaunch requested",
                "The desktop app closed, the selected credentials were saved, and macOS accepted the reopen request. Check the account in Codex before continuing.",
                SUCCESS_SOFT, SUCCESS,
            ),
            Self::RestartFailed(message) => ("Restart not completed", message, WARNING_SOFT, WARNING),
        }
    }
}

pub struct AccountHubApp {
    registry: Registry,
    search: String,
    account_filter: AccountFilter,
    scroll_to_account: Option<String>,
    worker: WorkerHandle,
    app_events: Receiver<AppEvent>,
    tray: Option<TrayIcon>,
    visible: bool,
    busy: HashSet<String>,
    errors: HashMap<String, RefreshFailure>,
    preferences: Preferences,
    disclosure_for: Option<String>,
    switch_guard: Option<SwitchGuard>,
    remove_confirm: Option<String>,
    editing_alias: Option<(String, String)>,
    login_active: bool,
    login_message: String,
    banner_error: Option<String>,
    session_notice: Option<SessionNotice>,
    restart_confirm: bool,
    restarting: bool,
    #[cfg(not(target_os = "macos"))]
    last_position: Option<egui::Pos2>,
    #[cfg(any(test, feature = "visual-qa"))]
    preview_mode: bool,
}

impl AccountHubApp {
    pub fn new(context: &egui::Context) -> Self {
        configure_style(context);
        let worker = WorkerHandle::start(context.clone());
        let (app_tx, app_rx) = unbounded();

        install_tray_handlers(context, app_tx);

        #[cfg(feature = "visual-qa")]
        let initial_registry = if std::env::var_os("EFRAME_SCREENSHOT_TO").is_some() {
            storage::codex_home()
                .and_then(|home| storage::load_registry(&home))
                .unwrap_or_default()
        } else {
            Registry::default()
        };
        #[cfg(not(feature = "visual-qa"))]
        let initial_registry = Registry::default();
        let (tray, tray_error) = match build_tray(&initial_registry) {
            Ok(tray) => (Some(tray), None),
            Err(error) => (
                None,
                Some(format!("Could not create the menu-bar icon: {error}")),
            ),
        };
        let preferences = prefs::load().unwrap_or_default();
        worker.send(WorkerCommand::Reload);
        let visible = crate::startup::start_visible() || tray.is_none();
        let app = Self {
            registry: initial_registry,
            search: String::new(),
            account_filter: AccountFilter::All,
            scroll_to_account: None,
            worker,
            app_events: app_rx,
            tray,
            visible,
            busy: HashSet::new(),
            errors: HashMap::new(),
            preferences,
            disclosure_for: None,
            switch_guard: None,
            remove_confirm: None,
            editing_alias: None,
            login_active: false,
            login_message: String::new(),
            banner_error: tray_error,
            session_notice: None,
            restart_confirm: false,
            restarting: false,
            #[cfg(not(target_os = "macos"))]
            last_position: None,
            #[cfg(any(test, feature = "visual-qa"))]
            preview_mode: false,
        };

        #[cfg(target_os = "macos")]
        if !app.visible {
            hide_native_window();
        }

        app
    }

    fn process_events(&mut self, context: &egui::Context) {
        while let Ok(event) = self.app_events.try_recv() {
            match event {
                #[cfg(target_os = "macos")]
                AppEvent::VisibilityChanged(visible) => {
                    self.visible = visible;
                    if visible {
                        self.worker.send(WorkerCommand::Reload);
                    }
                }
                #[cfg(not(target_os = "macos"))]
                AppEvent::Toggle { x, y } => {
                    self.visible = !self.visible;
                    if self.visible {
                        self.position_near_click(context, x, y);
                        self.worker.send(WorkerCommand::Reload);
                        self.reveal(context);
                    } else {
                        self.park(context);
                    }
                }
                AppEvent::Menu(id) => self.handle_menu(context, &id),
            }
        }

        while let Ok(event) = self.worker.events.try_recv() {
            self.handle_worker_event(context, event);
        }
    }

    fn handle_worker_event(&mut self, context: &egui::Context, event: WorkerEvent) {
        match event {
            WorkerEvent::RegistryLoaded(registry) => {
                self.registry = registry;
                self.errors.retain(|key, _| {
                    self.registry
                        .accounts
                        .iter()
                        .any(|account| &account.account_key == key)
                });
                if self.session_notice.is_some()
                    && self.registry.active_account_key.is_none()
                    && !self.restarting
                {
                    self.session_notice = None;
                }
                self.rebuild_tray_menu();
                self.banner_error = None;
            }
            WorkerEvent::Busy {
                account_key,
                active,
            } => {
                if active {
                    self.busy.insert(account_key);
                } else {
                    self.busy.remove(&account_key);
                }
            }
            WorkerEvent::SwitchBlocked { account_key, pids } => {
                self.switch_guard = Some(SwitchGuard { account_key, pids });
                self.show_at_last_position(context);
            }
            WorkerEvent::RefreshFailed {
                account_key,
                kind,
                message,
            } => {
                self.errors
                    .insert(account_key, RefreshFailure { kind, message });
            }
            WorkerEvent::LoginState { active, message } => {
                let changed = self.login_active != active;
                self.login_active = active;
                self.login_message = message;
                if changed {
                    self.rebuild_tray_menu();
                }
            }
            WorkerEvent::AccountSignedIn { account_key } => {
                self.errors.remove(&account_key);
            }
            WorkerEvent::CredentialsSelected { account_key } => {
                if self.registry.active_account_key.as_deref() == Some(&account_key) {
                    self.session_notice = Some(SessionNotice::Selected);
                    self.show_at_last_position(context);
                }
            }
            WorkerEvent::DesktopRestarted => {
                self.restarting = false;
                self.session_notice = Some(SessionNotice::RelaunchRequested);
                self.rebuild_tray_menu();
            }
            WorkerEvent::DesktopRestartFailed(message) => {
                self.restarting = false;
                // Keep restart failures visible even if a queued registry reload arrives.
                self.session_notice = Some(SessionNotice::RestartFailed(message));
                self.rebuild_tray_menu();
            }
            WorkerEvent::Error(message) => self.banner_error = Some(message),
        }
    }

    fn handle_menu(&mut self, context: &egui::Context, id: &str) {
        if self.restarting && id != "show" && id != "quit" {
            return;
        }
        match id {
            "show" => {
                self.show_at_last_position(context);
                self.worker.send(WorkerCommand::Reload);
            }
            "add" => {
                self.show_at_last_position(context);
                self.begin_login(false);
            }
            "device" => {
                self.show_at_last_position(context);
                self.begin_login(true);
            }
            "cancel-login" => self.worker.send(WorkerCommand::CancelLogin),
            "quit" => {
                self.worker.send(WorkerCommand::Quit);
                context.send_viewport_cmd(ViewportCommand::Close);
            }
            _ if id.starts_with("switch:") && !self.login_active => {
                self.worker.send(WorkerCommand::SwitchIntent {
                    account_key: id[7..].to_owned(),
                });
            }
            _ => {}
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn position_near_click(&mut self, context: &egui::Context, x: f64, y: f64) {
        let x = (x as f32 - WINDOW_WIDTH / 2.0).max(8.0);
        let y = if y as f32 > WINDOW_HEIGHT + 40.0 {
            y as f32 - WINDOW_HEIGHT - 8.0
        } else {
            y as f32 + 10.0
        };
        let position = egui::Pos2::new(x, y);
        self.last_position = Some(position);
        context.send_viewport_cmd(ViewportCommand::OuterPosition(position));
    }

    fn show_at_last_position(&mut self, context: &egui::Context) {
        self.visible = true;
        #[cfg(not(target_os = "macos"))]
        if let Some(position) = self.last_position {
            context.send_viewport_cmd(ViewportCommand::OuterPosition(position));
        } else {
            context.send_viewport_cmd(ViewportCommand::OuterPosition(egui::Pos2::new(
                100.0, 100.0,
            )));
        }
        self.reveal(context);
    }

    fn park(&self, context: &egui::Context) {
        #[cfg(target_os = "macos")]
        {
            let _ = context;
            hide_native_window();
        }
        #[cfg(not(target_os = "macos"))]
        context.send_viewport_cmd(ViewportCommand::OuterPosition(PARKED_POSITION.into()));
    }

    fn reveal(&self, context: &egui::Context) {
        #[cfg(not(target_os = "macos"))]
        context.send_viewport_cmd(ViewportCommand::Focus);
        #[cfg(target_os = "macos")]
        {
            let _ = context;
            show_native_window();
        }
    }

    fn rebuild_tray_menu(&mut self) {
        if let Some(tray) = self.tray.as_ref() {
            tray.set_menu(Some(Box::new(account_menu(
                &self.registry,
                self.login_active,
                self.restarting,
            ))));
        }
    }

    fn begin_login(&mut self, device_auth: bool) {
        if self.login_active || self.restarting {
            return;
        }
        self.login_active = true;
        self.restart_confirm = false;
        self.login_message.clear();
        self.banner_error = None;
        self.rebuild_tray_menu();
        self.worker.send(WorkerCommand::Login { device_auth });
    }

    fn request_refresh(&mut self, account_key: String) {
        if self.login_active || self.restarting || self.busy.contains(&account_key) {
            return;
        }
        if self.preferences.quota_disclosure_acknowledged {
            self.errors.remove(&account_key);
            self.busy.insert(account_key.clone());
            self.worker.send(WorkerCommand::Refresh { account_key });
        } else {
            self.disclosure_for = Some(account_key);
        }
    }

    fn hide(&mut self, context: &egui::Context) {
        self.visible = false;
        self.park(context);
    }

    fn header(&mut self, ui: &mut egui::Ui, context: &egui::Context) {
        let account_count = self.registry.accounts.len();
        let subtitle = match account_count {
            0 => "Your Codex accounts".to_owned(),
            1 => "1 saved account".to_owned(),
            count => format!("{count} saved accounts"),
        };

        Frame::new()
            .fill(SURFACE)
            .stroke(Stroke::new(1.0, BORDER))
            .inner_margin(Margin::symmetric(18, 14))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    paint_brand_mark(ui);
                    ui.add_space(5.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Account Hub").size(19.0).strong().color(INK));
                        ui.label(
                            RichText::new(format!("{subtitle} · v{}", env!("CARGO_PKG_VERSION")))
                                .size(11.5)
                                .color(MUTED),
                        );
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(RichText::new("×").size(20.0).color(MUTED))
                                    .frame(false)
                                    .min_size(Vec2::splat(30.0)),
                            )
                            .on_hover_text("Hide Account Hub")
                            .clicked()
                        {
                            self.hide(context);
                        }
                    });
                });
            });
    }

    fn disclosure_ui(&mut self, ui: &mut egui::Ui) {
        callout_heading(
            ui,
            "!",
            WARNING_SOFT,
            WARNING,
            "Before checking quota",
            "Please review this one-time notice.",
        );
        ui.add_space(18.0);
        Frame::new()
            .fill(SURFACE)
            .stroke(Stroke::new(1.0, BORDER))
            .corner_radius(12.0)
            .inner_margin(Margin::same(14))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.add(
                    egui::Label::new(
                        RichText::new("Checking quota calls an undocumented OpenAI backend endpoint, not the published API. OpenAI’s Terms of Use restrict automated access outside the API and circumventing rate limits.")
                            .size(13.0)
                            .color(INK),
                    )
                    .wrap(),
                );
                ui.add_space(10.0);
                ui.add(
                    egui::Label::new(
                        RichText::new("This app makes one manually triggered request, but the endpoint itself remains outside the API. codex-auth’s issue tracker includes a report of accounts being suspended after similar automated checks. Use at your own risk.")
                            .size(12.0)
                            .color(MUTED),
                    )
                    .wrap(),
                );
            });
        ui.add_space(18.0);
        let continue_clicked = ui
            .add_sized(
                [ui.available_width(), 40.0],
                primary_button("I understand, continue"),
            )
            .clicked();
        if ui
            .add_sized([ui.available_width(), 38.0], secondary_button("Cancel"))
            .clicked()
        {
            self.disclosure_for = None;
        }
        if continue_clicked {
            self.preferences.quota_disclosure_acknowledged = true;
            #[cfg(any(test, feature = "visual-qa"))]
            if self.preview_mode {
                self.disclosure_for = None;
                return;
            }
            match prefs::save(&self.preferences) {
                Ok(()) => {
                    if let Some(account_key) = self.disclosure_for.take() {
                        self.errors.remove(&account_key);
                        self.busy.insert(account_key.clone());
                        self.worker.send(WorkerCommand::Refresh { account_key });
                    }
                }
                Err(error) => {
                    self.preferences.quota_disclosure_acknowledged = false;
                    self.banner_error = Some(format!("Could not save disclosure choice: {error}"));
                }
            }
        }
    }

    fn switch_guard_ui(&mut self, ui: &mut egui::Ui) {
        let Some(guard) = self.switch_guard.clone() else {
            return;
        };
        let name = self
            .registry
            .accounts
            .iter()
            .find(|account| account.account_key == guard.account_key)
            .map(AccountRecord::display_name)
            .unwrap_or("this account");
        callout_heading(
            ui,
            "!",
            WARNING_SOFT,
            WARNING,
            "Codex is still running",
            "Choose how you want to continue.",
        );
        ui.add_space(18.0);
        Frame::new()
            .fill(SURFACE)
            .stroke(Stroke::new(1.0, BORDER))
            .corner_radius(12.0)
            .inner_margin(Margin::same(14))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.add(
                    egui::Label::new(
                        RichText::new(format!("Selecting {name} changes saved credentials only. Running sessions may retain the previous account and refresh token. Stopping Codex can interrupt work and does not reopen the desktop app or CLI."))
                            .size(12.5)
                            .color(INK),
                    )
                    .wrap(),
                );
            });
        ui.add_space(18.0);
        if ui
            .add_sized(
                [ui.available_width(), 40.0],
                primary_button("Stop Codex and switch"),
            )
            .clicked()
        {
            self.worker.send(WorkerCommand::SwitchStop {
                account_key: guard.account_key.clone(),
                pids: guard.pids.clone(),
            });
            self.switch_guard = None;
        }
        if ui
            .add_sized(
                [ui.available_width(), 38.0],
                danger_outline_button("Switch anyway"),
            )
            .clicked()
        {
            self.worker.send(WorkerCommand::SwitchForce {
                account_key: guard.account_key,
            });
            self.switch_guard = None;
        }
        if ui
            .add_sized([ui.available_width(), 38.0], secondary_button("Cancel"))
            .clicked()
        {
            self.switch_guard = None;
        }
    }

    fn restart_ui(&mut self, ui: &mut egui::Ui) {
        callout_heading(
            ui,
            "!",
            WARNING_SOFT,
            WARNING,
            "Restart desktop app?",
            "Finish or stop your running tasks first.",
        );
        ui.add_space(18.0);
        if let Some(account) =
            self.registry.accounts.iter().find(|a| {
                Some(a.account_key.as_str()) == self.registry.active_account_key.as_deref()
            })
        {
            ui.label(
                RichText::new(format!("Selected: {}", account.display_name()))
                    .strong()
                    .color(ACCENT),
            );
        }
        ui.add_space(8.0);
        ui.label(RichText::new("This requests a normal quit of the Codex desktop app (including installations named ChatGPT), reapplies the selected credentials, then reopens it.").size(13.0).color(INK));
        ui.add_space(12.0);
        ui.label("Running desktop tasks may be interrupted. If the app refuses to quit or stays open, Hub stops and does not force-close it.");
        ui.add_space(12.0);
        ui.label("CLI: exit existing Codex sessions first, then reopen them manually after switching. Hub does not recreate terminals or resume tasks.");
        ui.add_space(12.0);
        ui.label("Only clients using the same CODEX_HOME and file-based credentials can pick up this selection. Keychain or separately managed sign-ins may require signing in through that client. Reopening does not verify its account.");
        ui.add_space(20.0);
        if cfg!(target_os = "macos") {
            if ui
                .add_sized([ui.available_width(), 40.0], primary_button("Restart now"))
                .clicked()
            {
                if let Some(account_key) = self.registry.active_account_key.clone() {
                    self.restarting = true;
                    self.restart_confirm = false;
                    self.banner_error = None;
                    self.session_notice = Some(SessionNotice::Restarting);
                    self.rebuild_tray_menu();
                    self.worker
                        .send(WorkerCommand::RestartDesktop { account_key });
                }
            }
        } else {
            ui.label("On this platform, quit and reopen the desktop app manually.");
        }
        if ui
            .add_sized([ui.available_width(), 38.0], secondary_button("Cancel"))
            .clicked()
        {
            self.restart_confirm = false;
        }
    }

    fn account_list_ui(&mut self, ui: &mut egui::Ui) {
        if let Some(notice) = self.session_notice.clone() {
            let (title, message, fill, color) = notice.presentation();
            Frame::new()
                .fill(fill)
                .corner_radius(12.0)
                .inner_margin(Margin::same(12))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.label(RichText::new(title).strong().color(color));
                    ui.label(RichText::new(message).size(12.0).color(INK));
                    if self.restarting {
                        ui.spinner();
                    } else {
                        ui.horizontal(|ui| {
                            if matches!(
                                notice,
                                SessionNotice::Selected | SessionNotice::RestartFailed(_)
                            ) && ui.add(text_button("Restart guidance", color)).clicked()
                            {
                                self.restart_confirm = true;
                            }
                            if ui.add(text_button("Dismiss", MUTED)).clicked() {
                                self.session_notice = None;
                            }
                        });
                    }
                });
            ui.add_space(10.0);
        }
        ui.set_min_width(ui.available_width());
        if let Some(error) = self.banner_error.clone() {
            Frame::new()
                .fill(DANGER_SOFT)
                .stroke(Stroke::new(1.0, Color32::from_rgb(248, 206, 206)))
                .corner_radius(12.0)
                .inner_margin(Margin::same(12))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Something went wrong").strong().color(DANGER));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.add(text_button("Dismiss", DANGER)).clicked() {
                                self.banner_error = None;
                            }
                        });
                    });
                    ui.add(
                        egui::Label::new(RichText::new(error).size(12.0).color(MUTED))
                            .wrap()
                            .selectable(true),
                    );
                });
            ui.add_space(10.0);
        }
        if self.login_active {
            let message = if self.login_message.is_empty() {
                "Complete sign-in in your browser."
            } else {
                &self.login_message
            };
            Frame::new()
                .fill(ACCENT_SOFT)
                .stroke(Stroke::new(1.0, Color32::from_rgb(207, 213, 255)))
                .corner_radius(12.0)
                .inner_margin(Margin::same(12))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new().size(16.0).color(ACCENT));
                        ui.label(RichText::new("Sign-in in progress").strong().color(ACCENT));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.add(text_button("Cancel", ACCENT)).clicked() {
                                self.worker.send(WorkerCommand::CancelLogin);
                            }
                        });
                    });
                    egui::ScrollArea::vertical()
                        .id_salt("login-output")
                        .max_height(120.0)
                        .show(ui, |ui| {
                            ui.add(
                                egui::Label::new(RichText::new(message).size(12.0).color(MUTED))
                                    .wrap()
                                    .selectable(true),
                            );
                        });
                    if ui
                        .add(text_button("Copy sign-in details", ACCENT))
                        .clicked()
                    {
                        ui.ctx().copy_text(message.to_owned());
                    }
                });
            ui.add_space(10.0);
        }

        let accounts = self.filtered_accounts();
        // Consume a navigation request once. A removed target must not cause an
        // unexpected jump on a later reload or override subsequent manual scrolling.
        let scroll_target = self.scroll_to_account.take();
        if self.registry.accounts.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(58.0);
                paint_empty_state_icon(ui);
                ui.add_space(16.0);
                ui.label(
                    RichText::new("Bring your accounts together")
                        .size(17.0)
                        .strong()
                        .color(INK),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Add a Codex account to switch profiles and\nsee manually refreshed quota in one place.")
                        .size(12.0)
                        .color(MUTED),
                );
            });
        } else if accounts.is_empty() {
            ui.add_space(28.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("No matching accounts")
                        .size(16.0)
                        .strong()
                        .color(INK),
                );
                ui.label(
                    RichText::new("Try another name, email, or filter.")
                        .size(12.0)
                        .color(MUTED),
                );
                ui.add_space(10.0);
                if ui.add(secondary_button("Show all accounts")).clicked() {
                    self.search.clear();
                    self.account_filter = AccountFilter::All;
                }
            });
        } else {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!(
                        "{} OF {} ACCOUNTS",
                        accounts.len(),
                        self.registry.accounts.len()
                    ))
                    .size(10.0)
                    .strong()
                    .color(MUTED),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new("Manual quota checks").size(10.5).color(MUTED));
                });
            });
            ui.add_space(7.0);
            for account in accounts {
                let card = ui.push_id(&account.account_key, |ui| {
                    self.account_card(ui, &account);
                    ui.add_space(10.0);
                });
                if scroll_target.as_deref() == Some(account.account_key.as_str()) {
                    ui.scroll_to_rect_animation(
                        card.response.rect,
                        Some(Align::Min),
                        egui::style::ScrollAnimation::none(),
                    );
                }
            }
        }
    }

    fn account_card(&mut self, ui: &mut egui::Ui, account: &AccountRecord) {
        let is_active = self.registry.active_account_key.as_deref() == Some(&account.account_key);
        let is_busy = self.busy.contains(&account.account_key);
        let is_editing = self
            .editing_alias
            .as_ref()
            .is_some_and(|(key, _)| key == &account.account_key);
        let border = if is_active {
            Stroke::new(1.0, Color32::from_rgb(180, 188, 255))
        } else {
            Stroke::new(1.0, BORDER)
        };

        Frame::new()
            .fill(SURFACE)
            .stroke(border)
            .shadow(egui::epaint::Shadow {
                offset: [0, 2],
                blur: 5,
                spread: 0,
                color: Color32::from_black_alpha(9),
            })
            .corner_radius(14.0)
            .inner_margin(Margin::same(14))
            .show(ui, |ui| {
                if self.login_active || self.restarting {
                    ui.disable();
                }
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    paint_account_avatar(ui, account);
                    ui.add_space(4.0);
                    let identity_width = (ui.available_width() - 82.0).max(120.0);
                    ui.allocate_ui_with_layout(
                        Vec2::new(identity_width, 36.0),
                        Layout::top_down(Align::Min),
                        |ui| {
                            let response = ui
                                .add(
                                    egui::Label::new(
                                        RichText::new(account.display_name())
                                            .size(15.0)
                                            .strong()
                                            .color(INK),
                                    )
                                    .truncate()
                                    .sense(Sense::click()),
                                )
                                .on_hover_text(account.display_name());
                            if response.double_clicked() {
                                self.editing_alias =
                                    Some((account.account_key.clone(), account.alias.clone()));
                            }
                            let identity = if account.display_name() == account.email {
                                "ChatGPT account"
                            } else {
                                &account.email
                            };
                            ui.add(
                                egui::Label::new(RichText::new(identity).size(11.0).color(MUTED))
                                    .truncate(),
                            )
                            .on_hover_text(&account.email);
                        },
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if is_busy {
                            ui.spinner();
                        } else if !account.is_api_key()
                            && ui
                                .add(outline_button("Refresh").min_size(Vec2::new(66.0, 30.0)))
                                .on_hover_text("Check quota now")
                                .clicked()
                        {
                            self.request_refresh(account.account_key.clone());
                        }
                    });
                });

                ui.add_space(5.0);
                ui.horizontal(|ui| {
                    if is_active {
                        status_pill(ui, "Selected", SUCCESS_SOFT, SUCCESS, true);
                    }
                    status_pill(
                        ui,
                        &plan_label(account.display_plan()),
                        ACCENT_SOFT,
                        ACCENT,
                        false,
                    );
                    if account.is_api_key() {
                        status_pill(
                            ui,
                            "API key",
                            Color32::from_rgb(242, 244, 248),
                            MUTED,
                            false,
                        );
                    }
                });

                if is_editing {
                    ui.add_space(12.0);
                    Frame::new()
                        .fill(Color32::from_rgb(248, 249, 252))
                        .corner_radius(9.0)
                        .inner_margin(Margin::same(9))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.label(
                                RichText::new("ACCOUNT NAME")
                                    .size(9.0)
                                    .strong()
                                    .color(SUBTLE),
                            );
                            let mut save = false;
                            let mut cancel = false;
                            ui.horizontal(|ui| {
                                if let Some((_, value)) = self.editing_alias.as_mut() {
                                    let field_width = (ui.available_width() - 132.0).max(60.0);
                                    let response = ui.add(
                                        egui::TextEdit::singleline(value)
                                            .desired_width(field_width)
                                            .hint_text("Account name"),
                                    );
                                    save = response.lost_focus()
                                        && ui.input(|input| input.key_pressed(egui::Key::Enter));
                                    cancel = ui.input(|input| input.key_pressed(egui::Key::Escape));
                                }
                                if ui.add(text_button("Save", ACCENT)).clicked() {
                                    save = true;
                                }
                                if ui.add(text_button("Cancel", MUTED)).clicked() {
                                    cancel = true;
                                }
                            });
                            if save {
                                if let Some((key, alias)) = self.editing_alias.take() {
                                    self.worker.send(WorkerCommand::Rename {
                                        account_key: key,
                                        alias,
                                    });
                                }
                            } else if cancel {
                                self.editing_alias = None;
                            }
                        });
                }

                ui.add_space(9.0);

                if account.is_api_key() {
                    Frame::new()
                        .fill(Color32::from_rgb(248, 249, 252))
                        .corner_radius(9.0)
                        .inner_margin(Margin::same(10))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(
                                    "Quota and switching are unavailable for API-key accounts.",
                                )
                                .size(11.0)
                                .color(MUTED),
                            );
                        });
                } else if let Some(usage) = account.last_usage.as_ref() {
                    ui.columns(2, |columns| {
                        quota_tile(&mut columns[0], "5-hour", usage.primary.as_ref(), false);
                        quota_tile(&mut columns[1], "Weekly", usage.secondary.as_ref(), true);
                    });
                ui.add_space(6.0);
                    ui.label(
                        RichText::new(last_checked_label(account.last_usage_at))
                            .size(10.5)
                            .color(SUBTLE),
                    );
                } else {
                    Frame::new()
                        .fill(Color32::from_rgb(248, 249, 252))
                        .corner_radius(9.0)
                        .inner_margin(Margin::same(10))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.label(
                                RichText::new("Quota has not been checked yet")
                                    .size(11.5)
                                    .strong()
                                    .color(INK),
                            );
                            ui.label(
                                RichText::new("Use Refresh when you want the latest usage.")
                                    .size(10.5)
                                    .color(MUTED),
                            );
                        });
                }

                if let Some(failure) = self.errors.get(&account.account_key).cloned() {
                    ui.add_space(10.0);
                    Frame::new()
                        .fill(DANGER_SOFT)
                        .corner_radius(9.0)
                        .inner_margin(Margin::same(9))
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                let label = match failure.kind {
                                    RefreshFailureKind::NeedsRelogin => "Sign-in needs attention",
                                    RefreshFailureKind::Network => {
                                        "Could not reach the quota service"
                                    }
                                    RefreshFailureKind::Other => &failure.message,
                                };
                                ui.label(RichText::new(label).size(11.0).color(DANGER));
                                if failure.kind == RefreshFailureKind::NeedsRelogin
                                    && ui.add(text_button("Re-login", DANGER)).clicked()
                                {
                                    self.begin_login(false);
                                }
                            });
                        });
                }

                ui.add_space(8.0);
                if self.remove_confirm.as_deref() == Some(&account.account_key) {
                    Frame::new()
                        .fill(DANGER_SOFT)
                        .corner_radius(9.0)
                        .inner_margin(Margin::same(9))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.label(
                                RichText::new("Remove this saved account?")
                                    .size(11.0)
                                    .strong()
                                    .color(DANGER),
                            );
                            ui.horizontal(|ui| {
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if ui.add(text_button("Remove", DANGER)).clicked() {
                                        self.worker.send(WorkerCommand::Remove {
                                            account_key: account.account_key.clone(),
                                        });
                                        self.remove_confirm = None;
                                    }
                                    if ui.add(text_button("Cancel", MUTED)).clicked() {
                                        self.remove_confirm = None;
                                    }
                                });
                            });
                        });
                } else {
                    ui.horizontal(|ui| {
                        if !account.is_api_key() && !is_active {
                            if ui
                                .add(
                                    outline_button("Switch account")
                                        .min_size(Vec2::new(112.0, 32.0)),
                                )
                                .clicked()
                            {
                                self.worker.send(WorkerCommand::SwitchIntent {
                                    account_key: account.account_key.clone(),
                                });
                            }
                        } else if is_active {
                            ui.label(RichText::new("Selected credentials").size(10.5).color(MUTED))
                                .on_hover_text("Selected credentials are saved on disk. Hub cannot verify which account an open desktop app or CLI session is using.");
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.menu_button(RichText::new("...").size(20.0).color(MUTED), |ui| {
                                ui.set_min_width(130.0);
                                if is_active && ui.button("Session & restart").clicked() {
                                    self.restart_confirm = true;
                                    ui.close();
                                }
                                if ui.button("Rename").clicked() {
                                    self.editing_alias =
                                        Some((account.account_key.clone(), account.alias.clone()));
                                    ui.close();
                                }
                                ui.separator();
                                if ui.button(RichText::new("Remove").color(DANGER)).clicked() {
                                    self.remove_confirm = Some(account.account_key.clone());
                                    ui.close();
                                }
                            })
                            .response
                            .on_hover_text("Account options");
                        });
                    });
                }
            });
    }

    fn footer(&mut self, ui: &mut egui::Ui) {
        Frame::new()
            .fill(SURFACE)
            .stroke(Stroke::new(1.0, BORDER))
            .inner_margin(Margin::symmetric(16, 13))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    let secondary_width = 124.0;
                    let primary_width =
                        ui.available_width() - secondary_width - ui.spacing().item_spacing.x;
                    if ui
                        .add_enabled(
                            !self.login_active && !self.restarting,
                            primary_button("+  Add account")
                                .min_size(Vec2::new(primary_width, 38.0)),
                        )
                        .clicked()
                    {
                        self.begin_login(false);
                    }
                    if ui
                        .add_enabled(
                            !self.login_active && !self.restarting,
                            secondary_button("Device code")
                                .min_size(Vec2::new(secondary_width, 38.0)),
                        )
                        .clicked()
                    {
                        self.begin_login(true);
                    }
                });
            });
    }
}

impl AccountHubApp {
    fn render(&mut self, context: &egui::Context) {
        if context.input(|input| input.key_pressed(egui::Key::Escape)) {
            if self.restart_confirm {
                self.restart_confirm = false;
            } else if self.disclosure_for.is_some() {
                self.disclosure_for = None;
            } else if self.switch_guard.is_some() {
                self.switch_guard = None;
            } else if self.editing_alias.is_some() {
                self.editing_alias = None;
            } else if self.remove_confirm.is_some() {
                self.remove_confirm = None;
            } else if !self.search.is_empty() {
                self.search.clear();
            } else if self.account_filter != AccountFilter::All {
                self.account_filter = AccountFilter::All;
            } else {
                self.hide(context);
            }
        }
        if !self.visible {
            // Hide only on a visibility transition. A stale frame must never
            // enqueue another native hide after the tray has already reopened it.
            egui::CentralPanel::default()
                .frame(Frame::new().fill(CANVAS))
                .show(context, |_ui| {});
            return;
        }

        context.request_repaint_after(Duration::from_secs(60));
        egui::TopBottomPanel::top("header")
            .frame(Frame::NONE)
            .exact_height(70.0)
            .show_separator_line(false)
            .show(context, |ui| self.header(ui, context));
        let dialog_open =
            self.disclosure_for.is_some() || self.switch_guard.is_some() || self.restart_confirm;
        if !dialog_open {
            egui::TopBottomPanel::bottom("footer")
                .frame(Frame::NONE)
                .exact_height(66.0)
                .show_separator_line(false)
                .show(context, |ui| self.footer(ui));
            if !self.registry.accounts.is_empty() {
                egui::TopBottomPanel::top("account-tools")
                    .frame(
                        Frame::new()
                            .fill(SURFACE)
                            .inner_margin(Margin::symmetric(16, 9)),
                    )
                    .exact_height(88.0)
                    .show_separator_line(false)
                    .show(context, |ui| self.account_toolbar(ui));
            }
        }
        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(CANVAS)
                    .inner_margin(Margin::symmetric(16, 14)),
            )
            .show(context, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt(if dialog_open {
                        "dialog-scroll"
                    } else {
                        "accounts-scroll"
                    })
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        if self.restart_confirm {
                            self.restart_ui(ui);
                        } else if self.disclosure_for.is_some() {
                            self.disclosure_ui(ui);
                        } else if self.switch_guard.is_some() {
                            self.switch_guard_ui(ui);
                        } else {
                            self.account_list_ui(ui);
                        }
                    });
            });
    }
}

impl eframe::App for AccountHubApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [246.0 / 255.0, 247.0 / 255.0, 251.0 / 255.0, 1.0]
    }

    fn update(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        self.process_events(context);
        self.render(context);
    }
}

impl Drop for AccountHubApp {
    fn drop(&mut self) {
        TrayIconEvent::set_event_handler::<fn(TrayIconEvent)>(None);
        MenuEvent::set_event_handler::<fn(MenuEvent)>(None);
        self.worker.send(WorkerCommand::Quit);
    }
}

fn configure_style(context: &egui::Context) {
    let mut style = (*context.style()).clone();
    style.visuals = egui::Visuals::light();
    style.spacing.item_spacing = Vec2::new(7.0, 5.0);
    style.spacing.button_padding = Vec2::new(12.0, 7.0);
    style.spacing.interact_size = Vec2::new(40.0, 22.0);
    style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    style.visuals.panel_fill = CANVAS;
    style.visuals.window_fill = CANVAS;
    style.visuals.extreme_bg_color = SURFACE;
    style.visuals.selection.bg_fill = ACCENT;
    style.visuals.hyperlink_color = ACCENT;
    style.visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, INK);
    style.visuals.widgets.inactive.bg_fill = SURFACE;
    style.visuals.widgets.inactive.weak_bg_fill = SURFACE;
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
    style.visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, INK);
    style.visuals.widgets.hovered.bg_fill = ACCENT_SOFT;
    style.visuals.widgets.hovered.weak_bg_fill = ACCENT_SOFT;
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(190, 198, 255));
    style.visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, ACCENT);
    style.visuals.widgets.active.bg_fill = ACCENT;
    style.visuals.widgets.active.weak_bg_fill = ACCENT;
    style.visuals.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    style.visuals.widgets.active.fg_stroke = Stroke::new(1.0, SURFACE);
    style.visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(9);
    style.visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(9);
    style.visuals.widgets.active.corner_radius = egui::CornerRadius::same(9);
    style
        .text_styles
        .insert(egui::TextStyle::Body, FontId::proportional(13.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, FontId::proportional(12.5));
    style
        .text_styles
        .insert(egui::TextStyle::Small, FontId::proportional(11.0));
    context.set_style(style);
}

fn paint_brand_mark(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::hover());
    let id = egui::Id::new("account-hub-brand-texture");
    let cached = ui
        .ctx()
        .data_mut(|data| data.get_temp::<egui::TextureHandle>(id));
    let texture = cached.unwrap_or_else(|| {
        let image = egui::ColorImage::from_rgba_unmultiplied([128, 128], &icon::rgba_icon(128));
        let texture =
            ui.ctx()
                .load_texture("account-hub-brand", image, egui::TextureOptions::LINEAR);
        ui.ctx()
            .data_mut(|data| data.insert_temp(id, texture.clone()));
        texture
    });
    ui.painter().image(
        texture.id(),
        rect,
        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
        Color32::WHITE,
    );
}

fn paint_account_avatar(ui: &mut egui::Ui, account: &AccountRecord) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(36.0), Sense::hover());
    ui.painter()
        .circle_filled(rect.center(), 18.0, avatar_color(&account.account_key));
    let initial = account
        .display_name()
        .chars()
        .find(|character| character.is_alphanumeric())
        .unwrap_or('?')
        .to_uppercase()
        .next()
        .unwrap_or('?')
        .to_string();
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        initial,
        FontId::proportional(16.0),
        SURFACE,
    );
}

fn avatar_color(account_key: &str) -> Color32 {
    const COLORS: [Color32; 5] = [
        Color32::from_rgb(76, 92, 230),
        Color32::from_rgb(32, 139, 124),
        Color32::from_rgb(189, 91, 113),
        Color32::from_rgb(175, 105, 38),
        Color32::from_rgb(102, 83, 166),
    ];
    let hash = account_key.bytes().fold(0_usize, |value, byte| {
        value.wrapping_mul(31).wrapping_add(byte as usize)
    });
    COLORS[hash % COLORS.len()]
}

fn paint_empty_state_icon(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(68.0), Sense::hover());
    let painter = ui.painter();
    painter.circle_filled(rect.center(), 34.0, ACCENT_SOFT);
    painter.circle_filled(rect.center() + Vec2::new(0.0, -7.0), 8.0, ACCENT);
    let body =
        egui::Rect::from_center_size(rect.center() + Vec2::new(0.0, 12.0), Vec2::new(30.0, 15.0));
    painter.rect_filled(body, 8.0, ACCENT);
    let badge = rect.center() + Vec2::new(23.0, 21.0);
    painter.circle_filled(badge, 10.0, SUCCESS);
    painter.line_segment(
        [badge + Vec2::new(-4.0, 0.0), badge + Vec2::new(4.0, 0.0)],
        Stroke::new(2.0, SURFACE),
    );
    painter.line_segment(
        [badge + Vec2::new(0.0, -4.0), badge + Vec2::new(0.0, 4.0)],
        Stroke::new(2.0, SURFACE),
    );
}

fn callout_heading(
    ui: &mut egui::Ui,
    symbol: &str,
    fill: Color32,
    color: Color32,
    title: &str,
    subtitle: &str,
) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(44.0), Sense::hover());
        ui.painter().circle_filled(rect.center(), 22.0, fill);
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            symbol,
            FontId::proportional(18.0),
            color,
        );
        ui.add_space(4.0);
        ui.vertical(|ui| {
            ui.label(RichText::new(title).size(18.0).strong().color(INK));
            ui.label(RichText::new(subtitle).size(11.5).color(MUTED));
        });
    });
}

fn status_pill(ui: &mut egui::Ui, label: &str, fill: Color32, color: Color32, with_dot: bool) {
    Frame::new()
        .fill(fill)
        .corner_radius(20.0)
        .inner_margin(Margin::symmetric(9, 4))
        .show(ui, |ui| {
            ui.spacing_mut().interact_size.y = 14.0;
            ui.horizontal(|ui| {
                if with_dot {
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(7.0), Sense::hover());
                    ui.painter().circle_filled(rect.center(), 3.0, color);
                }
                ui.add(
                    egui::Label::new(RichText::new(label).size(11.0).strong().color(color))
                        .truncate(),
                );
            });
        });
}

fn primary_button(label: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(label.to_owned()).strong().color(SURFACE))
        .fill(ACCENT)
        .stroke(Stroke::NONE)
        .corner_radius(9.0)
}

fn secondary_button(label: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(label.to_owned()).strong().color(INK))
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(9.0)
}

fn outline_button(label: &str) -> egui::Button<'static> {
    egui::Button::new(
        RichText::new(label.to_owned())
            .size(11.0)
            .strong()
            .color(ACCENT),
    )
    .fill(ACCENT_SOFT)
    .stroke(Stroke::NONE)
    .corner_radius(8.0)
}

fn danger_outline_button(label: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(label.to_owned()).strong().color(DANGER))
        .fill(DANGER_SOFT)
        .stroke(Stroke::new(1.0, Color32::from_rgb(244, 198, 198)))
        .corner_radius(9.0)
}

fn text_button(label: &str, color: Color32) -> egui::Button<'static> {
    egui::Button::new(RichText::new(label.to_owned()).size(11.0).color(color))
        .frame(false)
        .min_size(Vec2::new(0.0, 26.0))
}

fn quota_tile(ui: &mut egui::Ui, label: &str, window: Option<&RateLimitWindow>, weekly: bool) {
    Frame::new()
        .fill(CANVAS)
        .corner_radius(8.0)
        .inner_margin(Margin::same(8))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.set_min_height(46.0);
            quota_row(ui, label, window, weekly);
        });
}

fn quota_row(ui: &mut egui::Ui, label: &str, window: Option<&RateLimitWindow>, weekly: bool) {
    let Some(window) = window else {
        ui.horizontal(|ui| {
            ui.label(RichText::new(label).size(11.5).strong().color(INK));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new("No data").size(10.5).color(SUBTLE));
            });
        });
        return;
    };
    let reset = reset_display(storage::now_seconds(), window.resets_at, weekly);
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).size(11.5).strong().color(INK));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if reset == ResetDisplay::Reset {
                ui.label(
                    RichText::new("Needs refresh")
                        .size(10.5)
                        .strong()
                        .color(WARNING),
                );
            } else {
                ui.label(
                    RichText::new(format!("{:.0}% used", window.used_percent))
                        .size(11.0)
                        .strong()
                        .color(usage_color(window.used_percent)),
                );
            }
        });
    });
    if reset != ResetDisplay::Reset {
        let fraction = (window.used_percent.clamp(0.0, 100.0) / 100.0) as f32;
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 8.0), Sense::hover());
        ui.painter()
            .rect_filled(rect, 4.0, Color32::from_rgb(232, 235, 242));
        if fraction > 0.0 {
            let fill_rect = egui::Rect::from_min_size(
                rect.min,
                Vec2::new(rect.width() * fraction, rect.height()),
            );
            ui.painter()
                .rect_filled(fill_rect, 4.0, usage_color(window.used_percent));
        }
    }
    ui.add_space(2.0);
    let (reset_text, reset_color) = match reset {
        ResetDisplay::Countdown(value) => (value, MUTED),
        ResetDisplay::Reset => (
            "Window reset - check again for current usage".to_owned(),
            WARNING,
        ),
        ResetDisplay::Unknown => ("Reset time unavailable".to_owned(), SUBTLE),
    };
    ui.label(RichText::new(reset_text).size(10.0).color(reset_color));
}

fn usage_color(used_percent: f64) -> Color32 {
    if used_percent >= 85.0 {
        DANGER
    } else if used_percent >= 65.0 {
        WARNING
    } else {
        SUCCESS
    }
}

fn plan_label(plan: &str) -> String {
    match plan {
        "prolite" => "Pro Lite".into(),
        "edu" => "Edu".into(),
        "unknown" => "Unknown plan".into(),
        other => {
            let mut chars = other.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect(),
                None => "Unknown plan".into(),
            }
        }
    }
}

fn build_tray(registry: &Registry) -> anyhow::Result<TrayIcon> {
    let pixels = if cfg!(target_os = "macos") {
        icon::rgba_tray_icon(32)
    } else {
        icon::rgba_icon(32)
    };
    let icon = tray_icon::Icon::from_rgba(pixels, 32, 32)?;
    Ok(TrayIconBuilder::new()
        .with_tooltip("Codex Account Hub")
        .with_icon(icon)
        .with_icon_as_template(cfg!(target_os = "macos"))
        .with_menu(Box::new(account_menu(registry, false, false)))
        .with_menu_on_left_click(false)
        .build()?)
}

fn account_menu(registry: &Registry, login_active: bool, restarting: bool) -> Menu {
    let menu = Menu::new();
    let show = MenuItem::with_id("show", "Show Account Hub", true, None);
    let _ = menu.append(&show);
    let _ = menu.append(&PredefinedMenuItem::separator());
    for account in &registry.accounts {
        let active = registry.active_account_key.as_deref() == Some(&account.account_key);
        let marker = if active { "Selected: " } else { "" };
        let label = format!("{marker}{} — {}", account.display_name(), account.email);
        let item = MenuItem::with_id(
            format!("switch:{}", account.account_key),
            label,
            !active && !account.is_api_key() && !login_active && !restarting,
            None,
        );
        let _ = menu.append(&item);
    }
    if !registry.accounts.is_empty() {
        let _ = menu.append(&PredefinedMenuItem::separator());
    }
    let add = MenuItem::with_id("add", "Add Account…", !login_active && !restarting, None);
    let device = MenuItem::with_id(
        "device",
        "Add with Device Code…",
        !login_active && !restarting,
        None,
    );
    let quit = MenuItem::with_id("quit", "Quit Codex Account Hub", true, None);
    let _ = menu.append(&add);
    let _ = menu.append(&device);
    if login_active {
        let _ = menu.append(&MenuItem::with_id(
            "cancel-login",
            "Cancel sign-in",
            true,
            None,
        ));
    }
    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&quit);
    menu
}
