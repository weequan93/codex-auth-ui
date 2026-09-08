use std::{
    thread,
    time::{Duration, Instant},
};

use anyhow::{bail, Result};
use sysinfo::{Pid, ProcessesToUpdate, System};

#[cfg(not(target_os = "windows"))]
use sysinfo::Signal;

pub fn codex_processes() -> Vec<Pid> {
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::All, true);
    system
        .processes()
        .iter()
        .filter_map(|(pid, process)| {
            let name = process.name().to_string_lossy();
            (name == "codex" || name.eq_ignore_ascii_case("codex.exe")).then_some(*pid)
        })
        .collect()
}

pub fn stop_codex_processes(pids: &[Pid]) -> Result<()> {
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::Some(pids), true);
    let mut starts = std::collections::HashMap::new();
    for pid in pids {
        if let Some(process) = system.process(*pid) {
            let name = process.name().to_string_lossy();
            if name != "codex" && !name.eq_ignore_ascii_case("codex.exe") {
                bail!("a selected process changed; please retry switching");
            }
            starts.insert(*pid, process.start_time());
        }
    }

    #[cfg(target_os = "windows")]
    for pid in pids {
        use std::os::windows::process::CommandExt;
        if !starts.contains_key(pid) {
            continue;
        }
        if let Ok(mut task) = std::process::Command::new("taskkill")
            .args(["/PID", &pid.as_u32().to_string()])
            .creation_flags(0x0800_0000)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            let deadline = Instant::now() + Duration::from_millis(800);
            while matches!(task.try_wait(), Ok(None)) && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(20));
            }
            if !matches!(task.try_wait(), Ok(Some(_))) {
                let _ = task.kill();
                let _ = task.wait();
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    for pid in pids {
        if let Some(process) = system.process(*pid) {
            let _ = process.kill_with(Signal::Term);
        }
    }

    let deadline = Instant::now() + Duration::from_millis(800);
    while Instant::now() < deadline {
        thread::sleep(Duration::from_millis(50));
        system.refresh_processes(ProcessesToUpdate::Some(pids), true);
        if pids.iter().all(|pid| system.process(*pid).is_none()) {
            return Ok(());
        }
    }

    system.refresh_processes(ProcessesToUpdate::Some(pids), true);
    for pid in pids {
        if let Some(process) = system.process(*pid) {
            if starts.get(pid) == Some(&process.start_time()) {
                let _ = process.kill();
            }
        }
    }
    let deadline = Instant::now() + Duration::from_millis(800);
    while Instant::now() < deadline {
        system.refresh_processes(ProcessesToUpdate::Some(pids), true);
        if pids.iter().all(|pid| system.process(*pid).is_none()) {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(50));
    }
    bail!("Codex could not be stopped. Close it manually before switching.")
}
