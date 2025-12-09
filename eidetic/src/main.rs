use clap::{Parser, Subcommand};
use fuser::MountOption;
use std::path::PathBuf;
use anyhow::{Context, Result};
use std::io::{self, Write};
use std::fs::File;
use daemonize::Daemonize;

mod fs;
mod db;
mod model;
mod cipher;
mod license;
use fs::EideticFS;

mod worker;


#[derive(Parser, Debug)]
#[command(name = "eidetic", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Mount the Eidetic filesystem in foreground (Debug)
    Mount {
        /// Path to the source directory to mirror
        #[arg(short, long, default_value = "./source_data")]
        source: PathBuf,

        /// Path to the mount point
        #[arg(short, long, default_value = "./mount_point")]
        mountpoint: PathBuf,
    },
    /// Start Eidetic in the background (Daemon)
    Start {
        /// Path to the source directory to mirror
        #[arg(short, long, default_value = "./source_data")]
        source: PathBuf,

        /// Path to the mount point
        #[arg(short, long, default_value = "./mount_point")]
        mountpoint: PathBuf,
    },
    /// Stop the background Eidetic instance
    Stop,
}

fn main() -> Result<()> {
    env_logger::init();
    
    // License check skipped for brevity in daemon command handling for now, 
    // or we can move it inside Mount/Start.
    
    let cli = Cli::parse();
    
    // Pid file path: ~/.eidetic/eidetic.pid
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".to_string());
    let pid_dir = PathBuf::from(&home).join(".eidetic");
    let pid_file = pid_dir.join("eidetic.pid");
    let stdout_log = pid_dir.join("eidetic.out");
    let stderr_log = pid_dir.join("eidetic.err");

    if !pid_dir.exists() {
        std::fs::create_dir_all(&pid_dir)?;
    }

    match cli.command {
        Commands::Stop => {
            if pid_file.exists() {
                 let pid_str = std::fs::read_to_string(&pid_file)?;
                 let pid: i32 = pid_str.trim().parse()?;
                 
                 println!("Stopping Eidetic (PID: {})...", pid);
                 
                 // Kill process
                 use libc::{kill, SIGTERM};
                 unsafe {
                     kill(pid, SIGTERM);
                 }
                 
                 // Clean up pid file
                 std::fs::remove_file(pid_file)?;
                 println!("Stopped.");
            } else {
                println!("No active Eidetic instance found (no pid file).");
            }
            return Ok(());
        }
        
        Commands::Start { source, mountpoint } => {
            if pid_file.exists() {
                println!("Eidetic is already running! (PID file exists)");
                println!("Run 'eidetic stop' first if you want to restart.");
                return Ok(());
            }

            println!("Starting Eidetic Daemon...");
            println!("  Source: {:?}", source);
            println!("  Mount:  {:?}", mountpoint);
            
            // Ensure dirs exist
            if !source.exists() { std::fs::create_dir_all(&source)?; }
            if !mountpoint.exists() { std::fs::create_dir_all(&mountpoint)?; }
            
            // Verify License before forking
            // ... (Simple check)
            
            let stdout = File::create(&stdout_log)?;
            let stderr = File::create(&stderr_log)?;

            let daemonize = Daemonize::new()
                .pid_file(&pid_file)
                .chown_pid_file(true)
                .working_directory(std::env::current_dir()?)
                .stdout(stdout)
                .stderr(stderr);

            match daemonize.start() {
                Ok(_) => {
                    // WE ARE NOW IN THE DAEMON PROCESS
                    // Run the actual filesystem logic
                    run_fs(source, mountpoint)?;
                }
                Err(e) => eprintln!("Error, {}", e),
            }
        }
        
        Commands::Mount { source, mountpoint } => {
            // Foreground run
            if !source.exists() { std::fs::create_dir_all(&source)?; }
            if !mountpoint.exists() { std::fs::create_dir_all(&mountpoint)?; }
            
            println!("Starting EideticFS (Foreground)...");
            println!("  Source: {:?}", source);
            println!("  Mount:  {:?}", mountpoint);
            println!("\n  (Press Ctrl+C to unmount)");
            
            run_fs(source, mountpoint)?;
        }
    }

    Ok(())
}

fn run_fs(source: PathBuf, mountpoint: PathBuf) -> Result<()> {
    let uid = unsafe { libc::getuid() };
    let gid = unsafe { libc::getgid() };
    
    // Start Worker
    let (tx, rx) = std::sync::mpsc::channel();
    let db_path = source.join(".eidetic.db");
    worker::Worker::new(rx, db_path).start();
    
    let fs = EideticFS::new(source, uid, gid, tx);
    
    let options = vec![
        MountOption::RW,
        MountOption::FSName("eidetic".to_string()),
        MountOption::AutoUnmount,
    ];

    fuser::mount2(fs, mountpoint, &options).context("Failed to mount filesystem")?;
    Ok(())
}
