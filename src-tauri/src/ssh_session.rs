use russh::*;
use russh_keys::*;
use std::sync::Arc;
use tokio::sync::mpsc;
use tauri::{Emitter, Manager}; // Added Manager, Removed Listener
use std::collections::HashMap;
use tokio::sync::Mutex;

// Structure to hold the SSH session sender for writing data
pub struct SshConnection {
    pub tx: mpsc::UnboundedSender<Vec<u8>>,
}

// Global state to manage active connections
pub struct AppState {
    pub connections: Arc<Mutex<HashMap<String, SshConnection>>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

use async_trait::async_trait;

// Russh Client Handler
struct Client {}

#[async_trait]
impl client::Handler for Client {
    type Error = russh::Error;
    
   async fn check_server_key(
        &mut self,
        _server_public_key: &key::PublicKey,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

pub async fn connect_and_stream(
    id: String,
    host: String,
    port: u16,
    user: String,
    password: Option<String>,
    app_handle: tauri::AppHandle,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = client::Config::default();
    let config = Arc::new(config);
    let sh = Client {};

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
    channel.request_pty(false, "xterm", 80, 24, 0, 0, &[]).await?;
    channel.request_shell(false).await?; // Changed exec_shell to request_shell

    // Create channel for input from Frontend -> SSH
    let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();

    // Store the sender in AppState
    {
        let state = app_handle.state::<AppState>();
        let mut connections = state.connections.lock().await;
        connections.insert(id.clone(), SshConnection { tx });
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
                        Some(ChannelMsg::Data { ref data }) => { // Removed Ok() wrapper
                            let _ = app_handle_clone.emit(&format!("ssh-output-{}", id_clone), data.to_vec());
                        }
                        Some(ChannelMsg::ExitStatus { exit_status }) => { // Removed Ok() wrapper
                             let _ = app_handle_clone.emit(&format!("ssh-exit-{}", id_clone), exit_status);
                             break;
                        }
                        Some(ChannelMsg::Close) | None => { // Handle Close explicitly
                            break;
                        }
                         _ => {}
                    }
                }
                // Read from Frontend (via mpsc) -> Write to SSH Channel
                Some(data) = rx.recv() => {
                    let _ = channel.data(&data[..]).await;
                }
            }
        }
        
        // Cleanup
        let state = app_handle_clone.state::<AppState>();
        let mut connections = state.connections.lock().await;
        connections.remove(&id_clone);
    });

    Ok(())
}
