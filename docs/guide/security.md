# Security & Encryption

Airlock is designed with a **Local-First** security model, enforcing cryptographic verification and rigorous file isolation.

## SSH Host Key Verification

Airlock cryptographically verifies the identity of SSH servers to protect against man-in-the-middle (MITM) attacks and DNS spoofing:

1. **Three Verification Outcomes:**
   - **Unknown Host (TOFU):** When connecting to an SSH server for the first time, Airlock retrieves the host public key and computes its SHA-256 fingerprint (`SHA256:...`). You are prompted to verify and trust the host key. Once trusted, the key is added to `known_hosts`.
   - **New Key Type (Warning):** If a host is already known under one key type (e.g. `ssh-rsa`), but subsequently presents an additional key type (e.g. `ssh-ed25519`), Airlock displays a warning presenting both the existing trusted fingerprint(s) and the new presented fingerprint. Trusting adds the new key without deleting existing keys.
   - **Host Key Mismatch / Changed Key (Critical Alert):** If a server presents a different key of the *same* algorithm type, Airlock immediately halts the connection and displays a critical warning showing both the previously trusted fingerprint and the newly presented fingerprint. The connection is blocked unless you explicitly check the confirmation box and choose "Replace Key".

2. **Per-Host Known-First Algorithm Negotiation:**
   - When connecting to a known host, client algorithm preference is ordered so that algorithms already trusted for that host appear first. This avoids unnecessary algorithm renegotiation and spurious prompts when servers support multiple key types.

3. **Known Hosts Storage & Concurrency:**
   - Keys are stored in standard OpenSSH format:
     - **Windows:** `%APPDATA%\com.airlock.app\known_hosts`
     - **macOS:** `~/Library/Application Support/com.airlock.app/known_hosts`
     - **Linux:** `~/.local/share/com.airlock.app/known_hosts` (per `$XDG_DATA_HOME/com.airlock.app/known_hosts`)
   - IPv6 literals (both bracketed `[::1]` and unbracketed `::1`) are normalized to ensure consistent entry matching.
   - On Unix systems (Linux and macOS), file permissions are strictly enforced to `0600` (read/write by owner only).
   - Atomic writes via temporary file rename prevent file corruption during updates. Comments, hashed entries (`|1|...`), markers (`@cert-authority`), and unmanaged lines are preserved verbatim.
   - Process-wide thread serialization ensures that concurrent SSH connections safely update `known_hosts` without race conditions or lost entries.
   - You can review and delete individual saved host keys at any time via **Settings & Security > Trusted Host Keys**.

## How Encryption Works

1. **Encryption at Rest:**
   - All your data (saved hosts, credentials, groups, settings) is stored locally at `%APPDATA%\com.airlock.app\store.bin` (or platform equivalent).
   - Sensitive fields (such as passwords, private keys, and VPN configurations) are encrypted using **AES-256-GCM**.

2. **Master Encryption Key Status:**
   - The master encryption key is generated locally on first run.
   - *Current Implementation:* The master key is currently stored in `store.bin` alongside the encrypted data payload. A migration to store the master key inside the operating system keyring (Windows Credential Manager, macOS Keychain, Linux Secret Service) is planned for a subsequent update.

## Network Security

- Airlock uses standard SSH-2 protocols (via the `russh` library in Rust) to connect directly to your remote servers.
- Both interactive terminal sessions and SFTP transfers share the same verified SSH connection handler.
- We do **not** use any intermediate relay or telemetry servers. All connections are strictly point-to-point.

## SFTP Security & File Handling

Airlock enforces strict isolation, non-overwriting guarantees, and defensive execution across all SFTP operations:

1. **Non-Overwriting & Destination-Confined Downloads:**
   - **Never Overwrites:** Downloads never overwrite existing local files. If a file collision occurs, Airlock automatically increments a collision counter (`file (1).ext`, `file (2).ext`, up to 9999 candidates) before erroring.
   - **Strict Confinement:** All local target paths are strictly verified with `is_path_confined` to remain inside the chosen destination directory. Symlink components along existing destination paths are walked and verified via `symlink_metadata` to block directory escapes and junction-based attacks.
   - **Filename Sanitization:** 
     - On Windows, characters `< > : " | ? *` and control characters are rejected, as well as names ending in `.` or space. Reserved device stems (`CON`, `PRN`, `AUX`, `NUL`, `COM1-9`, `LPT1-9`) and NTFS Alternate Data Streams (`:`) are rejected.
     - On Unix, null bytes and path separators (`/`) are rejected.
     - Directory traversals (`..`), symlinks, and unresolvable names are safely skipped during recursive downloads.
   - **Skipped & Renamed Item Reporting:** Any skipped entries (e.g. symlinks, unsafe characters) or renamed entries (collision increments, trimmed whitespace) are recorded in a transfer summary. If any items were skipped, the summary toast remains visible persistently so you can review each affected file and reason.

2. **Streaming Downloads vs. 20 MB Edit Limit:**
   - **Downloads Stream Chunk-by-Chunk:** Standard SFTP file and recursive downloads stream directly to disk without an arbitrary file size ceiling, allowing large file transfers without memory ballooning.
   - **20 MB Limit on Edit Locally Only:** The strict 20 MB size limit applies exclusively to the **Edit Locally** feature (`edit_remote_file`) to protect local system memory during in-memory reads and live file watching.

3. **Safe Local File Editing & Safe-Open Allowlist:**
   - **Safe Extension Allowlist:** Only verified plain text and configuration formats (`.txt`, `.md`, `.json`, `.yaml`, `.toml`, `.rs`, `.ts`, `.vue`, etc., and exact filenames like `Dockerfile`, `Makefile`) may open via the default system handler.
   - **Executable & Script Exclusion:** Executable scripts (`.py`, `.js`, `.sh`, `.html`, `.bat`, `.cmd`, `.ps1`) and spreadsheets (`.csv`, `.tsv`) are strictly excluded from default system opening to prevent accidental code execution and spreadsheet formula injection.
   - **Per-OS Safe Fallback:** When a non-allowlisted file is edited and no custom external editor is configured:
     - **Windows:** Opens safely inside `notepad.exe`.
     - **macOS:** Opens safely with `open -t` (plain text editor).
     - **Linux:** Opens via `xdg-open` only for safe-allowlisted files; non-allowlisted files require configuring an external editor in Settings.

4. **External Editor Stored in OS Keyring:**
   - You can configure a preferred external editor under **Settings & Security > External Editor**.
   - The editor executable path is picked via the native OS file dialog from Rust and validated with platform-specific rules (Windows: `.exe` only, rejecting `.bat`, `.cmd`, `.ps1`; macOS: `.app` bundle directory or executable binary; Linux: executable bit set; must exist and be an absolute path).
   - The path is stored securely in the operating system keyring/keychain (`com.airlock.app` service, `external_editor` account).
   - Launched directly without shell interpolation (`open -a` on macOS for `.app` bundles, direct process spawn with the file path for plain binaries).

5. **Per-Process Temp File Isolation & Unsaved Edit Protection:**
   - **Per-Process Directory:** Each Airlock instance creates its own isolated temporary directory: `<temp>/airlock_edits_<pid>_<random hex>`. On Unix, it is created with mode `0700` and verified with `symlink_metadata` (must be a real directory, owned by the user, mode `0700`).
   - **Advisory File Lock:** The instance acquires an exclusive advisory lock on `<instance dir>/.lock` held for the lifetime of the process.
   - **Confined Per-File Subdirectories:** Each edited file is isolated in `<instance dir>/<random hex>/<sanitized name>`. If the original name fails sanitization, it falls back safely to `file.<ext>`.
   - **Startup Sweep & 14-Day Keep Retention:** During startup, Airlock sweeps other `airlock_edits_*` directories and attempts a non-blocking lock. Active instances holding an exclusive lock are preserved. Unlocked stale directories from previous sessions are inspected for `.keep` markers: any instance directory containing a per-file folder with a `.keep` marker modified within the last 14 days is preserved (survives the sweep) and its files are recovered. Once older than 14 days, the stale directory is purged. The sweep also skips brand-new `airlock_edits_*` directories without `.lock` that are younger than 10 seconds. Legacy un-namespaced `airlock_edits` folders are purged.
   - **Never Deletes Unsaved Edits Across Restarts:** When an SSH session closes or the app window is closed while local edits remain un-uploaded, Airlock writes a `.keep` marker inside that file's per-file folder, **keeps** the modified temp file on disk, and emits an `edit-unsaved` alert notifying you of the exact local path where your changes are saved and kept for 14 days. On subsequent launches, recovered edits are detected and an `edits-recovered` notification reports all preserved unsaved files.

## Recommendations

- Always verify the fingerprint displayed in the Trust-On-First-Use dialog against your server administrator's known fingerprint.
- If you see a "Host Key Mismatch" warning unexpectedly, abort the connection immediately and investigate.
- Use strong, unique credentials for each SSH host and maintain local OS account security.
