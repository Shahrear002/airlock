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

## Recommendations

- Always verify the fingerprint displayed in the Trust-On-First-Use dialog against your server administrator's known fingerprint.
- If you see a "Host Key Mismatch" warning unexpectedly, abort the connection immediately and investigate.
- Use strong, unique passwords for each SSH host.
