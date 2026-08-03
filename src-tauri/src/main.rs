// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ssh_session;
mod sftp;
mod vpn;

use tauri::Emitter;
use ssh_session::{AppState, connect_and_stream, SshInput};
use sftp::{sftp_list_dir, local_list_dir, get_local_home_dir, sftp_upload, sftp_download, cancel_transfer};
use vpn::{VpnState, ActiveTunnel, start_vpn_tunnel, stop_vpn_tunnel, get_vpn_status, start_openconnect_tunnel, send_mfa_token};

#[tauri::command]
async fn connect_ssh(
    app: tauri::AppHandle,
    id: String,
    host: String,
    port: u16,
    user: String,
    password: Option<String>,
    cols: u32,
    rows: u32,
) -> Result<(), String> {
    let app_clone = app.clone();
    tokio::spawn(async move {
        if let Err(e) = connect_and_stream(id.clone(), host, port, user, password, cols, rows, app_clone).await {
            let _ = app.emit(&format!("ssh-error-{}", id), e.to_string());
        }
    });
    Ok(())
}

#[tauri::command]
async fn send_ssh_input(
    state: tauri::State<'_, AppState>,
    id: String,
    data: String,
) -> Result<(), String> {
    let connections = state.connections.lock().await;
    if let Some(conn) = connections.get(&id) {
        if let Some(tx) = &conn.terminal_tx {
            let _ = tx.send(SshInput::Data(data.into_bytes()));
        }
    }
    Ok(())
}

#[tauri::command]
async fn disconnect_ssh(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    let mut connections = state.connections.lock().await;
    if connections.remove(&id).is_some() {
        Ok(())
    } else {
        Err("Session not found".into())
    }
}

#[tauri::command]
async fn resize_pty(
    state: tauri::State<'_, AppState>,
    id: String,
    rows: u32,
    cols: u32,
) -> Result<(), String> {
    let connections = state.connections.lock().await;
    if let Some(conn) = connections.get(&id) {
        if let Some(tx) = &conn.terminal_tx {
            let _ = tx.send(SshInput::Resize(cols, rows));
        }
        Ok(())
    } else {
        Err("Session not found".into())
    }
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_sql::Builder::default().build())
        .plugin(tauri_plugin_log::Builder::default().build())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .manage(AppState::new())
        .manage(VpnState::new())
        .invoke_handler(tauri::generate_handler![
            // SSH
            connect_ssh,
            send_ssh_input,
            disconnect_ssh,
            resize_pty,
            // SFTP
            sftp_list_dir,
            local_list_dir,
            get_local_home_dir,
            sftp_upload,
            sftp_download,
            cancel_transfer,
            // VPN — WireGuard
            start_vpn_tunnel,
            stop_vpn_tunnel,
            get_vpn_status,
            // VPN — OpenConnect
            start_openconnect_tunnel,
            send_mfa_token,
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                use tauri::Manager;
                let vpn_state = window.state::<VpnState>();
                let tunnel = vpn_state.tunnel.clone();
                tauri::async_runtime::block_on(async move {
                    let mut t: tokio::sync::MutexGuard<'_, Option<ActiveTunnel>> = tunnel.lock().await;
                    match t.take() {
                        #[cfg(target_os = "windows")]
                        Some(ActiveTunnel::WireGuard(handle)) => {
                            handle.outbound_task.abort();
                            handle.inbound_task.abort();
                            if let Some(k) = handle.keepalive_task { k.abort(); }
                            drop(handle._adapter);
                            log::info!("WireGuard tunnel torn down on app close");
                        }
                        #[cfg(not(target_os = "windows"))]
                        Some(ActiveTunnel::WireGuard(_)) => {}
                        Some(ActiveTunnel::OpenConnect(handle)) => {
                            handle.status_task.abort();
                            let mut child = handle.child.lock().await;
                            let _ = child.kill().await;
                            log::info!("OpenConnect process killed on app close");
                        }
                        None => {}
                    }
                });
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
