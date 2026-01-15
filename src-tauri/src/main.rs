// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ssh_session;

use tauri::Emitter;
use ssh_session::{AppState, connect_and_stream};

#[tauri::command]
async fn connect_ssh(
    app: tauri::AppHandle,
    id: String,
    host: String,
    port: u16,
    user: String,
    password: Option<String>,
) -> Result<(), String> {
    // Spawn off the connection task so we don't block the command
    let app_clone = app.clone();
    tokio::spawn(async move {
        if let Err(e) = connect_and_stream(id.clone(), host, port, user, password, app_clone).await {
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
         // data should be bytes (or base64? xterm usually sends strings, but raw bytes might be needed for special chars)
         // treating as bytes from string for now
         let _ = conn.tx.send(data.into_bytes());
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
#[allow(dead_code)]
async fn resize_pty(
    // Implement resize later
) {}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_sql::Builder::default().build())
        .plugin(tauri_plugin_log::Builder::default().build())
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![connect_ssh, send_ssh_input, disconnect_ssh, resize_pty])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
