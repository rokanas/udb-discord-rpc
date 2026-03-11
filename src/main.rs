use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use sysinfo::{ProcessRefreshKind, RefreshKind, System};

// -- configuration --

// discord app id
const DISCORD_APP_ID: &str = "example";

/// udb executable to launch
const UDB_EXE_ORIGINAL: &str = "Builder.exe";

/// rate of polling (ms) window title for changes.
const POLL_RATE_MS: u64 = 2000;

// -- entry point --

fn main() {
    //dotenvy::dotenv().expect(".env file not found");

    // pass through any command-line args to udb
    let args: Vec<String> = std::env::args().skip(1).collect();

    println!("[UDB-RPC] Launching {}...", UDB_EXE_ORIGINAL);

    let mut udb_process = match launch_udb(&args) {
        Ok(child) => child,
        Err(e) => {
            eprintln!("[UDB-RPC] Failed to launch UDB: {}", e);
            eprintln!("[UDB-RPC] Make sure '{}' is in the same folder.", UDB_EXE_ORIGINAL);
            std::thread::sleep(Duration::from_secs(5));
            return;
        }
    };

    // signal flag — set to false when udb exits so rpc thread can stop
    let running = Arc::new(AtomicBool::new(true));
    let running_rpc = Arc::clone(&running);

    // spawn discord rpc updater on background thread
    let rpc_thread = std::thread::spawn(move || {
        run_rpc_loop(running_rpc);
    });

    // wait for udb to exit
    let _ = udb_process.wait();
    println!("[UDB-RPC] UDB closed. Cleaning up...");

    // signal rpc thread to stop and wait for it
    running.store(false, Ordering::Relaxed);
    let _ = rpc_thread.join();

    println!("[UDB-RPC] Done.");
}

// -- launch udb --

fn launch_udb(extra_args: &[String]) -> std::io::Result<Child> {
    Command::new(UDB_EXE_ORIGINAL).args(extra_args).spawn()
}