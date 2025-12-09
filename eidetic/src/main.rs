use clap::{Parser, Subcommand};
use fuser::MountOption;
use std::path::PathBuf;
use anyhow::{Context, Result};
use std::io::{self, Write};

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
    /// Mount the Eidetic filesystem (default)
    Mount {
        /// Path to the source directory to mirror
        #[arg(short, long, default_value = "./source_data")]
        source: PathBuf,

        /// Path to the mount point
        #[arg(short, long, default_value = "./mount_point")]
        mountpoint: PathBuf,
    },
}

fn main() -> Result<()> {
    env_logger::init();
    
    // LICENSE CHECK
    println!("Verifying license...");
    match license::check_license_status() {
        Ok(true) => {
            println!("License verified. Starting Eidetic...");
        }
        _ => {
            println!("No active license found.");
            println!("Get your license here: https://checkout.freemius.com/app/22217/plan/37168/"); 
            print!("Please enter your License Key: ");
            io::stdout().flush().unwrap();
            
            let mut key = String::new();
            io::stdin().read_line(&mut key).unwrap();
            let key = key.trim().to_string();
            
            match license::activate_license(key) {
                Ok(_) => println!("License activated successfully!"),
                Err(e) => {
                    eprintln!("Failed to activate license: {}", e);
                    std::process::exit(1);
                }
            }
        }
    }
    
    let cli = Cli::parse();

    match cli.command {
        Commands::Mount { source, mountpoint } => {
            // Ensure source exists
            if !source.exists() {
                std::fs::create_dir_all(&source).context("Failed to create source directory")?;
            }
            
            // Ensure mountpoint exists (fuser needs it)
            if !mountpoint.exists() {
                std::fs::create_dir_all(&mountpoint).context("Failed to create mountpoint")?;
            }

            println!("Starting EideticFS...");
            println!("  Source: {:?}", source);
            println!("  Mount:  {:?}", mountpoint);
            println!("\n  (Press Ctrl+C to unmount)");


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
        }
    }

    Ok(())
}
