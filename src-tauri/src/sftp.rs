use crate::ssh_session::AppState;
use tauri::State;
use russh_sftp::client::SftpSession;
use serde::Serialize;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Serialize)]
pub struct FileEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub mtime: u64,
}

#[tauri::command]
pub async fn sftp_list_dir(
    state: State<'_, AppState>,
    id: String,
    path: String,
) -> Result<Vec<FileEntry>, String> {
    let handle = {
        let connections = state.connections.lock().await;
        let conn = connections.get(&id).ok_or("Session not found")?;
        conn.handle.clone()
    };

    let channel = {
        let h = handle.lock().await;
        h.channel_open_session().await.map_err(|e| e.to_string())?
    };
    channel.request_subsystem(true, "sftp").await.map_err(|e| e.to_string())?;
    let sftp = SftpSession::new(channel.into_stream()).await.map_err(|e| e.to_string())?;

    let dir = sftp.read_dir(&path).await.map_err(|e| format!("{:?}", e))?;
    let mut entries = Vec::new();

    for entry in dir {
        let name = entry.file_name();
        let attrs = entry.metadata();
        if name == "." || name == ".." {
            continue;
        }
        entries.push(FileEntry {
            name,
            is_dir: attrs.is_dir(),
            size: attrs.size.unwrap_or(0),
            mtime: attrs.mtime.unwrap_or(0) as u64,
        });
    }

    // Sort by type (dir first) then name
    entries.sort_by(|a, b| {
        match b.is_dir.cmp(&a.is_dir) {
            std::cmp::Ordering::Equal => a.name.cmp(&b.name),
            other => other,
        }
    });

    Ok(entries)
}

#[tauri::command]
pub async fn local_list_dir(path: String) -> Result<Vec<FileEntry>, String> {
    let mut entries = Vec::new();
    let mut dir = tokio::fs::read_dir(&path).await.map_err(|e| e.to_string())?;

    while let Some(entry) = dir.next_entry().await.map_err(|e| e.to_string())? {
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "." || name == ".." {
            continue;
        }
        let metadata = entry.metadata().await.map_err(|e| e.to_string())?;
        entries.push(FileEntry {
            name,
            is_dir: metadata.is_dir(),
            size: metadata.len(),
            mtime: metadata.modified().map(|m| m.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs()).unwrap_or(0),
        });
    }

    entries.sort_by(|a, b| {
        match b.is_dir.cmp(&a.is_dir) {
            std::cmp::Ordering::Equal => a.name.cmp(&b.name),
            other => other,
        }
    });

    Ok(entries)
}

#[tauri::command]
pub fn get_local_home_dir() -> Result<String, String> {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map_err(|_| "Could not determine home directory".into())
}

async fn upload_recursive(
    sftp: &SftpSession,
    local_path: &Path,
    remote_path: &Path,
    cancel_token: Arc<AtomicBool>,
) -> Result<(), String> {
    let metadata = tokio::fs::metadata(local_path).await.map_err(|e| e.to_string())?;

    if metadata.is_dir() {
        let remote_path_str = remote_path.to_string_lossy().replace("\\", "/");
        let _ = sftp.create_dir(&remote_path_str).await;

        let mut entries = tokio::fs::read_dir(local_path).await.map_err(|e| e.to_string())?;
        while let Some(entry) = entries.next_entry().await.map_err(|e| e.to_string())? {
            if cancel_token.load(Ordering::Relaxed) {
                return Err("Transfer cancelled by user".into());
            }
            let entry_path = entry.path();
            let entry_name = entry.file_name();
            let remote_entry_path = remote_path.join(&entry_name);
            Box::pin(upload_recursive(sftp, &entry_path, &remote_entry_path, cancel_token.clone())).await?;
        }
    } else {
        let mut local_file = tokio::fs::File::open(local_path).await.map_err(|e| e.to_string())?;
        let remote_path_str = remote_path.to_string_lossy().replace("\\", "/");
        let mut remote_file = sftp.create(&remote_path_str).await.map_err(|e| e.to_string())?;
        
        let mut buf = vec![0; 65536];
        loop {
            if cancel_token.load(Ordering::Relaxed) {
                return Err("Transfer cancelled by user".into());
            }
            let n = local_file.read(&mut buf).await.map_err(|e| e.to_string())?;
            if n == 0 { break; }
            remote_file.write_all(&buf[..n]).await.map_err(|e| e.to_string())?;
        }
    }

    Ok(())
}

async fn download_recursive(
    sftp: &SftpSession,
    remote_path: &Path,
    local_path: &Path,
    cancel_token: Arc<AtomicBool>,
) -> Result<(), String> {
    let remote_path_str = remote_path.to_string_lossy().replace("\\", "/");
    
    match sftp.read_dir(&remote_path_str).await {
        Ok(dir) => {
            tokio::fs::create_dir_all(local_path).await.map_err(|e| e.to_string())?;
            for entry in dir {
                if cancel_token.load(Ordering::Relaxed) {
                    return Err("Transfer cancelled by user".into());
                }
                let name = entry.file_name();
                if name == "." || name == ".." { continue; }
                let remote_entry_path = remote_path.join(&name);
                let local_entry_path = local_path.join(&name);
                Box::pin(download_recursive(sftp, &remote_entry_path, &local_entry_path, cancel_token.clone())).await?;
            }
        }
        Err(_) => {
            let mut remote_file = sftp.open(&remote_path_str).await.map_err(|e| e.to_string())?;
            let mut local_file = tokio::fs::File::create(local_path).await.map_err(|e| e.to_string())?;
            
            let mut buf = vec![0; 65536];
            loop {
                if cancel_token.load(Ordering::Relaxed) {
                    return Err("Transfer cancelled by user".into());
                }
                let n = remote_file.read(&mut buf).await.map_err(|e| e.to_string())?;
                if n == 0 { break; }
                local_file.write_all(&buf[..n]).await.map_err(|e| e.to_string())?;
            }
        }
    }
    
    Ok(())
}

#[tauri::command]
pub async fn sftp_upload(
    state: State<'_, AppState>,
    id: String,
    transfer_id: String,
    local_path: String,
    remote_path: String,
) -> Result<(), String> {
    let handle = {
        let connections = state.connections.lock().await;
        let conn = connections.get(&id).ok_or("Session not found")?;
        conn.handle.clone()
    };
    
    let cancel_token = Arc::new(AtomicBool::new(false));
    {
        let mut transfers = state.active_transfers.lock().await;
        transfers.insert(transfer_id.clone(), cancel_token.clone());
    }

    let channel = {
        let h = handle.lock().await;
        h.channel_open_session().await.map_err(|e| e.to_string())?
    };
    
    channel.request_subsystem(true, "sftp").await.map_err(|e| e.to_string())?;
    let sftp = SftpSession::new(channel.into_stream()).await.map_err(|e| e.to_string())?;

    let res = upload_recursive(&sftp, Path::new(&local_path), Path::new(&remote_path), cancel_token).await;

    {
        let mut transfers = state.active_transfers.lock().await;
        transfers.remove(&transfer_id);
    }
    res
}

#[tauri::command]
pub async fn sftp_download(
    state: State<'_, AppState>,
    id: String,
    transfer_id: String,
    remote_path: String,
    local_path: String,
) -> Result<(), String> {
    let handle = {
        let connections = state.connections.lock().await;
        let conn = connections.get(&id).ok_or("Session not found")?;
        conn.handle.clone()
    };
    
    let cancel_token = Arc::new(AtomicBool::new(false));
    {
        let mut transfers = state.active_transfers.lock().await;
        transfers.insert(transfer_id.clone(), cancel_token.clone());
    }

    let channel = {
        let h = handle.lock().await;
        h.channel_open_session().await.map_err(|e| e.to_string())?
    };
    
    channel.request_subsystem(true, "sftp").await.map_err(|e| e.to_string())?;
    let sftp = SftpSession::new(channel.into_stream()).await.map_err(|e| e.to_string())?;

    let res = download_recursive(&sftp, Path::new(&remote_path), Path::new(&local_path), cancel_token).await;

    {
        let mut transfers = state.active_transfers.lock().await;
        transfers.remove(&transfer_id);
    }
    res
}

#[tauri::command]
pub async fn cancel_transfer(
    state: State<'_, AppState>,
    transfer_id: String,
) -> Result<(), String> {
    let transfers = state.active_transfers.lock().await;
    if let Some(token) = transfers.get(&transfer_id) {
        token.store(true, Ordering::Relaxed);
    }
    Ok(())
}
