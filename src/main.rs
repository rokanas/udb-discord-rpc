/* 
produces a proxy launcher for udb (to be run from the same directory as original udb exe) that:
  1 - launches original udb executable
  2 - monitors window title for current map/file names
  3 - updates discord rpc
  4 - exits cleanly when udb closes 
*/

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
        println!("[UDB-RPC] Raw title: '{}'", title);
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

// -- detect window titles --

/// on windows: enumerate all top level windows to find udb window title
/// on other platforms: fall back to process name detection
fn get_udb_window_title(_sys: &System) -> String {
    #[cfg(windows)]
    {
        return unsafe { find_udb_window_title() };
    }

    #[cfg(not(windows))]
    {
        // non-windows fallback: if udb process running, return generic string
        for (_pid, process) in _sys.processes() {
            let name = process.name().to_lowercase();
            if name.contains("ultimatedoombuilder") {
                return "Ultimate Doom Builder".to_string();
            }
        }
        String::new()
    }
}

#[cfg(windows)]
unsafe fn find_udb_window_title() -> String {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::sync::Mutex;
    use winapi::shared::minwindef::{BOOL, LPARAM};
    use winapi::shared::windef::HWND;
    use winapi::um::winuser::{EnumWindows, GetWindowTextW};

    // use thread-local to collect result from the callback
    static RESULT: std::sync::OnceLock<Mutex<Option<String>>> = std::sync::OnceLock::new();
    let mutex = RESULT.get_or_init(|| Mutex::new(None));
    {
        let mut guard = mutex.lock().unwrap();
        *guard = None;
    }

    unsafe extern "system" fn enum_callback(hwnd: HWND, _lparam: LPARAM) -> BOOL {
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStringExt;
        use winapi::um::winuser::GetWindowTextW;

        let mut buf = vec![0u16; 512];
        let len = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
        if len > 0 {
            buf.truncate(len as usize);
            let title = OsString::from_wide(&buf).to_string_lossy().into_owned();
            if title.contains("Ultimate Doom Builder") && !title.ends_with(".exe"){ // filter out additional console window (for testing)
                if let Some(mutex) = RESULT.get() {
                    if let Ok(mut guard) = mutex.lock() {
                        *guard = Some(title);
                    }
                }
                return 0; // stop enumeration
            }
        }
        1 // continue
    }

    EnumWindows(Some(enum_callback), 0);

    let guard = mutex.lock().unwrap();
    guard.clone().unwrap_or_default()
}

// -- parse title --

/* title format (as of udb v3.0.0.4305)
    "doomwad.wad (MAP01: Mapname) - Ultimate Doom Builder"  -> map + file open
    "doomwad.wad - Ultimate Doom Builder"                   -> file open, no map
    "Ultimate Doom Builder"                                 -> startup / no file
*/
fn parse_title(title: &str) -> (String, String) {
    if title.is_empty() || !title.contains("Ultimate Doom Builder") {
        return (
            "Ultimate Doom Builder".to_string(),
            "Idle".to_string(),
        );
    }

    // strip trailing " - Ultimate Doom Builder" suffix
    let stripped = if let Some(pos) = title.rfind(" - Ultimate Doom Builder") {
        title[..pos].trim()
    } else {
        // bare "Ultimate Doom Builder"
        return (
            "Ultimate Doom Builder".to_string(),
            "Starting up...".to_string(),
        );
    };

    if stripped.is_empty() {
        return (
            "Ultimate Doom Builder".to_string(),
            "Starting up...".to_string(),
        );
    }

    // split on first " - " to separate map name from filename
    let parts: Vec<&str> = stripped.splitn(2, " - ").collect();

    match parts.as_slice() {
        [map, file] => {
            let map_name = map.trim();
            let unsaved = file.trim().ends_with('*');
            let file_name = file.trim().trim_end_matches('*').trim();
            let details = format!("Editing {}", map_name);
            let state = if unsaved {
                format!("{} (unsaved changes)", file_name)
            } else {
                format!("in {}", file_name)
            };
            (details, state)
        }
        [file] => {
            let unsaved = file.trim().ends_with('*');
            let file_name = file.trim().trim_end_matches('*').trim();
            let state = if unsaved {
                format!("{} (unsaved changes)", file_name)
            } else {
                format!("Editing {}", file_name)
            };
            ("Ultimate Doom Builder".to_string(), state)
        }
        _ => (
            "Ultimate Doom Builder".to_string(),
            stripped.to_string(),
        ),
    }
}