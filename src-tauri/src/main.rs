// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

pub mod host_key;
pub mod known_hosts;
mod sftp;
mod ssh_session;
mod vpn;

use sftp::{
    cancel_transfer, edit_remote_file, get_local_home_dir, local_list_dir, sftp_download,
    sftp_list_dir, sftp_upload,
};
use ssh_session::{
    connect_and_stream, get_known_hosts, remove_known_host, respond_host_key, AppState, SshInput,
};
use tauri::Emitter;
use vpn::{
    get_vpn_status, send_mfa_token, start_openconnect_tunnel, start_vpn_tunnel, stop_vpn_tunnel,
    ActiveTunnel, VpnState,
};

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
        if let Err(e) = connect_and_stream(
            id.clone(),
            host,
            port,
            user,
            password,
            cols,
            rows,
            app_clone,
        )
        .await
        {
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
async fn disconnect_ssh(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
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

#[tauri::command]
async fn pick_external_editor(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    use tauri_plugin_dialog::DialogExt;

    let mut builder = app.dialog().file().set_title("Select External Editor");

    #[cfg(target_os = "windows")]
    {
        builder = builder.add_filter("Executable Files", &["exe"]);
    }
    #[cfg(target_os = "macos")]
    {
        builder = builder.add_filter("Applications & Binaries", &["app"]);
    }

    let file_path = builder
        .blocking_pick_file()
        .ok_or_else(|| "No file selected".to_string())?;

    let path_buf = file_path
        .into_path()
        .map_err(|_| "Failed to convert chosen path to filesystem path".to_string())?;

    sftp::validate_editor_path(&path_buf)?;

    let path_str = path_buf.to_string_lossy().to_string();
    state.editor_store.set(&path_str)?;
    Ok(path_str)
}

#[tauri::command]
async fn get_external_editor(state: tauri::State<'_, AppState>) -> Result<Option<String>, String> {
    state.editor_store.get()
}

#[tauri::command]
async fn clear_external_editor(state: tauri::State<'_, AppState>) -> Result<(), String> {
    state.editor_store.clear()
}

fn main() {
    // SEC-04: Purge orphaned temp edit files from previous crashes or ungraceful exits
    sftp::cleanup_temp_edits();

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
            respond_host_key,
            get_known_hosts,
            remove_known_host,
            // SFTP & Editor
            sftp_list_dir,
            local_list_dir,
            get_local_home_dir,
            sftp_upload,
            sftp_download,
            cancel_transfer,
            edit_remote_file,
            pick_external_editor,
            get_external_editor,
            clear_external_editor,
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
                    let mut t: tokio::sync::MutexGuard<'_, Option<ActiveTunnel>> =
                        tunnel.lock().await;
                    match t.take() {
                        #[cfg(target_os = "windows")]
                        Some(ActiveTunnel::WireGuard(handle)) => {
                            handle.outbound_task.abort();
                            handle.inbound_task.abort();
                            if let Some(k) = handle.keepalive_task {
                                k.abort();
                            }
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
                    sftp::cleanup_temp_edits();
                });
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
