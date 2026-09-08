//! Cooperative desktop restart. Never force-terminate the app or restart a shell.
use anyhow::{bail, Result};

#[cfg(target_os = "macos")]
extern "C" {
    fn cah_prepare_desktop_restart() -> i32;
    fn cah_desktop_is_running() -> bool;
    fn cah_launch_desktop() -> bool;
}

pub fn quit_for_restart(cancelled: impl Fn() -> bool) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        use std::{
            thread,
            time::{Duration, Instant},
        };
        match unsafe { cah_prepare_desktop_restart() } {
            0 => {}
            1 => bail!("The Codex desktop app was not found. Reopen it manually."),
            _ => bail!("The desktop app declined to quit. Finish your work and try again."),
        }
        wait_until_closed(
            Duration::from_secs(20),
            cancelled,
            || unsafe { cah_desktop_is_running() },
            || thread::sleep(Duration::from_millis(100)),
            Instant::now,
        )
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = cancelled;
        bail!("Restart the desktop app manually on this platform.")
    }
}

pub fn reopen() -> Result<()> {
    #[cfg(target_os = "macos")]
    if unsafe { cah_launch_desktop() } {
        return Ok(());
    }
    bail!("Could not reopen the desktop app. Open it manually; saved credentials are unchanged.")
}

#[cfg(any(target_os = "macos", test))]
fn wait_until_closed(
    timeout: std::time::Duration,
    cancelled: impl Fn() -> bool,
    mut running: impl FnMut() -> bool,
    mut pause: impl FnMut(),
    mut now: impl FnMut() -> std::time::Instant,
) -> Result<()> {
    let started = now();
    loop {
        if cancelled() {
            bail!("Desktop restart cancelled.");
        }
        if !running() {
            return Ok(());
        }
        if now().duration_since(started) >= timeout {
            bail!("The desktop app is still open. Finish any prompts and try again. It was not force-closed.");
        }
        pause();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn restart_wait_stops_on_cancellation_or_timeout() {
        assert!(wait_until_closed(
            Duration::from_secs(20),
            || true,
            || true,
            || {},
            Instant::now
        )
        .is_err());
        assert!(wait_until_closed(Duration::ZERO, || false, || true, || {}, Instant::now).is_err());
    }

    #[test]
    fn restart_wait_requires_confirmed_exit() {
        let mut checks = 0;
        assert!(wait_until_closed(
            Duration::from_secs(1),
            || false,
            || {
                checks += 1;
                checks < 3
            },
            || {},
            Instant::now
        )
        .is_ok());
        assert_eq!(checks, 3);
    }
}
