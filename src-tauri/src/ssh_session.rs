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

pub struct PendingPrompt {
    pub session_id: String,
    pub sender: tokio::sync::oneshot::Sender<crate::host_key::HostKeyDecision>,
}

// Global state to manage active connections
pub struct AppState {
    pub connections: Arc<Mutex<HashMap<String, ActiveSession>>>,
    pub active_transfers: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
    pub host_key_prompts: Arc<Mutex<HashMap<u64, PendingPrompt>>>,
    pub editor_store: Arc<dyn crate::sftp::EditorStore>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(Mutex::new(HashMap::new())),
            active_transfers: Arc::new(Mutex::new(HashMap::new())),
            host_key_prompts: Arc::new(Mutex::new(HashMap::new())),
            editor_store: Arc::new(crate::sftp::KeyringEditorStore::new()),
        }
    }

    #[allow(dead_code)]
    pub fn with_editor_store(editor_store: Arc<dyn crate::sftp::EditorStore>) -> Self {
        Self {
            connections: Arc::new(Mutex::new(HashMap::new())),
            active_transfers: Arc::new(Mutex::new(HashMap::new())),
            host_key_prompts: Arc::new(Mutex::new(HashMap::new())),
            editor_store,
        }
    }
}

pub fn get_known_hosts_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let base_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to resolve app data directory: {}", e))?;
    std::fs::create_dir_all(&base_dir)
        .map_err(|e| format!("Failed to create app data directory: {}", e))?;
    Ok(base_dir.join("known_hosts"))
}

static NEXT_PROMPT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

pub struct TauriPrompter {
    pub app_handle: tauri::AppHandle,
    pub session_id: String,
}

#[async_trait]
impl crate::host_key::HostKeyPrompter for TauriPrompter {
    async fn ask(
        &self,
        prompt: crate::host_key::HostKeyPrompt,
    ) -> crate::host_key::HostKeyDecision {
        let prompt_id = NEXT_PROMPT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let (tx, rx) = tokio::sync::oneshot::channel();

        {
            let state = self.app_handle.state::<AppState>();
            let mut prompts = state.host_key_prompts.lock().await;
            prompts.insert(
                prompt_id,
                PendingPrompt {
                    session_id: self.session_id.clone(),
                    sender: tx,
                },
            );
        }

        let payload = serde_json::json!({
            "prompt_id": prompt_id,
            "session_id": self.session_id,
            "kind": match prompt.kind {
                crate::host_key::HostKeyPromptKind::Unknown => "unknown",
                crate::host_key::HostKeyPromptKind::NewKeyType => "new_key_type",
                crate::host_key::HostKeyPromptKind::Changed => "changed",
            },
            "host": prompt.host,
            "port": prompt.port,
            "key_type": prompt.key_type,
            "fingerprint": prompt.fingerprint,
            "known": prompt.existing_fingerprints,
        });

        let _ = self.app_handle.emit("ssh-host-key-prompt", payload);

        // 120s timeout
        let decision_res = tokio::time::timeout(std::time::Duration::from_secs(120), rx).await;

        // On timeout OR cancellation, remove the entry and emit ssh-host-key-expired { prompt_id }
        let state = self.app_handle.state::<AppState>();
        let was_removed = {
            let mut prompts = state.host_key_prompts.lock().await;
            prompts.remove(&prompt_id).is_some()
        };

        if was_removed {
            let _ = self.app_handle.emit(
                "ssh-host-key-expired",
                serde_json::json!({ "prompt_id": prompt_id }),
            );
        }

        match decision_res {
            Ok(Ok(decision)) => decision,
            _ => crate::host_key::HostKeyDecision::Reject,
        }
    }
}

#[tauri::command]
pub async fn respond_host_key(
    state: tauri::State<'_, AppState>,
    prompt_id: u64,
    decision: String,
) -> Result<(), String> {
    let mut prompts = state.host_key_prompts.lock().await;
    if let Some(pending) = prompts.remove(&prompt_id) {
        let dec = match decision.as_str() {
            "trust" | "accept" | "replace" => crate::host_key::HostKeyDecision::Trust,
            _ => crate::host_key::HostKeyDecision::Reject,
        };
        let _ = pending.sender.send(dec);
        Ok(())
    } else {
        Err("No pending host key prompt with this ID".into())
    }
}

#[tauri::command]
pub async fn get_known_hosts(
    app: tauri::AppHandle,
) -> Result<Vec<crate::known_hosts::KnownHostEntry>, String> {
    let path = get_known_hosts_path(&app)?;
    let store = crate::known_hosts::KnownHostsStore::new(path);
    store.list()
}

#[tauri::command]
pub async fn remove_known_host(
    app: tauri::AppHandle,
    host: String,
    port: u16,
    fingerprint: String,
) -> Result<(), String> {
    let path = get_known_hosts_path(&app)?;
    let store = crate::known_hosts::KnownHostsStore::new(path);
    store.remove(&host, port, &fingerprint)
}

// Russh Client Handler
#[derive(Clone)]
pub struct Client {
    pub verifier: Arc<crate::host_key::HostKeyVerifier>,
}

#[async_trait]
impl client::Handler for Client {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &key::PublicKey,
    ) -> Result<bool, Self::Error> {
        let ok = self.verifier.verify(server_public_key).await;
        Ok(ok)
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
    let known_hosts_path = get_known_hosts_path(&app_handle)?;
    let store = Arc::new(crate::known_hosts::KnownHostsStore::new(known_hosts_path));
    let prompter = Arc::new(TauriPrompter {
        app_handle: app_handle.clone(),
        session_id: id.clone(),
    });
    let verifier = Arc::new(crate::host_key::HostKeyVerifier::new(
        store,
        prompter,
        host.clone(),
        port,
    ));

    let mut config = client::Config::default();
    config.preferred.key = std::borrow::Cow::Owned(verifier.preferred_algorithms());
    let config = Arc::new(config);

    let sh = Client {
        verifier: verifier.clone(),
    };

    let friendly_unknown_key_msg = |v: &crate::host_key::HostKeyVerifier| -> String {
        if let Some(fail) = v.take_failure() {
            fail
        } else {
            "Host key verification failed: the connection was cancelled because the server's identity was not confirmed.".to_string()
        }
    };

    let mut session = match client::connect(config, (host.as_str(), port), sh).await {
        Ok(s) => s,
        Err(russh::Error::UnknownKey) => {
            return Err(friendly_unknown_key_msg(&verifier).into());
        }
        Err(e) => return Err(e.into()),
    };

    // Auth
    let auth_res = if let Some(pwd) = password {
        match session.authenticate_password(&user, pwd).await {
            Ok(res) => res,
            Err(russh::Error::UnknownKey) => {
                return Err(friendly_unknown_key_msg(&verifier).into());
            }
            Err(e) => return Err(e.into()),
        }
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
        let store = crate::known_hosts::KnownHostsStore::new(&path);

        let kp1 = key::KeyPair::generate_ed25519().unwrap();
        let pk1 = kp1.clone_public_key().unwrap();
        let fp1 = format!("SHA256:{}", pk1.fingerprint());

        let kp2 = key::KeyPair::generate_ed25519().unwrap();
        let pk2 = kp2.clone_public_key().unwrap();

        let host = "192.168.1.100";
        let port = 22;

        // 1. Initial check on non-existent file -> Unknown
        assert_eq!(
            store.check(host, port, &pk1).unwrap(),
            crate::known_hosts::CheckResult::Unknown
        );

        // 2. Add key -> writes to known_hosts
        store.add(host, port, &pk1).unwrap();

        // 3. Check again with same key -> Trusted
        assert_eq!(
            store.check(host, port, &pk1).unwrap(),
            crate::known_hosts::CheckResult::Trusted
        );

        // 4. Check with different key of same type -> Changed
        match store.check(host, port, &pk2).unwrap() {
            crate::known_hosts::CheckResult::Changed(existing) => {
                assert_eq!(existing.len(), 1);
                assert_eq!(existing[0].fingerprint, fp1);
            }
            other => panic!("Expected Changed, got {:?}", other),
        }

        // 5. Port distinction: different port on same host is Unknown
        assert_eq!(
            store.check(host, 2222, &pk1).unwrap(),
            crate::known_hosts::CheckResult::Unknown
        );

        // 6. List known hosts
        let entries = store.list().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].host, host);
        assert_eq!(entries[0].port, port);
        assert_eq!(entries[0].key_type, "ssh-ed25519");
        assert_eq!(entries[0].fingerprint, fp1);

        // 7. Remove known host entry by fingerprint
        store.remove(host, port, &fp1).unwrap();
        let entries = store.list().unwrap();
        assert_eq!(entries.len(), 0, "Host entry must be removed");

        // 8. Host is now unknown again
        assert_eq!(
            store.check(host, port, &pk1).unwrap(),
            crate::known_hosts::CheckResult::Unknown
        );
    }
}
