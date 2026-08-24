use crate::ssh_session::AppState;
use tauri::State;
use russh_sftp::client::SftpSession;
use serde::Serialize;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;
use tauri::{AppHandle, Emitter};

#[derive(Clone, Serialize)]
struct TransferProgress {
    transfer_id: String,
    transferred: u64,
    total: u64,
    file_name: String,
}

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
    app: &AppHandle,
    transfer_id: &str,
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
            Box::pin(upload_recursive(sftp, &entry_path, &remote_entry_path, cancel_token.clone(), app, transfer_id)).await?;
        }
    } else {
        let mut local_file = tokio::fs::File::open(local_path).await.map_err(|e| e.to_string())?;
        let remote_path_str = remote_path.to_string_lossy().replace("\\", "/");
        let mut remote_file = sftp.create(&remote_path_str).await.map_err(|e| e.to_string())?;
        
        let total_size = metadata.len();
        let mut transferred_size = 0u64;
        let file_name = local_path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        
        let _ = app.emit("transfer-progress", TransferProgress {
            transfer_id: transfer_id.to_string(),
            transferred: transferred_size,
            total: total_size,
            file_name: file_name.clone(),
        });
        
        let mut buf = vec![0; 65536];
        let mut chunks = 0;
        loop {
            if cancel_token.load(Ordering::Relaxed) {
                return Err("Transfer cancelled by user".into());
            }
            let n = local_file.read(&mut buf).await.map_err(|e| e.to_string())?;
            if n == 0 { break; }
            remote_file.write_all(&buf[..n]).await.map_err(|e| e.to_string())?;
            
            transferred_size += n as u64;
            chunks += 1;
            if chunks % 8 == 0 || transferred_size == total_size {
                let _ = app.emit("transfer-progress", TransferProgress {
                    transfer_id: transfer_id.to_string(),
                    transferred: transferred_size,
                    total: total_size,
                    file_name: file_name.clone(),
                });
            }
        }
        
        if total_size == 0 {
            let _ = app.emit("transfer-progress", TransferProgress {
                transfer_id: transfer_id.to_string(),
                transferred: 0,
                total: 0,
                file_name: file_name.clone(),
            });
        }
    }

    Ok(())
}

async fn download_recursive(
    sftp: &SftpSession,
    remote_path: &Path,
    local_path: &Path,
    cancel_token: Arc<AtomicBool>,
    app: &AppHandle,
    transfer_id: &str,
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
                Box::pin(download_recursive(sftp, &remote_entry_path, &local_entry_path, cancel_token.clone(), app, transfer_id)).await?;
            }
        }
        Err(_) => {
            let mut remote_file = sftp.open(&remote_path_str).await.map_err(|e| e.to_string())?;
            let mut local_file = tokio::fs::File::create(local_path).await.map_err(|e| e.to_string())?;
            
            let total_size = match sftp.metadata(&remote_path_str).await {
                Ok(meta) => meta.size.unwrap_or(0),
                Err(_) => 0,
            };
            
            let mut transferred_size = 0u64;
            let file_name = remote_path.file_name().unwrap_or_default().to_string_lossy().into_owned();
            
            let _ = app.emit("transfer-progress", TransferProgress {
                transfer_id: transfer_id.to_string(),
                transferred: transferred_size,
                total: total_size,
                file_name: file_name.clone(),
            });
            
            let mut buf = vec![0; 65536];
            let mut chunks = 0;
            loop {
                if cancel_token.load(Ordering::Relaxed) {
                    return Err("Transfer cancelled by user".into());
                }
                let n = remote_file.read(&mut buf).await.map_err(|e| e.to_string())?;
                if n == 0 { break; }
                local_file.write_all(&buf[..n]).await.map_err(|e| e.to_string())?;
                
                transferred_size += n as u64;
                chunks += 1;
                if chunks % 8 == 0 || transferred_size == total_size {
                    let _ = app.emit("transfer-progress", TransferProgress {
                        transfer_id: transfer_id.to_string(),
                        transferred: transferred_size,
                        total: total_size,
                        file_name: file_name.clone(),
                    });
                }
            }
            
            if total_size == 0 {
                let _ = app.emit("transfer-progress", TransferProgress {
                    transfer_id: transfer_id.to_string(),
                    transferred: 0,
                    total: 0,
                    file_name: file_name.clone(),
                });
            }
        }
    }
    
    Ok(())
}

#[tauri::command]
pub async fn sftp_upload(
    app: tauri::AppHandle,
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

    let res = upload_recursive(&sftp, Path::new(&local_path), Path::new(&remote_path), cancel_token, &app, &transfer_id).await;

    {
        let mut transfers = state.active_transfers.lock().await;
        transfers.remove(&transfer_id);
    }
    res
}

#[tauri::command]
pub async fn sftp_download(
    app: tauri::AppHandle,
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

    let res = download_recursive(&sftp, Path::new(&remote_path), Path::new(&local_path), cancel_token, &app, &transfer_id).await;

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

/// Max file size allowed for "Edit Locally" (20 MB).
const MAX_EDIT_FILE_SIZE: u64 = 20 * 1024 * 1024;

/// Re-upload a local temp file back to the remote server via a fresh SFTP channel.
async fn upload_from_temp(
    handle: &Arc<Mutex<russh::client::Handle<crate::ssh_session::Client>>>,
    local_path: &std::path::Path,
    remote_path: &str,
) -> Result<(), String> {
    let contents = tokio::fs::read(local_path)
        .await
        .map_err(|e| format!("Failed to read temp file: {}", e))?;

    let channel = {
        let h = handle.lock().await;
        h.channel_open_session()
            .await
            .map_err(|e| format!("Failed to open channel: {}", e))?
    };
    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(|e| format!("Failed to start SFTP subsystem: {}", e))?;
    let sftp = SftpSession::new(channel.into_stream())
        .await
        .map_err(|e| format!("Failed to create SFTP session: {}", e))?;

    let mut remote_file = sftp
        .create(remote_path)
        .await
        .map_err(|e| format!("Failed to create remote file: {}", e))?;
    remote_file
        .write_all(&contents)
        .await
        .map_err(|e| format!("Failed to write remote file: {}", e))?;

    Ok(())
}

#[tauri::command]
pub async fn edit_remote_file(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    remote_path: String,
) -> Result<(), String> {
    // 1. Get the SSH handle
    let handle = {
        let connections = state.connections.lock().await;
        let conn = connections.get(&id).ok_or("Session not found")?;
        conn.handle.clone()
    };

    // 2. Open SFTP channel and check file size
    let channel = {
        let h = handle.lock().await;
        h.channel_open_session()
            .await
            .map_err(|e| e.to_string())?
    };
    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(|e| e.to_string())?;
    let sftp = SftpSession::new(channel.into_stream())
        .await
        .map_err(|e| e.to_string())?;

    // Size guard
    let metadata = sftp
        .metadata(&remote_path)
        .await
        .map_err(|e| format!("Failed to get file metadata: {:?}", e))?;
    let file_size = metadata.size.unwrap_or(0);
    if file_size > MAX_EDIT_FILE_SIZE {
        return Err(format!(
            "File too large ({:.1} MB). Maximum allowed size is 20 MB.",
            file_size as f64 / (1024.0 * 1024.0)
        ));
    }

    // 3. Download the file contents
    let mut remote_file = sftp
        .open(&remote_path)
        .await
        .map_err(|e| format!("Failed to open remote file: {:?}", e))?;
    let mut contents = Vec::new();
    loop {
        let mut buf = vec![0u8; 65536];
        let n = remote_file
            .read(&mut buf)
            .await
            .map_err(|e| format!("Failed to read remote file: {}", e))?;
        if n == 0 {
            break;
        }
        contents.extend_from_slice(&buf[..n]);
    }

    // 4. Write to a temp file (preserving the original extension)
    let remote_p = std::path::Path::new(&remote_path);
    let file_name = remote_p
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let extension = remote_p
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();

    // Build prefix from filename (without extension) for temp file identification
    let stem = remote_p
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let prefix = format!("airlock_{}_", stem);

    let airlock_temp_dir = std::env::temp_dir().join("airlock_edits");
    let _ = std::fs::create_dir_all(&airlock_temp_dir);

    let temp_file = tempfile::Builder::new()
        .prefix(&prefix)
        .suffix(&extension)
        .tempfile_in(&airlock_temp_dir)
        .map_err(|e| format!("Failed to create temp file: {}", e))?;

    // Write contents and persist (keep() prevents auto-deletion)
    let (mut file, temp_path) = temp_file.keep().map_err(|e| format!("Failed to persist temp file: {}", e))?;
    use std::io::Write;
    file.write_all(&contents)
        .map_err(|e| format!("Failed to write temp file: {}", e))?;
    file.flush()
        .map_err(|e| format!("Failed to flush temp file: {}", e))?;
    drop(file); // Close the file handle before opening in editor

    // 5. Open in system default editor
    open::that(&temp_path).map_err(|e| format!("Failed to open editor: {}", e))?;

    // 6. Emit "opened" event
    let _ = app.emit("edit-file-opened", serde_json::json!({
        "remote_path": remote_path,
        "file_name": file_name,
    }));

    // 7. Spawn the file watcher in a background task
    let handle_clone = handle.clone();
    let remote_path_clone = remote_path.clone();
    let file_name_clone = file_name.clone();
    let temp_path_clone = temp_path.clone();
    let app_clone = app.clone();

    tokio::task::spawn_blocking(move || {
        use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
        use std::sync::mpsc;
        use std::time::{Duration, Instant};

        let (tx, rx) = mpsc::channel::<Event>();

        let mut watcher = match RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                if let Ok(event) = res {
                    let _ = tx.send(event);
                }
            },
            Config::default(),
        ) {
            Ok(w) => w,
            Err(e) => {
                log::error!("Failed to create file watcher: {}", e);
                return;
            }
        };

        // Watch the parent directory to catch atomic saves (rename/replace patterns)
        let parent_dir = temp_path_clone
            .parent()
            .unwrap_or(&temp_path_clone);
        if let Err(e) = watcher.watch(parent_dir, RecursiveMode::NonRecursive) {
            log::error!("Failed to start watching: {}", e);
            return;
        }

        log::info!(
            "Watching temp file for changes: {} -> {}",
            temp_path_clone.display(),
            remote_path_clone
        );

        let debounce_duration = Duration::from_secs(2);
        let mut last_upload: Option<Instant> = None;

        // Block on the channel — this runs in a dedicated OS thread via spawn_blocking
        loop {
            match rx.recv_timeout(Duration::from_secs(5)) {
                Ok(event) => {
                    // Only react to Modify or Create events for our specific file
                    let dominated = matches!(
                        event.kind,
                        EventKind::Modify(_) | EventKind::Create(_)
                    );
                    let matches_file = event.paths.iter().any(|p| p == &temp_path_clone);

                    if dominated && matches_file {
                        // Debounce: skip if we uploaded very recently
                        if let Some(last) = last_upload {
                            if last.elapsed() < debounce_duration {
                                continue;
                            }
                        }

                        log::info!("File change detected, uploading: {}", file_name_clone);

                        // Use a new tokio runtime handle to perform the async upload
                        let rt = match tokio::runtime::Handle::try_current() {
                            Ok(h) => h,
                            Err(_) => {
                                log::error!("No tokio runtime available for upload");
                                continue;
                            }
                        };

                        let handle_ref = handle_clone.clone();
                        let rp = remote_path_clone.clone();
                        let tp = temp_path_clone.clone();
                        let app_ref = app_clone.clone();
                        let fn_clone = file_name_clone.clone();

                        let result = rt.block_on(async {
                            upload_from_temp(&handle_ref, &tp, &rp).await
                        });

                        match result {
                            Ok(()) => {
                                last_upload = Some(Instant::now());
                                let _ = app_ref.emit(
                                    "edit-file-saved",
                                    serde_json::json!({
                                        "remote_path": rp,
                                        "file_name": fn_clone,
                                    }),
                                );
                                log::info!("Successfully uploaded: {}", fn_clone);
                            }
                            Err(e) => {
                                let _ = app_ref.emit(
                                    "edit-file-error",
                                    serde_json::json!({
                                        "remote_path": rp,
                                        "file_name": fn_clone,
                                        "error": e.to_string(),
                                    }),
                                );
                                log::error!("Upload failed for {}: {}", fn_clone, e);
                            }
                        }
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    // Keep looping — the watcher stays alive
                    continue;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    log::info!("File watcher channel disconnected, stopping.");
                    break;
                }
            }
        }

        // Cleanup: attempt to remove the temp file
        let _ = std::fs::remove_file(&temp_path_clone);
    });

    Ok(())
}
