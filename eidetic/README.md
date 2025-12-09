# Eidetic 🧠
> *The Filesystem That Remembers.*

Eidetic is an experimental **Intelligent Filesystem** built in Rust. It runs in userspace (FUSE) and enhances your local storage with superpowers like Time Travel, Content-Aware Organization, and Transparent Encryption.

![Eidetic Demo](https://via.placeholder.com/800x400?text=Imagine+Cool+Demo+GIF+Here)

## 🚀 Key Features

### 1. ⏳ Time Travel
Never overwrite a file again. Eidetic snapshots every change instantly.
- **How**: Check the hidden `.file.history` folder (or virtual view) to see previous versions.
- **Tech**: Copy-On-Write logic backed by SQLite metadata.

### 2. 🛡️ The Vault
Professional-grade privacy for specific folders.
- **How**: Any file in `/vault` is encrypted on-disk.
- **Effect**: It looks like garbage in the source folder, but perfect in the Eidetic mount.

### 3. 🪄 Magic Views & Connected Files
- **Auto-Convert**: Transparently read `.png` files as `.jpg`. Transformations happen on the fly.
- **Web Links**: `.url` files that behave like the internet. Reading them fetches the live website HTML.
- **Context Bundler**: Coding with AI? `cat src/.context` to get a perfect Markdown bundle of your project for prompt engineering.

### 4. 🧹 Auto-Organizer
The filesystem watches what you write.
- Saves an "Invoice"? It moves to `/Finance`.
- Detects code? It tags it `#code` for easy virtual searching.

## 🛠️ Installation & Usage

### Prerequisites
- **Rust/Cargo**: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- **FUSE**:
    - macOS: `fuse-t` or `macFUSE`
    - Linux: `libfuse-dev`
    - Windows: `WinFSP` (Experimental)

### Running
```bash
# 1. Create a source directory (where data effectively lives)
element mkdir source_data

# 2. Create a mount point (where you see the Magic)
element mkdir mount_point

# 3. Ignite Eidetic
# Note: Use the flags --source and --mountpoint
cargo run -- --source ./source_data --mountpoint ./mount_point
```

## 🏗️ Architecture
Eidetic uses `fuser` for low-level filesystem operations and `rusqlite` for high-speed metadata tracking. It employs a background worker thread (`worker.rs`) to perform CPU-intensive tasks (AI tagging, OCR, Compression) so your filesystem never hangs.

## 📜 License
MIT License. Built for the community.
