# Documentation Guidelines

## Philosophy
Documentation should be **Action-Oriented**. Users generally don't read manuals; they scan for "How do I do X?".

## Structure
### 1. README.md
- **Purpose:** The "Hook". Conversion to download.
- **Content:** 
    - What is it? (1 sentence)
    - GIF Demo.
    - Quick Install (3 lines max).
    - Key Features bullet points.

### 2. USER_GUIDE.md
- **Purpose:** The "Manual".
- **Structure:**
    - **Installation**: Detailed per-OS steps.
    - **Features**: Break down by feature (Time Travel, Vault, etc.).
    - **Troubleshooting**: Common FUSE errors.

## Maintaining Docs
- **Sync Rule**: If a CLI argument changes (like adding `--verbose`), update `README.md` immediately.
- **Screenshot Rule**: All screenshots must be updated for major version releases (checking UI consistency).
- **Tone**: Helpful, slightly technical but accessible. Avoid jargon where simple words work ("File Saver" vs "Persistance Layer").
