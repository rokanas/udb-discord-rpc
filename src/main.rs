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

// -- discord rpc loop --

fn run_rpc_loop(running: Arc<AtomicBool>) {
    // give udb time to open window before polling
    std::thread::sleep(Duration::from_secs(2));

    let mut client = match DiscordIpcClient::new(DISCORD_APP_ID) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[UDB-RPC] Failed to create Discord client: {}", e);
            return;
        }
    };

    if let Err(e) = client.connect() {
        eprintln!("[UDB-RPC] Could not connect to Discord (is it running?): {}", e);
        return;
    }

    println!("[UDB-RPC] Connected to Discord.");

    let start_timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let mut last_state = String::new();
    let mut sys = System::new_with_specifics(
        RefreshKind::new().with_processes(ProcessRefreshKind::everything()),
    );

    while running.load(Ordering::Relaxed) {
        sys.refresh_processes_specifics(ProcessRefreshKind::everything());

        let title = get_udb_window_title(&sys);
        let (details, state) = parse_title(&title);

        // only update discord if something changed (avoids rate limiting)
        let new_state = format!("{}|{}", details, state);
        if new_state != last_state {
            last_state = new_state;

            let activity = activity::Activity::new()
                .details(&details)
                .state(&state)
                .timestamps(activity::Timestamps::new().start(start_timestamp))
                .assets(
                    activity::Assets::new()
                        .large_image("udb_logo")
                        .large_text("Ultimate Doom Builder")
                        .small_image("doom_icon")
                        .small_text("Mapping"),
                );

            if let Err(e) = client.set_activity(activity) {
                eprintln!("[UDB-RPC] Failed to set activity: {}", e);
                let _ = client.reconnect();
            } else {
                println!("[UDB-RPC] Updated presence → {} | {}", details, state);
            }
        }

        std::thread::sleep(Duration::from_millis(POLL_RATE_MS));
    }

    let _ = client.clear_activity();
    let _ = client.close();
    println!("[UDB-RPC] Discord RPC disconnected.");
}