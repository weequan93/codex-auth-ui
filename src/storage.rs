use std::{
    env,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use directories::BaseDirs;

use crate::{
    auth::{parse_auth_bytes, parse_auth_file, AuthKind},
    model::{AccountRecord, RateLimitSnapshot, Registry, REGISTRY_SCHEMA_VERSION},
};

pub fn codex_home() -> Result<PathBuf> {
    if let Some(value) = env::var_os("CODEX_HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(value));
    }
    let base = BaseDirs::new().context("resolve the user home directory")?;
    Ok(base.home_dir().join(".codex"))
}

pub fn accounts_dir(home: &Path) -> PathBuf {
    home.join("accounts")
}

pub fn registry_path(home: &Path) -> PathBuf {
    accounts_dir(home).join("registry.json")
}

pub fn active_auth_path(home: &Path) -> PathBuf {
    home.join("auth.json")
}

pub fn account_file_key(account_key: &str) -> String {
    let safe = !account_key.is_empty()
        && account_key != "."
        && account_key != ".."
        && account_key
            .bytes()
            .all(|value| value.is_ascii_alphanumeric() || matches!(value, b'-' | b'_' | b'.'));
    if safe {
        account_key.into()
    } else {
        URL_SAFE_NO_PAD.encode(account_key.as_bytes())
    }
}

pub fn account_auth_path(home: &Path, account_key: &str) -> PathBuf {
    accounts_dir(home).join(format!("{}.auth.json", account_file_key(account_key)))
}

pub fn resolve_account_auth_path(home: &Path, record: &AccountRecord) -> Result<PathBuf> {
    let preferred = account_auth_path(home, &record.account_key);
    if preferred.is_file() {
        return Ok(preferred);
    }
    let legacy = accounts_dir(home).join(format!(
        "{}.auth.json",
        URL_SAFE_NO_PAD.encode(record.email.as_bytes())
    ));
    if legacy.is_file() {
        return Ok(legacy);
    }
    bail!(
        "stored credentials are missing for {}",
        record.display_name()
    )
}

pub fn resolve_quota_auth_path(home: &Path, record: &AccountRecord) -> Result<PathBuf> {
    let active = active_auth_path(home);
    if let Ok(info) = parse_auth_file(&active) {
        if info.record_key.as_deref() == Some(&record.account_key) {
            return Ok(active);
        }
    }
    let snapshot = resolve_account_auth_path(home, record)?;
    if parse_auth_file(&snapshot)?.record_key.as_deref() != Some(&record.account_key) {
        bail!("stored credentials do not match this account; sign in again");
    }
    Ok(snapshot)
}

pub fn load_registry(home: &Path) -> Result<Registry> {
    let path = registry_path(home);
    let data = match fs::read(&path) {
        Ok(data) => data,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Registry::default())
        }
        Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
    };
    parse_registry(&data)
}

fn parse_registry(data: &[u8]) -> Result<Registry> {
    let mut registry: Registry =
        serde_json::from_slice(data).context("parse codex-auth registry")?;
    if registry.schema_version > REGISTRY_SCHEMA_VERSION {
        bail!(
            "registry schema {} is newer than this app supports ({REGISTRY_SCHEMA_VERSION})",
            registry.schema_version
        );
    }
    registry.schema_version = REGISTRY_SCHEMA_VERSION;
    Ok(registry)
}

fn load_registry_with_source(home: &Path) -> Result<(Registry, Option<Vec<u8>>)> {
    let path = registry_path(home);
    match fs::read(&path) {
        Ok(data) => Ok((parse_registry(&data)?, Some(data))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok((Registry::default(), None))
        }
        Err(error) => Err(error).with_context(|| format!("read {}", path.display())),
    }
}

pub fn save_registry(home: &Path, registry: &Registry) -> Result<()> {
    save_registry_if_unchanged(home, registry, None, false)
}

fn save_registry_if_unchanged(
    home: &Path,
    registry: &Registry,
    expected: Option<&[u8]>,
    enforce_expected: bool,
) -> Result<()> {
    let path = registry_path(home);
    if enforce_expected {
        let current = fs::read(&path).ok();
        if current.as_deref() != expected {
            bail!("registry changed in another process; please retry the action");
        }
    }

    let mut output = registry.clone();
    output.schema_version = REGISTRY_SCHEMA_VERSION;
    let mut data = serde_json::to_vec_pretty(&output)?;
    data.push(b'\n');
    if fs::read(&path).ok().as_deref() == Some(data.as_slice()) {
        harden_private_file(&path)?;
        return Ok(());
    }

    if let Ok(previous) = fs::read(&path) {
        atomic_write_private(&path.with_extension("json.bak"), &previous)?;
    }
    atomic_write_private(&path, &data)
}

pub fn sync_active_from_auth(home: &Path) -> Result<Registry> {
    let (mut registry, original) = load_registry_with_source(home)?;
    let key = read_optional(&active_auth_path(home))?
        .map(|data| parse_auth_bytes(&data))
        .transpose()?
        .and_then(|info| info.record_key)
        .filter(|key| {
            registry
                .accounts
                .iter()
                .any(|account| &account.account_key == key)
        });
    if registry.active_account_key == key {
        return Ok(registry);
    }

    registry.previous_active_account_key = registry.active_account_key.take();
    registry.active_account_key = key;
    registry.active_account_activated_at_ms =
        registry.active_account_key.as_ref().map(|_| now_millis());
    save_registry_if_unchanged(home, &registry, original.as_deref(), true)?;
    Ok(registry)
}

pub fn rename_account(home: &Path, account_key: &str, alias: &str) -> Result<Registry> {
    let (mut registry, original) = load_registry_with_source(home)?;
    let account = registry
        .accounts
        .iter_mut()
        .find(|account| account.account_key == account_key)
        .context("account no longer exists")?;
    account.alias = alias.trim().to_owned();
    save_registry_if_unchanged(home, &registry, original.as_deref(), true)?;
    Ok(registry)
}

pub fn remove_account(home: &Path, account_key: &str) -> Result<Registry> {
    let (mut registry, original) = load_registry_with_source(home)?;
    let index = registry
        .accounts
        .iter()
        .position(|account| account.account_key == account_key)
        .context("account no longer exists")?;
    let record = registry.accounts.remove(index);
    if registry.active_account_key.as_deref() == Some(account_key) {
        registry.active_account_key = None;
        registry.active_account_activated_at_ms = None;
    }
    if registry.previous_active_account_key.as_deref() == Some(account_key) {
        registry.previous_active_account_key = None;
    }
    save_registry_if_unchanged(home, &registry, original.as_deref(), true)?;

    let preferred = account_auth_path(home, &record.account_key);
    let legacy = accounts_dir(home).join(format!(
        "{}.auth.json",
        URL_SAFE_NO_PAD.encode(record.email.as_bytes())
    ));
    let _ = fs::remove_file(preferred);
    if legacy != account_auth_path(home, &record.account_key)
        && !registry
            .accounts
            .iter()
            .any(|other| other.email == record.email)
    {
        let _ = fs::remove_file(legacy);
    }
    Ok(registry)
}

/// Preserve rotated credentials only when the live identity matches the selection.
pub fn preserve_selected_live_credentials(home: &Path, account_key: &str) -> Result<()> {
    let Some(bytes) = read_optional(&active_auth_path(home))? else {
        return Ok(());
    };
    let info = parse_auth_bytes(&bytes)?;
    if info.kind == AuthKind::ChatGpt
        && info.record_key.as_deref() == Some(account_key)
        && info.access_token.is_some()
    {
        atomic_write_private(&account_auth_path(home, account_key), &bytes)?;
    }
    Ok(())
}

pub fn switch_account(home: &Path, account_key: &str) -> Result<Registry> {
    let (mut registry, original) = load_registry_with_source(home)?;
    let account = registry
        .accounts
        .iter()
        .find(|account| account.account_key == account_key)
        .context("account no longer exists")?;
    if account.is_api_key() {
        bail!("API-key accounts are not switchable in v1");
    }
    let snapshot_path = resolve_account_auth_path(home, account)?;
    let snapshot = fs::read(&snapshot_path)
        .with_context(|| format!("read credentials at {}", snapshot_path.display()))?;
    let target =
        parse_auth_bytes(&snapshot).context("validate stored credentials before switching")?;
    if target.kind != AuthKind::ChatGpt
        || target.record_key.as_deref() != Some(account_key)
        || target.access_token.is_none()
    {
        bail!("stored credentials do not match the selected account; sign in again");
    }

    let active_path = active_auth_path(home);
    let previous_auth = read_optional(&active_path)?;
    // The CLI may have rotated its tokens since this account was saved. Preserve
    // the live credentials when leaving it, never an older in-memory snapshot.
    let live_key = previous_auth
        .as_deref()
        .and_then(|data| parse_auth_bytes(data).ok())
        .and_then(|info| info.record_key);
    if let (Some(previous), Some(key)) = (&previous_auth, &live_key) {
        if key != account_key
            && registry
                .accounts
                .iter()
                .any(|account| &account.account_key == key)
        {
            atomic_write_private(&account_auth_path(home, key), previous)?;
        }
    }
    if previous_auth.as_deref() != Some(snapshot.as_slice()) {
        if let Some(previous) = previous_auth.as_deref() {
            atomic_write_private(&active_path.with_extension("json.bak"), previous)?;
        }
        atomic_write_private(&active_path, &snapshot)?;
    }

    let old_active = live_key.or_else(|| registry.active_account_key.clone());
    if old_active.as_deref() != Some(account_key) {
        registry.previous_active_account_key = old_active;
    }
    registry.active_account_key = Some(account_key.to_owned());
    registry.active_account_activated_at_ms = Some(now_millis());
    if let Some(account) = registry
        .accounts
        .iter_mut()
        .find(|account| account.account_key == account_key)
    {
        account.last_used_at = Some(now_seconds());
    }

    if let Err(error) = save_registry_if_unchanged(home, &registry, original.as_deref(), true) {
        rollback_write(&active_path, &snapshot, previous_auth.as_deref())?;
        return Err(error);
    }
    Ok(registry)
}

pub fn update_usage(
    home: &Path,
    account_key: &str,
    snapshot: RateLimitSnapshot,
) -> Result<Registry> {
    let (mut registry, original) = load_registry_with_source(home)?;
    let account = registry
        .accounts
        .iter_mut()
        .find(|account| account.account_key == account_key)
        .context("account no longer exists")?;
    account.last_usage = Some(snapshot);
    account.last_usage_at = Some(now_seconds());
    save_registry_if_unchanged(home, &registry, original.as_deref(), true)?;
    Ok(registry)
}

pub fn import_login_auth(home: &Path, auth_data: &[u8]) -> Result<Registry> {
    let info = parse_auth_bytes(auth_data)?;
    if info.access_token.is_none() {
        bail!("login result has no access token");
    }
    let key = info
        .record_key
        .clone()
        .context("login result is missing a stable account key")?;
    let (mut registry, original) = load_registry_with_source(home)?;
    let existing = registry
        .accounts
        .iter()
        .find(|account| account.account_key == key)
        .cloned();
    let alias = existing
        .as_ref()
        .map(|account| account.alias.clone())
        .unwrap_or_default();
    let mut new_record = info.into_account(alias, now_seconds())?;
    if let Some(old) = existing {
        new_record.created_at = old.created_at;
        new_record.last_used_at = old.last_used_at;
        new_record.last_usage = old.last_usage;
        new_record.last_usage_at = old.last_usage_at;
        new_record.last_local_rollout = old.last_local_rollout;
        new_record.account_name = old.account_name;
        new_record.extra = old.extra;
    }

    let snapshot_path = account_auth_path(home, &key);
    let previous_snapshot = read_optional(&snapshot_path)?;
    atomic_write_private(&snapshot_path, auth_data)?;

    if let Some(index) = registry
        .accounts
        .iter()
        .position(|account| account.account_key == key)
    {
        registry.accounts[index] = new_record;
    } else {
        registry.accounts.push(new_record);
    }
    // Import only saves the account. Activation must go through the same process
    // guard as an ordinary switch, even after a successful browser sign-in.
    if let Err(error) = save_registry_if_unchanged(home, &registry, original.as_deref(), true) {
        rollback_write(&snapshot_path, auth_data, previous_snapshot.as_deref())?;
        return Err(error);
    }
    Ok(registry)
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(data) => Ok(Some(data)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("read {}", path.display())),
    }
}

fn rollback_write(path: &Path, written: &[u8], previous: Option<&[u8]>) -> Result<()> {
    // Do not undo another application's subsequent credential update.
    if read_optional(path)?.as_deref() != Some(written) {
        bail!("credentials changed in another process; left its newer data intact");
    }
    match previous {
        Some(data) => atomic_write_private(path, data),
        None => fs::remove_file(path).context("remove uncommitted credentials"),
    }
}

pub(crate) fn create_private_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)?;
    }
    #[cfg(not(unix))]
    fs::create_dir_all(path)?;
    harden_private_directory(path)
}

pub fn atomic_write_private(path: &Path, data: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .context("target path has no parent directory")?;
    fs::create_dir_all(parent)
        .with_context(|| format!("create private directory {}", parent.display()))?;
    harden_private_directory(parent)?;

    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .context("target path has no valid file name")?;
    let temporary = parent.join(format!(
        ".{file_name}.tmp.{}.{}",
        std::process::id(),
        now_nanos()
    ));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .with_context(|| format!("create temporary file {}", temporary.display()))?;
    let result = (|| -> Result<()> {
        file.write_all(data)?;
        file.sync_all()?;
        drop(file);
        // std::fs::rename replaces the destination on Windows too. Deleting it
        // first would leave credentials missing if replacement failed.
        fs::rename(&temporary, path)
            .with_context(|| format!("replace {} atomically", path.display()))?;
        harden_private_file(path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(unix)]
fn harden_private_directory(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn harden_private_directory(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn harden_private_file(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn harden_private_file(_path: &Path) -> Result<()> {
    Ok(())
}

pub fn now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn now_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use serde_json::json;

    fn temp_home(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "codex-account-hub-{name}-{}-{}",
            std::process::id(),
            now_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn chatgpt_auth_bytes(email: &str, user: &str, account: &str) -> Vec<u8> {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#);
        let claims = json!({
            "email": email,
            "https://api.openai.com/auth": {
                "chatgpt_account_id": account,
                "chatgpt_user_id": user,
                "chatgpt_plan_type": "pro"
            }
        });
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap());
        serde_json::to_vec(&json!({
            "tokens": {
                "id_token": format!("{header}.{payload}.sig"),
                "access_token": "access",
                "account_id": account
            }
        }))
        .unwrap()
    }

    #[test]
    fn filename_encoding_matches_codex_auth() {
        assert_eq!(account_file_key("normal-key_1.json"), "normal-key_1.json");
        assert_eq!(
            account_file_key("user-a::account-a"),
            URL_SAFE_NO_PAD.encode("user-a::account-a")
        );
    }

    #[test]
    fn reload_clears_selection_after_logout_or_unknown_account() {
        let home = temp_home("reload-logout");
        let auth = chatgpt_auth_bytes("sample@example.com", "one", "a");
        import_login_auth(&home, &auth).unwrap();
        switch_account(&home, "one::a").unwrap();
        fs::remove_file(active_auth_path(&home)).unwrap();
        let logged_out = sync_active_from_auth(&home).unwrap();
        assert!(logged_out.active_account_key.is_none());
        assert!(logged_out.active_account_activated_at_ms.is_none());
        assert_eq!(logged_out.accounts.len(), 1);
        switch_account(&home, "one::a").unwrap();
        let other = chatgpt_auth_bytes("other@example.com", "other", "b");
        atomic_write_private(&active_auth_path(&home), &other).unwrap();
        assert!(sync_active_from_auth(&home)
            .unwrap()
            .active_account_key
            .is_none());
        assert_eq!(fs::read(active_auth_path(&home)).unwrap(), other);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn malformed_live_auth_is_reported_without_changing_registry() {
        let home = temp_home("reload-malformed");
        let auth = chatgpt_auth_bytes("sample@example.com", "one", "a");
        import_login_auth(&home, &auth).unwrap();
        switch_account(&home, "one::a").unwrap();
        let before = fs::read(registry_path(&home)).unwrap();
        atomic_write_private(&active_auth_path(&home), b"invalid-json").unwrap();
        assert!(sync_active_from_auth(&home).is_err());
        assert_eq!(fs::read(registry_path(&home)).unwrap(), before);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn registry_write_creates_backup_and_round_trips() {
        let home = temp_home("registry");
        let mut registry = Registry::default();
        save_registry(&home, &registry).unwrap();
        registry.previous_active_account_key = Some("old".into());
        save_registry(&home, &registry).unwrap();
        assert!(registry_path(&home).with_extension("json.bak").is_file());
        assert_eq!(
            load_registry(&home)
                .unwrap()
                .previous_active_account_key
                .as_deref(),
            Some("old")
        );
        fs::remove_dir_all(home).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn private_writes_use_owner_only_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let home = temp_home("permissions");
        let path = home.join("private.json");
        atomic_write_private(&path, b"{}").unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn expired_schema_is_rejected_without_rewrite() {
        let result = parse_registry(br#"{"schema_version":999,"accounts":[]}"#);
        assert!(result.unwrap_err().to_string().contains("newer"));
    }

    #[test]
    fn api_key_records_are_not_switchable() {
        let home = temp_home("apikey");
        let mut registry = Registry::default();
        registry.accounts.push(AccountRecord {
            account_key: "apikey:user".into(),
            chatgpt_account_id: String::new(),
            chatgpt_user_id: "user".into(),
            email: "a@example.com".into(),
            alias: String::new(),
            account_name: None,
            plan: None,
            auth_mode: Some("apikey".into()),
            created_at: 1,
            last_used_at: None,
            last_usage: None,
            last_usage_at: None,
            last_local_rollout: None,
            extra: BTreeMap::new(),
        });
        save_registry(&home, &registry).unwrap();
        assert!(switch_account(&home, "apikey:user").is_err());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn switch_copies_auth_and_updates_active_and_previous() {
        let home = temp_home("switch");
        let first = chatgpt_auth_bytes("one@example.com", "user-one", "account-one");
        let second = chatgpt_auth_bytes("two@example.com", "user-two", "account-two");
        import_login_auth(&home, &first).unwrap();
        import_login_auth(&home, &second).unwrap();
        switch_account(&home, "user-two::account-two").unwrap();

        let registry = switch_account(&home, "user-one::account-one").unwrap();
        assert_eq!(
            registry.active_account_key.as_deref(),
            Some("user-one::account-one")
        );
        assert_eq!(
            registry.previous_active_account_key.as_deref(),
            Some("user-two::account-two")
        );
        assert_eq!(fs::read(active_auth_path(&home)).unwrap(), first);
        assert!(registry.accounts[0].last_used_at.is_some());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn login_import_preserves_live_credentials_until_guarded_activation() {
        let home = temp_home("import-guard");
        let first = chatgpt_auth_bytes("one@example.com", "one", "a");
        let second = chatgpt_auth_bytes("two@example.com", "two", "b");
        import_login_auth(&home, &first).unwrap();
        assert!(!active_auth_path(&home).exists());
        switch_account(&home, "one::a").unwrap();
        let registry = import_login_auth(&home, &second).unwrap();
        assert_eq!(registry.active_account_key.as_deref(), Some("one::a"));
        assert_eq!(fs::read(active_auth_path(&home)).unwrap(), first);
        assert_eq!(registry.accounts.len(), 2);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn refreshed_live_tokens_survive_switching_away_and_back() {
        let home = temp_home("rotation");
        let first = chatgpt_auth_bytes("one@example.com", "one", "a");
        let second = chatgpt_auth_bytes("two@example.com", "two", "b");
        import_login_auth(&home, &first).unwrap();
        import_login_auth(&home, &second).unwrap();
        switch_account(&home, "one::a").unwrap();
        let mut refreshed: serde_json::Value = serde_json::from_slice(&first).unwrap();
        refreshed["tokens"]["access_token"] = json!("rotated-access");
        refreshed["tokens"]["refresh_token"] = json!("rotated-refresh");
        let refreshed = serde_json::to_vec(&refreshed).unwrap();
        atomic_write_private(&active_auth_path(&home), &refreshed).unwrap();
        let registry = load_registry(&home).unwrap();
        assert_eq!(
            resolve_quota_auth_path(&home, &registry.accounts[0]).unwrap(),
            active_auth_path(&home)
        );
        switch_account(&home, "two::b").unwrap();
        assert_eq!(
            fs::read(account_auth_path(&home, "one::a")).unwrap(),
            refreshed
        );
        switch_account(&home, "one::a").unwrap();
        assert_eq!(fs::read(active_auth_path(&home)).unwrap(), refreshed);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn restart_preserves_rotated_tokens_without_importing_another_identity() {
        let home = temp_home("restart-rotation");
        let original = chatgpt_auth_bytes("one@example.com", "one", "a");
        import_login_auth(&home, &original).unwrap();
        switch_account(&home, "one::a").unwrap();
        let mut refreshed: serde_json::Value = serde_json::from_slice(&original).unwrap();
        refreshed["tokens"]["access_token"] = json!("fresh-before-restart");
        let refreshed = serde_json::to_vec(&refreshed).unwrap();
        atomic_write_private(&active_auth_path(&home), &refreshed).unwrap();
        preserve_selected_live_credentials(&home, "one::a").unwrap();
        switch_account(&home, "one::a").unwrap();
        assert_eq!(fs::read(active_auth_path(&home)).unwrap(), refreshed);
        // A quitting client can flush the account it was using previously.
        let previous = chatgpt_auth_bytes("two@example.com", "two", "b");
        atomic_write_private(&active_auth_path(&home), &previous).unwrap();
        preserve_selected_live_credentials(&home, "one::a").unwrap();
        assert_eq!(
            fs::read(account_auth_path(&home, "one::a")).unwrap(),
            refreshed
        );
        switch_account(&home, "one::a").unwrap();
        assert_eq!(fs::read(active_auth_path(&home)).unwrap(), refreshed);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn mismatched_credentials_cannot_activate_another_account() {
        let home = temp_home("mismatch");
        let first = chatgpt_auth_bytes("one@example.com", "one", "a");
        let second = chatgpt_auth_bytes("two@example.com", "two", "b");
        import_login_auth(&home, &first).unwrap();
        atomic_write_private(&account_auth_path(&home, "one::a"), &second).unwrap();
        assert!(switch_account(&home, "one::a").is_err());
        assert!(!active_auth_path(&home).exists());
        let registry = load_registry(&home).unwrap();
        assert!(resolve_quota_auth_path(&home, &registry.accounts[0]).is_err());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn failed_registry_commit_rolls_back_a_first_activation() {
        let home = temp_home("rollback-new");
        let first = chatgpt_auth_bytes("one@example.com", "one", "a");
        import_login_auth(&home, &first).unwrap();
        // Make the backup destination unwritable as a file to inject a commit failure.
        fs::create_dir(registry_path(&home).with_extension("json.bak")).unwrap();
        assert!(switch_account(&home, "one::a").is_err());
        assert!(!active_auth_path(&home).exists());
        assert!(load_registry(&home).unwrap().active_account_key.is_none());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn rollback_leaves_another_writers_new_credentials_intact() {
        let home = temp_home("rollback-conflict");
        let path = active_auth_path(&home);
        atomic_write_private(&path, b"newer-writer").unwrap();
        assert!(rollback_write(&path, b"our-write", Some(b"old")).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"newer-writer");
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn private_atomic_write_replaces_an_existing_file() {
        let home = temp_home("replace");
        let path = home.join("replace.json");
        atomic_write_private(&path, b"old").unwrap();
        atomic_write_private(&path, b"new").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new");
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn removal_preserves_legacy_credentials_shared_by_another_workspace() {
        let home = temp_home("legacy-shared");
        let first = chatgpt_auth_bytes("shared@example.com", "one", "a");
        let second = chatgpt_auth_bytes("shared@example.com", "one", "b");
        import_login_auth(&home, &first).unwrap();
        import_login_auth(&home, &second).unwrap();
        let legacy = accounts_dir(&home).join(format!(
            "{}.auth.json",
            URL_SAFE_NO_PAD.encode("shared@example.com")
        ));
        atomic_write_private(&legacy, &second).unwrap();
        remove_account(&home, "one::a").unwrap();
        assert_eq!(fs::read(&legacy).unwrap(), second);
        assert!(account_auth_path(&home, "one::b").exists());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn remove_is_persisted_and_deletes_snapshot() {
        let home = temp_home("remove");
        let auth = chatgpt_auth_bytes("one@example.com", "user-one", "account-one");
        import_login_auth(&home, &auth).unwrap();
        let snapshot = account_auth_path(&home, "user-one::account-one");
        assert!(snapshot.is_file());

        let registry = remove_account(&home, "user-one::account-one").unwrap();
        assert!(registry.accounts.is_empty());
        assert!(registry.active_account_key.is_none());
        assert!(!snapshot.exists());
        assert!(load_registry(&home).unwrap().accounts.is_empty());
        fs::remove_dir_all(home).unwrap();
    }
}
