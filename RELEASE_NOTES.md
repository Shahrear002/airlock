# Airlock v1.0.5 Release Notes

This release brings significant improvements to terminal layout and usability.

## 🛠 Fixes & Improvements

### Terminal
- **Layout Fix**: Resolved an issue where the terminal text was not utilizing the full width of the container. Commands like `sudo tail -f` now correctly use the entire screen width.
- **Focus Fix**: Fixed a bug where the terminal would lose focus after pasting text via the context menu. Focus is now automatically restored to the terminal.
- **Resize Logic**: Improved the resize observer to better handle window resizing and tab switching, ensuring the terminal always fits the available space.

---

# Airlock v1.0.4 Release Notes

This release brings a powerhouse feature to Airlock: **Split Screen Terminal**! You can now manage multiple sessions side-by-side for maximum productivity.

## ✨ New Features

### Split Screen Terminal
- **Grid Layouts**: Split your terminal horizontally or vertically to view multiple sessions at once.
- **Tab Management**: Organize your workspace with a new robust tab system. Each tab can contain its own unique split layout.
- **Context Menu Controls**: Right-click on any terminal to access split controls (Horizontal/Vertical) or close the current pane.
- **Seamless Connectivity**: Easily connect a specific split pane to a host by selecting the pane and clicking your desired connection in the sidebar.
- **Draggable Resizing**: Customize your workspace by dragging the dividers to resize panes to your liking.

---

# Airlock v1.0.3 Release Notes

This release focuses on data security, storage reliability, and application stability.

## 🔒 Security & Storage
- **Secure File Storage**: Migrated from browser `localStorage` to a dedicated, encrypted-at-rest file storage (`store.bin`) in the AppData directory. This enhances security by moving sensitive data out of the browser sandbox.
- **Data Safety**: Implemented robust overwrite protection to prevent data loss during startup race conditions.
- **Self-Healing Hydration**: Added a failsafe mechanism to ensure your saved hosts always load, even if the primary storage plugin experiences delays.

## 🛠 Fixes & Improvements
- **Terminal Stability**: Fixed a race condition that caused "Session not found" errors when resizing the terminal immediately after connection.
- **Performance**: Optimized storage adapter with lazy loading for faster application startup.

---

# Airlock v1.0.2 Release Notes

This release introduces advanced customization options, layout improvements, and better terminal color support.

## ✨ New Features

### Customization
- **Theme Support**: Added support for color schemes! Choose from built-in themes like Dracula, One Dark, Monokai, and Campbell (Windows CMD/PowerShell).
- **Theme Import**: You can now import custom themes from JSON files stored on your disk.
- **Disk-Based Storage**: Custom themes are saved to your configuration folder for persistence.

### User Interface
- **Collapsible Sidebar**: Added a toggle button to show/hide the sidebar, giving you more space for your terminal.
- **Scrollable Tabs**: The session tab bar now supports horizontal scrolling, handling multiple active connections gracefully.

### Host Management
- **Improved Folder Selection**: Dropdowns now display the full folder path (e.g., `Parent > Child`) for better clarity when organizing hosts.
- **Context Menu Actions**: Added a "New Folder" option to the right-click context menu for quicker folder creation.

### Terminal
- **Enhanced Color Support**: Upgraded PTY request to `xterm-256color` to support rich terminal colors and prompts (e.g., green `user@host`).

---

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
