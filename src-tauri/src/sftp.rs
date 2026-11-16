use crate::ssh_session::AppState;
use tauri::State;
use russh_sftp::client::SftpSession;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

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

/// Strict allowlist of safe text and source-code file extensions that can be opened
/// via the system default handler. Non-allowlisted extensions (executables, scripts,
/// batch files) must never be launched via shell associations.
pub const SAFE_TEXT_EXTENSIONS: &[&str] = &[
    "txt", "md", "markdown", "json", "json5", "yml", "yaml", "toml",
    "ini", "conf", "cfg", "log", "env", "diff", "patch",
    "rs", "go", "c", "cpp", "cc", "cxx", "h", "hpp", "java", "kt",
    "ts", "jsx", "tsx", "vue", "svelte",
    "css", "scss", "sass", "less", "sql"
];

/// Strict allowlist of exact filenames that can be opened via the system default handler.
pub const SAFE_EXACT_FILENAMES: &[&str] = &[
    "Dockerfile",
    "Makefile",
    "Containerfile",
    "Vagrantfile",
    "Procfile",
    "Gemfile",
    "Rakefile",
    ".gitignore",
    ".env",
    ".bashrc",
    ".zshrc",
    ".profile",
];

/// Target operating system for editor dispatching.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetOs {
    Windows,
    Macos,
    Linux,
    Other,
}

/// Strategy for opening a file in an editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenMethod {
    ConfiguredEditor,
    SystemDefault,
    WindowsNotepad,
    MacosOpenText,
    LinuxXdgOpen,
}

/// Pure decision function determining how a file should be opened based on OS, filename, and editor configuration.
pub fn choose_open_method(
    os: TargetOs,
    file_name: &str,
    has_editor: bool,
) -> Result<OpenMethod, String> {
    if has_editor {
        return Ok(OpenMethod::ConfiguredEditor);
    }

    let is_safe = is_safe_file_for_open(file_name);
    match os {
        TargetOs::Windows => {
            if is_safe {
                Ok(OpenMethod::SystemDefault)
            } else {
                Ok(OpenMethod::WindowsNotepad)
            }
        }
        TargetOs::Macos => Ok(OpenMethod::MacosOpenText),
        TargetOs::Linux => {
            if is_safe {
                Ok(OpenMethod::LinuxXdgOpen)
            } else {
                Err(format!(
                    "File '{}' is not in the safe text allowlist. Please configure an external editor in Settings.",
                    file_name
                ))
            }
        }
        TargetOs::Other => {
            if is_safe {
                Ok(OpenMethod::SystemDefault)
            } else {
                Err(format!(
                    "File '{}' is not in the safe text allowlist. Please configure an external editor in Settings.",
                    file_name
                ))
            }
        }
    }
}

/// Check if a given file extension is in the safe text allowlist.
pub fn is_safe_text_extension(ext: &str) -> bool {
    let clean = ext.trim_start_matches('.');
    if clean.is_empty()
        || clean.ends_with(' ')
        || clean.ends_with('.')
        || clean.starts_with(' ')
        || clean.contains('/')
        || clean.contains('\\')
    {
        return false;
    }
    let lower = clean.to_ascii_lowercase();
    SAFE_TEXT_EXTENSIONS.contains(&lower.as_str())
}

/// Check if a filename or path is safe to open with the system's default text handler.
pub fn is_safe_file_for_open(name: &str) -> bool {
    let file_name = name
        .rsplit('/')
        .next()
        .unwrap_or(name)
        .rsplit('\\')
        .next()
        .unwrap_or(name);

    if file_name.is_empty() {
        return false;
    }

    // 1. Exact safe filenames (e.g. Dockerfile, Makefile, .bashrc)
    if SAFE_EXACT_FILENAMES.contains(&file_name) {
        return true;
    }

    // 2. Reject trailing dot or trailing space in the filename
    if file_name.ends_with('.') || file_name.ends_with(' ') {
        return false;
    }

    // 3. Dotfiles like ".js" (starts with '.' and has no secondary extension)
    if file_name.starts_with('.') {
        let after_dot = &file_name[1..];
        if let Some((_substem, ext)) = after_dot.rsplit_once('.') {
            if ext.is_empty() || ext.ends_with(' ') || ext.ends_with('.') {
                return false;
            }
            return is_safe_text_extension(ext);
        } else {
            // Bare dotfile like ".js", not in SAFE_EXACT_FILENAMES
            return false;
        }
    }

    // 4. Regular files with extensions (e.g. notes.TXT, a.txt.js)
    if let Some((_stem, ext)) = file_name.rsplit_once('.') {
        if ext.is_empty() || ext.ends_with(' ') || ext.ends_with('.') {
            return false;
        }
        is_safe_text_extension(ext)
    } else {
        // No extension and not an exact safe filename
        false
    }
}

/// Platform-aware filename sanitization with explicit `is_windows` flag for testability.
pub fn sanitize_name_with_rules(name: &str, is_windows: bool) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        return Err("Invalid file name".into());
    }

    if name.contains('/') || name.contains('\0') || name.chars().any(|c| c.is_control()) {
        return Err("File name contains path separators, null bytes, or control characters".into());
    }

    if is_windows {
        if name.contains('\\') {
            return Err("File name contains backslash".into());
        }
        if name.contains(':') {
            return Err("File name contains invalid character ':' for Windows NTFS".into());
        }

        let clean = trimmed.trim_end_matches(['.', ' ']);
        if clean.is_empty() {
            return Err("File name cannot consist solely of dots and spaces".into());
        }

        let upper = clean.to_ascii_uppercase();
        let base_stem = upper.split('.').next().unwrap_or("");
        const RESERVED: &[&str] = &[
            "CON", "PRN", "AUX", "NUL",
            "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
            "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9"
        ];
        if RESERVED.contains(&base_stem) {
            return Err(format!("Reserved Windows device name: {base_stem}"));
        }

        Ok(clean.to_string())
    } else {
        Ok(trimmed.to_string())
    }
}

/// Sanitize untrusted remote filenames to prevent path traversal, ADS streams, and device collision.
pub fn sanitize_remote_name(name: &str) -> Result<String, String> {
    sanitize_name_with_rules(name, cfg!(windows))
}

/// Verify that `target` joined to `base_dir` remains strictly confined within `base_dir`.
pub fn is_path_confined(base_dir: &Path, target: &Path) -> bool {
    let mut norm_base = PathBuf::new();
    for component in base_dir.components() {
        match component {
            std::path::Component::ParentDir => { norm_base.pop(); }
            std::path::Component::CurDir => {}
            c => norm_base.push(c),
        }
    }

    let combined = if target.is_absolute() {
        target.to_path_buf()
    } else {
        norm_base.join(target)
    };

    let mut norm_target = PathBuf::new();
    for component in combined.components() {
        match component {
            std::path::Component::ParentDir => {
                if !norm_target.pop() {
                    return false;
                }
            }
            std::path::Component::CurDir => {}
            c => norm_target.push(c),
        }
    }

    norm_target.starts_with(&norm_base)
}

/// Validate that target path is not an existing symlink.
pub async fn validate_download_target(target: &Path) -> Result<(), String> {
    if let Ok(meta) = tokio::fs::symlink_metadata(target).await {
        if meta.is_symlink() {
            return Err(format!("Target path '{}' is a symlink; download refused for security", target.display()));
        }
    }
    Ok(())
}

/// Validate and resolve a remote path and destination directory into a safe local target path.
pub fn resolve_download_target_with_rules(
    remote_path: &str,
    dest_dir: &Path,
    is_windows: bool,
) -> Result<(PathBuf, String, PathBuf), String> {
    let raw_name = remote_path
        .rsplit('/')
        .find(|s| !s.is_empty())
        .ok_or_else(|| "Remote path has no valid filename".to_string())?;

    if raw_name == "." || raw_name == ".." || raw_name.contains('/') || raw_name.contains('\0') {
        return Err(format!("Unsafe remote filename rejected: {:?}", raw_name));
    }

    let sanitized_name = sanitize_name_with_rules(raw_name, is_windows)?;

    let dest_canon = dunce::canonicalize(dest_dir)
        .map_err(|e| format!("Destination directory does not exist or is inaccessible: {}", e))?;
    if !dest_canon.is_dir() {
        return Err("Destination path is not a directory".into());
    }

    let target = dest_canon.join(&sanitized_name);

    if !is_path_confined(&dest_canon, &target) {
        return Err("Path traversal detected outside destination folder".into());
    }

    Ok((dest_canon, sanitized_name, target))
}

/// Validate and resolve a remote path and destination directory string using runtime OS rules.
pub fn resolve_download_target(
    remote_path: &str,
    destination_dir: &str,
) -> Result<(PathBuf, String, PathBuf), String> {
    resolve_download_target_with_rules(remote_path, Path::new(destination_dir), cfg!(windows))
}

/// Get or create the secure temporary directory for local file editing.
pub fn get_airlock_temp_dir() -> Result<PathBuf, std::io::Error> {
    let dir = std::env::temp_dir().join("airlock_edits");
    if !dir.exists() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            let mut builder = std::fs::DirBuilder::new();
            builder.recursive(true);
            builder.mode(0o700);
            builder.create(&dir)?;
        }
        #[cfg(not(unix))]
        {
            std::fs::create_dir_all(&dir)?;
        }
    }
    Ok(dir)
}

/// Clean up orphaned temporary edit files on startup or application exit.
pub fn cleanup_temp_edits() {
    let dir = std::env::temp_dir().join("airlock_edits");
    if dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    let _ = std::fs::remove_file(path);
                }
            }
        }
    }
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
    // SEC-05: Use symlink_metadata to prevent following directory symlinks recursively
    let metadata = tokio::fs::symlink_metadata(local_path).await.map_err(|e| e.to_string())?;

    if metadata.is_symlink() {
        log::warn!("Skipping local symlink during upload: {}", local_path.display());
        return Ok(());
    }

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
    base_destination: &Path,
    cancel_token: Arc<AtomicBool>,
    app: &AppHandle,
    transfer_id: &str,
) -> Result<(), String> {
    // SEC-05: Ensure current target path remains inside base destination directory
    if !is_path_confined(base_destination, local_path) {
        return Err(format!(
            "Path traversal attempt blocked: '{}' escapes destination '{}'",
            local_path.display(),
            base_destination.display()
        ));
    }

    let remote_path_str = remote_path.to_string_lossy().replace("\\", "/");
    
    match sftp.read_dir(&remote_path_str).await {
        Ok(dir) => {
            tokio::fs::create_dir_all(local_path).await.map_err(|e| e.to_string())?;
            for entry in dir {
                if cancel_token.load(Ordering::Relaxed) {
                    return Err("Transfer cancelled by user".into());
                }
                let raw_name = entry.file_name();
                if raw_name == "." || raw_name == ".." {
                    continue;
                }

                // SEC-05: Sanitize entry name against traversal and reserved names
                let sanitized_name = match sanitize_remote_name(&raw_name) {
                    Ok(n) => n,
                    Err(err) => {
                        log::warn!("Skipping unsafe remote entry '{}': {}", raw_name, err);
                        continue;
                    }
                };

                // SEC-05: Skip remote symlinks to prevent traversal
                let attrs = entry.metadata();
                if attrs.is_symlink() {
                    log::warn!("Skipping remote symlink: {}", raw_name);
                    continue;
                }

                let remote_entry_path = remote_path.join(&raw_name);
                let local_entry_path = local_path.join(&sanitized_name);

                if !is_path_confined(base_destination, &local_entry_path) {
                    log::warn!("Skipping path traversal entry: {}", raw_name);
                    continue;
                }

                Box::pin(download_recursive(
                    sftp,
                    &remote_entry_path,
                    &local_entry_path,
                    base_destination,
                    cancel_token.clone(),
                    app,
                    transfer_id,
                )).await?;
            }
        }
        Err(_) => {
            // Ensure destination directory exists
            if let Some(parent) = local_path.parent() {
                tokio::fs::create_dir_all(parent).await.map_err(|e| e.to_string())?;
            }

            let mut remote_file = sftp.open(&remote_path_str).await.map_err(|e| e.to_string())?;
            let mut local_file = tokio::fs::File::create(local_path).await.map_err(|e| e.to_string())?;
            
            let total_size = match sftp.metadata(&remote_path_str).await {
                Ok(meta) => meta.size.unwrap_or(0),
                Err(_) => 0,
            };
            
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

    let res = upload_recursive(
        &sftp,
        Path::new(&local_path),
        Path::new(&remote_path),
        cancel_token,
        &app,
        &transfer_id,
    ).await;

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
    destination_dir: String,
) -> Result<(), String> {
    let (dest_canon, _sanitized_name, target) = resolve_download_target(&remote_path, &destination_dir)?;
    validate_download_target(&target).await?;

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

    let res = download_recursive(
        &sftp,
        Path::new(&remote_path),
        &target,
        &dest_canon,
        cancel_token,
        &app,
        &transfer_id,
    ).await;

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
    local_path: &Path,
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
    editor: Option<String>,
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

    // 3. Download the file contents with streaming byte ceiling
    let mut remote_file = sftp
        .open(&remote_path)
        .await
        .map_err(|e| format!("Failed to open remote file: {:?}", e))?;
    let mut contents = Vec::new();
    let mut total_read = 0u64;
    loop {
        let mut buf = vec![0u8; 65536];
        let n = remote_file
            .read(&mut buf)
            .await
            .map_err(|e| format!("Failed to read remote file: {}", e))?;
        if n == 0 {
            break;
        }
        total_read += n as u64;
        if total_read > MAX_EDIT_FILE_SIZE {
            return Err(format!(
                "File exceeded maximum allowed edit size of 20 MB during transfer"
            ));
        }
        contents.extend_from_slice(&buf[..n]);
    }

    // 4. Write to a secure temp file
    let remote_p = Path::new(&remote_path);
    let file_name = remote_p
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let raw_ext = remote_p
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    let extension = if raw_ext.is_empty() {
        String::new()
    } else {
        format!(".{}", raw_ext)
    };

    let stem = remote_p
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let prefix = format!("airlock_{}_", stem);

    let airlock_temp_dir = get_airlock_temp_dir()
        .map_err(|e| format!("Failed to create temp edit directory: {}", e))?;

    let temp_file = tempfile::Builder::new()
        .prefix(&prefix)
        .suffix(&extension)
        .tempfile_in(&airlock_temp_dir)
        .map_err(|e| format!("Failed to create temp file: {}", e))?;

    // Write contents and persist
    let (mut file, temp_path) = temp_file.keep().map_err(|e| format!("Failed to persist temp file: {}", e))?;
    use std::io::Write;
    file.write_all(&contents)
        .map_err(|e| format!("Failed to write temp file: {}", e))?;
    file.flush()
        .map_err(|e| format!("Failed to flush temp file: {}", e))?;
    drop(file);

    // 5. SEC-04: Safe Editor Execution
    let custom_editor = editor.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty());
    let current_os = if cfg!(target_os = "windows") {
        TargetOs::Windows
    } else if cfg!(target_os = "macos") {
        TargetOs::Macos
    } else if cfg!(target_os = "linux") {
        TargetOs::Linux
    } else {
        TargetOs::Other
    };

    let method = choose_open_method(current_os, &file_name, custom_editor.is_some())?;

    match method {
        OpenMethod::ConfiguredEditor => {
            let cmd_str = custom_editor.unwrap();
            let mut parts = cmd_str.split_whitespace();
            let program = parts.next().ok_or_else(|| "Invalid editor command".to_string())?;
            let mut cmd = std::process::Command::new(program);
            for arg in parts {
                cmd.arg(arg);
            }
            cmd.arg(&temp_path);
            cmd.spawn().map_err(|e| format!("Failed to launch configured editor '{}': {}", cmd_str, e))?;
        }
        OpenMethod::SystemDefault => {
            open::that(&temp_path).map_err(|e| format!("Failed to open default editor: {}", e))?;
        }
        OpenMethod::WindowsNotepad => {
            log::info!(
                "Non-allowlisted file '{}' opened safely with explicit notepad.exe",
                file_name
            );
            let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
            let mut notepad_path = PathBuf::from(&system_root).join("System32").join("notepad.exe");
            if !notepad_path.exists() {
                notepad_path = PathBuf::from("C:\\Windows\\System32\\notepad.exe");
                if !notepad_path.exists() {
                    notepad_path = PathBuf::from("C:\\Windows\\notepad.exe");
                }
            }
            let mut cmd = std::process::Command::new(&notepad_path);
            cmd.arg(&temp_path);
            cmd.spawn().map_err(|e| format!("Failed to open in Notepad ('{}'): {}", notepad_path.display(), e))?;
        }
        OpenMethod::MacosOpenText => {
            let mut cmd = std::process::Command::new("open");
            cmd.arg("-t").arg(&temp_path);
            cmd.spawn().map_err(|e| format!("Failed to open default editor via open -t: {}", e))?;
        }
        OpenMethod::LinuxXdgOpen => {
            let mut cmd = std::process::Command::new("xdg-open");
            cmd.arg(&temp_path);
            cmd.spawn().map_err(|e| format!("Failed to open file via xdg-open: {}", e))?;
        }
    }

    // 6. Emit "opened" event
    let _ = app.emit("edit-file-opened", serde_json::json!({
        "remote_path": remote_path,
        "file_name": file_name,
    }));

    // 7. Spawn the file watcher with session liveness checks and guaranteed cleanup
    let handle_clone = handle.clone();
    let remote_path_clone = remote_path.clone();
    let file_name_clone = file_name.clone();
    let temp_path_clone = temp_path.clone();
    let app_clone = app.clone();
    let id_clone = id.clone();

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
                let _ = std::fs::remove_file(&temp_path_clone);
                return;
            }
        };

        let parent_dir = temp_path_clone
            .parent()
            .unwrap_or(&temp_path_clone);
        if let Err(e) = watcher.watch(parent_dir, RecursiveMode::NonRecursive) {
            log::error!("Failed to start watching: {}", e);
            let _ = std::fs::remove_file(&temp_path_clone);
            return;
        }

        log::info!(
            "Watching temp file for changes: {} -> {}",
            temp_path_clone.display(),
            remote_path_clone
        );

        let debounce_duration = Duration::from_secs(2);
        let mut last_upload: Option<Instant> = None;

        loop {
            // SEC-04: Terminate watcher if temp file was deleted
            if !temp_path_clone.exists() {
                log::info!("Temp file removed, stopping watcher: {}", temp_path_clone.display());
                break;
            }

            // SEC-04: Terminate watcher if SSH session is disconnected
            let session_active = {
                let state = app_clone.state::<AppState>();
                let conns = state.connections.blocking_lock();
                conns.contains_key(&id_clone)
            };
            if !session_active {
                log::info!("SSH session {} closed, stopping file watcher", id_clone);
                break;
            }

            match rx.recv_timeout(Duration::from_secs(3)) {
                Ok(event) => {
                    let dominated = matches!(
                        event.kind,
                        EventKind::Modify(_) | EventKind::Create(_)
                    );
                    let matches_file = event.paths.iter().any(|p| p == &temp_path_clone);

                    if dominated && matches_file {
                        if let Some(last) = last_upload {
                            if last.elapsed() < debounce_duration {
                                continue;
                            }
                        }

                        log::info!("File change detected, uploading: {}", file_name_clone);

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
                    continue;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    log::info!("File watcher channel disconnected, stopping.");
                    break;
                }
            }
        }

        // Drop watcher and clean up temp file
        drop(watcher);
        let _ = std::fs::remove_file(&temp_path_clone);
        log::info!("Cleaned up temp edit file: {}", temp_path_clone.display());
    });

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_filename_universal() {
        assert!(sanitize_name_with_rules("", true).is_err());
        assert!(sanitize_name_with_rules("   ", false).is_err());
        assert!(sanitize_name_with_rules(".", true).is_err());
        assert!(sanitize_name_with_rules("..", false).is_err());
        assert!(sanitize_name_with_rules("foo/bar", false).is_err());
        assert!(sanitize_name_with_rules("foo\0bar", true).is_err());
        assert!(sanitize_name_with_rules("foo\nbar", false).is_err());
    }

    #[test]
    fn test_sanitize_filename_windows_rules() {
        // Backslash disallowed
        assert!(sanitize_name_with_rules(r"foo\bar", true).is_err());
        // ADS colon disallowed on Windows
        assert!(sanitize_name_with_rules("file.txt:hidden", true).is_err());
        // Reserved device names disallowed on Windows
        assert!(sanitize_name_with_rules("CON", true).is_err());
        assert!(sanitize_name_with_rules("con.txt", true).is_err());
        assert!(sanitize_name_with_rules("nul.tar.gz", true).is_err());
        assert!(sanitize_name_with_rules("aux", true).is_err());
        assert!(sanitize_name_with_rules("com1.txt", true).is_err());
        assert!(sanitize_name_with_rules("lpt3", true).is_err());
        // Trailing dots and spaces stripped on Windows
        assert_eq!(sanitize_name_with_rules("test.txt...", true).unwrap(), "test.txt");
        assert_eq!(sanitize_name_with_rules("test.txt   ", true).unwrap(), "test.txt");
        assert_eq!(sanitize_name_with_rules("test.txt. . ", true).unwrap(), "test.txt");
    }

    #[test]
    fn test_sanitize_filename_unix_rules() {
        // Colon is legal in Unix/POSIX filenames
        assert_eq!(
            sanitize_name_with_rules("backup:2026-11-05.tar.gz", false).unwrap(),
            "backup:2026-11-05.tar.gz"
        );
        assert_eq!(
            sanitize_name_with_rules("log:12:00:00.txt", false).unwrap(),
            "log:12:00:00.txt"
        );
        // Reserved Windows names are permitted on Unix
        assert_eq!(sanitize_name_with_rules("con.txt", false).unwrap(), "con.txt");
        assert_eq!(sanitize_name_with_rules("nul", false).unwrap(), "nul");
    }

    #[test]
    fn test_path_confinement() {
        let base = Path::new("/var/data/downloads");

        // Safe relative children
        assert!(is_path_confined(base, Path::new("file.txt")));
        assert!(is_path_confined(base, Path::new("sub/folder/file.txt")));
        assert!(is_path_confined(base, Path::new("./sub/file.txt")));

        // Traversal attempts
        assert!(!is_path_confined(base, Path::new("../file.txt")));
        assert!(!is_path_confined(base, Path::new("../../etc/passwd")));
        assert!(!is_path_confined(base, Path::new("sub/../../escape.txt")));

        #[cfg(windows)]
        {
            // Windows drive traversal
            let win_base = Path::new(r"C:\Users\User\Downloads");
            assert!(is_path_confined(win_base, Path::new("safe.txt")));
            assert!(!is_path_confined(win_base, Path::new(r"..\..\Windows\System32\cmd.exe")));
            assert!(!is_path_confined(win_base, Path::new(r"C:\Windows\System32\cmd.exe")));
        }

        #[cfg(unix)]
        {
            let unix_base = Path::new("/home/user/downloads");
            assert!(is_path_confined(unix_base, Path::new("safe.txt")));
            assert!(is_path_confined(unix_base, Path::new("nested/dir/safe.txt")));
            assert!(!is_path_confined(unix_base, Path::new("../../etc/shadow")));
            assert!(!is_path_confined(unix_base, Path::new("/etc/shadow")));
        }
    }

    #[test]
    fn test_safe_text_extensions() {
        assert!(is_safe_text_extension("txt"));
        assert!(is_safe_text_extension(".rs"));
        assert!(is_safe_text_extension("JSON"));
        assert!(is_safe_text_extension("md"));
        assert!(is_safe_text_extension("yaml"));

        // Script, executable, spreadsheet, and xml extensions must be rejected (case-insensitive)
        for ext in &[
            "js", "py", "sh", "bash", "html", "htm", "csv", "tsv", "mjs", "cjs", "JS", "Py",
            "xml", "bat", "cmd", "exe", "vbs", "ps1", "hta", "lnk", ""
        ] {
            assert!(!is_safe_text_extension(ext), "Extension {:?} should be rejected", ext);
        }
    }

    #[test]
    fn test_safe_file_for_open() {
        // Assert real names are rejected
        assert!(!is_safe_file_for_open("x.js"));
        assert!(!is_safe_file_for_open("x.PY"));
        assert!(!is_safe_file_for_open("page.html"));
        assert!(!is_safe_file_for_open("data.csv"));
        assert!(!is_safe_file_for_open("run.sh"));
        assert!(!is_safe_file_for_open("notes.txt ")); // trailing space rejected
        assert!(!is_safe_file_for_open("run.bat."));  // trailing dot rejected
        assert!(!is_safe_file_for_open("run.js "));   // trailing space rejected
        assert!(!is_safe_file_for_open("a.txt.js"));
        assert!(!is_safe_file_for_open(".js"));
        assert!(!is_safe_file_for_open("data.tsv"));
        assert!(!is_safe_file_for_open("file.xml"));

        // Assert real names are accepted
        assert!(is_safe_file_for_open("notes.txt"));
        assert!(is_safe_file_for_open("a b.txt")); // space in filename accepted
        assert!(is_safe_file_for_open("notes.TXT"));
        assert!(is_safe_file_for_open("Dockerfile"));
        assert!(is_safe_file_for_open(".bashrc"));
        assert!(is_safe_file_for_open("Makefile"));
    }

    #[test]
    fn test_choose_open_method_matrix() {
        // 1. With configured editor (has_editor = true) -> always OpenMethod::ConfiguredEditor across all OSes
        for os in &[TargetOs::Windows, TargetOs::Macos, TargetOs::Linux, TargetOs::Other] {
            for file in &["notes.txt", "x.js", "page.html", "run.bat.", "notes.txt ", "Dockerfile"] {
                assert_eq!(
                    choose_open_method(*os, file, true),
                    Ok(OpenMethod::ConfiguredEditor),
                    "OS {:?} with file {:?} and has_editor=true must return ConfiguredEditor",
                    os, file
                );
            }
        }

        // 2. Without editor (has_editor = false)
        // Windows:
        // Allowlisted files -> SystemDefault
        assert_eq!(choose_open_method(TargetOs::Windows, "notes.txt", false), Ok(OpenMethod::SystemDefault));
        assert_eq!(choose_open_method(TargetOs::Windows, "a b.txt", false), Ok(OpenMethod::SystemDefault));
        assert_eq!(choose_open_method(TargetOs::Windows, "Dockerfile", false), Ok(OpenMethod::SystemDefault));
        // Non-allowlisted files -> WindowsNotepad
        assert_eq!(choose_open_method(TargetOs::Windows, "x.js", false), Ok(OpenMethod::WindowsNotepad));
        assert_eq!(choose_open_method(TargetOs::Windows, "x.PY", false), Ok(OpenMethod::WindowsNotepad));
        assert_eq!(choose_open_method(TargetOs::Windows, "page.html", false), Ok(OpenMethod::WindowsNotepad));
        assert_eq!(choose_open_method(TargetOs::Windows, "data.csv", false), Ok(OpenMethod::WindowsNotepad));
        assert_eq!(choose_open_method(TargetOs::Windows, "run.sh", false), Ok(OpenMethod::WindowsNotepad));
        assert_eq!(choose_open_method(TargetOs::Windows, "notes.txt ", false), Ok(OpenMethod::WindowsNotepad));
        assert_eq!(choose_open_method(TargetOs::Windows, "run.bat.", false), Ok(OpenMethod::WindowsNotepad));
        assert_eq!(choose_open_method(TargetOs::Windows, "file.xml", false), Ok(OpenMethod::WindowsNotepad));

        // macOS:
        // Always MacosOpenText regardless of file type when no editor configured
        assert_eq!(choose_open_method(TargetOs::Macos, "notes.txt", false), Ok(OpenMethod::MacosOpenText));
        assert_eq!(choose_open_method(TargetOs::Macos, "x.js", false), Ok(OpenMethod::MacosOpenText));
        assert_eq!(choose_open_method(TargetOs::Macos, "page.html", false), Ok(OpenMethod::MacosOpenText));
        assert_eq!(choose_open_method(TargetOs::Macos, "Dockerfile", false), Ok(OpenMethod::MacosOpenText));
        assert_eq!(choose_open_method(TargetOs::Macos, "notes.txt ", false), Ok(OpenMethod::MacosOpenText));

        // Linux:
        // Allowlisted files -> LinuxXdgOpen
        assert_eq!(choose_open_method(TargetOs::Linux, "notes.txt", false), Ok(OpenMethod::LinuxXdgOpen));
        assert_eq!(choose_open_method(TargetOs::Linux, "a b.txt", false), Ok(OpenMethod::LinuxXdgOpen));
        assert_eq!(choose_open_method(TargetOs::Linux, "Dockerfile", false), Ok(OpenMethod::LinuxXdgOpen));
        // Non-allowlisted files -> Err
        assert!(choose_open_method(TargetOs::Linux, "x.js", false).is_err());
        assert!(choose_open_method(TargetOs::Linux, "x.PY", false).is_err());
        assert!(choose_open_method(TargetOs::Linux, "page.html", false).is_err());
        assert!(choose_open_method(TargetOs::Linux, "data.csv", false).is_err());
        assert!(choose_open_method(TargetOs::Linux, "run.sh", false).is_err());
        assert!(choose_open_method(TargetOs::Linux, "notes.txt ", false).is_err());
        assert!(choose_open_method(TargetOs::Linux, "run.bat.", false).is_err());
        assert!(choose_open_method(TargetOs::Linux, "file.xml", false).is_err());

        // Other:
        assert_eq!(choose_open_method(TargetOs::Other, "notes.txt", false), Ok(OpenMethod::SystemDefault));
        assert!(choose_open_method(TargetOs::Other, "x.js", false).is_err());
    }

    #[test]
    fn test_download_confinement_backslash_traversal() {
        let temp = tempfile::tempdir().unwrap();
        let dest = temp.path();

        // (a) a name like "..\..\evil.txt" is rejected on Windows and treated as one ordinary file name on Unix
        #[cfg(windows)]
        {
            let res_win = resolve_download_target_with_rules(r"/remote/..\..\evil.txt", dest, true);
            assert!(res_win.is_err(), "Expected backslash path to be rejected on Windows");
            assert_eq!(sanitize_name_with_rules(r"..\..\evil.txt", false).unwrap(), r"..\..\evil.txt");
        }

        #[cfg(unix)]
        {
            let res_unix = resolve_download_target_with_rules(r"/remote/..\..\evil.txt", dest, false);
            assert!(res_unix.is_ok(), "Expected backslash path to be treated as ordinary file name on Unix");
            let (dest_c, name_unix, target_unix) = res_unix.unwrap();
            assert_eq!(name_unix, r"..\..\evil.txt");
            assert_eq!(target_unix, dest_c.join(r"..\..\evil.txt"));
        }
    }

    #[test]
    fn test_download_confinement_windows_drive_relative() {
        let temp = tempfile::tempdir().unwrap();
        let dest = temp.path();

        // (b) C:evil.txt is rejected (cfg windows)
        #[cfg(windows)]
        {
            let res_drive = resolve_download_target_with_rules("/remote/C:evil.txt", dest, true);
            assert!(res_drive.is_err(), "Expected C:evil.txt to be rejected on Windows");
        }
    }

    #[test]
    fn test_download_confinement_dot_and_nul() {
        let temp = tempfile::tempdir().unwrap();
        let dest = temp.path();

        // Rejection of "." and ".." and names with '/' or NUL
        assert!(resolve_download_target_with_rules("/remote/.", dest, true).is_err());
        assert!(resolve_download_target_with_rules("/remote/..", dest, true).is_err());
        assert!(resolve_download_target_with_rules("/remote/foo\0bar", dest, true).is_err());
    }

    #[test]
    fn test_download_confinement_root_and_trailing_slash() {
        let temp = tempfile::tempdir().unwrap();
        let dest = temp.path();

        // Root "/" and "///" must be rejected (no filename)
        assert!(resolve_download_target_with_rules("/", dest, cfg!(windows)).is_err());
        assert!(resolve_download_target_with_rules("///", dest, cfg!(windows)).is_err());
        assert!(resolve_download_target_with_rules("", dest, cfg!(windows)).is_err());

        // Trailing slashes should resolve the last non-empty segment
        let res_trailing = resolve_download_target_with_rules("/remote/my_folder/", dest, cfg!(windows)).unwrap();
        assert_eq!(res_trailing.1, "my_folder");
        assert_eq!(res_trailing.2, res_trailing.0.join("my_folder"));
        assert!(is_path_confined(&res_trailing.0, &res_trailing.2));

        let res_multi_trailing = resolve_download_target_with_rules("/remote/another_folder///", dest, cfg!(windows)).unwrap();
        assert_eq!(res_multi_trailing.1, "another_folder");
        assert_eq!(res_multi_trailing.2, res_multi_trailing.0.join("another_folder"));
        assert!(is_path_confined(&res_multi_trailing.0, &res_multi_trailing.2));
    }

    #[test]
    fn test_download_confinement_normal_name() {
        let temp = tempfile::tempdir().unwrap();
        let dest = temp.path();

        // (d) a normal name downloads into destination_dir
        let res_normal = resolve_download_target_with_rules("/var/log/report.txt", dest, cfg!(windows)).unwrap();
        assert_eq!(res_normal.1, "report.txt");
        assert_eq!(res_normal.2, res_normal.0.join("report.txt"));
        assert!(is_path_confined(&res_normal.0, &res_normal.2));
    }

    #[test]
    fn test_download_confinement_circular_anchor() {
        let temp = tempfile::tempdir().unwrap();
        let dest = temp.path();

        // (e) the circular-anchor case: <dest>/../../evil.txt must fail confinement against the chosen folder
        let circular_target = dest.join("../../evil.txt");
        let circular_base = circular_target.parent().unwrap();
        assert!(is_path_confined(circular_base, &circular_target), "Old circular anchor incorrectly allowed traversal");
        assert!(
            !is_path_confined(dest, &circular_target),
            "<dest>/../../evil.txt must fail confinement against the chosen folder"
        );
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_target_symlink_refused_unix() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let dest = temp.path();

        let real_file = dest.join("real_target.txt");
        tokio::fs::write(&real_file, b"test content").await.unwrap();

        let link_target = dest.join("symlink_target.txt");
        symlink(&real_file, &link_target).unwrap();

        // (c) a symlink at the target is refused
        assert!(validate_download_target(&link_target).await.is_err(), "Symlink at target must be refused");
        assert!(validate_download_target(&real_file).await.is_ok(), "Regular file must be accepted");
    }

    #[test]
    fn test_cleanup_temp_edits() {
        let temp_dir = std::env::temp_dir().join("airlock_edits");
        let _ = std::fs::create_dir_all(&temp_dir);
        let test_file = temp_dir.join("airlock_test_orphaned_file.tmp");
        std::fs::write(&test_file, b"test").unwrap();
        assert!(test_file.exists());

        cleanup_temp_edits();
        assert!(!test_file.exists(), "Cleanup must purge orphaned edit files");
    }
}
