# Eidetic 🧠
> *The Filesystem That Remembers.*

Eidetic enhances your local storage with **Time Travel**, **Privacy Vaults**, and **Magic Views**. It runs quietly in the background, giving your files superpowers.


## ✨ Features
- **⏳ Time Travel**: Instant snapshots of every change. Undo anything.
- **🛡️ The Vault**: Drop files in `/vault` to transparently encrypt them on disk.
- **📊 Live Stats**: Read `stats.md` in your root for real-time filesystem usage.
- **🪄 Magic Views**: 
    - Auto-convert images (Save `.png`, read `.jpg`).
    - Web Links (`.url` files become the actual webpage).
    - Code Context Bundles for AI.

## 📦 Installation

To install Eidetic globally on your system:

```bash
# Install via Cargo (Rust Package Manager)
cargo install --path ./eidetic
```
*Note: Pre-built installers for macOS/Windows coming soon.*

## 🚀 Usage

Once installed, you can use `eidetic` from any terminal, anywhere.

### 1. Start the Magic
You need two folders: one where your actual data lives ("Source") and one where you want to see the magic ("Mount").

```bash
# Syntax: eidetic mount --source <DATA_PATH> --mountpoint <VIEW_PATH>

eidetic mount --source ~/Documents/MyData --mountpoint ~/Desktop/EideticView
```

Now, open `~/Desktop/EideticView` in your file explorer. ✨

### 2. Stop
Press `Ctrl+C` in the terminal to unmount and stop Eidetic.

---

## 👨‍💻 For Developers
If you are contributing to Eidetic using the monorepo:
- **Core**: Located in `eidetic/`. Run with `cargo run`.
- **Backend**: Located in `backend/`. Cloudflare Worker for sync/licensing.
