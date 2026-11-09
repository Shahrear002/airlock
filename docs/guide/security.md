# Security & Encryption

Airlock is designed with a **Local-First** security model.

## SSH Host Key Verification

Airlock cryptographically verifies the identity of SSH servers to protect against man-in-the-middle (MITM) attacks and DNS spoofing:

1. **Trust-On-First-Use (TOFU):**
   - When connecting to an SSH server for the first time, Airlock retrieves the host public key and computes its SHA-256 fingerprint.
   - You will be presented with a verification dialog displaying the target host, key algorithm, and fingerprint (e.g. `SHA256:...`).
   - Once trusted, the host key is securely saved to Airlock's private `known_hosts` file.

2. **Strict Mismatch Detection (MITM Protection):**
   - If a known server's host key changes, Airlock immediately halts the connection and displays a critical warning showing both the previously trusted fingerprint and the newly presented fingerprint.
   - You can abort the connection safely (default) or explicitly choose to replace the stored key if the server was legitimately reinstalled or re-keyed.

3. **Known Hosts Storage:**
   - Keys are stored in standard OpenSSH format:
     - **Windows:** `%APPDATA%\com.airlock.app\known_hosts`
     - **macOS:** `~/Library/Application Support/com.airlock.app/known_hosts`
     - **Linux:** `~/.config/airlock/known_hosts` (or `$XDG_CONFIG_HOME/airlock/known_hosts`)
   - You can review, search, and delete saved host keys at any time via **Settings & Security > Trusted Host Keys**.

## How Encryption Works

1. **Encryption at Rest**:
   - All your data (hosts, passwords, folders) is saved locally: `%APPDATA%\com.airlock.app\store.bin`.
   - Sensitive fields (such as passwords and VPN configurations) are encrypted using **AES-GCM (256-bit)**.

2. **Master Key**:
   - The encryption key is unique to your device and generated on your first run.

## Network Security

- Airlock uses standard SSH-2 protocols (via the `russh` library in Rust) to connect directly to your servers.
- Both interactive terminal sessions and SFTP transfers share the same verified SSH connection handler.
- We do **not** use any intermediate relay or telemetry servers. All connections are strictly point-to-point.

## SFTP Security & File Handling

Airlock enforces strict isolation and sanitization guarantees across all SFTP operations:

1. **Path Traversal & Device Name Sanitization:**
   - All downloaded file and directory names are sanitized before being mapped to the local filesystem.
   - Remote directory entries containing traversal elements (`..`, absolute paths, control characters, null bytes) are blocked and confined strictly inside the target destination folder.
   - On Windows, NTFS Alternate Data Streams (`:`) and reserved device names (`CON`, `PRN`, `AUX`, `NUL`, `COM1-9`, `LPT1-9`) are rejected to prevent file system hijacking. On Unix systems, legitimate POSIX names containing colons remain fully supported.
   - Symlinks are detected via `symlink_metadata` and skipped during recursive transfers to prevent arbitrary local directory exfiltration.

2. **Sandboxed Local File Editing:**
   - **Safe Extension Allowlist:** Only verified text and source-code formats (`.txt`, `.md`, `.json`, `.rs`, `.py`, `.ts`, `.html`, etc.) may open via the default system handler.
   - **Execution Shielding:** Executable binaries, scripts, and batch files (`.bat`, `.cmd`, `.vbs`, `.ps1`, `.exe`, `.hta`) are strictly prohibited from invoking shell associations (`open::that`). On Windows, these files fall back safely to opening directly as text inside `notepad.exe`.
   - **Custom External Editor:** You can configure a preferred text editor (e.g. `code`, `subl`, `notepad.exe`, `gedit`) under **Settings & Security > Editor & SFTP**. Configured editors are invoked directly with the file path without shell interpolation.
   - **Safe Temp Lifecycle & Leak Prevention:** Edited files are stored in a restricted temporary folder (`airlock_edits`). File watchers monitor session liveness and automatically delete temporary files upon session termination or application exit. Any orphaned files from previous crashes are purged on startup.
   - **Memory Ceiling:** A strict 20 MB streaming byte limit is enforced during downloads and in-memory reads to prevent memory exhaustion (OOM).

## Recommendations

- Always verify the fingerprint displayed in the Trust-On-First-Use dialog against your server administrator's known fingerprint.
- If you see a "Host Key Mismatch" warning unexpectedly, abort the connection immediately and investigate.
- Use strong, unique passwords for each SSH host.
