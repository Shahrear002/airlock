# Airlock v1.0.1 Release Notes

This release addresses critical issues with terminal resizing and improves stability.

## 🛠 Fixes & Improvements

### Terminal
- **Fixed Terminal Resize Synchronization**: Resolved an issue where opening text editors like `vim` or `nano` would result in a partial screen render (80x24 characters) instead of using the full available terminal space.
- **Improved Connection Stability**: Implemented a retry mechanism for PTY resizing to ensure the correct terminal dimensions are applied even if the SSH session takes a moment to initialize.

---

# Airlock v1.0.0 Release Notes

We are thrilled to announce the first public release of **Airlock**, a modern, local-first SSH client! 🎉

## 🚀 What's New

### Core Features
- **Secure SSH Connectivity**: Connect to your servers securely with password authentication.
- **Local Encryption**: All sensitive data is encrypted at rest using AES-GCM.
- **Native Terminal**: Full xterm.js integration for a familiar and powerful command-line experience.

### Host Management
- **Folders & Nesting**: Organize your hosts into folders to keep your workspace clean.
- **Search & Explorer**: Quickly find and connect to your saved servers.

### Data Portability
- **Encrypted Exports**: Safely backup your entire configuration.
- **Portable Restoration**: Restore your data on any computer using a master backup password.

## 📦 Downloads

| Platform | Installer Type | Filename |
|----------|---------------|----------|
| **Windows** | NSIS Installer | `airlock_1.0.0_x64-setup.exe` |
| **Windows** | MSI Installer | `airlock_1.0.0_x64_en-US.msi` |
| **macOS** | Disk Image | `airlock_1.0.0_x64.dmg` |
| **Linux** | Debian Package | `airlock_1.0.0_amd64.deb` |
| **Linux** | AppImage | `airlock_1.0.0_amd64.AppImage` |
| **Linux** | RPM Package | `airlock-1.0.0-1.x86_64.rpm` |

## 🤝 Special Thanks

Thank you to everyone who contributed to this release!
