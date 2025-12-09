use fuser::{
    FileAttr, FileType, Filesystem, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry,
    ReplyWrite, Request,
};
#[cfg(unix)]
use libc::{ENOENT, ENOSYS, EIO};

#[cfg(not(unix))]
mod platform_constants {
    pub const ENOENT: i32 = 2;
    pub const ENOSYS: i32 = 38;
    pub const EIO: i32 = 5;
}
#[cfg(not(unix))]
use platform_constants::*;

use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::{self, File};
use crate::db::Database;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, UNIX_EPOCH};
use std::sync::mpsc::Sender;
use crate::worker::Job;

const TTL: Duration = Duration::from_secs(1); // 1 second attribute cache

pub struct EideticFS {
    source_path: PathBuf,
    // Inode management
    // We need Mutex for interior mutability strictly speaking,
    // though FUSE is multi-threaded by default.
    inodes: Mutex<InodeStore>,
    uid: u32,
    gid: u32,
    sender: Sender<Job>,
}

const MAGIC_ROOT: u64 = u64::MAX;
const MAGIC_TAGS: u64 = u64::MAX - 1;
const MAGIC_RECENT: u64 = u64::MAX - 2;
const MAGIC_SEARCH: u64 = u64::MAX - 3;
const MAGIC_SEARCH_RESULTS: u64 = u64::MAX - 4;
const CONTEXT_BIT: u64 = 1 << 63;
const CONVERT_BIT: u64 = 1 << 62;
const API_BIT: u64 = 1 << 61; // API Mounting
const MAGIC_API: u64 = u64::MAX - 5;
const MAGIC_WORMHOLE: u64 = u64::MAX - 6;
const MAGIC_STATS: u64 = u64::MAX - 7;

// If Inode X is a directory, Inode (X | CONTEXT_BIT) is its .context file.


struct InodeStore {
    db: Database,
}

impl InodeStore {
    fn new(path: PathBuf) -> Self {
        // We panic here if DB fails, as we can't recover in new() easily without changing signature heavily.
        // Ideally new() returns Result. For now, unwrap is acceptable for prototype -> production evolution.
        let db = Database::new(path).expect("Failed to initialize database");
        Self { db }
    }

    fn alloc_inode(&mut self, parent: u64, name: String) -> u64 {
        if let Ok(Some(inode)) = self.db.get_inode(parent, &name) {
            return inode;
        }
        self.db.create_inode(parent, &name).unwrap_or(0) // 0 is invalid/root-ish, but handle error ideally
    }
    
    fn get_inode(&self, parent: u64, name: &str) -> Option<u64> {
         self.db.get_inode(parent, name).unwrap_or(None)
    }

    fn get_path(&self, inode: u64) -> Option<String> {
        if inode == 1 {
            return Some("".to_string());
        }
        
        let mut parts = Vec::new();
        let mut current = inode;
        
        let mut loop_check = 0;
        
        while current != 1 && loop_check < 100 {
            if let Ok(Some((parent, name))) = self.db.get_inode_entry(current) {
                parts.push(name);
                current = parent;
            } else {
                return None;
            }
            loop_check += 1;
        }
        
        parts.reverse();
        Some(parts.join("/"))
    }
    
    fn remove_inode(&mut self, inode: u64) {
        let _ = self.db.delete_inode(inode);
    }
    
    fn move_inode(&mut self, inode: u64, new_parent: u64, new_name: String) {
        let _ = self.db.rename_inode(inode, new_parent, &new_name);
    }
    
    // Virtual Helpers
    fn get_tags(&self) -> Vec<String> {
        self.db.get_tags().unwrap_or_default()
    }
    
    fn get_files_with_tag(&self, tag: &str) -> Vec<(u64, String)> {
        self.db.get_files_with_tag(tag).unwrap_or_default()
    }
}

impl EideticFS {
    pub fn new(source_path: PathBuf, uid: u32, gid: u32, sender: Sender<Job>) -> Self {
        let db_path = source_path.join(".eidetic.db");
        Self {
            source_path,
            #[cfg(unix)]
            uid,
            #[cfg(unix)]
            gid,
            
            #[cfg(not(unix))]
            uid: 0,
            #[cfg(not(unix))]
            gid: 0,
            
            inodes: Mutex::new(InodeStore::new(db_path)),
            sender,
        }
    }

    // License Verification (Phase 11)
    // Checks ~/.eidetic/license for a key and calls the Worker API
    fn check_license(&self) -> bool {
        // 1. Look for license file
        let home = std::env::var("HOME").unwrap_or_else(|_| "/".to_string());
        let license_path = std::path::Path::new(&home).join(".eidetic").join("license");
        
        if let Ok(key) = std::fs::read_to_string(license_path) {
            let key = key.trim();
            if key.is_empty() { return false; }
            
            // 2. Call Worker API
            // In Prod: "https://your-worker.workers.dev/verify?key={}"
            // For Demo: We mock a "local" check or assume "ED-PRO" prefix overrides network.
            if key.starts_with("ED-PRO") { return true; }

            // Using curl for prototype network check
            let output = std::process::Command::new("curl")
                .arg("-s")
                .arg(format!("https://eidetic-license.saujanyayaya.workers.dev/verify?key={}", key)) 
                // NOTE: User must replace URL. We leave a valid-looking structure.
                .output();

            if let Ok(out) = output {
                if String::from_utf8_lossy(&out.stdout).contains("\"valid\":true") {
                    return true;
                }
            }
        }
        false 
    }

    fn real_path(&self, inode: u64) -> Option<PathBuf> {
        let store = self.inodes.lock().unwrap();
        store.get_path(inode).map(|p| self.source_path.join(p))
    }

    // Helper to map std::fs::Metadata to fuser::FileAttr
    fn fs_metadata_to_file_attr(&self, metadata: &fs::Metadata, inode: u64) -> FileAttr {
        // Virtual Context File
        if (inode & CONTEXT_BIT) != 0 {
             return FileAttr {
                ino: inode,
                size: 1024,
                blocks: 1,
                atime: UNIX_EPOCH,
                mtime: UNIX_EPOCH,
                ctime: UNIX_EPOCH,
                crtime: UNIX_EPOCH,
                kind: FileType::RegularFile,
                perm: 0o444,
                nlink: 1,
                uid: 0, gid: 0, rdev: 0, flags: 0, blksize: 512,
             };
        }

        if (inode & CONVERT_BIT) != 0 {
             // Virtual Converted File (e.g. .jpg)
             return FileAttr {
                ino: inode,
                size: 1024 * 1024, // Dummy size (1MB), accurate size requires conversion
                blocks: 1,
                atime: UNIX_EPOCH,
                mtime: UNIX_EPOCH,
                ctime: UNIX_EPOCH,
                crtime: UNIX_EPOCH,
                kind: FileType::RegularFile,
                perm: 0o444,
                nlink: 1,
                uid: 0, gid: 0, rdev: 0, flags: 0, blksize: 512,
             };
        }
        
        // Virtual Search File (Writable)
        if inode == MAGIC_SEARCH {
             return FileAttr {
                ino: inode,
                size: 0,
                blocks: 0,
                atime: UNIX_EPOCH,
                mtime: UNIX_EPOCH,
                ctime: UNIX_EPOCH,
                crtime: UNIX_EPOCH,
                kind: FileType::RegularFile,
                perm: 0o666, // Writable!
                nlink: 1,
                uid: 0, gid: 0, rdev: 0, flags: 0, blksize: 512,
             };
        }

        if inode == MAGIC_API || inode == MAGIC_WORMHOLE {
            return FileAttr {
                ino: inode,
                size: 0,
                blocks: 0,
                atime: UNIX_EPOCH,
                mtime: UNIX_EPOCH,
                ctime: UNIX_EPOCH,
                crtime: UNIX_EPOCH,
                kind: FileType::Directory,
                perm: 0o555,
                nlink: 2,
                uid: 0, gid: 0, rdev: 0, flags: 0, blksize: 512,
             };
        }
        
        if inode == MAGIC_STATS {
             return FileAttr {
                ino: inode,
                size: 1024, // Dynamic size usually
                blocks: 1,
                atime: UNIX_EPOCH,
                mtime: UNIX_EPOCH,
                ctime: UNIX_EPOCH,
                crtime: UNIX_EPOCH,
                kind: FileType::RegularFile,
                perm: 0o444,
                nlink: 1,
                uid: 0, gid: 0, rdev: 0, flags: 0, blksize: 512,
             };
        }

        if (inode & API_BIT) != 0 {
             return FileAttr {
                ino: inode,
                size: 1024, 
                blocks: 1,
                atime: UNIX_EPOCH,
                mtime: UNIX_EPOCH,
                ctime: UNIX_EPOCH,
                crtime: UNIX_EPOCH,
                kind: FileType::RegularFile,
                perm: 0o444,
                nlink: 1,
                uid: 0, gid: 0, rdev: 0, flags: 0, blksize: 512,
             };
        }

        let size = if inode >= MAGIC_SEARCH_RESULTS { 0 } else { metadata.len() };
        let kind = if inode >= MAGIC_SEARCH_RESULTS || metadata.is_dir() { FileType::Directory } else { FileType::RegularFile };
        
        FileAttr {
            ino: inode,
            size,
            blocks: size / 512 + 1, // Approximation
            atime: metadata.accessed().unwrap_or(UNIX_EPOCH),
            mtime: metadata.modified().unwrap_or(UNIX_EPOCH),
            ctime: metadata.created().unwrap_or(UNIX_EPOCH),
            crtime: metadata.created().unwrap_or(UNIX_EPOCH),
            kind,
             perm: if inode >= MAGIC_SEARCH_RESULTS { 0o555 } else { metadata.permissions().mode() as u16 }, // Requires unix extension trait usually
             
             #[cfg(unix)]
             nlink: 1, 
             #[cfg(unix)]
             uid: self.uid, 
             #[cfg(unix)]
             gid: self.gid,
             
             #[cfg(not(unix))]
             nlink: 1,
             #[cfg(not(unix))]
             uid: 0,
             #[cfg(not(unix))]
             gid: 0,
            rdev: 0,
            flags: 0,
            blksize: 512,
        }
    }
}

// Unix permission extension
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;

#[cfg(not(unix))]
trait PermissionsExt {
    fn mode(&self) -> u32;
}

#[cfg(not(unix))]
impl PermissionsExt for std::fs::Permissions {
    fn mode(&self) -> u32 {
        0o755 // Default mock mode for Windows
    }
}

impl Filesystem for EideticFS {
    fn lookup(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        let name_str = name.to_string_lossy();
        
        // Virtual Magic Lookup
        if parent == 1 && name_str == ".magic" {
             let attr = FileAttr {
                ino: MAGIC_ROOT,
                size: 0,
                blocks: 0,
                atime: UNIX_EPOCH,
                mtime: UNIX_EPOCH,
                ctime: UNIX_EPOCH,
                crtime: UNIX_EPOCH,
                kind: FileType::Directory,
                perm: 0o555,
                nlink: 2,
                uid: 0, gid: 0, rdev: 0, flags: 0, blksize: 512,
             };
             reply.entry(&TTL, &attr, 0);
             return;
        }

        if parent == MAGIC_ROOT && name_str == "tags" {
             let attr = FileAttr {
                ino: MAGIC_TAGS,
                size: 0,
                blocks: 0,
                atime: UNIX_EPOCH,
                mtime: UNIX_EPOCH,
                ctime: UNIX_EPOCH,
                crtime: UNIX_EPOCH,
                kind: FileType::Directory,
                perm: 0o555,
                nlink: 2,
                uid: 0, gid: 0, rdev: 0, flags: 0, blksize: 512,
             };
             reply.entry(&TTL, &attr, 0);
             return;
        }

        if parent == MAGIC_ROOT && name_str == "recent" {
             let attr = FileAttr {
                ino: MAGIC_RECENT,
                size: 0,
                blocks: 0,
                atime: UNIX_EPOCH,
                mtime: UNIX_EPOCH,
                ctime: UNIX_EPOCH,
                crtime: UNIX_EPOCH,
                kind: FileType::Directory,
                perm: 0o555,
                nlink: 2,
                uid: 0, gid: 0, rdev: 0, flags: 0, blksize: 512,
             };
             reply.entry(&TTL, &attr, 0);
             return;
        }

        if parent == MAGIC_ROOT && name_str == "search" {
             // ...
             // ... (Keep existing)
             let attr = FileAttr { ino: MAGIC_SEARCH, size: 0, blocks: 0, atime: UNIX_EPOCH, mtime: UNIX_EPOCH, ctime: UNIX_EPOCH, crtime: UNIX_EPOCH, kind: FileType::RegularFile, perm: 0o666, nlink: 1, uid: 0, gid: 0, rdev: 0, flags: 0, blksize: 512 }; 
             reply.entry(&TTL, &attr, 0); return; 
        }

        if parent == MAGIC_ROOT && name_str == "api" {
             let attr = FileAttr {
                ino: MAGIC_API,
                size: 0, blocks: 0, atime: UNIX_EPOCH, mtime: UNIX_EPOCH, ctime: UNIX_EPOCH, crtime: UNIX_EPOCH, kind: FileType::Directory, perm: 0o555, nlink: 2, uid: 0, gid: 0, rdev: 0, flags: 0, blksize: 512,
             };
             reply.entry(&TTL, &attr, 0);
             return;
        }

        if parent == MAGIC_ROOT && name_str == "wormhole" {
             // GATE: Wormhole is PRO only (directory listing allowed, but inside...?)
             // Actually, let's keep directory open but show "Upgrade" file inside if not pro.
             let attr = FileAttr {
                ino: MAGIC_WORMHOLE,
                size: 0, blocks: 0, atime: UNIX_EPOCH, mtime: UNIX_EPOCH, ctime: UNIX_EPOCH, crtime: UNIX_EPOCH, kind: FileType::Directory, perm: 0o555, nlink: 2, uid: 0, gid: 0, rdev: 0, flags: 0, blksize: 512,
             };
             reply.entry(&TTL, &attr, 0);
             return;
        }

        if parent == MAGIC_ROOT && name_str == "stats.md" {
             let attr = FileAttr {
                ino: MAGIC_STATS,
                size: 1024,
                blocks: 1,
                atime: UNIX_EPOCH,
                mtime: UNIX_EPOCH,
                ctime: UNIX_EPOCH,
                crtime: UNIX_EPOCH,
                kind: FileType::RegularFile,
                perm: 0o444,
                nlink: 1,
                uid: 0, gid: 0, rdev: 0, flags: 0, blksize: 512,
             };
             reply.entry(&TTL, &attr, 0);
             return;
        }
        
        if parent == MAGIC_API && name_str == "bitcoin.json" {
             let attr = FileAttr {
                ino: MAGIC_API | API_BIT,
                size: 1024, blocks: 1, atime: UNIX_EPOCH, mtime: UNIX_EPOCH, ctime: UNIX_EPOCH, crtime: UNIX_EPOCH, kind: FileType::RegularFile, perm: 0o444, nlink: 1, uid: 0, gid: 0, rdev: 0, flags: 0, blksize: 512,
             };
             reply.entry(&TTL, &attr, 0);
             return;
        }
        
        // Lookup specific tag directory (e.g., /magic/tags/finance)
        if parent == MAGIC_TAGS {
            // We mock an inode logic: use hash of tag name mapped to high range?
            // For V1, we are lazy: we check if tag exists in DB.
            // If yes, return a "virtual inode" derived from hash, or dynamically allocate.
            // To simplify: we'll use a very simple hash or just say YES if it looks like a tag.
            // But we need a stable INODE.
            // Let's use crc64 or similar? Or just simple bytes sum + MAGIC_BASE.
            // Quick hack:
            let mut h = 0u64;
            for b in name_str.bytes() { h = h.wrapping_add(b as u64); }
            let inode = MAGIC_TAGS - 1000 - (h % 1000); 
            
            let attr = FileAttr {
                ino: inode,
                size: 0,
                blocks: 0,
                atime: UNIX_EPOCH,
                mtime: UNIX_EPOCH,
                ctime: UNIX_EPOCH,
                crtime: UNIX_EPOCH,
                kind: FileType::Directory,
                perm: 0o555,
                nlink: 2,
                uid: 0, gid: 0, rdev: 0, flags: 0, blksize: 512,
             };
             reply.entry(&TTL, &attr, 0);
             return;
        }


        let parent_path = {
            let store = self.inodes.lock().unwrap();
            match store.get_path(parent) {
                Some(p) => p,
                None => {
                    reply.error(ENOENT);
                    return;
                }
            }
        };

        // Virtual .context file check
        if name_str == ".context" {
             // ... existing context logic ...
             let attr = FileAttr {
                ino: parent | CONTEXT_BIT,
                size: 1024,
                blocks: 1,
                atime: UNIX_EPOCH,
                mtime: UNIX_EPOCH,
                ctime: UNIX_EPOCH,
                crtime: UNIX_EPOCH,
                kind: FileType::RegularFile,
                perm: 0o444,
                nlink: 1,
                uid: 0, gid: 0, rdev: 0, flags: 0, blksize: 512,
             };
             reply.entry(&TTL, &attr, 0);
             return;
        }

        // Auto-Convert Lookup: If asking for .jpg and it doesn't exist, check for .png
        if name_str.ends_with(".jpg") {
            let png_name = name_str.replace(".jpg", ".png");
            if let Some(png_inode) = {
                let store = self.inodes.lock().unwrap();
                store.get_inode(parent, &png_name)
            } {
                // Found a backing PNG! Return virtual JPG inode
                let attr = FileAttr {
                    ino: png_inode | CONVERT_BIT,
                    size: 1024 * 1024,
                    blocks: 1,
                    atime: UNIX_EPOCH,
                    mtime: UNIX_EPOCH,
                    ctime: UNIX_EPOCH,
                    crtime: UNIX_EPOCH,
                    kind: FileType::RegularFile,
                    perm: 0o444,
                    nlink: 1,
                    uid: 0, gid: 0, rdev: 0, flags: 0, blksize: 512,
                };
                reply.entry(&TTL, &attr, 0);
                return;
            }
        }

        let child_path_str = if parent_path.is_empty() {
            name_str.to_string()
        } else {
            format!("{}/{}", parent_path, name_str)
        };
        
        let real_path = self.source_path.join(&child_path_str);

        match fs::metadata(&real_path) {
            Ok(metadata) => {
                let mut store = self.inodes.lock().unwrap();
                // alloc_inode using parent and name
                let inode = store.alloc_inode(parent, name_str.to_string());
                drop(store); 

                let attr = self.fs_metadata_to_file_attr(&metadata, inode);
                reply.entry(&TTL, &attr, 0);
            }
            Err(_) => reply.error(ENOENT),
        }
    }

    fn getattr(&mut self, _req: &Request, inode: u64, reply: ReplyAttr) {
        if (inode & CONTEXT_BIT) != 0 {
             let attr = FileAttr {
                ino: inode,
                size: 1024,
                blocks: 1,
                atime: UNIX_EPOCH,
                mtime: UNIX_EPOCH,
                ctime: UNIX_EPOCH,
                crtime: UNIX_EPOCH,
                kind: FileType::RegularFile,
                perm: 0o444,
                nlink: 1,
                uid: 0, gid: 0, rdev: 0, flags: 0, blksize: 512,
             };
             reply.attr(&TTL, &attr);
             return;
        }

        if (inode & CONVERT_BIT) != 0 {
             let attr = FileAttr {
                ino: inode,
                size: 1024 * 1024,
                blocks: 1,
                atime: UNIX_EPOCH,
                mtime: UNIX_EPOCH,
                ctime: UNIX_EPOCH,
                crtime: UNIX_EPOCH,
                kind: FileType::RegularFile,
                perm: 0o444,
                nlink: 1,
                uid: 0, gid: 0, rdev: 0, flags: 0, blksize: 512,
             };
             reply.attr(&TTL, &attr);
             return;
        }
        
        if inode == MAGIC_SEARCH {
             let attr = FileAttr {
                ino: inode,
                size: 0,
                blocks: 0,
                atime: UNIX_EPOCH,
                mtime: UNIX_EPOCH,
                ctime: UNIX_EPOCH,
                crtime: UNIX_EPOCH,
                kind: FileType::RegularFile,
                perm: 0o666,
                nlink: 1,
                uid: 0, gid: 0, rdev: 0, flags: 0, blksize: 512,
             };
             reply.attr(&TTL, &attr);
             return;
        }

        if (inode & API_BIT) != 0 {
             let attr = FileAttr {
                ino: inode,
                size: 1024,
                blocks: 1,
                atime: UNIX_EPOCH,
                mtime: UNIX_EPOCH,
                ctime: UNIX_EPOCH,
                crtime: UNIX_EPOCH,
                kind: FileType::RegularFile,
                perm: 0o444,
                nlink: 1,
                uid: 0, gid: 0, rdev: 0, flags: 0, blksize: 512,
             };
             reply.attr(&TTL, &attr);
             return;
        }

        if inode == MAGIC_API || inode == MAGIC_WORMHOLE {
             let attr = FileAttr {
                ino: inode,
                size: 0,
                blocks: 0,
                atime: UNIX_EPOCH,
                mtime: UNIX_EPOCH,
                ctime: UNIX_EPOCH,
                crtime: UNIX_EPOCH,
                kind: FileType::Directory,
                perm: 0o555,
                nlink: 2,
                uid: 0, gid: 0, rdev: 0, flags: 0, blksize: 512,
             };
             reply.attr(&TTL, &attr);
             return;
        }

        if inode == MAGIC_STATS {
             let attr = FileAttr {
                ino: inode,
                size: 1024,
                blocks: 1,
                atime: UNIX_EPOCH,
                mtime: UNIX_EPOCH,
                ctime: UNIX_EPOCH,
                crtime: UNIX_EPOCH,
                kind: FileType::RegularFile,
                perm: 0o444,
                nlink: 1,
                uid: 0, gid: 0, rdev: 0, flags: 0, blksize: 512,
             };
             reply.attr(&TTL, &attr);
             return;
        }

        if inode >= MAGIC_SEARCH_RESULTS - 2000 {
             // UPGRADE_TO_PRO.txt or similar virtual files
             let attr = FileAttr {
                ino: inode,
                size: 100,
                blocks: 1,
                atime: UNIX_EPOCH,
                mtime: UNIX_EPOCH,
                ctime: UNIX_EPOCH,
                crtime: UNIX_EPOCH,
                kind: FileType::RegularFile, // Changed to Regular File for text
                perm: 0o444,
                nlink: 1,
                uid: 0, gid: 0, rdev: 0, flags: 0, blksize: 512,
             };
             reply.attr(&TTL, &attr);
             return;
        }

        if let Some(real_path) = self.real_path(inode) {
             match fs::metadata(&real_path) {
                Ok(metadata) => {
                    let attr = self.fs_metadata_to_file_attr(&metadata, inode);
                    reply.attr(&TTL, &attr);
                }
                Err(_) => reply.error(ENOENT),
            }
        } else {
            reply.error(ENOENT);
        }
    }

    fn read(
        &mut self,
        _req: &Request,
        inode: u64,
        _fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyData,
    ) {
        if let Some(real_path) = self.real_path(inode) {
             match File::open(&real_path) {
                 Ok(mut file) => {
                     use std::io::{Read, Seek, SeekFrom};
                     if let Err(_) = file.seek(SeekFrom::Start(offset as u64)) {
                         reply.error(EIO);
                         return;
                     }
                     let mut buffer = vec![0; size as usize];
                     match file.read(&mut buffer) {
                         Ok(bytes_read) => {
                             // Vault Logic: Decrypt on Read
                             if real_path.to_string_lossy().contains("/vault/") {
                                 let decrypted = crate::cipher::decrypt(&buffer[..bytes_read]);
                                 reply.data(&decrypted);
                             } else if real_path.extension().map_or(false, |e| e == "url") {
                                 // Web-Link Logic: Fetch URL!
                                 if let Ok(content) = std::str::from_utf8(&buffer[..bytes_read]) {
                                     let url = content.trim();
                                     if url.starts_with("http") {
                                         // Execute curl
                                         let output = std::process::Command::new("curl")
                                             .arg("-s") // silent
                                             .arg(url)
                                             .output();
                                         if let Ok(out) = output {
                                             reply.data(&out.stdout); 
                                             // Note: This replaces the .url file content with the HTML content view!
                                             // This matches the "Web-Link File" feature description.
                                         } else {
                                             reply.data(b"Error fetching URL");
                                         }
                                     } else {
                                        reply.data(&buffer[..bytes_read]); 
                                     }
                                 } else {
                                     reply.data(&buffer[..bytes_read]);
                                 }
                             } else {
                                 reply.data(&buffer[..bytes_read]);
                             }
                         },
                         Err(_) => reply.error(EIO),
                     }
                 },
                 Err(_) => reply.error(ENOENT),
             }
        } else if (inode & CONTEXT_BIT) != 0 {
             // DEEP CONTEXT: Recursive & Git-Aware
             // No license check required anymore.

             // Generate Context!
             let dir_inode = inode & !CONTEXT_BIT;
             if let Some(dir_path) = self.real_path(dir_inode) {
                  let mut content = String::new();
                  content.push_str(&format!("# Deep Context for {:?}\n\n", dir_path.file_name().unwrap_or_default()));
                  content.push_str("> Generated by Eidetic. Includes all source files recursively (respecting .gitignore).\n\n");
                  
                  // Use 'ignore' crate for recursive walking with gitignore support
                  use ignore::WalkBuilder;
                  
                  let walker = WalkBuilder::new(&dir_path)
                      .hidden(false) // Allow hidden files? Maybe no.
                      .git_ignore(true)
                      .build();

                  for result in walker {
                      if let Ok(entry) = result {
                          let p = entry.path();
                          if p.is_file() {
                              // Filter binary/large files roughly
                              let ext = p.extension().unwrap_or_default().to_string_lossy();
                              let allowed_exts = [
                                  "rs", "toml", "md", "txt", "js", "ts", "jsx", "tsx", "json", 
                                  "py", "c", "h", "cpp", "hpp", "go", "java", "kt", "swift",
                                  "html", "css", "scss", "sql", "sh", "yaml", "yml"
                              ];
                              
                              if allowed_exts.contains(&ext.as_ref()) {
                                  // Relative path for cleanliness
                                  let rel_path = p.strip_prefix(&dir_path).unwrap_or(p);
                                  
                                  if let Ok(code) = std::fs::read_to_string(&p) {
                                      content.push_str(&format!("## {}\n```{}\n{}\n```\n\n", rel_path.display(), ext, code));
                                  }
                              }
                          }
                      }
                  }
                  
                  // Handle offset read
                  let bytes = content.as_bytes();
                  if offset as usize >= bytes.len() {
                      reply.data(&[]);
                  } else {
                      let end = std::cmp::min(offset as usize + size as usize, bytes.len());
                      reply.data(&bytes[offset as usize..end]);
                  }
             } else {
                 reply.error(ENOENT);
             }
        } else if (inode & CONVERT_BIT) != 0 {
            // Auto-Convert Read: PNG -> JPG
            let raw_inode = inode & !CONVERT_BIT;
            if let Some(real_path) = self.real_path(raw_inode) {
                // Read PNG, Convert to JPG, Return
                if let Ok(img) = image::open(&real_path) {
                    let mut bytes: Vec<u8> = Vec::new();
                    // Use cursor to write to memory
                    let mut cursor = std::io::Cursor::new(&mut bytes);
                    if img.write_to(&mut cursor, image::ImageFormat::Jpeg).is_ok() {
                         // Handle offset
                          if offset as usize >= bytes.len() {
                              reply.data(&[]);
                          } else {
                              let end = std::cmp::min(offset as usize + size as usize, bytes.len());
                              reply.data(&bytes[offset as usize..end]);
                          }
                    } else {
                        reply.error(EIO);
                    }
                } else {
                    reply.error(EIO);
                }
            } else {
                reply.error(ENOENT);
            }
        } else if inode == MAGIC_STATS {
            // Generate Stats Content
            let tags = {
                 let store = self.inodes.lock().unwrap();
                 store.get_tags()
            };
            
            let mut content = String::new();
            content.push_str("# 📊 Eidetic Stats\n\n");
            content.push_str("## System Status\n");
            content.push_str("- **State**: Online 🟢\n");
            content.push_str(&format!("- **Total Tags**: {}\n", tags.len()));
            
            content.push_str("\n## Tags Distribution\n");
            if tags.is_empty() {
                content.push_str("_No tags found yet._\n");
            } else {
                for tag in tags {
                     let count = {
                         let store = self.inodes.lock().unwrap();
                         store.get_files_with_tag(&tag).len()
                     };
                     content.push_str(&format!("- **#{}**: {} files\n", tag, count));
                }
            }
            content.push_str("\n> *Generated by Eidetic Intelligent Filesystem*\n");

            let bytes = content.as_bytes();
            if offset as usize >= bytes.len() {
                reply.data(&[]);
            } else {
                let end = std::cmp::min(offset as usize + size as usize, bytes.len());
                reply.data(&bytes[offset as usize..end]);
            }
        } else {
            reply.error(ENOENT);
        }
    }

    fn readdir(
        &mut self,
        _req: &Request,
        inode: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        if offset > 0 {
            reply.ok();
            return;
        }

        // Virtual Readdir
        if inode == MAGIC_ROOT {
            let _ = reply.add(MAGIC_ROOT, 1, FileType::Directory, ".");
            let _ = reply.add(1, 2, FileType::Directory, "..");
            let _ = reply.add(MAGIC_TAGS, 3, FileType::Directory, "tags");
            let _ = reply.add(MAGIC_RECENT, 4, FileType::Directory, "recent");
            let _ = reply.add(MAGIC_SEARCH, 5, FileType::RegularFile, "search");
            let _ = reply.add(MAGIC_API, 6, FileType::Directory, "api");
            let _ = reply.add(MAGIC_WORMHOLE, 7, FileType::Directory, "wormhole");
            let _ = reply.add(MAGIC_STATS, 8, FileType::RegularFile, "stats.md");
            reply.ok();
            return;
        }
        
        // API Directory
        if inode == MAGIC_API {
            let _ = reply.add(MAGIC_API, 1, FileType::Directory, ".");
            let _ = reply.add(MAGIC_ROOT, 2, FileType::Directory, "..");
            let _ = reply.add(MAGIC_API | API_BIT, 3, FileType::RegularFile, "bitcoin.json");
             // In real app: read from config file list of APIs
            reply.ok();
            return;
        }
        
        // Wormhole (Mock P2P)
        if inode == MAGIC_WORMHOLE {
            if !self.check_license() {
                // Not Pro: Show Upgrade Info
                let _ = reply.add(MAGIC_WORMHOLE, 1, FileType::Directory, ".");
                let _ = reply.add(MAGIC_ROOT, 2, FileType::Directory, "..");
                let _ = reply.add(MAGIC_WORMHOLE - 999, 3, FileType::RegularFile, "UPGRADE_TO_PRO.txt");
                reply.ok();
                return;
            }

            let _ = reply.add(MAGIC_WORMHOLE, 1, FileType::Directory, ".");
            let _ = reply.add(MAGIC_ROOT, 2, FileType::Directory, "..");
            // Mock peer
            let _ = reply.add(MAGIC_WORMHOLE - 100, 3, FileType::Directory, "Peer_Node_1");
             reply.ok();
            return;
        }
        
        // Recent Files
        if inode == MAGIC_RECENT {
            let _ = reply.add(MAGIC_RECENT, 1, FileType::Directory, ".");
            let _ = reply.add(MAGIC_ROOT, 2, FileType::Directory, "..");
            // Mock recent files
            let _ = reply.add(MAGIC_RECENT-1, 3, FileType::RegularFile, "last_edited_file.rs");
            reply.ok();
            return;
        }

        if inode == MAGIC_TAGS {
            let _ = reply.add(MAGIC_TAGS, 1, FileType::Directory, ".");
            let _ = reply.add(MAGIC_ROOT, 2, FileType::Directory, "..");
            
            // Query DB for tags
            let store = self.inodes.lock().unwrap();
            let tags = store.get_tags();
            drop(store);
            
            for (i, tag) in tags.iter().enumerate() {
                // Stable inode hash
                let mut h = 0u64;
                for b in tag.bytes() { h = h.wrapping_add(b as u64); }
                let tag_inode = MAGIC_TAGS - 1000 - (h % 1000); 
                
                // +3 offset because of . and ..
                if reply.add(tag_inode, (i+3) as i64, FileType::Directory, tag) { break; }
            }
            reply.ok();
            return;
        }
        
        // Tag Directory Listing (e.g. inside "finance")
        if inode < MAGIC_TAGS && inode > MAGIC_TAGS - 2000 {
            // We need to know WHICH tag this inode corresponds to. 
            // Reverse lookup hash? Unreliable.
            // Ideally we store map. For prototype, we unfortunately can't know easily without store.
            // Assumption: This is "finance".
            // Since we don't have the Tag Name here (FUSE stateless), we strictly can't know.
            // Workaround: We will skip listing specific files for this step and leave it empty,
            // OR we fix lookup to store "Virtual Inodes".
            
            // Because fixing lookup is hard in this context without a VirtualInodeStore,
            // We will just return empty for safety on this pass to avoid crashing. 
            // In a real V4 we would implement VirtualInodeStore.
            
            let _ = reply.add(inode, 1, FileType::Directory, ".");
            let _ = reply.add(MAGIC_TAGS, 2, FileType::Directory, "..");
            reply.ok();
            return;
        }

        let store_lock = self.inodes.lock().unwrap();
        let parent_path_opt = store_lock.get_path(inode);
        drop(store_lock); // Release lock

        if let Some(parent_path) = parent_path_opt {
             let real_path = self.source_path.join(&parent_path);
             
             match fs::read_dir(real_path) {
                 Ok(entries) => {
                     let mut current_offset = 1;
                     
                     // Helper to add entry
                     let mut add_entry = |inode: u64, name: &str, kind: FileType| {
                         if reply.add(inode, current_offset, kind, name) {
                             // Buffer full
                             return true; 
                         }
                         current_offset += 1;
                         false
                     };

                     // Add . and ..
                     if add_entry(inode, ".", FileType::Directory) { reply.ok(); return; }
                     // Note: Parent inode '..' calculation is simplified here (usually should track parent)
                     if add_entry(1, "..", FileType::Directory) { reply.ok(); return; } 

                     // Add .magic to root
                     if inode == 1 {
                         if add_entry(MAGIC_ROOT, ".magic", FileType::Directory) { reply.ok(); return; }
                     }
                     
                     // Add .context to ALL directories
                     let ctx_inode = inode | CONTEXT_BIT;
                     if add_entry(ctx_inode, ".context", FileType::RegularFile) { reply.ok(); return; }

                     for entry in entries {
                         if let Ok(entry) = entry {
                             let file_name = entry.file_name();
                             let file_name_str = file_name.to_string_lossy();
                             let child_path_str = if parent_path.is_empty() {
                                 file_name_str.to_string()
                             } else {
                                 format!("{}/{}", parent_path, file_name_str)
                             };
                             
                             let mut store = self.inodes.lock().unwrap();
                             let child_inode = store.alloc_inode(inode, file_name_str.to_string());
                             // For readdir, we don't strictly need full attributes, just name and inode
                             let file_type = if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) { FileType::Directory } else { FileType::RegularFile };
                             drop(store);

                             if add_entry(child_inode, &file_name_str, file_type) {
                                  break;
                             }
                         }
                     }
                     reply.ok();
                 }
                 Err(_) => reply.error(ENOENT),
             }
        } else {
            reply.error(ENOENT);
        }
    }

    fn mkdir(
        &mut self,
        _req: &Request,
        parent: u64,
        name: &OsStr,
        _mode: u32,
        _umask: u32,
        reply: ReplyEntry,
    ) {
         let name_str = name.to_string_lossy();
         let store_lock = self.inodes.lock().unwrap();
         let parent_path_opt = store_lock.get_path(parent);
         drop(store_lock);

         if let Some(parent_path) = parent_path_opt {
             let child_path_str = if parent_path.is_empty() {
                name_str.to_string()
             } else {
                format!("{}/{}", parent_path, name_str)
             };
             let real_path = self.source_path.join(&child_path_str);

             match fs::create_dir(&real_path) {
                 Ok(_) => {
                     let metadata = fs::metadata(&real_path).unwrap();
                     let mut store = self.inodes.lock().unwrap();
                     let inode = store.alloc_inode(parent, name_str.to_string());
                     drop(store);
                     
                     let attr = self.fs_metadata_to_file_attr(&metadata, inode);
                     reply.entry(&TTL, &attr, 0);
                 }
                 Err(e) => reply.error(e.raw_os_error().unwrap_or(libc::EIO)),
             }
         } else {
             reply.error(ENOENT);
         }
    }

    fn rmdir(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: fuser::ReplyEmpty) {
        let name_str = name.to_string_lossy();
        let mut store = self.inodes.lock().unwrap();
        // Check lookup directly first
        if let Some(child_inode) = store.get_inode(parent, &name_str) {
            let child_path = store.get_path(child_inode);
            drop(store); // Release lock before IO

            if let Some(path) = child_path {
                let real_path = self.source_path.join(path);
                match fs::remove_dir(real_path) {
                    Ok(_) => {
                        self.inodes.lock().unwrap().remove_inode(child_inode);
                        reply.ok();
                    },
                    Err(e) => reply.error(e.raw_os_error().unwrap_or(libc::EIO)),
                }
            } else {
                reply.error(ENOENT);
            }
        } else {
             reply.error(ENOENT);
        }
    }

    fn unlink(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: fuser::ReplyEmpty) {
        let mut store = self.inodes.lock().unwrap();
        let name_str = name.to_string_lossy().to_string();
        
        if let Some(child_inode) = store.get_inode(parent, &name_str) {
            let child_path = store.get_path(child_inode);
            
            // Trash Logic
            if let Some(real_path_str) = child_path {
                 let full_path = self.source_path.join(&real_path_str);
                 let trash_dir = self.source_path.join(".eidetic/trash");
                 std::fs::create_dir_all(&trash_dir).unwrap_or(());
                 
                 let timestamp = std::time::SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
                 let backup_name = format!("{}_{}", timestamp, name_str);
                 let backup_path = trash_dir.join(&backup_name);
                 
                 if std::fs::rename(&full_path, &backup_path).is_ok() {
                     let _ = store.db.add_trash(&real_path_str, backup_path.to_string_lossy().as_ref());
                     let _ = store.remove_inode(child_inode); // Corrected Arg: just inode
                     reply.ok();
                     return;
                 }
            }

            // Fallback if move to trash fails (or logic error)
             let res = unsafe { libc::unlink(
                 std::ffi::CString::new(
                     self.source_path.join(store.get_path(child_inode).unwrap()).as_os_str().as_bytes()
                 ).unwrap().as_ptr()
             ) };

             if res == 0 {
                 store.remove_inode(child_inode);
                 reply.ok();
             } else {
                 reply.error(std::io::Error::last_os_error().raw_os_error().unwrap_or(EIO));
             }
        } else {
            reply.error(ENOENT);
        }
    }

    fn rename(
        &mut self,
        _req: &Request,
        parent: u64,
        name: &OsStr,
        newparent: u64,
        newname: &OsStr,
        _flags: u32,
        reply: fuser::ReplyEmpty,
    ) {
        let name_str = name.to_string_lossy();
        let newname_str = newname.to_string_lossy();
        
        let mut store = self.inodes.lock().unwrap(); // Changed to `mut store`
        // Resolve paths
        let old_parent_path = store.get_path(parent);
        let new_parent_path = store.get_path(newparent);
        let inode_to_move = store.get_inode(parent, &name_str);
        // drop(store); // REMOVED

        if let (Some(old_p), Some(new_p), Some(inode)) = (old_parent_path, new_parent_path, inode_to_move) {
             let old_path_str = if old_p.is_empty() { name_str.to_string() } else { format!("{}/{}", old_p, name_str) };
             let new_path_str = if new_p.is_empty() { newname_str.to_string() } else { format!("{}/{}", new_p, newname_str) };
             
             let real_old = self.source_path.join(old_path_str);
             let real_new = self.source_path.join(new_path_str);
             
             match fs::rename(real_old, real_new) {
                 Ok(_) => {
                     // Update InodeStore
                     self.inodes.lock().unwrap().move_inode(inode, newparent, newname_str.to_string());
                     reply.ok();
                 },
                 Err(e) => reply.error(e.raw_os_error().unwrap_or(libc::EIO)),
             }
        } else {
            reply.error(ENOENT);
        }
    }

    fn setattr(
        &mut self,
        _req: &Request,
        inode: u64,
        mode: Option<u32>,
        uid: Option<u32>,
        gid: Option<u32>,
        size: Option<u64>,
        _atime: Option<fuser::TimeOrNow>,
        _mtime: Option<fuser::TimeOrNow>,
        _ctime: Option<std::time::SystemTime>,
        _fh: Option<u64>,
        _crtime: Option<std::time::SystemTime>,
        _chgtime: Option<std::time::SystemTime>,
        _bkuptime: Option<std::time::SystemTime>,
        _flags: Option<u32>,
        reply: ReplyAttr,
    ) {
        if let Some(real_path) = self.real_path(inode) {
            // Handle chmod
            if let Some(m) = mode {
                if let Err(e) = fs::set_permissions(&real_path, fs::Permissions::from_mode(m)) {
                     reply.error(e.raw_os_error().unwrap_or(libc::EIO));
                     return;
                }
            }
            
            // Handle chown
            #[cfg(unix)]
            if uid.is_some() || gid.is_some() {
                 use std::os::unix::ffi::OsStrExt;
                 #[cfg(unix)] use libc::EIO; // Added EIO constant and guarded libc import
                 let c_path = std::ffi::CString::new(real_path.as_os_str().as_bytes()).unwrap();
                 let c_uid = uid.unwrap_or(u32::MAX); 
                 let c_gid = gid.unwrap_or(u32::MAX);
                 unsafe {
                     if libc::chown(c_path.as_ptr(), c_uid, c_gid) != 0 {
                          reply.error(EIO);
 
                          return;
                     }
                 }
            }
            #[cfg(not(unix))]
            if uid.is_some() || gid.is_some() {
                // Windows chown is complex (ACLs), skip for V1 prototype
            }

            // Handle truncate
            if let Some(s) = size {
                 if let Ok(file) = File::open(&real_path) {
                     if let Err(e) = file.set_len(s) {
                          reply.error(e.raw_os_error().unwrap_or(libc::EIO));
                          return;
                     }
                 }
            }
            
            // Handle times (utimens) - simplified, ignoring for now or using filetime if added
            // For now, we return updated attr
             match fs::metadata(&real_path) {
                Ok(metadata) => {
                    let attr = self.fs_metadata_to_file_attr(&metadata, inode);
                    reply.attr(&TTL, &attr);
                }
                Err(_) => reply.error(ENOENT),
            }

        } else {
            reply.error(ENOENT);
        }
    }

    fn write(
        &mut self,
        _req: &Request,
        inode: u64,
        _fh: u64,
        offset: i64,
        data: &[u8],
        _write_flags: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyWrite,
    ) {
        // Handle Search Write
        if inode == MAGIC_SEARCH {
            if let Ok(query) = std::str::from_utf8(data) {
                println!("[Search] Query received: {}", query.trim());
                // In V4: Trigger search, populate .magic/search_results
            }
            reply.written(data.len() as u32);
            return;
        }
        
        if let Some(real_path) = self.real_path(inode) {
            // Time Travel Logic: Snapshot before write (Copy-On-Writeish)
            // Only do this if offset == 0 or specific flags? Doing on every write is expensive.
            // For V1 PRO, we do it if file size > 0.
            // Optimization: Check DB if we already snapshotted this file in the last 5 minutes?
            
            // Simplified: Just copy to .eidetic/history/
            let history_dir = self.source_path.join(".eidetic/history");
            let _ = std::fs::create_dir_all(&history_dir);
            let timestamp = std::time::SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
            let backup_name = format!("{}_{}_{}", inode, timestamp, real_path.file_name().unwrap().to_string_lossy());
            let backup_path = history_dir.join(&backup_name);
            
            // Try copy (silently ignore failure for performance)
            if std::fs::copy(&real_path, &backup_path).is_ok() {
                let store = self.inodes.lock().unwrap();
                let _ = store.db.add_history(inode, backup_path.to_string_lossy().as_ref());
            }

            match std::fs::OpenOptions::new().write(true).open(&real_path) {
                Ok(mut file) => {
                    if file.seek(SeekFrom::Start(offset as u64)).is_ok() {
                        // Vault Logic: Encrypt on Write
                        let final_data = if real_path.to_string_lossy().contains("/vault/") {
                            crate::cipher::encrypt(data)
                        } else {
                            data.to_vec()
                        };
                        
                        // Deduplication Logic Check (Phase 9)
                        // In a real CAS, we would hash 'final_data', check DB, and if exists, point inode to blob store.
                        // Here we just simulate/log it for the prototype to avoid massive FS restructure.
                        // Ideally:
                        // let hash = sha256(&final_data);
                        // if db.has_blob(hash) { inode.set_pointer(hash); }
                        if final_data.len() > 1024 * 1024 {
                            println!("[Deduplication] Large file write detected. Hash check skipped for prototype safety.");
                        }

                        match file.write_all(&final_data) {
                            Ok(_) => reply.written(data.len() as u32),
                            Err(e) => reply.error(e.raw_os_error().unwrap_or(EIO)),
                        }
                    } else {
                        reply.error(EIO);
                    }
                },
                Err(e) => reply.error(e.raw_os_error().unwrap_or(ENOENT)),
            }
        } else {
            reply.error(ENOENT);
        }
    }

    fn create(
        &mut self,
        _req: &Request,
        parent: u64,
        name: &OsStr,
        _mode: u32,
        _umask: u32,
        _flags: i32,
        reply: fuser::ReplyCreate,
    ) {
         let name_str = name.to_string_lossy();
         let store_lock = self.inodes.lock().unwrap();
         let parent_path_opt = store_lock.get_path(parent);
         drop(store_lock);

         if let Some(parent_path) = parent_path_opt {
             let child_path_str = if parent_path.is_empty() {
                name_str.to_string()
             } else {
                format!("{}/{}", parent_path, name_str)
             };
             let real_path = self.source_path.join(&child_path_str);

             match File::create(&real_path) {
                 Ok(file) => {
                     // Get metadata
                     if let Ok(metadata) = file.metadata() {
                         let mut store = self.inodes.lock().unwrap();
                         let inode = store.alloc_inode(parent, name_str.to_string());
                         drop(store);
                         let attr = self.fs_metadata_to_file_attr(&metadata, inode);
                         reply.created(&TTL, &attr, 0, 0, 0); // Generation 0, fh 0, flags 0
                     } else {
                         reply.error(EIO);
                     }
                 }
                 Err(_) => reply.error(libc::EACCES),
             }
         } else {
             reply.error(ENOENT);
        }
    }

    fn release(
        &mut self,
        _req: &Request,
        inode: u64,
        _fh: u64,
        _flags: i32,
        _lock_owner: Option<u64>,
        _flush: bool,
        reply: fuser::ReplyEmpty,
    ) {
         if let Some(real_path) = self.real_path(inode) {
             let _ = self.sender.send(Job::Analyze { inode, path: real_path });
         }
         reply.ok();
    }
    
    // TODO: Implement mkdir, unlink, rmdir, rename, etc.
}
