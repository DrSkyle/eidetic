# Eidetic 🧠
> *The Filesystem That Remembers.*

Eidetic enhances your local storage with **Time Travel**, **Privacy Vaults**, and **Magic Views**. It runs quietly in the background, giving your files superpowers.

![Eidetic Usage](https://via.placeholder.com/800x400?text=Eidetic+CLI+Demo)

## ✨ Features
- **⏳ Time Travel**: Instant snapshots of every change. Undo anything.
- **🛡️ The Vault**: Drop files in `/vault` to transparently encrypt them on disk.
- **📊 Live Stats**: Read `stats.md` in your root for real-time filesystem usage.
- **🤖 Deep Context**: `cat .context` to get a perfect, git-aware markdown bundle of your **entire codebase** for AI prompting.
- **🪄 Magic Views**: 
    - Auto-convert images (Save `.png`, read `.jpg`).
    - Web Links (`.url` files become the actual webpage).

## 📦 Installation

To install Eidetic globally on your system:

```bash
# Install via Cargo (Rust Package Manager)
cargo install --path ./eidetic
```
*Note: Pre-built installers for macOS/Windows coming soon.*

## 🚀 Usage

### 1. Start Daemon
Start Eidetic in the background using `start`. It will keep running even if you close the terminal.

```bash
# Syntax: eidetic start --source <DATA_PATH> --mountpoint <VIEW_PATH>

eidetic start --source ~/Documents/MyData --mountpoint ~/Desktop/EideticView
```

### 2. Interaction
Open `~/Desktop/EideticView` in your file explorer.
- **AI Coding**: `cat ~/Desktop/EideticView/.context | pbcopy` (Mac) -> Paste into ChatGPT.
- **Check Stats**: `cat ~/Desktop/EideticView/stats.md`

### 3. Stop
To stop the background process:

```bash
eidetic stop
```

---

## 👨‍💻 For Developers
If you are contributing to Eidetic using the monorepo:
- **Core**: Located in `eidetic/`. Run with `cargo run`.
- **Backend**: Located in `backend/`. Cloudflare Worker for sync/licensing.
