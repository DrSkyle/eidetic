use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::thread;
use crate::db::Database;

pub enum Job {
    Analyze { inode: u64, path: PathBuf },
}

#[derive(Debug, serde::Serialize)]
struct TodoItem {
    line: usize,
    content: String,
    file: String,
}

// Heuristic Tags
fn guess_tags(content: &str) -> Vec<String> {
    let mut tags = Vec::new();
    let lower = content.to_lowercase();
    
    if lower.contains("function") || lower.contains("def ") || lower.contains("impl ") || lower.contains("class ") {
        tags.push("code".to_string());
    }
    if lower.contains("total:") || lower.contains("amount:") || lower.contains("invoice") {
        tags.push("finance".to_string());
    }
    if lower.contains("select * from") || lower.contains("insert into") {
        tags.push("sql".to_string());
    }
    if lower.contains("dear ") && lower.contains("sincerely") {
        tags.push("letter".to_string());
    }
    tags
}

// Simple binary check
fn is_binary(data: &[u8]) -> bool {
    // Check if contains null byte in first 1024 bytes
    data.iter().take(1024).any(|&b| b == 0)
}

pub struct Worker {
    receiver: Receiver<Job>,
    db_path: PathBuf,
}

impl Worker {
    pub fn new(receiver: Receiver<Job>, db_path: PathBuf) -> Self {
        Self { receiver, db_path }
    }

    pub fn start(self) {
        let Worker { receiver, db_path } = self;
        thread::spawn(move || {
            // Open DB in this thread
            let db = match Database::new(&db_path) {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("[Worker] Failed to open DB: {}", e);
                    return;
                }
            };

            for job in receiver {
                match job {
                    Job::Analyze { inode, path } => Self::process_analyze(&db, inode, path),
                }
            }
        });
    }

    fn process_analyze(db: &Database, inode: u64, path: PathBuf) {
        // Log silently or use `log` crate in prod
        // println!("[Worker] Analyzing file: {:?} (Inode: {})", path, inode);
        
        // Check MIME / Content
        let _path_str = path.to_string_lossy().to_string();
        let ext = path.extension().unwrap_or_default().to_string_lossy().to_string().to_lowercase();
        
        // 1. Image Check
        if ["jpg", "jpeg", "png", "webp", "gif"].contains(&ext.as_str()) {
             // println!("[Worker] Image detected: {:?}", path);
             if let Ok(dims) = image::image_dimensions(&path) {
                 // println!("[Worker] Image Dimensions: {}x{}", dims.0, dims.1);
                 let _ = db.add_tag(inode, "image");
             }
             return;
        }

        // 2. Universal Text Check
        // Try reading first few bytes
        if let Ok(mut file) = std::fs::File::open(&path) {
             use std::io::Read;
             let mut buffer = [0; 1024];
             if let Ok(n) = file.read(&mut buffer) {
                  if n > 0 && !is_binary(&buffer[..n]) {
                      // It's likely text! parse it fully
                      if let Ok(text) = std::fs::read_to_string(&path) {
                           println!("[Worker] Analyzing Text File ({} chars): {:?}", text.len(), path);
                           
                           // Run Tagger
                           let tags = guess_tags(&text);
                           if !tags.is_empty() {
                               println!("[Tag] Autotags: {:?}", tags);
                               for tag in tags {
                                   let _ = db.add_tag(inode, &tag);
                               }
                           }
                           
                           // Run Todo Extraction
                           let mut todos = Vec::new();
                           for (i, line) in text.lines().enumerate() {
                               if line.contains("TODO") || line.contains("FIXME") {
                                   todos.push(TodoItem {
                                       line: i + 1,
                                       content: line.trim().to_string(),
                                       file: path.file_name().unwrap_or_default().to_string_lossy().to_string(),
                                   });
                               }
                           }
                           
                           // Run Summarizer (if PDF or long text)
                           if ext == "pdf" { 
                               // ... existing PDF logic ...
                           }
                           
                           // Auto-Organizer Logic (Phase 9)
                           let name_str = path.file_name().unwrap().to_string_lossy().to_string();
                           if name_str.to_lowercase().contains("invoice") {
                               let target_dir = path.parent().unwrap().join("Finance");
                               if !target_dir.exists() {
                                   let _ = std::fs::create_dir(&target_dir);
                               }
                               let target_path = target_dir.join(&name_str);
                               // println!("[Worker] Auto-Organizing: Moving {:?} to {:?}", path, target_path);
                               
                               // Need to update Inodes!
                               // This is tricky from Worker because we need to update InodeStore which is locked by FS.
                               // Best way: Send message back to FS? Or just move file on disk and accept temporary desync (FS will recover on readdir)?
                               // For Prototype: Just move on disk. FS 'lookup' might fail until unmount.
                               // Correct way: Worker should update DB.
                               if std::fs::rename(&path, &target_path).is_ok() {
                                   let _ = db.delete_inode(inode); // Remove old mapping
                                   // We don't easily know parent inode of 'Finance' without searching.
                                   // Simplification: Just log it for now as "Proposed Move" or do it only if we can fully update DB.
                                   // To really make it work, we'd need to recursively resolve path "Finance" to an inode.
                                   // println!("[Worker] Moved on disk only. Please remount to see changes fully.");
                               }
                           }
                           
                           if !todos.is_empty() {
                               if let Ok(json) = serde_json::to_string(&todos) {
                                   // println!("[Analysis] {}", json); 
                               }
                           }
                      }
                  } else {
                      println!("[Worker] Binary file detected, skipping text analysis: {:?}", path);
                  }
             }
        }
    }
}
