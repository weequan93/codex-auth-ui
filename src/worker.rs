use std::{
    collections::VecDeque,
    fs,
    io::{BufRead, BufReader, Read},
    path::PathBuf,
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

use crossbeam_channel::{unbounded, Receiver, Sender};
use eframe::egui;
use sysinfo::Pid;

use crate::{
    model::Registry,
    process_guard::{codex_processes, stop_codex_processes},
    storage,
    usage::{self, RefreshError},
};

#[derive(Debug, Clone)]
pub enum WorkerCommand {
    Reload,
    Refresh { account_key: String },
    SwitchIntent { account_key: String },
    SwitchForce { account_key: String },
    SwitchStop { account_key: String, pids: Vec<Pid> },
    Rename { account_key: String, alias: String },
    Remove { account_key: String },
    Login { device_auth: bool },
    CancelLogin,
    RestartDesktop { account_key: String },
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshFailureKind {
    NeedsRelogin,
    Network,
    Other,
}

#[derive(Debug, Clone)]
pub enum WorkerEvent {
    RegistryLoaded(Registry),
    Busy {
        account_key: String,
        active: bool,
    },
    SwitchBlocked {
        account_key: String,
        pids: Vec<Pid>,
    },
    RefreshFailed {
        account_key: String,
        kind: RefreshFailureKind,
        message: String,
    },
    LoginState {
        active: bool,
        message: String,
    },
    AccountSignedIn {
        account_key: String,
    },
    CredentialsSelected {
        account_key: String,
    },
    DesktopRestarted,
    DesktopRestartFailed(String),
    Error(String),
}

pub struct WorkerHandle {
    command_tx: Sender<WorkerCommand>,
    pub events: Receiver<WorkerEvent>,
    control: Arc<LoginControl>,
    thread: Option<thread::JoinHandle<()>>,
}

#[derive(Default)]
struct LoginControl {
    pending: AtomicBool,
    cancel: AtomicBool,
    shutdown: AtomicBool,
}

impl WorkerHandle {
    #[cfg(any(test, feature = "visual-qa"))]
    pub(crate) fn preview() -> (Self, Receiver<WorkerCommand>) {
        let (command_tx, commands) = unbounded();
        let (_, events) = unbounded();
        (
            Self {
                command_tx,
                events,
                control: Arc::default(),
                thread: None,
            },
            commands,
        )
    }

    pub fn start(context: egui::Context) -> Self {
        let (command_tx, command_rx) = unbounded();
        let (event_tx, event_rx) = unbounded();
        let control = Arc::new(LoginControl::default());
        let worker_control = control.clone();
        let thread = thread::Builder::new()
            .name("codex-account-hub-worker".into())
            .spawn(move || run_worker(command_rx, event_tx, context, worker_control))
            .expect("start background worker");
        Self {
            command_tx,
            events: event_rx,
            control,
            thread: Some(thread),
        }
    }

    pub fn send(&self, command: WorkerCommand) {
        match &command {
            WorkerCommand::Login { .. } => {
                if self.control.pending.swap(true, Ordering::SeqCst) {
                    return;
                }
                self.control.cancel.store(false, Ordering::SeqCst);
            }
            WorkerCommand::CancelLogin => {
                self.control.cancel.store(true, Ordering::SeqCst);
                return;
            }
            WorkerCommand::Quit => {
                self.control.shutdown.store(true, Ordering::SeqCst);
                self.control.cancel.store(true, Ordering::SeqCst);
            }
            _ => {}
        }
        let _ = self.command_tx.send(command);
    }
}

impl Drop for WorkerHandle {
    fn drop(&mut self) {
        self.send(WorkerCommand::Quit);
        if let Some(thread) = self.thread.take() {
            // Wait for the owned login process to be reaped before the app exits.
            let _ = thread.join();
        }
    }
}

fn run_worker(
    commands: Receiver<WorkerCommand>,
    events: Sender<WorkerEvent>,
    context: egui::Context,
    control: Arc<LoginControl>,
) {
    let home = match storage::codex_home() {
        Ok(home) => home,
        Err(error) => {
            emit(&events, &context, WorkerEvent::Error(error.to_string()));
            return;
        }
    };
    let client = match usage::build_client() {
        Ok(client) => client,
        Err(error) => {
            emit(&events, &context, WorkerEvent::Error(error.to_string()));
            return;
        }
    };

    while let Ok(command) = commands.recv() {
        if control.shutdown.load(Ordering::SeqCst) {
            break;
        }
        match command {
            WorkerCommand::Reload => match storage::sync_active_from_auth(&home) {
                Ok(registry) => emit(&events, &context, WorkerEvent::RegistryLoaded(registry)),
                Err(error) => emit(&events, &context, WorkerEvent::Error(error.to_string())),
            },
            WorkerCommand::Refresh { account_key } => {
                emit(
                    &events,
                    &context,
                    WorkerEvent::Busy {
                        account_key: account_key.clone(),
                        active: true,
                    },
                );
                let result = refresh_account(&home, &client, &account_key);
                match result {
                    Ok(registry) => emit(&events, &context, WorkerEvent::RegistryLoaded(registry)),
                    Err(error) => emit(
                        &events,
                        &context,
                        WorkerEvent::RefreshFailed {
                            account_key: account_key.clone(),
                            kind: match error {
                                RefreshError::NeedsRelogin => RefreshFailureKind::NeedsRelogin,
                                RefreshError::Network(_) => RefreshFailureKind::Network,
                                _ => RefreshFailureKind::Other,
                            },
                            message: error.to_string(),
                        },
                    ),
                }
                emit(
                    &events,
                    &context,
                    WorkerEvent::Busy {
                        account_key,
                        active: false,
                    },
                );
            }
            WorkerCommand::SwitchIntent { account_key } => {
                guarded_switch(&home, &events, &context, account_key);
            }
            WorkerCommand::SwitchForce { account_key } => {
                run_switch(&home, &events, &context, account_key)
            }
            WorkerCommand::SwitchStop { account_key, pids } => match stop_codex_processes(&pids) {
                Ok(()) => guarded_switch(&home, &events, &context, account_key),
                Err(error) => emit(&events, &context, WorkerEvent::Error(error.to_string())),
            },
            WorkerCommand::Rename { account_key, alias } => {
                match storage::rename_account(&home, &account_key, &alias) {
                    Ok(registry) => emit(&events, &context, WorkerEvent::RegistryLoaded(registry)),
                    Err(error) => emit(&events, &context, WorkerEvent::Error(error.to_string())),
                }
            }
            WorkerCommand::Remove { account_key } => {
                match storage::remove_account(&home, &account_key) {
                    Ok(registry) => emit(&events, &context, WorkerEvent::RegistryLoaded(registry)),
                    Err(error) => emit(&events, &context, WorkerEvent::Error(error.to_string())),
                }
            }
            WorkerCommand::Login { device_auth } => {
                emit(
                    &events,
                    &context,
                    WorkerEvent::LoginState {
                        active: true,
                        message: "Waiting for sign-in…".into(),
                    },
                );
                let result = run_login(&home, device_auth, &events, &context, &control);
                control.pending.store(false, Ordering::SeqCst);
                emit(
                    &events,
                    &context,
                    WorkerEvent::LoginState {
                        active: false,
                        message: String::new(),
                    },
                );
                match result {
                    Ok(Some((registry, key))) => {
                        emit(
                            &events,
                            &context,
                            WorkerEvent::AccountSignedIn {
                                account_key: key.clone(),
                            },
                        );
                        emit(&events, &context, WorkerEvent::RegistryLoaded(registry));
                        if !control.shutdown.load(Ordering::SeqCst) {
                            guarded_switch(&home, &events, &context, key);
                        }
                    }
                    Ok(None) => {}
                    Err(error) => emit(&events, &context, WorkerEvent::Error(error.to_string())),
                }
            }
            WorkerCommand::CancelLogin => {}
            WorkerCommand::RestartDesktop { account_key } => {
                let result = restart_desktop(&home, &account_key, &control);
                match result {
                    Ok(registry) => {
                        emit(&events, &context, WorkerEvent::RegistryLoaded(registry));
                        emit(&events, &context, WorkerEvent::DesktopRestarted);
                    }
                    Err(error) => emit(
                        &events,
                        &context,
                        WorkerEvent::DesktopRestartFailed(error.to_string()),
                    ),
                }
            }
            WorkerCommand::Quit => break,
        }
    }
}

fn guarded_switch(
    home: &std::path::Path,
    events: &Sender<WorkerEvent>,
    context: &egui::Context,
    account_key: String,
) {
    let pids = codex_processes();
    if pids.is_empty() {
        run_switch(home, events, context, account_key);
    } else {
        emit(
            events,
            context,
            WorkerEvent::SwitchBlocked { account_key, pids },
        );
    }
}

fn restart_desktop(
    home: &std::path::Path,
    account_key: &str,
    control: &LoginControl,
) -> anyhow::Result<Registry> {
    // Validate before requesting quit, then validate again after the app exits.
    validate_restart_target(home, account_key)?;
    storage::preserve_selected_live_credentials(home, account_key)?;
    crate::desktop::quit_for_restart(|| control.shutdown.load(Ordering::SeqCst))?;
    if !codex_processes().is_empty() {
        anyhow::bail!("Codex CLI or background processes are still running. Close them, then retry. The desktop app has not been reopened.");
    }
    validate_restart_target(home, account_key)?;
    storage::preserve_selected_live_credentials(home, account_key)?;
    // Quitting may flush older credentials. Reapply only after all Codex processes exit.
    let registry = storage::switch_account(home, account_key)?;
    crate::desktop::reopen()?;
    Ok(registry)
}

fn validate_restart_target(home: &std::path::Path, account_key: &str) -> anyhow::Result<()> {
    let registry = storage::load_registry(home)?;
    let record = registry
        .accounts
        .iter()
        .find(|a| a.account_key == account_key)
        .ok_or_else(|| anyhow::anyhow!("The selected account no longer exists."))?;
    if registry.active_account_key.as_deref() != Some(account_key) {
        anyhow::bail!("The selected account changed. Review it before restarting.");
    }
    let snapshot = storage::resolve_account_auth_path(home, record)?;
    let info = crate::auth::parse_auth_file(&snapshot)?;
    if record.is_api_key()
        || info.kind != crate::auth::AuthKind::ChatGpt
        || info.record_key.as_deref() != Some(account_key)
        || info.access_token.is_none()
    {
        anyhow::bail!("Selected credentials are invalid. Sign in again before restarting.");
    }
    Ok(())
}

fn refresh_account(
    home: &std::path::Path,
    client: &reqwest::blocking::Client,
    account_key: &str,
) -> std::result::Result<Registry, RefreshError> {
    let registry =
        storage::load_registry(home).map_err(|error| RefreshError::Other(error.to_string()))?;
    let record = registry
        .accounts
        .iter()
        .find(|account| account.account_key == account_key)
        .ok_or_else(|| RefreshError::Other("Account no longer exists".into()))?;
    let auth_path = storage::resolve_quota_auth_path(home, record)
        .map_err(|error| RefreshError::Other(error.to_string()))?;
    let snapshot = usage::fetch_for_auth(client, &auth_path)?;
    storage::update_usage(home, account_key, snapshot)
        .map_err(|error| RefreshError::Other(error.to_string()))
}

fn run_switch(
    home: &std::path::Path,
    events: &Sender<WorkerEvent>,
    context: &egui::Context,
    account_key: String,
) {
    emit(
        events,
        context,
        WorkerEvent::Busy {
            account_key: account_key.clone(),
            active: true,
        },
    );
    match storage::switch_account(home, &account_key) {
        Ok(registry) => {
            emit(events, context, WorkerEvent::RegistryLoaded(registry));
            emit(
                events,
                context,
                WorkerEvent::CredentialsSelected {
                    account_key: account_key.clone(),
                },
            );
        }
        Err(error) => emit(events, context, WorkerEvent::Error(error.to_string())),
    }
    emit(
        events,
        context,
        WorkerEvent::Busy {
            account_key,
            active: false,
        },
    );
}

fn run_login(
    home: &std::path::Path,
    device_auth: bool,
    events: &Sender<WorkerEvent>,
    context: &egui::Context,
    control: &LoginControl,
) -> anyhow::Result<Option<(Registry, String)>> {
    if control.cancel.load(Ordering::SeqCst) || control.shutdown.load(Ordering::SeqCst) {
        return Ok(None);
    }
    let scratch = storage::accounts_dir(home).join(format!(
        "login-{}-{}",
        std::process::id(),
        storage::now_millis()
    ));
    storage::create_private_directory(&scratch)?;
    let cleanup = ScratchCleanup(scratch.clone());

    let mut command = Command::new("codex");
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW; progress is shown in the app.
    }
    command.arg("login");
    #[cfg(target_os = "macos")]
    if let Some(base) = directories::BaseDirs::new() {
        command.env(
            "PATH",
            crate::startup::cli_search_path(std::env::var_os("PATH").as_deref(), base.home_dir()),
        );
    }
    if device_auth {
        command.arg("--device-auth");
    }
    command
        .env("CODEX_HOME", &scratch)
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = command
        .spawn()
        .map_err(|error| anyhow::anyhow!("Could not start `codex login`: {error}"))?;

    let Some(status) = wait_for_login(child, control, Duration::from_secs(600), |message| {
        emit(
            events,
            context,
            WorkerEvent::LoginState {
                active: true,
                message,
            },
        );
    })?
    else {
        return Ok(None);
    };
    if !status.success() {
        anyhow::bail!("Codex sign-in exited without completing");
    }

    let auth = fs::read(scratch.join("auth.json")).map_err(|error| {
        anyhow::anyhow!("Sign-in completed but no auth.json was produced: {error}")
    })?;
    if control.cancel.load(Ordering::SeqCst) || control.shutdown.load(Ordering::SeqCst) {
        return Ok(None);
    }
    let key = crate::auth::parse_auth_bytes(&auth)?
        .record_key
        .ok_or_else(|| anyhow::anyhow!("sign-in result has no account identity"))?;
    let registry = storage::import_login_auth(home, &auth)?;
    drop(cleanup);
    Ok(Some((registry, key)))
}

fn stream_login_output(
    reader: impl Read + Send + 'static,
    output: Sender<String>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        for line in BufReader::new(reader).lines().map_while(Result::ok) {
            let message = sanitize_terminal_line(&line);
            if !message.is_empty() && output.send(message).is_err() {
                break;
            }
        }
    })
}

struct LoginChild(Child);

impl Drop for LoginChild {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn wait_for_login(
    child: Child,
    control: &LoginControl,
    timeout: Duration,
    mut update: impl FnMut(String),
) -> anyhow::Result<Option<ExitStatus>> {
    let mut child = LoginChild(child);
    let (output_tx, output_rx) = unbounded();
    if let Some(stdout) = child.0.stdout.take() {
        stream_login_output(stdout, output_tx.clone());
    }
    if let Some(stderr) = child.0.stderr.take() {
        stream_login_output(stderr, output_tx.clone());
    }
    drop(output_tx);
    let started = Instant::now();
    let mut transcript = VecDeque::new();
    loop {
        if control.cancel.load(Ordering::SeqCst) || control.shutdown.load(Ordering::SeqCst) {
            return Ok(None);
        }
        if started.elapsed() >= timeout {
            anyhow::bail!("Sign-in timed out. Please start again when you are ready.");
        }
        if let Ok(message) = output_rx.try_recv() {
            // Keep the device URL/code visible when later CLI instructions arrive.
            transcript.push_back(message.chars().take(2048).collect::<String>());
            while transcript.len() > 32 {
                transcript.pop_front();
            }
            update(transcript.iter().cloned().collect::<Vec<_>>().join("\n"));
        }
        if let Some(status) = child.0.try_wait()? {
            return Ok(Some(status));
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn sanitize_terminal_line(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut characters = input.chars().peekable();

    while let Some(character) = characters.next() {
        if character == '\u{1b}' {
            match characters.next() {
                Some('[') => {
                    for code in characters.by_ref() {
                        if ('@'..='~').contains(&code) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    let mut previous_escape = false;
                    for code in characters.by_ref() {
                        if code == '\u{7}' || (previous_escape && code == '\\') {
                            break;
                        }
                        previous_escape = code == '\u{1b}';
                    }
                }
                Some(_) | None => {}
            }
        } else if character == '\t' {
            output.push(' ');
        } else if !character.is_control() {
            output.push(character);
        }
    }

    output.split_whitespace().collect::<Vec<_>>().join(" ")
}

struct ScratchCleanup(PathBuf);

impl Drop for ScratchCleanup {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn emit(events: &Sender<WorkerEvent>, context: &egui::Context, event: WorkerEvent) {
    let _ = events.send(event);
    context.request_repaint();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_terminal_formatting_from_login_messages() {
        assert_eq!(
            sanitize_terminal_line("\u{1b}[90mContinue only if you started this login\u{1b}[0m"),
            "Continue only if you started this login"
        );
        assert_eq!(
            sanitize_terminal_line("\u{1b}]8;;https://example.com\u{7} Open browser \t now "),
            "Open browser now"
        );
        assert_eq!(
            sanitize_terminal_line(
                "\u{1b}]8;;https://example.com\u{1b}\\Sign in\u{1b}]8;;\u{1b}\\"
            ),
            "Sign in"
        );
    }

    #[cfg(unix)]
    fn fake_login() -> Child {
        Command::new("/bin/sh")
            .args(["-c", r"printf '\033[90mOpen https://example.com/device\033[0m\nABCD-EFGH\nContinue only if you started this login\n'; exec sleep 30"])
            .stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap()
    }

    #[cfg(unix)]
    fn assert_reaped(pid: u32) {
        let mut system = sysinfo::System::new();
        let pid = Pid::from_u32(pid);
        system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
        assert!(
            system.process(pid).is_none(),
            "owned login child was not reaped"
        );
    }

    #[cfg(unix)]
    #[test]
    fn cancel_reaps_login_and_preserves_device_instructions() {
        let control = LoginControl::default();
        let child = fake_login();
        let pid = child.id();
        let mut transcript = String::new();
        let result = wait_for_login(child, &control, Duration::from_secs(5), |message| {
            transcript = message;
            if transcript.contains("Continue only") {
                control.cancel.store(true, Ordering::SeqCst);
            }
        })
        .unwrap();
        assert!(result.is_none());
        assert!(transcript.contains("https://example.com/device"));
        assert!(transcript.contains("ABCD-EFGH"));
        assert!(!transcript.contains('\u{1b}'));
        assert_reaped(pid);
    }

    #[cfg(unix)]
    #[test]
    fn timeout_and_shutdown_reap_owned_login_processes() {
        let control = LoginControl::default();
        let child = fake_login();
        let pid = child.id();
        let error = wait_for_login(child, &control, Duration::from_millis(80), |_| {}).unwrap_err();
        assert!(error.to_string().contains("timed out"));
        assert_reaped(pid);
        control.shutdown.store(true, Ordering::SeqCst);
        let child = fake_login();
        let pid = child.id();
        assert!(
            wait_for_login(child, &control, Duration::from_secs(5), |_| {})
                .unwrap()
                .is_none()
        );
        assert_reaped(pid);
    }
}
