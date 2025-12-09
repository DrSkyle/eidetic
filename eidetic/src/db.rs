use rusqlite::{params, Connection, Result, OptionalExtension};
use std::path::Path;
use anyhow::Context;

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn new<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let conn = Connection::open(path)?;
        
        // Optimize for performance
        conn.execute("PRAGMA journal_mode = WAL;", [])?;
        conn.execute("PRAGMA synchronous = NORMAL;", [])?;
        
        // Create tables
        conn.execute(
            "CREATE TABLE IF NOT EXISTS inodes (
                id INTEGER PRIMARY KEY,
                parent_id INTEGER,
                name TEXT NOT NULL,
                UNIQUE(parent_id, name)
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS file_tags (
                inode_id INTEGER,
                tag TEXT,
                PRIMARY KEY(inode_id, tag)
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS file_history (
                id INTEGER PRIMARY KEY,
                inode_id INTEGER,
                timestamp INTEGER,
                backup_path TEXT
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS trash (
                id INTEGER PRIMARY KEY,
                original_path TEXT,
                backup_path TEXT,
                deleted_at INTEGER
            )",
            [],
        )?;
        
        // Ensure root exists (inode 1)
        // We use INSERT OR IGNORE. 
        // Note: SQLite autoincrement usually starts at 1, but we can force it.
        conn.execute(
            "INSERT OR IGNORE INTO inodes (id, parent_id, name) VALUES (1, 1, '')",
            [],
        )?;

        Ok(Self { conn })
    }

    pub fn get_inode(&self, parent: u64, name: &str) -> Result<Option<u64>> {
        self.conn.query_row(
            "SELECT id FROM inodes WHERE parent_id = ?1 AND name = ?2",
            params![parent, name],
            |row| row.get(0),
        ).optional()
    }
    
    pub fn create_inode(&self, parent: u64, name: &str) -> Result<u64> {
        self.conn.execute(
            "INSERT INTO inodes (parent_id, name) VALUES (?1, ?2)",
            params![parent, name],
        )?;
        Ok(self.conn.last_insert_rowid() as u64)
    }

    pub fn get_inode_entry(&self, inode: u64) -> Result<Option<(u64, String)>> {
         self.conn.query_row(
            "SELECT parent_id, name FROM inodes WHERE id = ?1",
            params![inode],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).optional()
    }

    pub fn add_tag(&self, inode: u64, tag: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO file_tags (inode_id, tag) VALUES (?1, ?2)",
            params![inode, tag],
        )?;
        Ok(())
    }

    pub fn get_tags(&self) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare("SELECT DISTINCT tag FROM file_tags")?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        let mut tags = Vec::new();
        for tag in rows {
            tags.push(tag?);
        }
        Ok(tags)
    }

    pub fn get_files_with_tag(&self, tag: &str) -> Result<Vec<(u64, String)>> {
        // returning inode and name
        let mut stmt = self.conn.prepare(
            "SELECT i.id, i.name FROM inodes i JOIN file_tags t ON i.id = t.inode_id WHERE t.tag = ?1"
        )?;
        let rows = stmt.query_map(params![tag], |row| Ok((row.get(0)?, row.get(1)?)))?;
        let mut files = Vec::new();
        for file in rows {
            files.push(file?);
        }
        Ok(files)
    }

    pub fn add_history(&self, inode: u64, path: &str) -> Result<()> {
        let timestamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        self.conn.execute(
            "INSERT INTO file_history (inode_id, timestamp, backup_path) VALUES (?1, ?2, ?3)",
            params![inode, timestamp, path],
        )?;
        Ok(())
    }

    pub fn add_trash(&self, original_path: &str, backup_path: &str) -> Result<()> {
        let timestamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        self.conn.execute(
            "INSERT INTO trash (original_path, backup_path, deleted_at) VALUES (?1, ?2, ?3)",
            params![original_path, backup_path, timestamp],
        )?;
        Ok(())
    }

    pub fn delete_inode(&self, inode: u64) -> Result<()> {
        self.conn.execute("DELETE FROM inodes WHERE id = ?", params![inode])?;
        Ok(())
    }

    pub fn rename_inode(&self, inode: u64, new_parent: u64, new_name: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE inodes SET parent_id = ?1, name = ?2 WHERE id = ?3",
            params![new_parent, new_name, inode],
        )?;
        Ok(())
    }
}
