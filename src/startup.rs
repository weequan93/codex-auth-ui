use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

/// Packaged launches must present a usable window even when the tray is unavailable.
pub fn is_app_bundle_executable(executable: &Path) -> bool {
    let Some(macos) = executable.parent() else {
        return false;
    };
    let Some(contents) = macos.parent() else {
        return false;
    };
    macos.file_name().is_some_and(|s| s == "MacOS")
        && contents.file_name().is_some_and(|s| s == "Contents")
        && contents
            .parent()
            .and_then(Path::extension)
            .is_some_and(|s| s == "app")
}

pub fn start_visible() -> bool {
    std::env::var_os("CODEX_ACCOUNT_HUB_START_VISIBLE").is_some()
        || (cfg!(target_os = "macos")
            && std::env::current_exe().is_ok_and(|path| is_app_bundle_executable(&path)))
}

/// Build the login child's PATH, not the GUI's process environment. No shell is used.
pub fn cli_search_path(existing: Option<&std::ffi::OsStr>, home: &Path) -> OsString {
    let mut paths: Vec<PathBuf> = existing
        .into_iter()
        .flat_map(std::env::split_paths)
        .filter(|path| path.is_absolute())
        .collect();
    for path in [
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
        home.join(".local/bin"),
        home.join(".cargo/bin"),
        PathBuf::from("/usr/bin"),
        PathBuf::from("/bin"),
        PathBuf::from("/usr/sbin"),
        PathBuf::from("/sbin"),
    ] {
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    std::env::join_paths(paths).unwrap_or_else(|_| OsString::from("/usr/bin:/bin:/usr/sbin:/sbin"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_packaged_launches_without_changing_standalone_startup() {
        assert!(is_app_bundle_executable(Path::new(
            "/Applications/Account Hub.app/Contents/MacOS/codex-account-hub"
        )));
        assert!(!is_app_bundle_executable(Path::new(
            "/tmp/target/release/codex-account-hub"
        )));
        assert!(!is_app_bundle_executable(Path::new(
            "/tmp/Test.app/codex-account-hub"
        )));
    }

    #[cfg(unix)]
    #[test]
    fn login_path_keeps_explicit_locations_and_adds_finder_defaults() {
        let path = cli_search_path(
            Some(std::ffi::OsStr::new("/custom/bin::.:/usr/bin")),
            Path::new("/Users/test"),
        );
        let entries: Vec<_> = std::env::split_paths(&path).collect();
        assert_eq!(entries[0], Path::new("/custom/bin"));
        assert!(entries.contains(&PathBuf::from("/Users/test/.local/bin")));
        assert!(entries.contains(&PathBuf::from("/opt/homebrew/bin")));
        assert!(entries.iter().all(|p| p.is_absolute()));
        assert_eq!(
            entries
                .iter()
                .filter(|p| *p == Path::new("/usr/bin"))
                .count(),
            1
        );
    }
}
