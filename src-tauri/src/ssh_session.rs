use russh::*;
use russh_keys::key;

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tauri::{Emitter, Manager};
use tokio::sync::mpsc;
use tokio::sync::Mutex;

// Input types for the SSH loop
pub enum SshInput {
    Data(Vec<u8>),
    Resize(u32, u32), // cols, rows
}

// Structure to hold the SSH session sender and the client handle
#[derive(Clone)]
pub struct ActiveSession {
    pub handle: Arc<Mutex<russh::client::Handle<Client>>>,
    pub terminal_tx: Option<mpsc::UnboundedSender<SshInput>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostKeyDecision {
    Accept,
    Replace,
    Reject,
}

#[derive(Clone, serde::Serialize)]
pub struct HostKeyPromptPayload {
    pub session_id: String,
    pub host: String,
    pub port: u16,
    pub key_type: String,
    pub fingerprint: String,
    pub is_mismatch: bool,
    pub existing_fingerprint: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct KnownHostItem {
    pub host: String,
    pub port: u16,
    pub key_type: String,
    pub fingerprint: String,
}

// Global state to manage active connections
pub struct AppState {
    pub connections: Arc<Mutex<HashMap<String, ActiveSession>>>,
    pub active_transfers: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
    pub host_key_prompts:
        Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<HostKeyDecision>>>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(Mutex::new(HashMap::new())),
            active_transfers: Arc::new(Mutex::new(HashMap::new())),
            host_key_prompts: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

pub fn get_known_hosts_path(app: &tauri::AppHandle) -> std::path::PathBuf {
    let base_dir = app.path().app_data_dir().unwrap_or_else(|_| {
        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
    });
    let _ = std::fs::create_dir_all(&base_dir);
    base_dir.join("known_hosts")
}

pub fn remove_known_host_entry<P: AsRef<std::path::Path>>(
    path: P,
    host: &str,
    port: u16,
) -> Result<(), std::io::Error> {
    let p = path.as_ref();
    if !p.exists() {
        return Ok(());
    }
    let content = std::fs::read_to_string(p)?;
    let target_prefix = if port == 22 {
        format!("{} ", host)
    } else {
        format!("[{}]:{} ", host, port)
    };
    let target_prefix_comma = if port == 22 {
        format!("{},", host)
    } else {
        format!("[{}]:{},", host, port)
    };

    let new_lines: Vec<&str> = content
        .lines()
        .filter(|line| {
            let l = line.trim();
            if l.is_empty() || l.starts_with('#') {
                return true;
            }
            !l.starts_with(&target_prefix) && !l.starts_with(&target_prefix_comma)
        })
        .collect();

    let mut output = new_lines.join("\n");
    if !output.is_empty() {
        output.push('\n');
    }
    std::fs::write(p, output)?;
    Ok(())
}

pub fn list_known_hosts<P: AsRef<std::path::Path>>(
    path: P,
) -> Result<Vec<KnownHostItem>, std::io::Error> {
    let p = path.as_ref();
    if !p.exists() {
        return Ok(vec![]);
    }
    let content = std::fs::read_to_string(p)?;
    let mut items = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let mut parts = trimmed.split_whitespace();
        let Some(host_part) = parts.next() else {
            continue;
        };
        let Some(key_type) = parts.next() else {
            continue;
        };
        let Some(b64_key) = parts.next() else {
            continue;
        };

        let (host, port) = if host_part.starts_with('[') {
            if let Some(idx) = host_part.find("]:") {
                let h = &host_part[1..idx];
                let pt = host_part[idx + 2..].parse::<u16>().unwrap_or(22);
                (h.to_string(), pt)
            } else {
                (host_part.to_string(), 22)
            }
        } else {
            (host_part.to_string(), 22)
        };

        if let Ok(pubkey) = russh_keys::parse_public_key_base64(b64_key) {
            let fingerprint = format!("SHA256:{}", pubkey.fingerprint());
            items.push(KnownHostItem {
                host,
                port,
                key_type: key_type.to_string(),
                fingerprint,
            });
        }
    }

    Ok(items)
}

#[tauri::command]
pub async fn respond_host_key(
    state: tauri::State<'_, AppState>,
    session_id: String,
    decision: String,
) -> Result<(), String> {
    let mut prompts = state.host_key_prompts.lock().await;
    if let Some(tx) = prompts.remove(&session_id) {
        let dec = match decision.as_str() {
            "accept" => HostKeyDecision::Accept,
            "replace" => HostKeyDecision::Replace,
            _ => HostKeyDecision::Reject,
        };
        let _ = tx.send(dec);
        Ok(())
    } else {
        Err("No pending host key prompt for this session".into())
    }
}

#[tauri::command]
pub async fn get_known_hosts(app: tauri::AppHandle) -> Result<Vec<KnownHostItem>, String> {
    let path = get_known_hosts_path(&app);
    list_known_hosts(path).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn remove_known_host(
    app: tauri::AppHandle,
    host: String,
    port: u16,
) -> Result<(), String> {
    let path = get_known_hosts_path(&app);
    remove_known_host_entry(&path, &host, port).map_err(|e| e.to_string())
}

// Russh Client Handler
#[derive(Clone)]
pub struct Client {
    pub session_id: String,
    pub host: String,
    pub port: u16,
    pub known_hosts_path: std::path::PathBuf,
    pub app_handle: tauri::AppHandle,
}

#[async_trait]
impl client::Handler for Client {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &key::PublicKey,
    ) -> Result<bool, Self::Error> {
        let check = russh_keys::check_known_hosts_path(
            &self.host,
            self.port,
            server_public_key,
            &self.known_hosts_path,
        );
        match check {
            Ok(true) => {
                log::info!(
                    "SSH host key verified from known_hosts for {}:{}",
                    self.host,
                    self.port
                );
                Ok(true)
            }
            Ok(false) => {
                // Unknown host key: Trust-On-First-Use (TOFU) flow
                let fingerprint = format!("SHA256:{}", server_public_key.fingerprint());
                let key_type = server_public_key.name().to_string();
                log::warn!(
                    "Unknown SSH host key for {}:{} ({}) - requesting TOFU confirmation",
                    self.host,
                    self.port,
                    fingerprint
                );

                let (tx, rx) = tokio::sync::oneshot::channel::<HostKeyDecision>();
                {
                    let state = self.app_handle.state::<AppState>();
                    let mut prompts = state.host_key_prompts.lock().await;
                    prompts.insert(self.session_id.clone(), tx);
                }

                let payload = HostKeyPromptPayload {
                    session_id: self.session_id.clone(),
                    host: self.host.clone(),
                    port: self.port,
                    key_type,
                    fingerprint,
                    is_mismatch: false,
                    existing_fingerprint: None,
                };

                let _ = self.app_handle.emit("ssh-host-key-prompt", payload);

                // Wait up to 60 seconds for user confirmation
                let decision =
                    match tokio::time::timeout(std::time::Duration::from_secs(60), rx).await {
                        Ok(Ok(d)) => d,
                        _ => HostKeyDecision::Reject,
                    };

                match decision {
                    HostKeyDecision::Accept | HostKeyDecision::Replace => {
                        let _ = russh_keys::learn_known_hosts_path(
                            &self.host,
                            self.port,
                            server_public_key,
                            &self.known_hosts_path,
                        );
                        log::info!(
                            "Learned trusted SSH host key for {}:{}",
                            self.host,
                            self.port
                        );
                        Ok(true)
                    }
                    HostKeyDecision::Reject => {
                        log::warn!(
                            "User rejected unknown SSH host key for {}:{}",
                            self.host,
                            self.port
                        );
                        Ok(false)
                    }
                }
            }
            Err(russh_keys::Error::KeyChanged { line }) => {
                // Host key MISMATCH: potential Man-in-the-Middle attack
                let fingerprint = format!("SHA256:{}", server_public_key.fingerprint());
                let key_type = server_public_key.name().to_string();
                let existing_keys =
                    russh_keys::known_host_keys_path(&self.host, self.port, &self.known_hosts_path)
                        .unwrap_or_default();
                let existing_fp = existing_keys
                    .first()
                    .map(|(_, k)| format!("SHA256:{}", k.fingerprint()));

                log::error!(
                    "CRITICAL: SSH host key mismatch detected for {}:{} at line {}",
                    self.host,
                    self.port,
                    line
                );

                let (tx, rx) = tokio::sync::oneshot::channel::<HostKeyDecision>();
                {
                    let state = self.app_handle.state::<AppState>();
                    let mut prompts = state.host_key_prompts.lock().await;
                    prompts.insert(self.session_id.clone(), tx);
                }

                let payload = HostKeyPromptPayload {
                    session_id: self.session_id.clone(),
                    host: self.host.clone(),
                    port: self.port,
                    key_type,
                    fingerprint,
                    is_mismatch: true,
                    existing_fingerprint: existing_fp,
                };

                let _ = self.app_handle.emit("ssh-host-key-prompt", payload);

                let decision =
                    match tokio::time::timeout(std::time::Duration::from_secs(60), rx).await {
                        Ok(Ok(d)) => d,
                        _ => HostKeyDecision::Reject,
                    };

                match decision {
                    HostKeyDecision::Replace => {
                        let _ =
                            remove_known_host_entry(&self.known_hosts_path, &self.host, self.port);
                        let _ = russh_keys::learn_known_hosts_path(
                            &self.host,
                            self.port,
                            server_public_key,
                            &self.known_hosts_path,
                        );
                        log::info!(
                            "Replaced SSH host key for {}:{} in known_hosts",
                            self.host,
                            self.port
                        );
                        Ok(true)
                    }
                    _ => {
                        log::warn!(
                            "Connection aborted due to SSH host key mismatch for {}:{}",
                            self.host,
                            self.port
                        );
                        Ok(false)
                    }
                }
            }
            Err(e) => {
                log::error!(
                    "Error reading known_hosts for {}:{}: {}",
                    self.host,
                    self.port,
                    e
                );
                Ok(false)
            }
        }
    }
}

pub async fn connect_and_stream(
    id: String,
    host: String,
    port: u16,
    user: String,
    password: Option<String>,
    cols: u32,
    rows: u32,
    app_handle: tauri::AppHandle,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = client::Config::default();
    let config = Arc::new(config);
    let known_hosts_path = get_known_hosts_path(&app_handle);
    let sh = Client {
        session_id: id.clone(),
        host: host.clone(),
        port,
        known_hosts_path,
        app_handle: app_handle.clone(),
    };

    let mut session = client::connect(config, (host.as_str(), port), sh).await?;

    // Auth
    let auth_res = if let Some(pwd) = password {
        session.authenticate_password(&user, pwd).await?
    } else {
        return Err("Only password auth supported in this step".into());
    };

    if !auth_res {
        return Err("Authentication failed".into());
    }

    let mut channel = session.channel_open_session().await?;
    // request_pty takes (want_reply, term, col_width, row_height, pix_width, pix_height, modes)
    channel
        .request_pty(false, "xterm-256color", cols, rows, 0, 0, &[])
        .await?;
    channel.request_shell(false).await?;

    // Create channel for input from Frontend -> SSH
    // CHANGED: Use SshInput enum
    let (tx, mut rx) = mpsc::unbounded_channel::<SshInput>();

    // Store the sender in AppState
    {
        let state = app_handle.state::<AppState>();
        let mut connections = state.connections.lock().await;
        connections.insert(
            id.clone(),
            ActiveSession {
                handle: Arc::new(Mutex::new(session)),
                terminal_tx: Some(tx),
            },
        );
    }

    let id_clone = id.clone();
    let app_handle_clone = app_handle.clone();

    // Spawn async loop to handle I/O
    tokio::spawn(async move {
        loop {
            tokio::select! {
                // Read from SSH Channel -> Send to Frontend
                msg = channel.wait() => {
                    match msg {
                        Some(ChannelMsg::Data { ref data }) => {
                            let _ = app_handle_clone.emit(&format!("ssh-output-{}", id_clone), data.to_vec());
                        }
                        Some(ChannelMsg::ExitStatus { exit_status }) => {
                             let _ = app_handle_clone.emit(&format!("ssh-exit-{}", id_clone), exit_status);
                             break;
                        }
                        Some(ChannelMsg::Close) | None => {
                            break;
                        }
                         _ => {}
                    }
                }
                // Read from Frontend (via mpsc) -> Write to SSH Channel
                msg = rx.recv() => {
                    match msg {
                        Some(SshInput::Data(data)) => {
                             let _ = channel.data(&data[..]).await;
                        }
                        Some(SshInput::Resize(cols, rows)) => {
                            // window_change(col_width, row_height, pix_width, pix_height)
                            let _ = channel.window_change(cols, rows, 0, 0).await;
                        }
                        None => {
                            // Sender dropped (disconnect called), break loop
                            break;
                        }
                    }
                }
            }
        }

        // Cleanup - only remove terminal_tx, keep connection alive for other subsystems
        let state = app_handle_clone.state::<AppState>();
        let mut connections = state.connections.lock().await;
        if let Some(sess) = connections.get_mut(&id_clone) {
            sess.terminal_tx = None;
        }
    });

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_known_hosts_lifecycle() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("known_hosts");

        let kp1 = key::KeyPair::generate_ed25519().unwrap();
        let pk1 = kp1.clone_public_key().unwrap();

        let kp2 = key::KeyPair::generate_ed25519().unwrap();
        let pk2 = kp2.clone_public_key().unwrap();

        let host = "192.168.1.100";
        let port = 22;

        // 1. Initial check on non-existent file -> Unknown host (false)
        let res = russh_keys::check_known_hosts_path(host, port, &pk1, &path).unwrap();
        assert!(!res, "Unknown host must return false (triggering TOFU)");

        // 2. Learn key -> writes to known_hosts
        russh_keys::learn_known_hosts_path(host, port, &pk1, &path).unwrap();

        // 3. Check again with same key -> Match (true)
        let res = russh_keys::check_known_hosts_path(host, port, &pk1, &path).unwrap();
        assert!(res, "Known host with matching key must return true");

        // 4. Check with different key for same host and algorithm -> KeyChanged error (mismatch)
        let res = russh_keys::check_known_hosts_path(host, port, &pk2, &path);
        match res {
            Err(russh_keys::Error::KeyChanged { line }) => {
                assert!(line >= 1, "Mismatch must pinpoint the changed line");
            }
            other => panic!("Expected KeyChanged error on key mismatch, got {:?}", other),
        }

        // 5. Port distinction: different port on same host is treated as unknown host
        let res = russh_keys::check_known_hosts_path(host, 2222, &pk1, &path).unwrap();
        assert!(
            !res,
            "Different port must be treated as a distinct host entry"
        );

        // 6. List known hosts
        let entries = list_known_hosts(&path).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].host, host);
        assert_eq!(entries[0].port, port);
        assert_eq!(entries[0].key_type, "ssh-ed25519");
        assert!(entries[0].fingerprint.starts_with("SHA256:"));

        // 7. Remove known host entry
        remove_known_host_entry(&path, host, port).unwrap();
        let entries = list_known_hosts(&path).unwrap();
        assert_eq!(entries.len(), 0, "Host entry must be removed");

        // 8. Host is now unknown again
        let res = russh_keys::check_known_hosts_path(host, port, &pk1, &path).unwrap();
        assert!(!res, "Removed host must return false (unknown)");
    }
}
