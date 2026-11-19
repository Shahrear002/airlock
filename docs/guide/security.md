# Security & Encryption

Airlock is designed with a **Local-First** security model.

## SSH Host Key Verification

Airlock cryptographically verifies the identity of SSH servers to protect against man-in-the-middle (MITM) attacks and DNS spoofing:

1. **Three Verification Outcomes:**
   - **Unknown Host (TOFU):** When connecting to an SSH server for the first time, Airlock retrieves the host public key and computes its SHA-256 fingerprint (`SHA256:...`). You are prompted to verify and trust the host key. Once trusted, the key is added to `known_hosts`.
   - **New Key Type (Warning):** If a host is already known under one key type (e.g. `ssh-rsa`), but subsequently presents an additional key type (e.g. `ssh-ed25519`), Airlock displays a warning presenting both the existing trusted fingerprint(s) and the new presented fingerprint. Trusting adds the new key without deleting existing keys.
   - **Host Key Mismatch / Changed Key (Critical Alert):** If a server presents a different key of the *same* algorithm type, Airlock immediately halts the connection and displays a critical warning showing both the previously trusted fingerprint and the newly presented fingerprint. The connection is blocked unless you explicitly check the confirmation box and choose "Replace Key".

2. **Per-Host Known-First Algorithm Negotiation:**
   - When connecting to a known host, client algorithm preference is ordered so that algorithms already trusted for that host appear first. This avoids unnecessary algorithm renegotiation and spurious prompts when servers support multiple key types.

3. **Known Hosts Storage & Security:**
   - Keys are stored in standard OpenSSH format:
     - **Windows:** `%APPDATA%\com.airlock.app\known_hosts`
     - **macOS:** `~/Library/Application Support/com.airlock.app/known_hosts`
     - **Linux:** `~/.local/share/com.airlock.app/known_hosts` (per `$XDG_DATA_HOME/com.airlock.app/known_hosts`)
   - On Unix systems (Linux and macOS), file permissions are enforced to `0600` (read/write by owner only).
   - Atomic writes via temporary file rename prevent file corruption during updates. Comments, hashed entries (`|1|...`), markers (`@cert-authority`), and unmanaged lines are preserved verbatim.
   - You can review and delete individual saved host keys at any time via **Settings & Security > Trusted Host Keys**.

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
   - **Safe Extension Allowlist:** Only verified text and configuration formats (`.txt`, `.md`, `.json`, `.yaml`, `.toml`, `.rs`, `.ts`, etc., and exact filenames like `Dockerfile`, `Makefile`) may open via the default system handler. Executable scripts (`.py`, `.js`, `.sh`, `.html`) and spreadsheet files (`.csv`, `.tsv`) are strictly excluded from default opening.
   - **Execution Shielding:** Executable binaries, scripts, and batch files (`.bat`, `.cmd`, `.vbs`, `.ps1`, `.exe`, `.hta`) are strictly prohibited from invoking shell associations (`open::that`). On Windows, these files fall back safely to opening directly as text inside `notepad.exe`.
   - **Custom External Editor:** You can configure a preferred text editor (e.g. `code`, `subl`, `notepad.exe`, `gedit`) under **Settings & Security > Editor & SFTP**. Configured editors are invoked directly with the file path without shell interpolation.
   - **Safe Temp Lifecycle & Leak Prevention:** Edited files are stored in a restricted temporary folder (`airlock_edits`). File watchers monitor session liveness and automatically delete temporary files upon session termination or application exit. Any orphaned files from previous crashes are purged on startup.
   - **Memory Ceiling:** A strict 20 MB streaming byte limit is enforced during downloads and in-memory reads to prevent memory exhaustion (OOM).

## Recommendations

- Always verify the fingerprint displayed in the Trust-On-First-Use dialog against your server administrator's known fingerprint.
- If you see a "Host Key Mismatch" warning unexpectedly, abort the connection immediately and investigate.
- Use strong, unique passwords for each SSH host.
