# How to Use Eidetic

Eidetic is an "Intelligent Filesystem" that lives on your computer. It looks like a normal folder, but it has a brain. It mirrors an existing folder (the "Source") to a new location (the "Mountpoint") and analyzes every file you save.

## 1. Installation

### Pre-requisites
- **Rust**: You need the Rust toolchain installed.
- **Mac/Linux**: 
    - **Mac**: FUSE for macOS (macFUSE) is recommended.
    - **Linux**: Install `libfuse-dev` (e.g., `sudo apt install libfuse-dev`).
- **Windows**:
    - Install **WinFSP** (Windows File System Proxy).

### Building
Open your terminal in the `eidetic` folder and run:
```bash
cargo build --release
```
The executable will be created at `./target/release/eidetic`.

## 2. Basic Usage

To start Eidetic, you need two folders:
1.  **Source**: The folder you want to "supercharge" (e.g., `~/Documents`).
2.  **Mountpoint**: An empty folder where the magic happens (e.g., `~/EideticMount`).

**Command:**
```bash
# Mac / Linux
./target/release/eidetic --source ~/Documents --mountpoint ~/EideticMount

# Windows (Powershell)
.\target\release\eidetic.exe --source C:\Users\You\Documents --mountpoint Z:
```

Once running, **do almost everything in the Mountpoint**. 
- Open `~/EideticMount` in your file explorer.
- Create files, delete files, rename folders—it behaves just like a normal drive.

## 3. Intelligent Features

### 📄 Smart PDF Processing
**How to use:**
1.  Copy or Save a `.pdf` file into your Mountpoint.
2.  **That's it!**

**What happens:**
Eidetic instantly detects the PDF, reads it in the background, extracts the text, and generates a summary (check the application logs to see this in action). Future versions will let you search this text instantly.

### 💻 Developer Mode (Code Analysis)
**How to use:**
1.  Open a code file (like `.rs`, `.py`, `.js`) inside the Mountpoint.
2.  Add a comment like this:
    ```rust
    // TODO: Fix this bug before release!
    ```
3.  Save the file.

**What happens:**
Eidetic parses your code, finds the `TODO`, and adds it to its internal database. It logs: `[Worker] Found 1 TODOs in main.rs`.

### 🧠 Persistent Memory
Eidetic remembers everything. Even if you crash or restart the app, it keeps a database (`.eidetic.db`) in your Source folder. This ensures that your file structure and all the "smart data" (summaries, todos) are safe.

## 4. Stopping
To stop the filesystem:
- **Mac/Linux**: Run `umount <mountpoint>` (e.g., `umount ~/EideticMount`) or simply hit `Ctrl+C` in the terminal running Eidetic.
- **Windows**: Hit `Ctrl+C` or eject the drive.
