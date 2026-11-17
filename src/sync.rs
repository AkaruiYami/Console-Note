use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::Engine;
use reqwest::StatusCode;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::config::Config;
use crate::note::Note;
use crate::storage::Storage;

const SERVICE: &str = "console_note_github";
const API: &str = "https://api.github.com";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncState {
    pub client_id: String,
    pub repository: String,
    pub branch: String,
    pub notes_dir: std::path::PathBuf,
    pub baseline: BTreeMap<u64, String>,
    pub last_success: Option<u64>,
    #[serde(default)]
    pending_conflict: Option<PendingConflict>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingConflict {
    original_id: u64,
    copy_id: u64,
    local_content: String,
}

impl SyncState {
    fn path(config: &Config) -> std::path::PathBuf {
        config.config_dir.join("sync.json")
    }

    pub fn load(config: &Config) -> io::Result<Option<Self>> {
        let path = Self::path(config);
        if !path.exists() {
            return Ok(None);
        }
        let raw = fs::read(path)?;
        serde_json::from_slice(&raw)
            .map(Some)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
    }

    fn save(&self, config: &Config) -> io::Result<()> {
        let path = Self::path(config);
        let temporary = path.with_extension("json.tmp");
        let raw = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        fs::write(&temporary, raw)?;
        fs::rename(temporary, path)
    }
}

fn error(message: impl ToString) -> io::Error {
    io::Error::other(message.to_string())
}

fn credential(config: &Config) -> io::Result<keyring::Entry> {
    if let Err(err) = keyring::Entry::store_status() {
        return Err(error(format!(
            "OS credential store is unavailable: {err}. On Linux, enable and unlock a Secret Service provider (org.freedesktop.secrets), then rerun sync setup"
        )));
    }
    keyring::Entry::new(SERVICE, &config.config_dir.to_string_lossy())
        .map_err(|err| error(format!("cannot open OS credential store: {err}")))
}

fn setup_credential(config: &Config) -> io::Result<keyring::Entry> {
    let entry = credential(config)?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(error)?
        .as_nanos();
    let probe_user = format!("console_note_setup_probe_{}_{}", std::process::id(), nonce);
    let probe = keyring::Entry::new(SERVICE, &probe_user)
        .map_err(|err| error(format!("cannot open OS credential store: {err}")))?;
    if let Err(err) = probe.set_password("console_note_keyring_check") {
        let _ = probe.delete_credential();
        return Err(error(format!(
            "OS credential store cannot save tokens: {err}. On Linux, enable and unlock a Secret Service provider (org.freedesktop.secrets), then rerun sync setup"
        )));
    }
    let read_result = probe.get_password();
    let delete_result = probe.delete_credential();
    let saved = read_result.map_err(|err| {
        error(format!(
            "OS credential store cannot read saved tokens: {err}"
        ))
    })?;
    delete_result.map_err(|err| {
        error(format!(
            "OS credential store could not remove its setup check: {err}"
        ))
    })?;
    if saved != "console_note_keyring_check" {
        return Err(error("OS credential store failed its setup check"));
    }
    Ok(entry)
}

fn client() -> io::Result<Client> {
    Client::builder()
        .user_agent("console_note/0.2")
        .timeout(Duration::from_secs(20))
        .default_headers({
            let mut headers = reqwest::header::HeaderMap::new();
            headers.insert(
                "Accept",
                "application/vnd.github+json"
                    .parse()
                    .expect("static header"),
            );
            headers.insert(
                "X-GitHub-Api-Version",
                "2022-11-28".parse().expect("static header"),
            );
            headers
        })
        .build()
        .map_err(error)
}

fn response_json(response: reqwest::blocking::Response) -> io::Result<Value> {
    let status = response.status();
    let body: Value = response.json().map_err(error)?;
    if !status.is_success() {
        let message = body["message"].as_str().unwrap_or("GitHub request failed");
        return Err(error(format!("GitHub {status}: {message}")));
    }
    Ok(body)
}

fn token(config: &Config, state: &SyncState, http: &Client) -> io::Result<String> {
    let entry = credential(config)?;
    let stored = entry.get_password().map_err(error)?;
    let mut auth: Value = serde_json::from_str(&stored).map_err(error)?;
    let expires = auth["expires_at"].as_u64().unwrap_or(0);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(error)?
        .as_secs();
    if expires > now + 60 {
        return auth["access_token"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| error("stored GitHub token is invalid"));
    }
    let refresh = auth["refresh_token"]
        .as_str()
        .ok_or_else(|| error("GitHub login expired; run sync setup again"))?;
    let reply = response_json(
        http.post("https://github.com/login/oauth/access_token")
            .header("Accept", "application/json")
            .form(&[
                ("client_id", state.client_id.as_str()),
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh),
            ])
            .send()
            .map_err(error)?,
    )?;
    if reply["error"].is_string() {
        return Err(error("GitHub login expired; run sync setup again"));
    }
    auth = auth_value(&reply)?;
    entry.set_password(&auth.to_string()).map_err(error)?;
    auth["access_token"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| error("GitHub token response is invalid"))
}

fn auth_value(reply: &Value) -> io::Result<Value> {
    let access = reply["access_token"]
        .as_str()
        .ok_or_else(|| error("GitHub did not provide an access token"))?;
    let expires = reply["expires_in"].as_u64();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(error)?
        .as_secs();
    Ok(json!({
        "access_token": access,
        "refresh_token": reply["refresh_token"],
        "expires_at": expires.map_or(u64::MAX, |seconds| now.saturating_add(seconds)),
    }))
}

fn api_get(http: &Client, access: &str, url: &str) -> io::Result<Value> {
    response_json(http.get(url).bearer_auth(access).send().map_err(error)?)
}

pub fn setup(config: &Config, client_id: &str, target: &str) -> io::Result<()> {
    setup_with_preflight(config, client_id, target, || setup_credential(config))
}

fn setup_with_preflight<F>(
    config: &Config,
    client_id: &str,
    target: &str,
    preflight: F,
) -> io::Result<()>
where
    F: FnOnce() -> io::Result<keyring::Entry>,
{
    if SyncState::load(config)?.is_some() {
        return Err(error(
            "sync is already configured; disconnect it before setting up another repository",
        ));
    }
    if client_id.trim().is_empty() || target.trim().is_empty() {
        return Err(error("client ID and repository are required"));
    }
    let entry = preflight()?;
    let http = client()?;
    let code = response_json(
        http.post("https://github.com/login/device/code")
            .header("Accept", "application/json")
            .form(&[("client_id", client_id)])
            .send()
            .map_err(error)?,
    )?;
    let device_code = code["device_code"]
        .as_str()
        .ok_or_else(|| error("missing device code"))?;
    let user_code = code["user_code"]
        .as_str()
        .ok_or_else(|| error("missing user code"))?;
    let uri = code["verification_uri"]
        .as_str()
        .unwrap_or("https://github.com/login/device");
    println!("Open {uri} and enter code {user_code}");
    io::stdout().flush()?;
    let mut interval = code["interval"].as_u64().unwrap_or(5).max(1);
    let deadline = Instant::now() + Duration::from_secs(code["expires_in"].as_u64().unwrap_or(900));
    let access_reply = loop {
        if Instant::now() + Duration::from_secs(interval) >= deadline {
            return Err(error("GitHub device code expired"));
        }
        std::thread::sleep(Duration::from_secs(interval));
        let reply = response_json(
            http.post("https://github.com/login/oauth/access_token")
                .header("Accept", "application/json")
                .form(&[
                    ("client_id", client_id),
                    ("device_code", device_code),
                    ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ])
                .send()
                .map_err(error)?,
        )?;
        if reply["access_token"].is_string() {
            break reply;
        }
        match reply["error"].as_str() {
            Some("authorization_pending") => {}
            Some("slow_down") => interval += 5,
            other => {
                return Err(error(format!(
                    "GitHub login: {}",
                    other.unwrap_or("unknown error")
                )));
            }
        }
    };
    let auth = auth_value(&access_reply)?;
    let access = auth["access_token"]
        .as_str()
        .ok_or_else(|| error("missing token"))?;
    let repository = if let Some(name) = target.strip_prefix("new:") {
        if name.is_empty() || name.contains('/') {
            return Err(error("new repository name must be a single name"));
        }
        let repo = response_json(
            http.post(format!("{API}/user/repos"))
                .bearer_auth(access)
                .json(&json!({"name": name, "private": true, "auto_init": true}))
                .send()
                .map_err(error)?,
        )?;
        repo["full_name"]
            .as_str()
            .ok_or_else(|| error("missing repository name"))?
            .to_string()
    } else {
        target.to_string()
    };
    let parts: Vec<_> = repository.split('/').collect();
    if parts.len() != 2
        || parts.iter().any(|part| {
            part.is_empty()
                || !part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        })
    {
        return Err(error("repository must be OWNER/REPO"));
    }
    let repo = api_get(&http, access, &format!("{API}/repos/{repository}"))?;
    if repo["private"] != true {
        return Err(error("repository must be private"));
    }
    let branch = repo["default_branch"]
        .as_str()
        .ok_or_else(|| error("repository has no default branch"))?;
    let state = SyncState {
        client_id: client_id.to_string(),
        repository,
        branch: branch.to_string(),
        notes_dir: config.notes_dir.clone(),
        baseline: BTreeMap::new(),
        last_success: None,
        pending_conflict: None,
    };
    persist_setup(
        config,
        &state,
        || entry.set_password(&auth.to_string()).map_err(error),
        || entry.delete_credential().map_err(error),
    )?;
    println!("Connected to {}", state.repository);
    Ok(())
}

fn persist_setup<W, D>(
    config: &Config,
    state: &SyncState,
    write_token: W,
    delete_token: D,
) -> io::Result<()>
where
    W: FnOnce() -> io::Result<()>,
    D: FnOnce() -> io::Result<()>,
{
    write_token()?;
    if let Err(save_error) = state.save(config) {
        if let Err(cleanup_error) = delete_token() {
            return Err(error(format!(
                "sync settings could not be saved: {save_error}; stored token cleanup failed: {cleanup_error}"
            )));
        }
        return Err(save_error);
    }
    Ok(())
}

pub fn disconnect(config: &Config) -> io::Result<()> {
    match credential(config)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => {}
        Err(err) => return Err(error(err)),
    }
    let path = SyncState::path(config);
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct RemoteFile {
    content: String,
    sha: String,
}

fn remote_files(
    http: &Client,
    access: &str,
    state: &SyncState,
) -> io::Result<BTreeMap<u64, RemoteFile>> {
    let url = format!("{API}/repos/{}/contents/notes", state.repository);
    let response = http
        .get(url)
        .bearer_auth(access)
        .query(&[("ref", &state.branch)])
        .send()
        .map_err(error)?;
    if response.status() == StatusCode::NOT_FOUND {
        return Ok(BTreeMap::new());
    }
    let listing = response_json(response)?;
    let entries = listing
        .as_array()
        .ok_or_else(|| error("notes path is not a directory"))?;
    if entries.len() >= 1000 {
        return Err(error(
            "GitHub notes directory has 1000 or more entries; sync cannot list all notes",
        ));
    }
    let mut files = BTreeMap::new();
    for entry in entries {
        let Some(id) = entry["name"].as_str().and_then(note_id) else {
            continue;
        };
        let path = entry["path"]
            .as_str()
            .ok_or_else(|| error("invalid GitHub file path"))?;
        let detail = response_json(
            http.get(format!("{API}/repos/{}/contents/{path}", state.repository))
                .bearer_auth(access)
                .query(&[("ref", &state.branch)])
                .send()
                .map_err(error)?,
        )?;
        let encoded = detail["content"]
            .as_str()
            .ok_or_else(|| error("missing note content"))?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded.replace('\n', ""))
            .map_err(error)?;
        let content = String::from_utf8(bytes).map_err(error)?;
        let note = Note::from_file_content(&content).map_err(error)?;
        if note.id != id {
            return Err(error(format!("note ID does not match filename: {path}")));
        }
        let sha = detail["sha"]
            .as_str()
            .ok_or_else(|| error("missing note SHA"))?;
        files.insert(
            id,
            RemoteFile {
                content,
                sha: sha.to_string(),
            },
        );
    }
    Ok(files)
}

fn note_id(name: &str) -> Option<u64> {
    name.strip_suffix(".md")?.parse().ok()
}

fn local_files(dir: &Path) -> io::Result<BTreeMap<u64, String>> {
    let mut files = BTreeMap::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let Some(id) = entry.file_name().to_str().and_then(note_id) else {
            continue;
        };
        let content = fs::read_to_string(entry.path())?;
        let note = Note::from_file_content(&content).map_err(error)?;
        if note.id != id {
            return Err(error(format!("note ID does not match filename: {id}.md")));
        }
        files.insert(id, content);
    }
    Ok(files)
}

fn write_local(dir: &Path, id: u64, content: Option<&str>) -> io::Result<()> {
    let path = dir.join(format!("{id}.md"));
    if let Some(content) = content {
        let temporary = dir.join(format!(".{id}.sync.tmp"));
        fs::write(&temporary, content)?;
        fs::rename(temporary, path)
    } else if path.exists() {
        fs::remove_file(path)
    } else {
        Ok(())
    }
}

fn write_local_if_unchanged(
    dir: &Path,
    id: u64,
    expected: Option<&str>,
    content: Option<&str>,
) -> io::Result<()> {
    let current = match fs::read_to_string(dir.join(format!("{id}.md"))) {
        Ok(content) => Some(content),
        Err(err) if err.kind() == io::ErrorKind::NotFound => None,
        Err(err) => return Err(err),
    };
    if current.as_deref() != expected {
        return Err(error(
            "note changed during sync; retry after the current save completes",
        ));
    }
    write_local(dir, id, content)
}

fn write_remote(
    http: &Client,
    access: &str,
    state: &SyncState,
    id: u64,
    content: Option<&str>,
    sha: Option<&str>,
) -> io::Result<()> {
    let url = format!("{API}/repos/{}/contents/notes/{id}.md", state.repository);
    let message = format!("Sync note {id}");
    let response = if let Some(content) = content {
        let mut body = json!({"message": message, "branch": state.branch,
            "content": base64::engine::general_purpose::STANDARD.encode(content)});
        if let Some(sha) = sha {
            body["sha"] = json!(sha);
        }
        http.put(url)
            .bearer_auth(access)
            .json(&body)
            .send()
            .map_err(error)?
    } else {
        let Some(sha) = sha else { return Ok(()) };
        http.delete(url)
            .bearer_auth(access)
            .json(&json!({"message": message, "branch": state.branch, "sha": sha}))
            .send()
            .map_err(error)?
    };
    response_json(response).map(|_| ())
}

fn next_free_id(
    local: &BTreeMap<u64, String>,
    remote: &BTreeMap<u64, RemoteFile>,
    baseline: &BTreeMap<u64, String>,
) -> io::Result<u64> {
    local
        .keys()
        .chain(remote.keys())
        .chain(baseline.keys())
        .copied()
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| error("note ID limit reached"))
}

fn copied_note(content: &str, new_id: u64) -> io::Result<String> {
    let mut note = Note::from_file_content(content).map_err(error)?;
    note.id = new_id;
    note.title = format!("{} (sync conflict)", note.title);
    Ok(note.to_file_content())
}

#[derive(Debug, PartialEq)]
enum Decision {
    Unchanged,
    Pull,
    Push,
    Conflict,
}

fn decide(local: Option<&str>, remote: Option<&str>, baseline: Option<&str>) -> Decision {
    if local == remote {
        Decision::Unchanged
    } else if local == baseline {
        Decision::Pull
    } else if remote == baseline {
        Decision::Push
    } else {
        Decision::Conflict
    }
}

pub fn run(config: &Config) -> io::Result<String> {
    if SyncState::load(config)?.is_none() {
        return Ok("Sync is not configured.".to_string());
    }
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(config.config_dir.join("sync.lock"))?;
    lock.try_lock()
        .map_err(|_| error("another sync is already running"))?;
    for attempt in 0..3 {
        match run_once(config) {
            Err(err) if attempt < 2 && err.to_string().starts_with("GitHub 409") => continue,
            result => return result,
        }
    }
    unreachable!("three attempts always return")
}

fn run_once(config: &Config) -> io::Result<String> {
    let Some(mut state) = SyncState::load(config)? else {
        return Ok("Sync is not configured.".to_string());
    };
    if state.notes_dir != config.notes_dir {
        return Err(error(
            "notes directory changed since sync setup; restore the directory or disconnect sync",
        ));
    }
    let http = client()?;
    let access = token(config, &state, &http)?;
    let mut remote = remote_files(&http, &access, &state)?;
    if state.pending_conflict.is_some() {
        finish_pending_conflict(config, &http, &access, &mut state, &remote)?;
        remote = remote_files(&http, &access, &state)?;
    }
    let local = local_files(&config.notes_dir)?;
    let ids: BTreeSet<u64> = local
        .keys()
        .chain(remote.keys())
        .chain(state.baseline.keys())
        .copied()
        .collect();
    let mut changes = 0;
    let mut conflicts = 0;
    let mut free_id = next_free_id(&local, &remote, &state.baseline)?;
    for id in ids {
        let local_content = local.get(&id).map(String::as_str);
        let remote_file = remote.get(&id);
        let remote_content = remote_file.map(|f| f.content.as_str());
        let base = state.baseline.get(&id).map(String::as_str);
        match decide(local_content, remote_content, base) {
            Decision::Unchanged => set_baseline(&mut state, id, local_content),
            Decision::Pull => {
                write_local_if_unchanged(&config.notes_dir, id, local_content, remote_content)?;
                set_baseline(&mut state, id, remote_content);
                state.save(config)?;
                changes += 1;
            }
            Decision::Push => {
                write_remote(
                    &http,
                    &access,
                    &state,
                    id,
                    local_content,
                    remote_file.map(|f| f.sha.as_str()),
                )?;
                set_baseline(&mut state, id, local_content);
                state.save(config)?;
                changes += 1;
            }
            Decision::Conflict => {
                if let Some(content) = local_content {
                    state.pending_conflict = Some(PendingConflict {
                        original_id: id,
                        copy_id: free_id,
                        local_content: content.to_string(),
                    });
                    state.save(config)?;
                    finish_pending_conflict(config, &http, &access, &mut state, &remote)?;
                    free_id =
                        next_free_id(&local_files(&config.notes_dir)?, &remote, &state.baseline)?;
                } else {
                    write_local_if_unchanged(&config.notes_dir, id, local_content, remote_content)?;
                    set_baseline(&mut state, id, remote_content);
                    state.save(config)?;
                }
                conflicts += 1;
            }
        }
    }
    for (id, content) in local_files(&config.notes_dir)? {
        if !local.contains_key(&id) && !state.baseline.contains_key(&id) {
            write_remote(&http, &access, &state, id, Some(&content), None)?;
            state.baseline.insert(id, content);
            state.save(config)?;
            changes += 1;
        }
    }
    Storage::new(config).reconcile_counter()?;
    state.last_success = Some(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(error)?
            .as_secs(),
    );
    state.save(config)?;
    Ok(format!(
        "Synced {changes} change(s), preserved {conflicts} conflict(s)."
    ))
}

fn finish_pending_conflict(
    config: &Config,
    http: &Client,
    access: &str,
    state: &mut SyncState,
    remote: &BTreeMap<u64, RemoteFile>,
) -> io::Result<()> {
    let mut pending = state
        .pending_conflict
        .clone()
        .ok_or_else(|| error("missing conflict state"))?;
    let original_path = config.notes_dir.join(format!("{}.md", pending.original_id));
    let local = local_files(&config.notes_dir)?;
    let old_copy = copied_note(&pending.local_content, pending.copy_id)?;
    if remote
        .get(&pending.copy_id)
        .is_some_and(|file| file.content != old_copy)
    {
        let old_id = pending.copy_id;
        pending.copy_id = next_free_id(&local, remote, &state.baseline)?;
        state.pending_conflict = Some(pending.clone());
        state.save(config)?;
        if local
            .get(&old_id)
            .is_some_and(|content| content == &old_copy)
        {
            write_local(&config.notes_dir, old_id, None)?;
        }
    }
    let copy = copied_note(&pending.local_content, pending.copy_id)?;
    match remote.get(&pending.copy_id) {
        Some(file) if file.content == copy => {}
        Some(_) => unreachable!("new copy ID exceeds all remote IDs"),
        None => write_remote(http, access, state, pending.copy_id, Some(&copy), None)?,
    }
    if original_path.exists() {
        let current = fs::read_to_string(&original_path)?;
        if current != pending.local_content {
            let current_note = Note::from_file_content(&current).map_err(error)?;
            let copy_title = format!("{} (sync conflict)", current_note.title);
            let local = local_files(&config.notes_dir)?;
            let already_copied = local
                .values()
                .filter_map(|raw| Note::from_file_content(raw).ok())
                .any(|note| note.title == copy_title && note.content == current_note.content);
            if !already_copied {
                let new_id = next_free_id(&local, remote, &state.baseline)?.max(
                    pending
                        .copy_id
                        .checked_add(1)
                        .ok_or_else(|| error("note ID limit reached"))?,
                );
                let extra_copy = copied_note(&current, new_id)?;
                write_local(&config.notes_dir, new_id, Some(&extra_copy))?;
            }
        }
    }
    write_local(&config.notes_dir, pending.copy_id, Some(&copy))?;
    state.baseline.insert(pending.copy_id, copy);
    state.save(config)?;
    let remote_content = remote
        .get(&pending.original_id)
        .map(|file| file.content.as_str());
    write_local(&config.notes_dir, pending.original_id, remote_content)?;
    set_baseline(state, pending.original_id, remote_content);
    state.pending_conflict = None;
    state.save(config)
}

fn set_baseline(state: &mut SyncState, id: u64, content: Option<&str>) {
    if let Some(content) = content {
        state.baseline.insert(id, content.to_string());
    } else {
        state.baseline.remove(&id);
    }
}

pub fn status(config: &Config) -> io::Result<String> {
    let Some(state) = SyncState::load(config)? else {
        return Ok("Sync is not configured.".to_string());
    };
    Ok(format!(
        "GitHub: {} ({}), last success: {}",
        state.repository,
        state.branch,
        state
            .last_success
            .map_or_else(|| "never".to_string(), |time| time.to_string())
    ))
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    fn test_config(tag: &str) -> Config {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let config_dir = std::env::temp_dir().join(format!("console_note_{tag}_{nonce}"));
        fs::create_dir_all(&config_dir).expect("create test directory");
        Config {
            editor: "test".to_string(),
            notes_dir: config_dir.join("notes"),
            config_dir,
            hide_extension: true,
        }
    }

    fn test_state(config: &Config) -> SyncState {
        SyncState {
            client_id: "client".to_string(),
            repository: "owner/private-notes".to_string(),
            branch: "main".to_string(),
            notes_dir: config.notes_dir.clone(),
            baseline: BTreeMap::new(),
            last_success: None,
            pending_conflict: None,
        }
    }

    #[test]
    fn unavailable_keyring_stops_setup_before_github_login() {
        let config = test_config("preflight");
        let result = setup_with_preflight(&config, "client", "owner/private-notes", || {
            Err(error("Secret Service is unavailable"))
        });
        assert!(result.is_err());
        assert!(
            result
                .expect_err("setup must fail")
                .to_string()
                .contains("Secret Service")
        );
        assert!(!SyncState::path(&config).exists());
        fs::remove_dir_all(config.config_dir).expect("remove test directory");
    }

    #[test]
    fn failed_token_storage_leaves_sync_unconfigured() {
        let config = test_config("token_failure");
        let state = test_state(&config);
        let result = persist_setup(
            &config,
            &state,
            || Err(error("keyring is locked")),
            || panic!("cleanup should not run when token was never saved"),
        );
        assert!(result.is_err());
        assert!(!SyncState::path(&config).exists());
        fs::remove_dir_all(config.config_dir).expect("remove test directory");
    }

    #[test]
    fn failed_settings_save_removes_the_stored_token() {
        let mut config = test_config("settings_failure");
        let parent_dir = config.config_dir.clone();
        config.config_dir = parent_dir.join("not_a_directory");
        fs::write(&config.config_dir, "occupied").expect("create blocking file");
        let state = test_state(&config);
        let cleaned = Cell::new(false);
        let result = persist_setup(
            &config,
            &state,
            || Ok(()),
            || {
                cleaned.set(true);
                Ok(())
            },
        );
        assert!(result.is_err());
        assert!(cleaned.get());
        fs::remove_dir_all(parent_dir).expect("remove test directory");
    }

    #[test]
    fn copy_preserves_body_and_assigns_new_id() {
        let original = "---\nid=2\ntitle=Test\ncreated=now\nupdated=now\n---\nbody";
        let copy = copied_note(original, 8).expect("copy note");
        let note = Note::from_file_content(&copy).expect("parse note");
        assert_eq!(note.id, 8);
        assert_eq!(note.content, "body");
        assert!(note.title.contains("sync conflict"));
    }

    #[test]
    fn three_way_decisions_preserve_competing_changes() {
        assert_eq!(decide(Some("local"), None, None), Decision::Push);
        assert_eq!(decide(None, Some("remote"), None), Decision::Pull);
        assert_eq!(
            decide(None, Some("edited"), Some("old")),
            Decision::Conflict
        );
        assert_eq!(
            decide(Some("edited"), None, Some("old")),
            Decision::Conflict
        );
        assert_eq!(
            decide(Some("a"), Some("b"), Some("old")),
            Decision::Conflict
        );
        assert_eq!(
            decide(Some("new"), Some("new"), Some("old")),
            Decision::Unchanged
        );
    }

    #[test]
    fn remote_pull_refuses_to_replace_a_newer_local_save() {
        let dir =
            std::env::temp_dir().join(format!("console_note_sync_guard_{}", std::process::id()));
        fs::create_dir_all(&dir).expect("create test directory");
        fs::write(dir.join("1.md"), "new local edit").expect("write note");
        let result = write_local_if_unchanged(&dir, 1, Some("old snapshot"), Some("remote edit"));
        assert!(result.is_err());
        assert_eq!(
            fs::read_to_string(dir.join("1.md")).expect("read note"),
            "new local edit"
        );
        fs::remove_dir_all(dir).expect("remove test directory");
    }
}
