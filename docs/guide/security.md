# Security & Encryption

Airlock is designed with a **Local-First** security model.

## How it Works
1.  **Encryption at Rest**:
    -   All your data (hosts, passwords, folders) is saved to a local file: `%APPDATA%\com.airlock.app\store.bin`.
    -   Before saving, sensitive fields (like passwords) are encrypted using **AES-GCM (256-bit)**.

2.  **Master Key**:
    -   The encryption key is unique to your device.
    -   It is generated randomly on your first run.
    -   It is stored securely using the OS's native secure storage mechanisms where possible, or in a protected app data file.

## Network Security
-   Airlock uses standard SSH protocols (via the `russh` library in Rust) to connect to your servers.
-   We do **not** use any intermediate relay servers. The connection is direct from your computer to your server.
-   We do **not** collect any telemetry or usage data.

## Recommendations
-   Always lock your computer when unattended.
-   Use strong passwords for your SSH servers.
-   Consider using SSH Keys (Key-based authentication support is coming soon!).
