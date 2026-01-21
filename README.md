
# 🚀 Airlock

<p align="center">
  <img src="https://img.shields.io/github/downloads/Shahrear002/airlock/total?style=flat&color=2ea44f&label=Downloads" alt="Downloads">
  <img src="https://img.shields.io/github/stars/Shahrear002/airlock?style=flat&color=e3b341&label=Stars" alt="Stars">
  <img src="https://img.shields.io/github/v/release/Shahrear002/airlock?style=flat&color=3fb950&label=Release" alt="Release">
  <img src="https://img.shields.io/badge/Platform-Windows_|_macOS_|_Linux-blue?style=flat&color=0078D6&label=Platform" alt="Platform">
  <img src="https://img.shields.io/github/license/Shahrear002/airlock?style=flat&color=2ea44f&label=License" alt="License">
</p>

**Airlock** is a secure, local-first, and cross-platform SSH client built for developers who value privacy, portability, and modern design. Built with the power of **Rust** and **Tauri**, it delivers a native-performance terminal experience wrapped in a beautiful **Vue 3** interface.

![Airlock Banner](Screenshot-1.png)

📖 **[View Documentation](https://shahrear002.github.io/airlock/)** | 📥 **[Download Latest Release](https://github.com/Shahrear002/airlock/releases/latest)**

## ✨ Features

- **🔒 Local-First Security**: All your host credentials and passwords are encrypted locally using **AES-256**. No data ever leaves your device unless you move it yourself.
- **📂 Smart Organization**: Group your servers with folders and nested hierarchies for easy access.
- **💾 Portable Backups**: Export your entire configuration securely. Passwords are re-encrypted with a master backup password, allowing you to restore your setup on any machine (Windows, macOS, or Linux).
- **⚡ Native Performance**: Powered by a Rust backend using `russh`, providing low-latency connections and resource efficiency.
- **🎨 Modern UI**: A sleek, dark-mode-first interface using **Shadcn** and **Tailwind CSS**.
- **💻 Full Terminal**: Integrated **xterm.js** provides a rich terminal experience with support for resizing, copy-paste, and standard ANSI colors.

## 🛠️ Tech Stack

- **Core**: [Tauri v2](https://tauri.app) (Rust)
- **Frontend Framework**: [Nuxt 3](https://nuxt.com)
- **Language**: TypeScript & Rust
- **Styling**: Tailwind CSS & Radix Vue
- **State Management**: Pinia
- **Terminal**: xterm.js

## 📦 Installation

### Windows
- **Installer (.exe)**: [Download Latest](https://github.com/your-username/airlock/releases/latest)
- **MSI**: Available for enterprise deployments.

### macOS
- **DMG**: [Download Latest](https://github.com/your-username/airlock/releases/latest)
- Drag and drop to Applications folder.

### Linux
- **AppImage / DEB**: Check the releases page for your distribution.

## 🏗️ Development

To build Airlock from source, ensure you have **Rust**, **Node.js**, and **pnpm/npm** installed.

1.  **Clone the repository:**
    ```bash
    git clone https://github.com/your-username/airlock.git
    cd airlock
    ```

2.  **Install dependencies:**
    ```bash
    npm install
    # or
    pnpm install
    ```

3.  **Run in development mode:**
    ```bash
    npm run tauri dev
    ```

## 🤝 Contributing

Contributions are welcome! Whether it's fixing bugs, improving documentation, or suggesting new features.

1.  Fork the Project
2.  Create your Feature Branch (`git checkout -b feature/AmazingFeature`)
3.  Commit your Changes (`git commit -m 'Add some AmazingFeature'`)
4.  Push to the Branch (`git push origin feature/AmazingFeature`)
5.  Open a Pull Request

## 📄 License

Distributed under the MIT License. See `LICENSE` for more information.
