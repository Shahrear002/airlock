use crate::ssh_session::AppState;
#[allow(unused_imports)]
use fs4::FileExt;
use russh_sftp::client::SftpSession;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::State;
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkippedItem {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferSummary {
    pub transfer_id: String,
    pub total_bytes: u64,
    pub files_transferred: u64,
    pub destination_path: String,
    pub skipped: Vec<SkippedItem>,
    pub renamed: Vec<(String, String)>,
}

#[derive(Debug, Default)]
pub struct TransferStats {
    pub total_bytes: u64,
    pub files_transferred: u64,
    pub skipped: Vec<SkippedItem>,
    pub renamed: Vec<(String, String)>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum UploadTargetKind {
    File(u64),
    FollowedFileSymlink(u64),
    Directory,
    SkippedSymlink(String),
}

/// Resolves whether a local upload target is a file, followed file symlink, directory, or skipped symlink.
pub async fn resolve_upload_target(
    local_path: &Path,
    is_top_level: bool,
) -> Result<UploadTargetKind, String> {
    let sym_meta = tokio::fs::symlink_metadata(local_path)
        .await
        .map_err(|e| e.to_string())?;

    if is_symlink_or_reparse(&sym_meta) {
        if is_top_level {
            match tokio::fs::metadata(local_path).await {
                Ok(target_meta) => {
                    if target_meta.is_file() {
                        return Ok(UploadTargetKind::FollowedFileSymlink(target_meta.len()));
                    } else {
                        return Ok(UploadTargetKind::SkippedSymlink(
                            "symbolic link".to_string(),
                        ));
                    }
                }
                Err(_) => {
                    return Ok(UploadTargetKind::SkippedSymlink(
                        "symbolic link".to_string(),
                    ));
                }
            }
        } else {
            return Ok(UploadTargetKind::SkippedSymlink(
                "symbolic link".to_string(),
            ));
        }
    }

    if sym_meta.is_dir() {
        Ok(UploadTargetKind::Directory)
    } else {
        Ok(UploadTargetKind::File(sym_meta.len()))
    }
}

/// Checks if the transfer directory depth exceeds the 64-level limit.
pub fn check_transfer_depth(depth: usize, path: &str, stats: &mut TransferStats) -> bool {
    if depth > 64 {
        stats.skipped.push(SkippedItem {
            path: path.to_string(),
            reason: "directory depth exceeds 64 levels".to_string(),
        });
        false
    } else {
        true
    }
}

/// Strict allowlist of safe text and source-code file extensions that can be opened
/// via the system default handler. Non-allowlisted extensions (executables, scripts,
/// batch files) must never be launched via shell associations.
pub const SAFE_TEXT_EXTENSIONS: &[&str] = &[
    "txt", "md", "markdown", "json", "json5", "yml", "yaml", "toml", "ini", "conf", "cfg", "log",
    "env", "diff", "patch", "rs", "go", "c", "cpp", "cc", "cxx", "h", "hpp", "java", "kt", "ts",
    "jsx", "tsx", "vue", "svelte", "css", "scss", "sass", "less", "sql",
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

impl AsRef<str> for TargetOs {
    fn as_ref(&self) -> &str {
        match self {
            TargetOs::Windows => "windows",
            TargetOs::Macos => "macos",
            TargetOs::Linux => "linux",
            TargetOs::Other => "other",
        }
    }
}

/// Trait for persisting and retrieving the user's configured external editor path.
pub trait EditorStore: Send + Sync {
    fn get(&self) -> Result<Option<String>, String>;
    fn set(&self, path: &str) -> Result<(), String>;
    fn clear(&self) -> Result<(), String>;
}

/// Keyring-backed external editor store targeting the OS secure credential store.
pub struct KeyringEditorStore {
    service: String,
    account: String,
}

impl KeyringEditorStore {
    pub fn new() -> Self {
        Self {
            service: "com.airlock.app".to_string(),
            account: "external_editor".to_string(),
        }
    }
}

impl Default for KeyringEditorStore {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorStore for KeyringEditorStore {
    fn get(&self) -> Result<Option<String>, String> {
        let entry = keyring::Entry::new(&self.service, &self.account).map_err(|e| {
            format!(
                "Failed to access OS keyring: {}. On Linux, ensure Secret Service is running.",
                e
            )
        })?;
        match entry.get_password() {
            Ok(val) => {
                let trimmed = val.trim().to_string();
                if trimmed.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(trimmed))
                }
            }
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(format!(
                "Failed to read editor from OS keyring: {}. On Linux, ensure Secret Service is running.",
                e
            )),
        }
    }

    fn set(&self, path: &str) -> Result<(), String> {
        let entry = keyring::Entry::new(&self.service, &self.account).map_err(|e| {
            format!(
                "Failed to access OS keyring: {}. On Linux, ensure Secret Service is running.",
                e
            )
        })?;
        entry.set_password(path).map_err(|e| {
            format!(
                "Failed to store editor in OS keyring: {}. On Linux, ensure Secret Service is running.",
                e
            )
        })?;
        Ok(())
    }

    fn clear(&self) -> Result<(), String> {
        let entry = keyring::Entry::new(&self.service, &self.account).map_err(|e| {
            format!(
                "Failed to access OS keyring: {}. On Linux, ensure Secret Service is running.",
                e
            )
        })?;
        match entry.delete_credential() {
            Ok(_) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(format!(
                "Failed to clear editor from OS keyring: {}. On Linux, ensure Secret Service is running.",
                e
            )),
        }
    }
}

/// In-memory mock store for unit testing without touching the OS keyring.
#[derive(Default)]
#[allow(dead_code)]
pub struct MockEditorStore {
    editor: std::sync::Mutex<Option<String>>,
}

#[allow(dead_code)]
impl MockEditorStore {
    pub fn new() -> Self {
        Self {
            editor: std::sync::Mutex::new(None),
        }
    }

    pub fn with_editor(path: &str) -> Self {
        Self {
            editor: std::sync::Mutex::new(Some(path.to_string())),
        }
    }
}

impl EditorStore for MockEditorStore {
    fn get(&self) -> Result<Option<String>, String> {
        let lock = self.editor.lock().map_err(|e| e.to_string())?;
        Ok(lock.clone())
    }

    fn set(&self, path: &str) -> Result<(), String> {
        let mut lock = self.editor.lock().map_err(|e| e.to_string())?;
        *lock = Some(path.to_string());
        Ok(())
    }

    fn clear(&self) -> Result<(), String> {
        let mut lock = self.editor.lock().map_err(|e| e.to_string())?;
        *lock = None;
        Ok(())
    }
}

/// Facts about a filesystem path used for validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathFacts {
    pub exists: bool,
    pub is_file: bool,
    pub is_dir: bool,
    pub is_executable: bool,
}

#[allow(dead_code)]
impl PathFacts {
    pub fn new(exists: bool, is_file: bool, is_dir: bool, is_executable: bool) -> Self {
        Self {
            exists,
            is_file,
            is_dir,
            is_executable,
        }
    }

    pub fn file(executable: bool) -> Self {
        Self {
            exists: true,
            is_file: true,
            is_dir: false,
            is_executable: executable,
        }
    }

    pub fn dir() -> Self {
        Self {
            exists: true,
            is_file: false,
            is_dir: true,
            is_executable: false,
        }
    }

    pub fn not_found() -> Self {
        Self {
            exists: false,
            is_file: false,
            is_dir: false,
            is_executable: false,
        }
    }
}

/// Helper to determine if a path is absolute for a given target OS.
pub fn is_path_absolute_for_os(os: &str, path: &Path) -> bool {
    let path_str = path.to_str().unwrap_or("");
    match os {
        "windows" => {
            let bytes = path_str.as_bytes();
            (bytes.len() >= 3
                && bytes[0].is_ascii_alphabetic()
                && bytes[1] == b':'
                && (bytes[2] == b'\\' || bytes[2] == b'/'))
                || (bytes.len() >= 2 && bytes[0] == b'\\' && bytes[1] == b'\\')
        }
        "macos" | "linux" => path_str.starts_with('/'),
        _ => path.is_absolute(),
    }
}

/// Pure validation logic checking whether an external editor path meets platform security rules.
pub fn validate_editor_path_with_rules<O: AsRef<str>>(
    os: O,
    path: &Path,
    facts: &PathFacts,
) -> Result<(), String> {
    let os_str = os.as_ref().to_ascii_lowercase();

    // 1. Must be absolute
    if !is_path_absolute_for_os(&os_str, path) {
        return Err("Editor path must be an absolute path".into());
    }

    // 2. Must exist
    if !facts.exists {
        return Err(format!("Editor path '{}' does not exist", path.display()));
    }

    // Extract filename / bundle name
    let path_str = path.to_str().unwrap_or("");
    let file_name = path_str
        .rsplit('/')
        .next()
        .unwrap_or(path_str)
        .rsplit('\\')
        .next()
        .unwrap_or(path_str);

    match os_str.as_str() {
        "windows" => {
            // Must be a file
            if !facts.is_file {
                return Err("Editor path on Windows must be a file".into());
            }

            // Must be .exe only
            if let Some((stem, ext)) = file_name.rsplit_once('.') {
                if stem.is_empty() || !ext.eq_ignore_ascii_case("exe") {
                    return Err(format!(
                        "External editor on Windows must be an executable (.exe) file, got '{}'",
                        file_name
                    ));
                }
            } else {
                return Err(format!(
                    "External editor on Windows must have a .exe extension, got '{}'",
                    file_name
                ));
            }

            Ok(())
        }
        "macos" => {
            if facts.is_dir {
                // Must be a .app bundle directory
                if file_name.to_ascii_lowercase().ends_with(".app") {
                    Ok(())
                } else {
                    Err(format!(
                        "Directory '{}' is not a valid macOS .app bundle",
                        file_name
                    ))
                }
            } else if facts.is_file {
                if !facts.is_executable {
                    return Err(format!(
                        "macOS editor binary '{}' must have execute permissions",
                        file_name
                    ));
                }
                Ok(())
            } else {
                Err("macOS editor must be a .app bundle directory or an executable file".into())
            }
        }
        "linux" => {
            if !facts.is_file {
                return Err("Editor path on Linux must be a file".into());
            }
            if !facts.is_executable {
                return Err(format!(
                    "Editor path on Linux '{}' must have execute permissions",
                    file_name
                ));
            }
            Ok(())
        }
        _ => {
            if !facts.is_file {
                return Err("Editor path must be a file".into());
            }
            if !facts.is_executable {
                return Err(format!(
                    "Editor path '{}' must have execute permissions",
                    file_name
                ));
            }
            Ok(())
        }
    }
}

/// Retrieve path facts from the local filesystem for path validation.
pub fn get_path_facts(path: &Path) -> PathFacts {
    let metadata = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(_) => return PathFacts::not_found(),
    };

    let is_dir = metadata.is_dir();
    let is_file = metadata.is_file();

    #[cfg(unix)]
    let is_executable = {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    };

    #[cfg(windows)]
    let is_executable = {
        is_file
            && path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("exe"))
                .unwrap_or(false)
    };

    #[cfg(not(any(unix, windows)))]
    let is_executable = is_file;

    PathFacts {
        exists: true,
        is_file,
        is_dir,
        is_executable,
    }
}

/// Thin wrapper that validates an external editor path against the current operating system rules.
pub fn validate_editor_path(path: &Path) -> Result<(), String> {
    let current_os = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "other"
    };

    let facts = get_path_facts(path);
    validate_editor_path_with_rules(current_os, path, &facts)
}

/// Pure helper to resolve the external editor from the store and validate it.
/// If reading from the store fails (e.g. OS keyring unavailable), it logs a warning
/// and falls back to `Ok(None)` so Edit Locally is never blocked.
/// If a configured path fails validation, returns an error pointing the user to Settings.
pub fn resolve_configured_editor(
    store: &dyn EditorStore,
    validate: impl Fn(&Path) -> Result<(), String>,
) -> Result<Option<String>, String> {
    let configured_editor = match store.get() {
        Ok(Some(path)) => {
            let trimmed = path.trim().to_string();
            if trimmed.is_empty() {
                return Ok(None);
            }
            trimmed
        }
        Ok(None) => return Ok(None),
        Err(e) => {
            log::warn!(
                "Failed to read external editor from keyring: {}. Defaulting to system editor.",
                e
            );
            return Ok(None);
        }
    };

    let p = Path::new(&configured_editor);
    validate(p).map_err(|e| {
        format!(
            "Configured external editor '{}' is invalid: {}. Please configure a valid editor in Settings.",
            configured_editor, e
        )
    })?;

    Ok(Some(configured_editor))
}

/// Kind of launch mechanism for the configured external editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorLaunchKind {
    MacosOpenApp,
    DirectExecutable,
}

/// Pure decision function determining how the configured editor should be launched.
/// On macOS, `.app` bundle directories must be launched via `open -a <app> <file>`,
/// while plain binaries (e.g. /usr/local/bin/code) and editors on other OSes
/// are spawned directly via `Command::new(path).arg(file)`.
pub fn editor_launch_kind<O: AsRef<str>>(os: O, path: &Path, is_dir: bool) -> EditorLaunchKind {
    let os_str = os.as_ref().to_ascii_lowercase();
    if os_str == "macos" && is_dir {
        let path_str = path.to_str().unwrap_or("");
        let file_name = path_str
            .rsplit('/')
            .next()
            .unwrap_or(path_str)
            .rsplit('\\')
            .next()
            .unwrap_or(path_str);
        if file_name.to_ascii_lowercase().ends_with(".app") {
            return EditorLaunchKind::MacosOpenApp;
        }
    }
    EditorLaunchKind::DirectExecutable
}

/// Builds the process command for launching the configured editor without shell execution or whitespace splitting.
pub fn build_configured_editor_command(
    editor_path: &Path,
    file_path: &Path,
) -> std::process::Command {
    let current_os = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "other"
    };

    let kind = editor_launch_kind(current_os, editor_path, editor_path.is_dir());
    match kind {
        EditorLaunchKind::MacosOpenApp => {
            let mut cmd = std::process::Command::new("open");
            cmd.arg("-a").arg(editor_path).arg(file_path);
            cmd
        }
        EditorLaunchKind::DirectExecutable => {
            let mut cmd = std::process::Command::new(editor_path);
            cmd.arg(file_path);
            cmd
        }
    }
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
        if name.ends_with('.') || name.ends_with(' ') {
            return Err("File name cannot end in a dot or space on Windows".into());
        }
        if name
            .chars()
            .any(|c| matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
        {
            return Err("File name contains invalid character for Windows (< > : \" | ? *)".into());
        }

        let upper = trimmed.to_ascii_uppercase();
        let base_stem = upper.split('.').next().unwrap_or("");
        const RESERVED: &[&str] = &[
            "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
            "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
        ];
        if RESERVED.contains(&base_stem) {
            return Err(format!("Reserved Windows device name: {base_stem}"));
        }

        Ok(trimmed.to_string())
    } else {
        Ok(trimmed.to_string())
    }
}

/// Sanitize untrusted remote filenames to prevent path traversal, ADS streams, and device collision.
#[allow(dead_code)]
pub fn sanitize_remote_name(name: &str) -> Result<String, String> {
    sanitize_name_with_rules(name, cfg!(windows))
}

/// Verify that `target` joined to `base_dir` remains strictly confined within `base_dir`.
pub fn is_path_confined(base_dir: &Path, target: &Path) -> bool {
    let mut norm_base = PathBuf::new();
    for component in base_dir.components() {
        match component {
            std::path::Component::ParentDir => {
                norm_base.pop();
            }
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

/// Decision outcome for classifying a remote directory entry during download.
#[derive(Debug, PartialEq, Eq)]
pub enum EntryDecision {
    Download { sanitized: String },
    Skip { reason: String },
}

/// Pure classifier for remote directory entries during recursive download.
pub fn classify_remote_entry(
    raw_name: &str,
    is_symlink: bool,
    is_windows: bool,
    base: &Path,
    local_parent: &Path,
) -> EntryDecision {
    if is_symlink {
        return EntryDecision::Skip {
            reason: "symbolic link".to_string(),
        };
    }

    let sanitized = match sanitize_name_with_rules(raw_name, is_windows) {
        Ok(name) => name,
        Err(reason) => return EntryDecision::Skip { reason },
    };

    let local_entry_path = local_parent.join(&sanitized);
    if !is_path_confined(base, &local_entry_path) {
        return EntryDecision::Skip {
            reason: "path traversal outside destination".to_string(),
        };
    }

    EntryDecision::Download { sanitized }
}

#[cfg(windows)]
pub fn is_symlink_or_reparse(meta: &std::fs::Metadata) -> bool {
    // std::fs::Metadata::is_symlink() on Windows already treats symlinks and directory junctions
    // as symlinks, while avoiding false positives on OneDrive / cloud-sync placeholder reparse points.
    meta.is_symlink()
}

#[cfg(not(windows))]
pub fn is_symlink_or_reparse(meta: &std::fs::Metadata) -> bool {
    meta.is_symlink()
}

/// Before creating any directory or file under dest_canon, walk each existing path component
/// from dest_canon down with symlink_metadata; if any existing component is a symlink
/// (on Windows also a reparse point/junction via file_attributes & 0x400) refuse that entry.
pub fn check_components_for_symlinks(base_canon: &Path, target: &Path) -> Result<(), String> {
    let rel = match target.strip_prefix(base_canon) {
        Ok(r) => r,
        Err(_) => {
            return Err(format!(
                "Target path '{}' is outside base destination '{}'",
                target.display(),
                base_canon.display()
            ));
        }
    };

    let mut current = base_canon.to_path_buf();
    for comp in rel.components() {
        match comp {
            std::path::Component::Normal(c) => {
                current.push(c);
                if let Ok(meta) = std::fs::symlink_metadata(&current) {
                    if is_symlink_or_reparse(&meta) {
                        return Err(format!(
                            "Path component '{}' is a symlink or reparse point; download refused for security",
                            current.display()
                        ));
                    }
                }
            }
            std::path::Component::CurDir => {}
            _ => {
                return Err(format!(
                    "Invalid path component in relative path '{}'",
                    rel.display()
                ));
            }
        }
    }

    Ok(())
}

/// Formats a collision filename: "name (1).ext", "name (2).ext", etc.
pub fn format_collision_name(file_name: &str, counter: u32) -> String {
    let p = Path::new(file_name);
    if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
        let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or(file_name);
        format!("{} ({}).{}", stem, counter, ext)
    } else {
        format!("{} ({})", file_name, counter)
    }
}

/// Gets a collision candidate filename, erroring if attempt exceeds 9999.
#[allow(dead_code)]
pub fn get_collision_candidate(file_name: &str, counter: u32) -> Result<String, String> {
    if counter > 9999 {
        return Err(format!(
            "Too many file collisions for '{}': exceeded maximum limit of 9999",
            file_name
        ));
    }
    Ok(format_collision_name(file_name, counter))
}

/// Opens a downloaded file non-overwriting with an explicit cap on collision attempts.
pub async fn open_download_file_with_cap(
    parent_dir: &Path,
    sanitized_name: &str,
    max_cap: u32,
) -> Result<(tokio::fs::File, PathBuf, String), String> {
    let initial_path = parent_dir.join(sanitized_name);
    match tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&initial_path)
        .await
    {
        Ok(f) => return Ok((f, initial_path, sanitized_name.to_string())),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => {
            return Err(format!(
                "Failed to open download file '{}': {}",
                initial_path.display(),
                e
            ))
        }
    }

    for i in 1..=max_cap {
        let candidate_name = format_collision_name(sanitized_name, i);
        let candidate_path = parent_dir.join(&candidate_name);
        match tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate_path)
            .await
        {
            Ok(f) => return Ok((f, candidate_path, candidate_name)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                return Err(format!(
                    "Failed to open download file '{}': {}",
                    candidate_path.display(),
                    e
                ))
            }
        }
    }

    Err(format!(
        "Too many file collisions for '{}': exceeded maximum limit of {}",
        sanitized_name, max_cap
    ))
}

/// Opens a downloaded file non-overwriting up to 9999 collisions then errors.
pub async fn open_download_file_non_overwriting(
    parent_dir: &Path,
    sanitized_name: &str,
) -> Result<(tokio::fs::File, PathBuf, String), String> {
    open_download_file_with_cap(parent_dir, sanitized_name, 9999).await
}

/// Validate that target path is not an existing symlink or reparse point.
pub async fn validate_download_target(target: &Path) -> Result<(), String> {
    if let Ok(meta) = tokio::fs::symlink_metadata(target).await {
        if is_symlink_or_reparse(&meta) {
            return Err(format!(
                "Target path '{}' is a symlink; download refused for security",
                target.display()
            ));
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

    let dest_canon = dunce::canonicalize(dest_dir).map_err(|e| {
        format!(
            "Destination directory does not exist or is inaccessible: {}",
            e
        )
    })?;
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

/// Computes the SHA-256 digest of arbitrary bytes.
pub fn compute_sha256(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
}

struct InstanceTempState {
    dir: PathBuf,
    #[allow(dead_code)]
    lock_file: std::fs::File,
}

static INSTANCE_TEMP_STATE: std::sync::Mutex<Option<InstanceTempState>> =
    std::sync::Mutex::new(None);

#[allow(dead_code)]
#[derive(Clone)]
pub struct ActiveEditTracker {
    pub local_path: PathBuf,
    pub remote_path: String,
    pub file_name: String,
    pub last_uploaded_hash: Arc<std::sync::Mutex<[u8; 32]>>,
}

static ACTIVE_EDITS: std::sync::Mutex<Vec<ActiveEditTracker>> = std::sync::Mutex::new(Vec::new());

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecoveredEdit {
    pub local_path: String,
    pub file_name: String,
}

static RECOVERED_EDITS: std::sync::Mutex<Option<Vec<RecoveredEdit>>> = std::sync::Mutex::new(None);

pub fn check_and_emit_recovered_edits(app: &AppHandle) {
    let to_emit = {
        if let Ok(mut lock) = RECOVERED_EDITS.lock() {
            lock.take()
        } else {
            None
        }
    };
    if let Some(files) = to_emit {
        if !files.is_empty() {
            let _ = app.emit("edits-recovered", &files);
        }
    }
}

const KEEP_RETENTION_SECS: u64 = 14 * 24 * 3600;
const NO_LOCK_GRACE_SECS: u64 = 10;

/// Validates or creates an instance temporary directory under `base_temp` named
/// `airlock_edits_<pid>_<random hex>`, acquiring an exclusive advisory lock on `.lock`.
pub fn create_and_validate_instance_dir(
    base_temp: &Path,
    pid: u32,
    random_hex: &str,
) -> Result<(PathBuf, std::fs::File), std::io::Error> {
    let dir_name = format!("airlock_edits_{}_{}", pid, random_hex);
    let instance_dir = base_temp.join(&dir_name);

    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};

        if let Ok(meta) = std::fs::symlink_metadata(&instance_dir) {
            if meta.is_symlink() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    format!("Path '{}' is a symlink", instance_dir.display()),
                ));
            }
            if !meta.is_dir() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    format!("Path '{}' is not a directory", instance_dir.display()),
                ));
            }
            let mode = meta.permissions().mode() & 0o777;
            if mode != 0o700 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    format!(
                        "Pre-existing directory '{}' has mode {:o}, expected 0700",
                        instance_dir.display(),
                        mode
                    ),
                ));
            }
            let uid = meta.uid();
            let current_uid = unsafe { libc::getuid() };
            if current_uid != 0 && uid != current_uid {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    format!(
                        "Pre-existing directory '{}' not owned by current user",
                        instance_dir.display()
                    ),
                ));
            }
        } else {
            let mut builder = std::fs::DirBuilder::new();
            builder.mode(0o700);
            builder.create(&instance_dir)?;
            let _ = std::fs::set_permissions(&instance_dir, std::fs::Permissions::from_mode(0o700));
        }

        // Verify with symlink_metadata
        let meta = std::fs::symlink_metadata(&instance_dir)?;
        if meta.is_symlink() || !meta.is_dir() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!(
                    "Path '{}' is a symlink or not a directory",
                    instance_dir.display()
                ),
            ));
        }
        let mode = meta.permissions().mode() & 0o777;
        if mode != 0o700 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!(
                    "Directory '{}' has mode {:o}, expected 0700",
                    instance_dir.display(),
                    mode
                ),
            ));
        }
        let uid = meta.uid();
        let current_uid = unsafe { libc::getuid() };
        if current_uid != 0 && uid != current_uid {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!(
                    "Directory '{}' not owned by current user",
                    instance_dir.display()
                ),
            ));
        }
    }

    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(&instance_dir)?;
    }

    let lock_path = instance_dir.join(".lock");
    let lock_file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)?;

    lock_file.try_lock().map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::WouldBlock,
            format!(
                "Failed to lock instance lockfile '{}': {}",
                lock_path.display(),
                e
            ),
        )
    })?;

    Ok((instance_dir, lock_file))
}

/// Get or create the secure per-process temporary directory for local file editing.
pub fn get_airlock_temp_dir() -> Result<PathBuf, std::io::Error> {
    let mut guard = INSTANCE_TEMP_STATE
        .lock()
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "Lock poisoned"))?;
    if let Some(state) = guard.as_ref() {
        return Ok(state.dir.clone());
    }

    let pid = std::process::id();
    let random_hex: String = (0..8)
        .map(|_| format!("{:02x}", rand::random::<u8>()))
        .collect();
    let (dir, lock_file) =
        create_and_validate_instance_dir(&std::env::temp_dir(), pid, &random_hex)?;
    let result = dir.clone();
    *guard = Some(InstanceTempState { dir, lock_file });
    Ok(result)
}

/// Resolves a safe filename for editing, falling back to "file" plus a validated lowercase
/// alphanumeric extension if the sanitizer rejects the name.
pub fn resolve_edit_temp_file_name(raw_name: &str, is_windows: bool) -> String {
    match sanitize_name_with_rules(raw_name, is_windows) {
        Ok(sanitized) => sanitized,
        Err(_) => {
            if let Some((_stem, ext)) = raw_name.rsplit_once('.') {
                let clean_ext = ext.trim();
                if !clean_ext.is_empty() && clean_ext.chars().all(|c| c.is_ascii_alphanumeric()) {
                    format!("file.{}", clean_ext.to_ascii_lowercase())
                } else {
                    "file".to_string()
                }
            } else {
                "file".to_string()
            }
        }
    }
}

/// Constructs the per-file temporary path inside the instance directory:
/// `<instance dir>/<random hex>/<sanitized name>`.
pub fn create_edit_temp_file_path(
    instance_dir: &Path,
    raw_name: &str,
    is_windows: bool,
    random_hex: &str,
) -> Result<PathBuf, String> {
    let sanitized_name = resolve_edit_temp_file_name(raw_name, is_windows);
    let subdir = instance_dir.join(random_hex);
    let target = subdir.join(&sanitized_name);
    if !is_path_confined(instance_dir, &target) {
        return Err(format!(
            "Path traversal detected: '{}' escapes instance directory '{}'",
            target.display(),
            instance_dir.display()
        ));
    }
    Ok(target)
}

/// Inspects an instance directory for `.keep` markers inside per-file folders.
/// Returns (has_fresh_keep, recovered_files).
pub fn inspect_instance_dir_for_keep(instance_dir: &Path) -> (bool, Vec<RecoveredEdit>) {
    let mut has_fresh_keep = false;
    let mut recovered = Vec::new();

    let subdirs = match std::fs::read_dir(instance_dir) {
        Ok(s) => s,
        Err(_) => return (false, recovered),
    };

    for entry in subdirs.flatten() {
        let sub_path = entry.path();
        if !sub_path.is_dir() {
            continue;
        }
        if sub_path.file_name().and_then(|n| n.to_str()) == Some(".lock") {
            continue;
        }

        let keep_path = sub_path.join(".keep");
        if keep_path.exists() {
            let is_fresh = match std::fs::metadata(&keep_path) {
                Ok(meta) => match meta.modified() {
                    Ok(mtime) => match std::time::SystemTime::now().duration_since(mtime) {
                        Ok(age) => age < std::time::Duration::from_secs(KEEP_RETENTION_SECS),
                        Err(_) => true,
                    },
                    Err(_) => false,
                },
                Err(_) => false,
            };

            if is_fresh {
                has_fresh_keep = true;
                if let Ok(files) = std::fs::read_dir(&sub_path) {
                    for f_entry in files.flatten() {
                        let fp = f_entry.path();
                        if fp.is_file() {
                            if let Some(name) = fp.file_name().and_then(|n| n.to_str()) {
                                if name != ".keep" && name != ".lock" {
                                    recovered.push(RecoveredEdit {
                                        local_path: fp.to_string_lossy().to_string(),
                                        file_name: name.to_string(),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    (has_fresh_keep, recovered)
}

/// Sweeps stale `airlock_edits_*` directories, removing unlocked stale directories
/// while skipping active instances holding an exclusive lock, preserving directories
/// with fresh .keep markers (< 14 days), skipping brand-new directories (< 10 seconds)
/// without .lock, and removing legacy `airlock_edits`.
pub fn sweep_stale_temp_edits_in(
    temp_base: &Path,
    our_instance: Option<&Path>,
) -> Vec<RecoveredEdit> {
    let mut all_recovered = Vec::new();

    // 1. Delete the legacy un-namespaced airlock_edits folder once
    let legacy_dir = temp_base.join("airlock_edits");
    if legacy_dir.exists() {
        let _ = std::fs::remove_dir_all(&legacy_dir);
    }

    // 2. Scan for airlock_edits_* directories
    let entries = match std::fs::read_dir(temp_base) {
        Ok(e) => e,
        Err(_) => return all_recovered,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };

        if !file_name.starts_with("airlock_edits_") {
            continue;
        }

        if let Some(ours) = our_instance {
            if path == ours {
                continue;
            }
        }

        if !path.is_dir() {
            continue;
        }

        let lock_path = path.join(".lock");
        if lock_path.exists() {
            if let Ok(file) = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&lock_path)
            {
                match file.try_lock() {
                    Ok(()) => {
                        // Lock obtained! The owner process is gone.
                        // Drop the file lock handle first before deleting (Windows cannot delete an open file)
                        drop(file);

                        let (has_fresh_keep, recovered) = inspect_instance_dir_for_keep(&path);
                        if has_fresh_keep {
                            all_recovered.extend(recovered);
                        } else {
                            let _ = std::fs::remove_dir_all(&path);
                        }
                    }
                    Err(_) => {
                        // Locked: another instance is running, skip it
                    }
                }
            }
        } else {
            // No .lock file:
            // "The sweep skips (does not delete) an airlock_edits_* directory that has no .lock and is younger than 10 seconds."
            let is_young = match entry.metadata().or_else(|_| std::fs::metadata(&path)) {
                Ok(meta) => {
                    let now = std::time::SystemTime::now();
                    let mtime_age = meta
                        .modified()
                        .ok()
                        .and_then(|t| now.duration_since(t).ok());
                    let ctime_age = meta.created().ok().and_then(|t| now.duration_since(t).ok());
                    let age = match (mtime_age, ctime_age) {
                        (Some(m), Some(c)) => m.min(c),
                        (Some(m), None) => m,
                        (None, Some(c)) => c,
                        (None, None) => std::time::Duration::from_secs(0),
                    };
                    age < std::time::Duration::from_secs(NO_LOCK_GRACE_SECS)
                }
                Err(_) => false,
            };

            if is_young {
                continue;
            }

            let (has_fresh_keep, recovered) = inspect_instance_dir_for_keep(&path);
            if has_fresh_keep {
                all_recovered.extend(recovered);
            } else {
                let _ = std::fs::remove_dir_all(&path);
            }
        }
    }

    all_recovered
}

/// Startup sweep of stale temporary directories.
pub fn cleanup_temp_edits() {
    let our_instance = match get_airlock_temp_dir() {
        Ok(dir) => Some(dir),
        Err(e) => {
            log::warn!(
                "Failed to initialize instance temp dir during startup sweep: {}",
                e
            );
            None
        }
    };
    let recovered = sweep_stale_temp_edits_in(&std::env::temp_dir(), our_instance.as_deref());
    if let Ok(mut lock) = RECOVERED_EDITS.lock() {
        *lock = Some(recovered);
    }
}

/// Called on window close: deletes only our instance directory, keeping any files
/// with un-uploaded local changes.
pub fn cleanup_current_instance_on_close() {
    let mut unsaved_paths = std::collections::HashSet::new();
    if let Ok(edits) = ACTIVE_EDITS.lock() {
        for edit in edits.iter() {
            if edit.local_path.exists() {
                if let Ok(bytes) = std::fs::read(&edit.local_path) {
                    let cur_hash = compute_sha256(&bytes);
                    let last_hash = edit
                        .last_uploaded_hash
                        .lock()
                        .map(|h| *h)
                        .unwrap_or([0; 32]);
                    if cur_hash != last_hash {
                        unsaved_paths.insert(edit.local_path.clone());
                    }
                }
            }
        }
    }

    let mut guard = match INSTANCE_TEMP_STATE.lock() {
        Ok(g) => g,
        Err(_) => return,
    };

    if let Some(state) = guard.take() {
        let instance_dir = state.dir;
        let lock_file = state.lock_file;

        let mut has_kept_files = false;
        if let Ok(subdirs) = std::fs::read_dir(&instance_dir) {
            for subdir_entry in subdirs.flatten() {
                let sub_path = subdir_entry.path();
                if sub_path.file_name().and_then(|n| n.to_str()) == Some(".lock") {
                    continue;
                }
                if sub_path.is_dir() {
                    let mut sub_has_unsaved = false;
                    if let Ok(files) = std::fs::read_dir(&sub_path) {
                        for f_entry in files.flatten() {
                            let fp = f_entry.path();
                            if fp.file_name().and_then(|n| n.to_str()) == Some(".keep") {
                                continue;
                            }
                            if unsaved_paths.contains(&fp) {
                                sub_has_unsaved = true;
                                has_kept_files = true;
                            } else {
                                let _ = std::fs::remove_file(&fp);
                            }
                        }
                    }
                    if sub_has_unsaved {
                        let _ = std::fs::write(sub_path.join(".keep"), "");
                    } else {
                        let _ = std::fs::remove_dir_all(&sub_path);
                    }
                } else if !unsaved_paths.contains(&sub_path) {
                    let _ = std::fs::remove_file(&sub_path);
                } else {
                    has_kept_files = true;
                }
            }
        }

        drop(lock_file);

        if !has_kept_files {
            let _ = std::fs::remove_file(instance_dir.join(".lock"));
            let _ = std::fs::remove_dir(&instance_dir);
        }
    }
}

#[tauri::command]
pub async fn sftp_list_dir(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    path: String,
) -> Result<Vec<FileEntry>, String> {
    check_and_emit_recovered_edits(&app);
    let handle = {
        let connections = state.connections.lock().await;
        let conn = connections.get(&id).ok_or("Session not found")?;
        conn.handle.clone()
    };

    let channel = {
        let h = handle.lock().await;
        h.channel_open_session().await.map_err(|e| e.to_string())?
    };
    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(|e| e.to_string())?;
    let sftp = SftpSession::new(channel.into_stream())
        .await
        .map_err(|e| e.to_string())?;

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

    entries.sort_by(|a, b| match b.is_dir.cmp(&a.is_dir) {
        std::cmp::Ordering::Equal => a.name.cmp(&b.name),
        other => other,
    });

    Ok(entries)
}

#[tauri::command]
pub async fn local_list_dir(app: AppHandle, path: String) -> Result<Vec<FileEntry>, String> {
    check_and_emit_recovered_edits(&app);
    let mut entries = Vec::new();
    let mut dir = tokio::fs::read_dir(&path)
        .await
        .map_err(|e| e.to_string())?;

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
            mtime: metadata
                .modified()
                .map(|m| {
                    m.duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs()
                })
                .unwrap_or(0),
        });
    }

    entries.sort_by(|a, b| match b.is_dir.cmp(&a.is_dir) {
        std::cmp::Ordering::Equal => a.name.cmp(&b.name),
        other => other,
    });

    Ok(entries)
}

#[tauri::command]
pub fn get_local_home_dir(app: AppHandle) -> Result<String, String> {
    check_and_emit_recovered_edits(&app);
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
    depth: usize,
    is_top_level: bool,
    stats: &mut TransferStats,
) -> Result<(), String> {
    if !check_transfer_depth(depth, &local_path.to_string_lossy(), stats) {
        return Ok(());
    }

    let target_kind = resolve_upload_target(local_path, is_top_level).await?;
    match target_kind {
        UploadTargetKind::SkippedSymlink(reason) => {
            log::warn!("Skipping local symlink: {}", local_path.display());
            stats.skipped.push(SkippedItem {
                path: local_path.to_string_lossy().to_string(),
                reason,
            });
            Ok(())
        }
        UploadTargetKind::Directory => {
            let remote_path_str = remote_path.to_string_lossy().replace("\\", "/");
            let _ = sftp.create_dir(&remote_path_str).await;

            let mut entries = tokio::fs::read_dir(local_path)
                .await
                .map_err(|e| e.to_string())?;
            while let Some(entry) = entries.next_entry().await.map_err(|e| e.to_string())? {
                if cancel_token.load(Ordering::Relaxed) {
                    return Err("Transfer cancelled by user".into());
                }
                let entry_path = entry.path();
                let entry_name = entry.file_name();
                let remote_entry_path = remote_path.join(&entry_name);
                Box::pin(upload_recursive(
                    sftp,
                    &entry_path,
                    &remote_entry_path,
                    cancel_token.clone(),
                    app,
                    transfer_id,
                    depth + 1,
                    false,
                    stats,
                ))
                .await?;
            }
            Ok(())
        }
        UploadTargetKind::File(total_size) | UploadTargetKind::FollowedFileSymlink(total_size) => {
            let mut local_file = tokio::fs::File::open(local_path)
                .await
                .map_err(|e| e.to_string())?;
            let remote_path_str = remote_path.to_string_lossy().replace("\\", "/");
            let mut remote_file = sftp
                .create(&remote_path_str)
                .await
                .map_err(|e| e.to_string())?;

            let mut transferred_size = 0u64;
            let file_name = local_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();

            let _ = app.emit(
                "transfer-progress",
                TransferProgress {
                    transfer_id: transfer_id.to_string(),
                    transferred: transferred_size,
                    total: total_size,
                    file_name: file_name.clone(),
                },
            );

            let mut buf = vec![0; 65536];
            let mut chunks = 0;
            loop {
                if cancel_token.load(Ordering::Relaxed) {
                    return Err("Transfer cancelled by user".into());
                }
                let n = local_file.read(&mut buf).await.map_err(|e| e.to_string())?;
                if n == 0 {
                    break;
                }
                remote_file
                    .write_all(&buf[..n])
                    .await
                    .map_err(|e| e.to_string())?;

                transferred_size += n as u64;
                chunks += 1;
                if chunks % 8 == 0 || transferred_size == total_size {
                    let _ = app.emit(
                        "transfer-progress",
                        TransferProgress {
                            transfer_id: transfer_id.to_string(),
                            transferred: transferred_size,
                            total: total_size,
                            file_name: file_name.clone(),
                        },
                    );
                }
            }

            if total_size == 0 {
                let _ = app.emit(
                    "transfer-progress",
                    TransferProgress {
                        transfer_id: transfer_id.to_string(),
                        transferred: 0,
                        total: 0,
                        file_name: file_name.clone(),
                    },
                );
            }

            stats.files_transferred += 1;
            stats.total_bytes += total_size;
            Ok(())
        }
    }
}

async fn download_recursive(
    sftp: &SftpSession,
    remote_path: &Path,
    local_path: &Path,
    base_destination: &Path,
    cancel_token: Arc<AtomicBool>,
    app: &AppHandle,
    transfer_id: &str,
    depth: usize,
    is_top_level: bool,
    stats: &mut TransferStats,
) -> Result<(), String> {
    let remote_path_str = remote_path.to_string_lossy().replace("\\", "/");

    if !check_transfer_depth(depth, &remote_path_str, stats) {
        return Ok(());
    }

    // SEC-05: Ensure current target path remains inside base destination directory
    if !is_path_confined(base_destination, local_path) {
        return Err(format!(
            "Path traversal attempt blocked: '{}' escapes destination '{}'",
            local_path.display(),
            base_destination.display()
        ));
    }

    // Component symlink check before creating directory or file
    check_components_for_symlinks(base_destination, local_path)?;

    // Check top-level remote symlink
    if is_top_level {
        if let Ok(sym_meta) = sftp.symlink_metadata(&remote_path_str).await {
            if sym_meta.is_symlink() {
                match sftp.metadata(&remote_path_str).await {
                    Ok(target_meta) => {
                        if target_meta.is_dir() || target_meta.is_symlink() {
                            stats.skipped.push(SkippedItem {
                                path: remote_path_str,
                                reason: "symbolic link".to_string(),
                            });
                            return Ok(());
                        }
                    }
                    Err(_) => {
                        stats.skipped.push(SkippedItem {
                            path: remote_path_str,
                            reason: "symbolic link".to_string(),
                        });
                        return Ok(());
                    }
                }
            }
        }
    }

    match sftp.read_dir(&remote_path_str).await {
        Ok(dir) => {
            tokio::fs::create_dir_all(local_path)
                .await
                .map_err(|e| e.to_string())?;
            for entry in dir {
                if cancel_token.load(Ordering::Relaxed) {
                    return Err("Transfer cancelled by user".into());
                }
                let raw_name = entry.file_name();
                if raw_name == "." || raw_name == ".." {
                    continue;
                }

                let remote_entry_path = remote_path.join(&raw_name);
                let attrs = entry.metadata();

                match classify_remote_entry(
                    &raw_name,
                    attrs.is_symlink(),
                    cfg!(windows),
                    base_destination,
                    local_path,
                ) {
                    EntryDecision::Skip { reason } => {
                        log::warn!("Skipping remote entry '{}': {}", raw_name, reason);
                        stats.skipped.push(SkippedItem {
                            path: remote_entry_path.to_string_lossy().replace("\\", "/"),
                            reason,
                        });
                        continue;
                    }
                    EntryDecision::Download { sanitized } => {
                        if sanitized != raw_name {
                            stats.renamed.push((raw_name.clone(), sanitized.clone()));
                        }
                        let local_entry_path = local_path.join(&sanitized);
                        Box::pin(download_recursive(
                            sftp,
                            &remote_entry_path,
                            &local_entry_path,
                            base_destination,
                            cancel_token.clone(),
                            app,
                            transfer_id,
                            depth + 1,
                            false,
                            stats,
                        ))
                        .await?;
                    }
                }
            }
        }
        Err(_) => {
            // Ensure destination directory exists
            if let Some(parent) = local_path.parent() {
                check_components_for_symlinks(base_destination, parent)?;
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|e| e.to_string())?;
            }

            let file_name = local_path
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| "Invalid local filename".to_string())?;
            let parent_dir = local_path.parent().unwrap_or(base_destination);

            let mut remote_file = sftp
                .open(&remote_path_str)
                .await
                .map_err(|e| e.to_string())?;
            let (mut local_file, _actual_path, actual_name) =
                open_download_file_non_overwriting(parent_dir, file_name).await?;

            if actual_name != file_name {
                stats
                    .renamed
                    .push((file_name.to_string(), actual_name.clone()));
            }

            let total_size = match sftp.metadata(&remote_path_str).await {
                Ok(meta) => meta.size.unwrap_or(0),
                Err(_) => 0,
            };

            let mut transferred_size = 0u64;

            let _ = app.emit(
                "transfer-progress",
                TransferProgress {
                    transfer_id: transfer_id.to_string(),
                    transferred: transferred_size,
                    total: total_size,
                    file_name: actual_name.clone(),
                },
            );

            let mut buf = vec![0; 65536];
            let mut chunks = 0;
            loop {
                if cancel_token.load(Ordering::Relaxed) {
                    return Err("Transfer cancelled by user".into());
                }
                let n = remote_file
                    .read(&mut buf)
                    .await
                    .map_err(|e| e.to_string())?;
                if n == 0 {
                    break;
                }
                local_file
                    .write_all(&buf[..n])
                    .await
                    .map_err(|e| e.to_string())?;

                transferred_size += n as u64;
                chunks += 1;
                if chunks % 8 == 0 || transferred_size == total_size {
                    let _ = app.emit(
                        "transfer-progress",
                        TransferProgress {
                            transfer_id: transfer_id.to_string(),
                            transferred: transferred_size,
                            total: total_size,
                            file_name: actual_name.clone(),
                        },
                    );
                }
            }

            if total_size == 0 {
                let _ = app.emit(
                    "transfer-progress",
                    TransferProgress {
                        transfer_id: transfer_id.to_string(),
                        transferred: 0,
                        total: 0,
                        file_name: actual_name.clone(),
                    },
                );
            }

            stats.files_transferred += 1;
            stats.total_bytes += total_size;
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
) -> Result<TransferSummary, String> {
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

    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(|e| e.to_string())?;
    let sftp = SftpSession::new(channel.into_stream())
        .await
        .map_err(|e| e.to_string())?;

    let mut stats = TransferStats::default();
    let res = upload_recursive(
        &sftp,
        Path::new(&local_path),
        Path::new(&remote_path),
        cancel_token,
        &app,
        &transfer_id,
        0,
        true,
        &mut stats,
    )
    .await;

    {
        let mut transfers = state.active_transfers.lock().await;
        transfers.remove(&transfer_id);
    }
    res.map(|_| TransferSummary {
        transfer_id,
        total_bytes: stats.total_bytes,
        files_transferred: stats.files_transferred,
        destination_path: remote_path,
        skipped: stats.skipped,
        renamed: stats.renamed,
    })
}

#[tauri::command]
pub async fn sftp_download(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    transfer_id: String,
    remote_path: String,
    destination_dir: String,
) -> Result<TransferSummary, String> {
    let (dest_canon, sanitized_name, target) =
        resolve_download_target(&remote_path, &destination_dir)?;
    check_components_for_symlinks(&dest_canon, &target)?;
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

    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(|e| e.to_string())?;
    let sftp = SftpSession::new(channel.into_stream())
        .await
        .map_err(|e| e.to_string())?;

    let raw_name = remote_path
        .rsplit('/')
        .find(|s| !s.is_empty())
        .unwrap_or("");
    let mut stats = TransferStats::default();
    if !raw_name.is_empty() && raw_name != sanitized_name {
        stats
            .renamed
            .push((raw_name.to_string(), sanitized_name.clone()));
    }
    let res = download_recursive(
        &sftp,
        Path::new(&remote_path),
        &target,
        &dest_canon,
        cancel_token,
        &app,
        &transfer_id,
        0,
        true,
        &mut stats,
    )
    .await;

    {
        let mut transfers = state.active_transfers.lock().await;
        transfers.remove(&transfer_id);
    }
    res.map(|_| TransferSummary {
        transfer_id,
        total_bytes: stats.total_bytes,
        files_transferred: stats.files_transferred,
        destination_path: dest_canon.to_string_lossy().to_string(),
        skipped: stats.skipped,
        renamed: stats.renamed,
    })
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

/// Re-upload contents back to the remote server via a fresh SFTP channel.
async fn upload_bytes_to_remote(
    handle: &Arc<Mutex<russh::client::Handle<crate::ssh_session::Client>>>,
    contents: &[u8],
    remote_path: &str,
) -> Result<(), String> {
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
        .write_all(contents)
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
        h.channel_open_session().await.map_err(|e| e.to_string())?
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
            return Err(
                "File exceeded maximum allowed edit size of 20 MB during transfer".to_string(),
            );
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
    let airlock_temp_dir = get_airlock_temp_dir()
        .map_err(|e| format!("Failed to create temp edit directory: {}", e))?;

    let random_hex_dir: String = (0..8)
        .map(|_| format!("{:02x}", rand::random::<u8>()))
        .collect();
    let temp_path = create_edit_temp_file_path(
        &airlock_temp_dir,
        &file_name,
        cfg!(windows),
        &random_hex_dir,
    )?;

    if let Some(parent) = temp_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create edit temp directory: {}", e))?;
    }

    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp_path)
        .map_err(|e| {
            format!(
                "Failed to create temp file '{}': {}",
                temp_path.display(),
                e
            )
        })?;
    file.write_all(&contents)
        .map_err(|e| format!("Failed to write temp file: {}", e))?;
    file.flush()
        .map_err(|e| format!("Failed to flush temp file: {}", e))?;
    drop(file);

    let initial_hash = compute_sha256(&contents);
    let last_uploaded_hash = Arc::new(std::sync::Mutex::new(initial_hash));

    // Register active edit
    if let Ok(mut edits) = ACTIVE_EDITS.lock() {
        edits.push(ActiveEditTracker {
            local_path: temp_path.clone(),
            remote_path: remote_path.clone(),
            file_name: file_name.clone(),
            last_uploaded_hash: last_uploaded_hash.clone(),
        });
    }

    // 5. SEC-04: Safe Editor Execution
    let validated_editor = resolve_configured_editor(&*state.editor_store, validate_editor_path)?;
    let has_editor = validated_editor.is_some();

    let current_os = if cfg!(target_os = "windows") {
        TargetOs::Windows
    } else if cfg!(target_os = "macos") {
        TargetOs::Macos
    } else if cfg!(target_os = "linux") {
        TargetOs::Linux
    } else {
        TargetOs::Other
    };

    let method = choose_open_method(current_os, &file_name, has_editor)?;

    match method {
        OpenMethod::ConfiguredEditor => {
            let editor_path =
                validated_editor.ok_or_else(|| "No external editor configured".to_string())?;
            let mut cmd = build_configured_editor_command(Path::new(&editor_path), &temp_path);
            let mut child = cmd.spawn().map_err(|e| {
                format!(
                    "Failed to launch configured editor '{}': {}",
                    editor_path, e
                )
            })?;
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        OpenMethod::SystemDefault => {
            open::that(&temp_path).map_err(|e| format!("Failed to open default editor: {}", e))?;
        }
        OpenMethod::WindowsNotepad => {
            log::info!(
                "Non-allowlisted file '{}' opened safely with explicit notepad.exe",
                file_name
            );
            let system_root =
                std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
            let mut notepad_path = PathBuf::from(&system_root)
                .join("System32")
                .join("notepad.exe");
            if !notepad_path.exists() {
                notepad_path = PathBuf::from("C:\\Windows\\System32\\notepad.exe");
                if !notepad_path.exists() {
                    notepad_path = PathBuf::from("C:\\Windows\\notepad.exe");
                }
            }
            let mut cmd = std::process::Command::new(&notepad_path);
            cmd.arg(&temp_path);
            let mut child = cmd.spawn().map_err(|e| {
                format!(
                    "Failed to open in Notepad ('{}'): {}",
                    notepad_path.display(),
                    e
                )
            })?;
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        OpenMethod::MacosOpenText => {
            let mut cmd = std::process::Command::new("open");
            cmd.arg("-t").arg(&temp_path);
            let mut child = cmd
                .spawn()
                .map_err(|e| format!("Failed to open default editor via open -t: {}", e))?;
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        OpenMethod::LinuxXdgOpen => {
            let mut cmd = std::process::Command::new("xdg-open");
            cmd.arg(&temp_path);
            let mut child = cmd
                .spawn()
                .map_err(|e| format!("Failed to open file via xdg-open: {}", e))?;
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
    }

    // 6. Emit "opened" event
    let _ = app.emit(
        "edit-file-opened",
        serde_json::json!({
            "remote_path": remote_path,
            "file_name": file_name,
        }),
    );

    // 7. Spawn the file watcher with session liveness checks and guaranteed cleanup
    let handle_clone = handle.clone();
    let remote_path_clone = remote_path.clone();
    let file_name_clone = file_name.clone();
    let temp_path_clone = temp_path.clone();
    let app_clone = app.clone();
    let id_clone = id.clone();
    let last_uploaded_hash_clone = last_uploaded_hash.clone();

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

        let parent_dir = temp_path_clone.parent().unwrap_or(&temp_path_clone);
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
                log::info!(
                    "Temp file removed, stopping watcher: {}",
                    temp_path_clone.display()
                );
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
                    let dominated =
                        matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_));
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

                        // Upload the bytes you hashed: read the file once, upload those bytes, store the hash of those same bytes.
                        let bytes = match std::fs::read(&temp_path_clone) {
                            Ok(b) => b,
                            Err(e) => {
                                log::error!("Failed to read temp file for upload: {}", e);
                                continue;
                            }
                        };
                        let file_hash = compute_sha256(&bytes);

                        let handle_ref = handle_clone.clone();
                        let rp = remote_path_clone.clone();
                        let app_ref = app_clone.clone();
                        let fn_clone = file_name_clone.clone();

                        let result = rt.block_on(async {
                            upload_bytes_to_remote(&handle_ref, &bytes, &rp).await
                        });

                        match result {
                            Ok(()) => {
                                last_upload = Some(Instant::now());
                                if let Ok(mut luh) = last_uploaded_hash_clone.lock() {
                                    *luh = file_hash;
                                }
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

        // Drop watcher and clean up temp file if not unsaved
        drop(watcher);

        let mut is_unsaved = false;
        if temp_path_clone.exists() {
            if let Ok(bytes) = std::fs::read(&temp_path_clone) {
                let cur_hash = compute_sha256(&bytes);
                let last_hash = last_uploaded_hash_clone
                    .lock()
                    .map(|h| *h)
                    .unwrap_or([0; 32]);
                if cur_hash != last_hash {
                    is_unsaved = true;
                }
            }
        }

        if is_unsaved {
            log::warn!(
                "Watcher ended with unsaved changes. Preserving temp file: {}",
                temp_path_clone.display()
            );
            if let Some(parent) = temp_path_clone.parent() {
                let _ = std::fs::write(parent.join(".keep"), "");
            }
            let _ = app_clone.emit(
                "edit-unsaved",
                serde_json::json!({
                    "local_path": temp_path_clone.to_string_lossy(),
                    "remote_path": remote_path_clone,
                    "file_name": file_name_clone,
                }),
            );
        } else {
            let _ = std::fs::remove_file(&temp_path_clone);
            if let Some(parent) = temp_path_clone.parent() {
                let _ = std::fs::remove_dir(parent);
            }
            log::info!("Cleaned up temp edit file: {}", temp_path_clone.display());
            if let Ok(mut edits) = ACTIVE_EDITS.lock() {
                edits.retain(|e| e.local_path != temp_path_clone);
            }
        }
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
        // Trailing dots and spaces rejected (not stripped) on Windows
        assert!(sanitize_name_with_rules("test.txt...", true).is_err());
        assert!(sanitize_name_with_rules("test.txt   ", true).is_err());
        assert!(sanitize_name_with_rules("test.txt. . ", true).is_err());
    }

    #[test]
    fn test_windows_rejection_all_chars_and_trailing_dot_space() {
        // Rejects any of < > : " | ? * / \ plus control characters
        for ch in ['<', '>', ':', '"', '|', '?', '*', '/', '\\'] {
            let name = format!("bad{}file.txt", ch);
            assert!(
                sanitize_name_with_rules(&name, true).is_err(),
                "Character {:?} must be rejected on Windows",
                ch
            );
        }
        for ctrl in ['\0', '\x01', '\x08', '\x0A', '\x1F'] {
            let name = format!("bad{}file.txt", ctrl);
            assert!(
                sanitize_name_with_rules(&name, true).is_err(),
                "Control character {:?} must be rejected on Windows",
                ctrl
            );
        }
        // Rejects names ending in '.' or ' '
        for name in [
            "test.",
            "test ",
            "file.txt.",
            "file.txt ",
            "a...",
            "a   ",
            "b. . ",
            "data.csv.",
            "data.csv ",
        ] {
            assert!(
                sanitize_name_with_rules(name, true).is_err(),
                "Trailing dot or space in {:?} must be rejected on Windows",
                name
            );
        }
    }

    #[tokio::test]
    async fn test_download_file_collision_produces_name_1() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let file_path = base.join("report.txt");
        tokio::fs::write(&file_path, b"original content")
            .await
            .unwrap();

        let (mut file, actual_path, actual_name) =
            open_download_file_non_overwriting(base, "report.txt")
                .await
                .unwrap();
        file.write_all(b"new content").await.unwrap();
        file.flush().await.unwrap();

        assert_eq!(actual_name, "report (1).txt");
        assert_eq!(actual_path, base.join("report (1).txt"));
        // Original file must be untouched
        let orig = tokio::fs::read(&file_path).await.unwrap();
        assert_eq!(orig, b"original content");
        let new_content = tokio::fs::read(&actual_path).await.unwrap();
        assert_eq!(new_content, b"new content");
    }

    #[tokio::test]
    async fn test_download_collision_9999_cap_errors() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();

        tokio::fs::write(base.join("cap.txt"), b"0").await.unwrap();
        tokio::fs::write(base.join("cap (1).txt"), b"1")
            .await
            .unwrap();
        tokio::fs::write(base.join("cap (2).txt"), b"2")
            .await
            .unwrap();

        let res = open_download_file_with_cap(base, "cap.txt", 2).await;
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("exceeded maximum limit of 2"));

        // Test 9999 cap boundary
        assert!(get_collision_candidate("cap.txt", 9999).is_ok());
        let cap_err = get_collision_candidate("cap.txt", 10000);
        assert!(cap_err.is_err());
        assert!(cap_err.unwrap_err().contains("9999"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_component_symlink_refused_outside_dest() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("dest");
        let outside = dir.path().join("outside");
        tokio::fs::create_dir_all(&dest.join("photos"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(&outside).await.unwrap();

        // dest/photos is a real directory containing a symlink "sub" that points outside
        std::os::unix::fs::symlink(&outside, dest.join("photos").join("sub")).unwrap();

        let dest_canon = dunce::canonicalize(&dest).unwrap();
        let target = dest_canon.join("photos").join("sub").join("evil.txt");

        // Component symlink check must refuse
        let check = check_components_for_symlinks(&dest_canon, &target);
        assert!(check.is_err(), "Symlink component 'sub' must be refused");
        assert!(check.unwrap_err().contains("symlink or reparse point"));

        // Verify evil.txt must NOT be written outside dest
        assert!(!outside.join("evil.txt").exists());
    }

    #[test]
    fn test_summary_lists_skipped_symlink_and_unsafe_name() {
        let mut stats = TransferStats::default();

        // 1. Record a skipped symlink
        stats.skipped.push(SkippedItem {
            path: "/remote/path/link_to_dir".to_string(),
            reason: "symbolic link".to_string(),
        });

        // 2. Record an unsafe remote name
        let unsafe_name = "invalid:filename<test>.txt";
        let sanitizer_result = sanitize_name_with_rules(unsafe_name, true);
        assert!(sanitizer_result.is_err());
        let reason = sanitizer_result.unwrap_err();
        stats.skipped.push(SkippedItem {
            path: format!("/remote/path/{}", unsafe_name),
            reason: reason.clone(),
        });

        let summary = TransferSummary {
            transfer_id: "test-transfer-123".to_string(),
            total_bytes: 1024,
            files_transferred: 3,
            destination_path: "/local/dest".to_string(),
            skipped: stats.skipped,
            renamed: stats.renamed,
        };

        assert_eq!(summary.files_transferred, 3);
        assert_eq!(summary.skipped.len(), 2);
        assert_eq!(summary.skipped[0].path, "/remote/path/link_to_dir");
        assert_eq!(summary.skipped[0].reason, "symbolic link");
        assert_eq!(
            summary.skipped[1].path,
            format!("/remote/path/{}", unsafe_name)
        );
        assert_eq!(summary.skipped[1].reason, reason);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_top_level_file_symlink_uploads_target() {
        let dir = tempfile::tempdir().unwrap();
        let target_file = dir.path().join("real_target.txt");
        tokio::fs::write(&target_file, b"sample content for symlink test")
            .await
            .unwrap();

        let link_file = dir.path().join("link_file.txt");
        std::os::unix::fs::symlink(&target_file, &link_file).unwrap();

        // Top-level file symlink picked directly must follow metadata and return target length
        let res = resolve_upload_target(&link_file, true).await.unwrap();
        assert_eq!(res, UploadTargetKind::FollowedFileSymlink(31));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_nested_symlink_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let target_file = dir.path().join("target.txt");
        tokio::fs::write(&target_file, b"content").await.unwrap();

        let link_file = dir.path().join("nested_link.txt");
        std::os::unix::fs::symlink(&target_file, &link_file).unwrap();

        // Nested file symlink (is_top_level = false) must be skipped
        let res = resolve_upload_target(&link_file, false).await.unwrap();
        assert_eq!(
            res,
            UploadTargetKind::SkippedSymlink("symbolic link".to_string())
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_directory_symlink_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let target_dir = dir.path().join("real_directory");
        tokio::fs::create_dir(&target_dir).await.unwrap();

        let link_dir = dir.path().join("link_directory");
        std::os::unix::fs::symlink(&target_dir, &link_dir).unwrap();

        // Top-level directory symlink must be skipped with "symbolic link"
        let top_res = resolve_upload_target(&link_dir, true).await.unwrap();
        assert_eq!(
            top_res,
            UploadTargetKind::SkippedSymlink("symbolic link".to_string())
        );

        // Nested directory symlink must also be skipped with "symbolic link"
        let nested_res = resolve_upload_target(&link_dir, false).await.unwrap();
        assert_eq!(
            nested_res,
            UploadTargetKind::SkippedSymlink("symbolic link".to_string())
        );
    }

    #[tokio::test]
    async fn test_tree_65_levels_deep_skipped_at_limit() {
        let mut stats = TransferStats::default();

        // Levels 0 to 64 should succeed
        for d in 0..=64 {
            let path = format!("/dir/level_{}", d);
            assert!(
                check_transfer_depth(d, &path, &mut stats),
                "Depth {} should not be skipped",
                d
            );
        }
        assert!(stats.skipped.is_empty());

        // Level 65 exceeds the 64-level limit and must be skipped
        let deep_path = "/dir/level_65";
        let allowed = check_transfer_depth(65, deep_path, &mut stats);
        assert!(!allowed, "Depth 65 must be blocked/skipped");
        assert_eq!(stats.skipped.len(), 1);
        assert_eq!(stats.skipped[0].path, deep_path);
        assert_eq!(stats.skipped[0].reason, "directory depth exceeds 64 levels");
    }

    #[tokio::test]
    async fn test_renamed_pairs_are_recorded() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let file_name = "document.pdf";
        let initial_file = base.join(file_name);
        tokio::fs::write(&initial_file, b"version 0").await.unwrap();

        let mut stats = TransferStats::default();

        // Download collision 1 -> document (1).pdf
        let (_f1, _p1, actual_name1) = open_download_file_non_overwriting(base, file_name)
            .await
            .unwrap();
        if actual_name1 != file_name {
            stats
                .renamed
                .push((file_name.to_string(), actual_name1.clone()));
        }
        assert_eq!(actual_name1, "document (1).pdf");

        // Download collision 2 -> document (2).pdf
        let (_f2, _p2, actual_name2) = open_download_file_non_overwriting(base, file_name)
            .await
            .unwrap();
        if actual_name2 != file_name {
            stats
                .renamed
                .push((file_name.to_string(), actual_name2.clone()));
        }
        assert_eq!(actual_name2, "document (2).pdf");

        assert_eq!(stats.renamed.len(), 2);
        assert_eq!(
            stats.renamed,
            vec![
                ("document.pdf".to_string(), "document (1).pdf".to_string()),
                ("document.pdf".to_string(), "document (2).pdf".to_string())
            ]
        );
    }

    #[test]
    fn test_classify_remote_entry_decision_matrix() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let sub = base.join("subfolder");
        let _ = std::fs::create_dir_all(&sub);

        // 1. Normal name
        assert_eq!(
            classify_remote_entry("report.txt", false, true, base, &sub),
            EntryDecision::Download {
                sanitized: "report.txt".to_string()
            }
        );

        // 2. Symlink
        assert_eq!(
            classify_remote_entry("symlink_entry", true, false, base, &sub),
            EntryDecision::Skip {
                reason: "symbolic link".to_string()
            }
        );

        // 3. Unsafe name (forbidden characters on Windows)
        let unsafe_decision = classify_remote_entry("bad:name<test>.txt", false, true, base, &sub);
        assert!(matches!(unsafe_decision, EntryDecision::Skip { .. }));
        if let EntryDecision::Skip { reason } = unsafe_decision {
            assert!(reason.contains("invalid character"));
        }

        // 4. Reserved name on Windows (e.g. CON, NUL, AUX)
        let reserved_decision = classify_remote_entry("con.txt", false, true, base, &sub);
        assert!(matches!(reserved_decision, EntryDecision::Skip { .. }));
        if let EntryDecision::Skip { reason } = reserved_decision {
            assert!(reason.contains("Reserved Windows device name"));
        }

        // 5. Trailing dot on Windows rules
        let trailing_dot_decision = classify_remote_entry("file.txt.", false, true, base, &sub);
        assert!(matches!(trailing_dot_decision, EntryDecision::Skip { .. }));
        if let EntryDecision::Skip { reason } = trailing_dot_decision {
            assert!(reason.contains("cannot end in a dot or space"));
        }

        // 6. Traversal escaping base destination
        let outside = dir.path().parent().unwrap();
        let traversal_decision = classify_remote_entry("normal.txt", false, false, base, outside);
        assert_eq!(
            traversal_decision,
            EntryDecision::Skip {
                reason: "path traversal outside destination".to_string()
            }
        );
    }

    #[test]
    fn test_whitespace_trimmed_name_recorded_as_renamed_or_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let mut stats = TransferStats::default();

        // Leading whitespace: on Windows and Unix, trimmed to "spaced.txt"
        let raw_name = "  spaced.txt";
        let decision = classify_remote_entry(raw_name, false, true, base, base);
        match decision {
            EntryDecision::Download { sanitized } => {
                if sanitized != raw_name {
                    stats.renamed.push((raw_name.to_string(), sanitized));
                }
            }
            EntryDecision::Skip { .. } => panic!("Expected download with sanitized name"),
        }
        assert_eq!(stats.renamed.len(), 1);
        assert_eq!(
            stats.renamed[0],
            ("  spaced.txt".to_string(), "spaced.txt".to_string())
        );

        // Trailing space on Windows rules: rejected and recorded in stats.skipped
        let raw_trailing = "spaced.txt ";
        let trailing_decision = classify_remote_entry(raw_trailing, false, true, base, base);
        match trailing_decision {
            EntryDecision::Skip { reason } => {
                stats.skipped.push(SkippedItem {
                    path: format!("/remote/{}", raw_trailing),
                    reason,
                });
            }
            EntryDecision::Download { .. } => panic!("Expected skip on Windows trailing space"),
        }
        assert_eq!(stats.skipped.len(), 1);
        assert!(stats.skipped[0]
            .reason
            .contains("cannot end in a dot or space"));
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
        assert_eq!(
            sanitize_name_with_rules("con.txt", false).unwrap(),
            "con.txt"
        );
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
            assert!(!is_path_confined(
                win_base,
                Path::new(r"..\..\Windows\System32\cmd.exe")
            ));
            assert!(!is_path_confined(
                win_base,
                Path::new(r"C:\Windows\System32\cmd.exe")
            ));
        }

        #[cfg(unix)]
        {
            let unix_base = Path::new("/home/user/downloads");
            assert!(is_path_confined(unix_base, Path::new("safe.txt")));
            assert!(is_path_confined(
                unix_base,
                Path::new("nested/dir/safe.txt")
            ));
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
            "js", "py", "sh", "bash", "html", "htm", "csv", "tsv", "mjs", "cjs", "JS", "Py", "xml",
            "bat", "cmd", "exe", "vbs", "ps1", "hta", "lnk", "",
        ] {
            assert!(
                !is_safe_text_extension(ext),
                "Extension {:?} should be rejected",
                ext
            );
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
        assert!(!is_safe_file_for_open("run.bat.")); // trailing dot rejected
        assert!(!is_safe_file_for_open("run.js ")); // trailing space rejected
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
        for os in &[
            TargetOs::Windows,
            TargetOs::Macos,
            TargetOs::Linux,
            TargetOs::Other,
        ] {
            for file in &[
                "notes.txt",
                "x.js",
                "page.html",
                "run.bat.",
                "notes.txt ",
                "Dockerfile",
            ] {
                assert_eq!(
                    choose_open_method(*os, file, true),
                    Ok(OpenMethod::ConfiguredEditor),
                    "OS {:?} with file {:?} and has_editor=true must return ConfiguredEditor",
                    os,
                    file
                );
            }
        }

        // 2. Without editor (has_editor = false)
        // Windows:
        // Allowlisted files -> SystemDefault
        assert_eq!(
            choose_open_method(TargetOs::Windows, "notes.txt", false),
            Ok(OpenMethod::SystemDefault)
        );
        assert_eq!(
            choose_open_method(TargetOs::Windows, "a b.txt", false),
            Ok(OpenMethod::SystemDefault)
        );
        assert_eq!(
            choose_open_method(TargetOs::Windows, "Dockerfile", false),
            Ok(OpenMethod::SystemDefault)
        );
        // Non-allowlisted files -> WindowsNotepad
        assert_eq!(
            choose_open_method(TargetOs::Windows, "x.js", false),
            Ok(OpenMethod::WindowsNotepad)
        );
        assert_eq!(
            choose_open_method(TargetOs::Windows, "x.PY", false),
            Ok(OpenMethod::WindowsNotepad)
        );
        assert_eq!(
            choose_open_method(TargetOs::Windows, "page.html", false),
            Ok(OpenMethod::WindowsNotepad)
        );
        assert_eq!(
            choose_open_method(TargetOs::Windows, "data.csv", false),
            Ok(OpenMethod::WindowsNotepad)
        );
        assert_eq!(
            choose_open_method(TargetOs::Windows, "run.sh", false),
            Ok(OpenMethod::WindowsNotepad)
        );
        assert_eq!(
            choose_open_method(TargetOs::Windows, "notes.txt ", false),
            Ok(OpenMethod::WindowsNotepad)
        );
        assert_eq!(
            choose_open_method(TargetOs::Windows, "run.bat.", false),
            Ok(OpenMethod::WindowsNotepad)
        );
        assert_eq!(
            choose_open_method(TargetOs::Windows, "file.xml", false),
            Ok(OpenMethod::WindowsNotepad)
        );

        // macOS:
        // Always MacosOpenText regardless of file type when no editor configured
        assert_eq!(
            choose_open_method(TargetOs::Macos, "notes.txt", false),
            Ok(OpenMethod::MacosOpenText)
        );
        assert_eq!(
            choose_open_method(TargetOs::Macos, "x.js", false),
            Ok(OpenMethod::MacosOpenText)
        );
        assert_eq!(
            choose_open_method(TargetOs::Macos, "page.html", false),
            Ok(OpenMethod::MacosOpenText)
        );
        assert_eq!(
            choose_open_method(TargetOs::Macos, "Dockerfile", false),
            Ok(OpenMethod::MacosOpenText)
        );
        assert_eq!(
            choose_open_method(TargetOs::Macos, "notes.txt ", false),
            Ok(OpenMethod::MacosOpenText)
        );

        // Linux:
        // Allowlisted files -> LinuxXdgOpen
        assert_eq!(
            choose_open_method(TargetOs::Linux, "notes.txt", false),
            Ok(OpenMethod::LinuxXdgOpen)
        );
        assert_eq!(
            choose_open_method(TargetOs::Linux, "a b.txt", false),
            Ok(OpenMethod::LinuxXdgOpen)
        );
        assert_eq!(
            choose_open_method(TargetOs::Linux, "Dockerfile", false),
            Ok(OpenMethod::LinuxXdgOpen)
        );
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
        assert_eq!(
            choose_open_method(TargetOs::Other, "notes.txt", false),
            Ok(OpenMethod::SystemDefault)
        );
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
            assert!(
                res_win.is_err(),
                "Expected backslash path to be rejected on Windows"
            );
            assert_eq!(
                sanitize_name_with_rules(r"..\..\evil.txt", false).unwrap(),
                r"..\..\evil.txt"
            );
        }

        #[cfg(unix)]
        {
            let res_unix =
                resolve_download_target_with_rules(r"/remote/..\..\evil.txt", dest, false);
            assert!(
                res_unix.is_ok(),
                "Expected backslash path to be treated as ordinary file name on Unix"
            );
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
            assert!(
                res_drive.is_err(),
                "Expected C:evil.txt to be rejected on Windows"
            );
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
        let res_trailing =
            resolve_download_target_with_rules("/remote/my_folder/", dest, cfg!(windows)).unwrap();
        assert_eq!(res_trailing.1, "my_folder");
        assert_eq!(res_trailing.2, res_trailing.0.join("my_folder"));
        assert!(is_path_confined(&res_trailing.0, &res_trailing.2));

        let res_multi_trailing =
            resolve_download_target_with_rules("/remote/another_folder///", dest, cfg!(windows))
                .unwrap();
        assert_eq!(res_multi_trailing.1, "another_folder");
        assert_eq!(
            res_multi_trailing.2,
            res_multi_trailing.0.join("another_folder")
        );
        assert!(is_path_confined(
            &res_multi_trailing.0,
            &res_multi_trailing.2
        ));
    }

    #[test]
    fn test_download_confinement_normal_name() {
        let temp = tempfile::tempdir().unwrap();
        let dest = temp.path();

        // (d) a normal name downloads into destination_dir
        let res_normal =
            resolve_download_target_with_rules("/var/log/report.txt", dest, cfg!(windows)).unwrap();
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
        assert!(
            is_path_confined(circular_base, &circular_target),
            "Old circular anchor incorrectly allowed traversal"
        );
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
        assert!(
            validate_download_target(&link_target).await.is_err(),
            "Symlink at target must be refused"
        );
        assert!(
            validate_download_target(&real_file).await.is_ok(),
            "Regular file must be accepted"
        );
    }

    #[test]
    fn test_cleanup_temp_edits() {
        let temp_dir = std::env::temp_dir().join("airlock_edits");
        let _ = std::fs::create_dir_all(&temp_dir);
        let test_file = temp_dir.join("airlock_test_orphaned_file.tmp");
        std::fs::write(&test_file, b"test").unwrap();
        assert!(test_file.exists());

        cleanup_temp_edits();
        assert!(
            !test_file.exists(),
            "Cleanup must purge orphaned edit files"
        );
    }

    #[test]
    fn test_validate_editor_path_matrix_windows() {
        // Windows rejects .bat .cmd .ps1 .vbs .js
        let bad_exts = ["bat", "cmd", "ps1", "vbs", "js"];
        for ext in &bad_exts {
            let p_str = format!(r"C:\tools\editor.{}", ext);
            let p = Path::new(&p_str);
            let res = validate_editor_path_with_rules("windows", p, &PathFacts::file(true));
            assert!(res.is_err(), "Windows must reject .{} external editor", ext);
        }

        // Relative paths rejected
        assert!(
            validate_editor_path_with_rules(
                "windows",
                Path::new("notepad.exe"),
                &PathFacts::file(true)
            )
            .is_err(),
            "Windows must reject relative path"
        );
        assert!(
            validate_editor_path_with_rules(
                "windows",
                Path::new(r".\notepad.exe"),
                &PathFacts::file(true)
            )
            .is_err(),
            "Windows must reject relative path .\\"
        );
        assert!(
            validate_editor_path_with_rules(
                "windows",
                Path::new(r"tools\notepad.exe"),
                &PathFacts::file(true)
            )
            .is_err(),
            "Windows must reject relative path tools\\"
        );

        // Directories rejected
        assert!(
            validate_editor_path_with_rules(
                "windows",
                Path::new(r"C:\Windows\System32"),
                &PathFacts::dir()
            )
            .is_err(),
            "Windows must reject directories"
        );

        // Missing files rejected
        assert!(
            validate_editor_path_with_rules(
                "windows",
                Path::new(r"C:\Windows\nonexistent.exe"),
                &PathFacts::not_found()
            )
            .is_err(),
            "Windows must reject missing files"
        );

        // Accepts .exe (with spaces, different cases)
        assert!(
            validate_editor_path_with_rules(
                "windows",
                Path::new(r"C:\Windows\notepad.exe"),
                &PathFacts::file(true)
            )
            .is_ok(),
            "Windows must accept .exe"
        );
        assert!(
            validate_editor_path_with_rules(
                "windows",
                Path::new(r"C:\Program Files\Notepad++\notepad++.exe"),
                &PathFacts::file(true)
            )
            .is_ok(),
            "Windows must accept .exe path with spaces"
        );
        assert!(
            validate_editor_path_with_rules(
                "windows",
                Path::new(r"C:\Windows\NOTEPAD.EXE"),
                &PathFacts::file(true)
            )
            .is_ok(),
            "Windows must accept .EXE in uppercase"
        );
    }

    #[test]
    fn test_validate_editor_path_matrix_macos() {
        // Accepts .app directories
        assert!(
            validate_editor_path_with_rules(
                "macos",
                Path::new("/Applications/Visual Studio Code.app"),
                &PathFacts::dir()
            )
            .is_ok(),
            "macOS must accept .app bundle directory"
        );
        // Accepts executable files
        assert!(
            validate_editor_path_with_rules(
                "macos",
                Path::new("/usr/local/bin/code"),
                &PathFacts::file(true)
            )
            .is_ok(),
            "macOS must accept executable file"
        );
        // Rejects non-executable files
        assert!(
            validate_editor_path_with_rules(
                "macos",
                Path::new("/usr/local/bin/script.sh"),
                &PathFacts::file(false)
            )
            .is_err(),
            "macOS must reject non-executable file"
        );
        // Rejects non-.app directories
        assert!(
            validate_editor_path_with_rules(
                "macos",
                Path::new("/Applications/Visual Studio Code"),
                &PathFacts::dir()
            )
            .is_err(),
            "macOS must reject directory without .app extension"
        );
        // Rejects relative paths
        assert!(
            validate_editor_path_with_rules("macos", Path::new("Code.app"), &PathFacts::dir())
                .is_err(),
            "macOS must reject relative path"
        );
        // Rejects missing paths
        assert!(
            validate_editor_path_with_rules(
                "macos",
                Path::new("/Applications/Code.app"),
                &PathFacts::not_found()
            )
            .is_err(),
            "macOS must reject missing file"
        );
    }

    #[test]
    fn test_validate_editor_path_matrix_linux() {
        // Rejects non-executable files
        assert!(
            validate_editor_path_with_rules(
                "linux",
                Path::new("/usr/bin/nano"),
                &PathFacts::file(false)
            )
            .is_err(),
            "Linux must reject non-executable file"
        );
        // Accepts executable files
        assert!(
            validate_editor_path_with_rules(
                "linux",
                Path::new("/usr/bin/nano"),
                &PathFacts::file(true)
            )
            .is_ok(),
            "Linux must accept executable file"
        );
        // Rejects directories
        assert!(
            validate_editor_path_with_rules("linux", Path::new("/usr/bin"), &PathFacts::dir())
                .is_err(),
            "Linux must reject directories"
        );
        // Rejects relative paths
        assert!(
            validate_editor_path_with_rules("linux", Path::new("nano"), &PathFacts::file(true))
                .is_err(),
            "Linux must reject relative path"
        );
        // Rejects missing files
        assert!(
            validate_editor_path_with_rules(
                "linux",
                Path::new("/usr/bin/nano"),
                &PathFacts::not_found()
            )
            .is_err(),
            "Linux must reject missing file"
        );
    }

    #[allow(dead_code)]
    pub struct FailingEditorStore;

    impl EditorStore for FailingEditorStore {
        fn get(&self) -> Result<Option<String>, String> {
            Err("Keyring daemon is unavailable".to_string())
        }
        fn set(&self, _path: &str) -> Result<(), String> {
            Err("Keyring daemon is unavailable".to_string())
        }
        fn clear(&self) -> Result<(), String> {
            Err("Keyring daemon is unavailable".to_string())
        }
    }

    #[test]
    fn test_editor_store_mock_roundtrip() {
        let store = MockEditorStore::new();
        assert_eq!(store.get().unwrap(), None);

        store.set("/usr/bin/custom-editor").unwrap();
        assert_eq!(
            store.get().unwrap(),
            Some("/usr/bin/custom-editor".to_string())
        );

        store.clear().unwrap();
        assert_eq!(store.get().unwrap(), None);
    }

    #[test]
    fn test_resolve_configured_editor() {
        // 1. FailingEditorStore (store error -> Ok(None))
        let failing_store = FailingEditorStore;
        let res = resolve_configured_editor(&failing_store, |_| Ok(()));
        assert_eq!(
            res,
            Ok(None),
            "Keyring read error must be swallowed and treated as no editor configured"
        );

        // 2. Invalid configured path (store returns Some(...) but validator fails -> Err pointing to Settings)
        let invalid_store = MockEditorStore::with_editor("notepad.exe");
        let res =
            resolve_configured_editor(&invalid_store, |_| Err("path must be absolute".to_string()));
        assert!(res.is_err(), "Invalid configured path must return Err");
        let err_msg = res.unwrap_err();
        assert!(
            err_msg.contains("Configured external editor 'notepad.exe' is invalid"),
            "Error must specify invalid editor"
        );
        assert!(
            err_msg.contains("Settings"),
            "Error must point user to Settings"
        );

        // 3. No editor configured (store returns None -> Ok(None))
        let empty_store = MockEditorStore::new();
        let res = resolve_configured_editor(&empty_store, |_| Ok(()));
        assert_eq!(res, Ok(None), "Empty editor store must return Ok(None)");

        // 4. Valid editor (store returns Some(...) and validator succeeds -> Ok(Some(...)))
        let valid_store = MockEditorStore::with_editor("/usr/bin/custom-editor");
        let res = resolve_configured_editor(&valid_store, |_| Ok(()));
        assert_eq!(res, Ok(Some("/usr/bin/custom-editor".to_string())));
    }

    #[test]
    fn test_editor_launch_kind_all_os() {
        let mac_app = Path::new("/Applications/Visual Studio Code.app");
        let mac_bin = Path::new("/usr/local/bin/code");
        let win_exe = Path::new(r"C:\Program Files\Notepad++\notepad++.exe");
        let linux_bin = Path::new("/usr/bin/nano");

        // macOS: .app directory launches via open -a
        assert_eq!(
            editor_launch_kind("macos", mac_app, true),
            EditorLaunchKind::MacosOpenApp
        );
        // macOS: .app that is not a directory launches directly as executable
        assert_eq!(
            editor_launch_kind("macos", mac_app, false),
            EditorLaunchKind::DirectExecutable
        );
        // macOS: plain binary launches directly as executable
        assert_eq!(
            editor_launch_kind("macos", mac_bin, false),
            EditorLaunchKind::DirectExecutable
        );
        assert_eq!(
            editor_launch_kind("macos", mac_bin, true),
            EditorLaunchKind::DirectExecutable
        );

        // Windows: .exe launches directly as executable
        assert_eq!(
            editor_launch_kind("windows", win_exe, false),
            EditorLaunchKind::DirectExecutable
        );
        assert_eq!(
            editor_launch_kind("windows", win_exe, true),
            EditorLaunchKind::DirectExecutable
        );

        // Linux: binary launches directly as executable
        assert_eq!(
            editor_launch_kind("linux", linux_bin, false),
            EditorLaunchKind::DirectExecutable
        );

        // Other OS: launches directly as executable
        assert_eq!(
            editor_launch_kind("other", linux_bin, false),
            EditorLaunchKind::DirectExecutable
        );
    }

    #[test]
    fn test_edit_remote_file_uses_configured_editor_without_param() {
        let store = MockEditorStore::with_editor("/usr/bin/custom-editor");
        let configured = store.get().unwrap();
        assert_eq!(configured, Some("/usr/bin/custom-editor".to_string()));

        let method =
            choose_open_method(TargetOs::Linux, "notes.txt", configured.is_some()).unwrap();
        assert_eq!(method, OpenMethod::ConfiguredEditor);

        // Verify command construction preserves spaces and uses exact executable path without shell
        let ed_path = Path::new(r"C:\Program Files\Notepad++\notepad++.exe");
        let temp_file = Path::new(r"C:\temp\file.txt");
        let cmd = build_configured_editor_command(ed_path, temp_file);
        assert_eq!(cmd.get_program(), ed_path.as_os_str());
        let args: Vec<&std::ffi::OsStr> = cmd.get_args().collect();
        assert_eq!(args, vec![temp_file.as_os_str()]);
    }

    #[test]
    fn test_instance_dir_naming() {
        let temp_base = std::env::temp_dir();
        let pid = 12345;
        let hex = "deadbeef01234567";
        let (dir, lock_file) = create_and_validate_instance_dir(&temp_base, pid, hex).unwrap();
        let name = dir.file_name().unwrap().to_str().unwrap();
        assert_eq!(name, format!("airlock_edits_{}_{}", pid, hex));
        assert!(dir.is_dir());
        assert!(dir.join(".lock").is_file());
        drop(lock_file);
        let _ = std::fs::remove_file(dir.join(".lock"));
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn test_sweep_skips_held_lock_and_deletes_stale_and_removes_legacy() {
        let base = std::env::temp_dir().join(format!("test_sweep_{}", rand::random::<u64>()));
        std::fs::create_dir_all(&base).unwrap();

        // Legacy directory
        let legacy = base.join("airlock_edits");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("old.txt"), "legacy data").unwrap();

        // Stale instance directory (unlocked)
        let stale_dir = base.join("airlock_edits_1111_stale00000000");
        std::fs::create_dir_all(&stale_dir).unwrap();
        std::fs::write(stale_dir.join(".lock"), "").unwrap();
        std::fs::write(stale_dir.join("stale_edit.txt"), "stale").unwrap();

        // Active instance 1 (locked)
        let active_dir1 = base.join("airlock_edits_2222_active1111111");
        std::fs::create_dir_all(&active_dir1).unwrap();
        let lock_file1 = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(active_dir1.join(".lock"))
            .unwrap();
        lock_file1.try_lock().unwrap();

        // Active instance 2 (locked)
        let active_dir2 = base.join("airlock_edits_3333_active2222222");
        std::fs::create_dir_all(&active_dir2).unwrap();
        let lock_file2 = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(active_dir2.join(".lock"))
            .unwrap();
        lock_file2.try_lock().unwrap();

        // Perform startup sweep
        sweep_stale_temp_edits_in(&base, None);

        // Assertions:
        // 1. Legacy directory removed
        assert!(
            !legacy.exists(),
            "Legacy airlock_edits directory must be removed"
        );
        // 2. Stale directory removed
        assert!(
            !stale_dir.exists(),
            "Stale unlocked directory must be removed"
        );
        // 3. Active locked directories preserved
        assert!(
            active_dir1.exists(),
            "Active locked directory 1 must NOT be removed"
        );
        assert!(
            active_dir2.exists(),
            "Active locked directory 2 must NOT be removed"
        );

        // Unlock and cleanup
        drop(lock_file1);
        drop(lock_file2);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn test_per_file_path_inside_instance_dir_using_sanitized_name() {
        let instance_dir = Path::new(if cfg!(windows) {
            r"C:\temp\airlock_edits_100_abc"
        } else {
            "/tmp/airlock_edits_100_abc"
        });
        let random_hex = "0123456789abcdef";

        // Normal safe file
        let path =
            create_edit_temp_file_path(instance_dir, "document.txt", false, random_hex).unwrap();
        assert_eq!(path, instance_dir.join(random_hex).join("document.txt"));

        // Name with spaces and mixed case
        let path =
            create_edit_temp_file_path(instance_dir, "My Notes.md", false, random_hex).unwrap();
        assert_eq!(path, instance_dir.join(random_hex).join("My Notes.md"));

        // Windows unsafe name fallback
        let path = create_edit_temp_file_path(instance_dir, "CON.txt", true, random_hex).unwrap();
        assert_eq!(path, instance_dir.join(random_hex).join("file.txt"));
    }

    #[test]
    fn test_path_traversal_cannot_escape_instance_dir() {
        let instance_dir = Path::new(if cfg!(windows) {
            r"C:\temp\airlock_edits_100_abc"
        } else {
            "/tmp/airlock_edits_100_abc"
        });
        let random_hex = "sub1";

        let traversals = [
            "../../etc/passwd",
            "../../../evil.sh",
            r"..\..\windows\system32\cmd.exe",
            "foo/../../bar.txt",
            "/etc/shadow",
        ];

        for bad in traversals {
            let res = create_edit_temp_file_path(instance_dir, bad, false, random_hex);
            match res {
                Ok(p) => {
                    assert!(
                        is_path_confined(instance_dir, &p),
                        "Path '{:?}' escaped instance directory '{:?}'",
                        p,
                        instance_dir
                    );
                    assert!(p.starts_with(instance_dir.join(random_hex)));
                }
                Err(_) => {}
            }
        }
    }

    #[test]
    fn test_resolve_edit_temp_file_name_fallback_matrix() {
        // Valid names
        assert_eq!(resolve_edit_temp_file_name("hello.rs", false), "hello.rs");
        assert_eq!(resolve_edit_temp_file_name("notes.txt", true), "notes.txt");

        // Invalid Windows names fall back to "file.<ext>" or "file"
        assert_eq!(
            resolve_edit_temp_file_name("bad:stream.txt", true),
            "file.txt"
        );
        assert_eq!(resolve_edit_temp_file_name("trailing.dot.", true), "file");
        assert_eq!(resolve_edit_temp_file_name("COM1.log", true), "file.log");
        assert_eq!(
            resolve_edit_temp_file_name("has?question.py", true),
            "file.py"
        );
        assert_eq!(
            resolve_edit_temp_file_name("../escaped.sh", false),
            "file.sh"
        );
        assert_eq!(resolve_edit_temp_file_name("../noext", false), "file");
        assert_eq!(resolve_edit_temp_file_name("has?bad.ext#1", true), "file");
        assert_eq!(
            resolve_edit_temp_file_name("has?good.TXT", true),
            "file.txt"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_unix_instance_dir_validation_refusals() {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

        let base = std::env::temp_dir().join(format!("test_unix_refuse_{}", rand::random::<u64>()));
        std::fs::create_dir_all(&base).unwrap();

        // 1. Pre-existing directory with mode 0777 must be refused
        let dir_777 = base.join("airlock_edits_9999_mode777");
        let mut builder = std::fs::DirBuilder::new();
        builder.mode(0o777);
        builder.create(&dir_777).unwrap();
        std::fs::set_permissions(&dir_777, std::fs::Permissions::from_mode(0o777)).unwrap();

        let res = create_and_validate_instance_dir(&base, 9999, "mode777");
        assert!(res.is_err(), "Pre-existing 0777 directory must be refused");
        let err = res.err().unwrap();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);

        // 2. Pre-existing symlink must be refused
        let real_dir = base.join("target_dir");
        std::fs::create_dir_all(&real_dir).unwrap();
        let symlink_dir = base.join("airlock_edits_8888_symlink");
        std::os::unix::fs::symlink(&real_dir, &symlink_dir).unwrap();

        let res = create_and_validate_instance_dir(&base, 8888, "symlink");
        assert!(res.is_err(), "Symlink directory must be refused");

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn test_sweep_keep_markers_and_grace_periods() {
        let base = std::env::temp_dir().join(format!("test_sweep_keep_{}", rand::random::<u64>()));
        std::fs::create_dir_all(&base).unwrap();

        // 1. Legacy directory
        let legacy = base.join("airlock_edits");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("old.txt"), "legacy").unwrap();

        // 2. Stale directory with a fresh .keep marker (< 14 days)
        let stale_fresh = base.join("airlock_edits_1001_freshkeep");
        let per_file_fresh = stale_fresh.join("sub_fresh");
        std::fs::create_dir_all(&per_file_fresh).unwrap();
        std::fs::write(stale_fresh.join(".lock"), "").unwrap();
        let fresh_file = per_file_fresh.join("notes.txt");
        std::fs::write(&fresh_file, "unsaved changes").unwrap();
        let fresh_keep = per_file_fresh.join(".keep");
        std::fs::write(&fresh_keep, "").unwrap();

        // 3. Stale directory with an old .keep marker (aged to > 14 days)
        let stale_old = base.join("airlock_edits_1002_oldkeep");
        let per_file_old = stale_old.join("sub_old");
        std::fs::create_dir_all(&per_file_old).unwrap();
        std::fs::write(stale_old.join(".lock"), "").unwrap();
        std::fs::write(per_file_old.join("old_notes.txt"), "stale old changes").unwrap();
        let old_keep = per_file_old.join(".keep");
        std::fs::write(&old_keep, "").unwrap();
        // Age the .keep marker to 15 days ago using File::set_modified
        let keep_file = std::fs::File::options()
            .write(true)
            .open(&old_keep)
            .unwrap();
        let fifteen_days_ago = std::time::SystemTime::now()
            .checked_sub(std::time::Duration::from_secs(15 * 24 * 3600))
            .unwrap();
        keep_file.set_modified(fifteen_days_ago).unwrap();
        drop(keep_file);

        // 4. Brand-new directory without .lock (< 10 seconds old)
        let brand_new = base.join("airlock_edits_1003_brandnew");
        std::fs::create_dir_all(&brand_new).unwrap();

        // Run the sweep
        let recovered = sweep_stale_temp_edits_in(&base, None);

        // Assertions:
        // 1. Legacy folder is removed
        assert!(
            !legacy.exists(),
            "Legacy airlock_edits directory must be removed"
        );

        // 2. Stale directory with fresh .keep survives the sweep
        assert!(
            stale_fresh.exists(),
            "Stale directory with fresh .keep must survive the sweep"
        );
        assert!(fresh_file.exists(), "Kept file must still exist");
        assert!(
            recovered.iter().any(|r| r.file_name == "notes.txt"
                && r.local_path == fresh_file.to_string_lossy()),
            "Fresh unsaved edit must be recovered in sweep results"
        );

        // 3. Stale directory with old .keep is deleted
        assert!(
            !stale_old.exists(),
            "Stale directory with old .keep (> 14 days) must be deleted"
        );

        // 4. Brand-new directory without .lock survives the sweep
        assert!(
            brand_new.exists(),
            "Brand-new directory without .lock (< 10s) must survive the sweep"
        );

        // Cleanup
        let _ = std::fs::remove_dir_all(&base);
    }
}
