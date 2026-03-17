/* 
produces proxy launcher for udb (to be run from the same directory as original udb exe) that:
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

const DISCORD_APP_ID: &str = "placeholder";
const UDB_EXE_ORIGINAL: &str = "Builder.exe";
const POLL_RATE_MS: u64 = 2000;

// -- entry point --

fn main() {
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

    let udb_pid = udb_process.id();

    let running = Arc::new(AtomicBool::new(true));
    let running_rpc = Arc::clone(&running);

    let rpc_thread = std::thread::spawn(move || {
        run_rpc_loop(running_rpc, udb_pid);
    });

    let _ = udb_process.wait();
    println!("[UDB-RPC] UDB closed. Cleaning up...");

    running.store(false, Ordering::Relaxed);
    let _ = rpc_thread.join();

    println!("[UDB-RPC] Done.");
}

// -- launch udb --

fn launch_udb(extra_args: &[String]) -> std::io::Result<Child> {
    Command::new(UDB_EXE_ORIGINAL).args(extra_args).spawn()
}

// -- discord rpc loop --

fn run_rpc_loop(running: Arc<AtomicBool>, udb_pid: u32) {
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

        let title = get_udb_window_title(udb_pid);
        println!("[UDB-RPC] Raw title: '{}'", title);
        let (details, state) = parse_title(&title);

        let new_state = format!("{}|{}", details, state);
        if new_state != last_state {
            last_state = new_state;

            let mut act = activity::Activity::new()
                .details(&details)
                .timestamps(activity::Timestamps::new().start(start_timestamp))
                .assets(
                    activity::Assets::new()
                        .large_image("udb_logo")
                        .large_text("Ultimate Doom Builder")
                        .small_image("doom_icon")
                        .small_text("Mapping"),
                );

            if !state.is_empty() {
                act = act.state(&state);
            }

            if let Err(e) = client.set_activity(act) {
                eprintln!("[UDB-RPC] Failed to set activity: {}", e);
                let _ = client.reconnect();
            } else {
                println!("[UDB-RPC] Updated presence -> {} | {}", details, state);
            }
        }

        std::thread::sleep(Duration::from_millis(POLL_RATE_MS));
    }

    let _ = client.clear_activity();
    let _ = client.close();
    println!("[UDB-RPC] Discord RPC disconnected.");
}

// -- detect window title by pid --

fn get_udb_window_title(udb_pid: u32) -> String {
    #[cfg(windows)]
    {
        return unsafe { find_window_title_by_pid(udb_pid) };
    }

    #[cfg(not(windows))]
    {
        let _ = udb_pid;
        String::new()
    }
}

#[cfg(windows)]
unsafe fn find_window_title_by_pid(target_pid: u32) -> String {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::sync::Mutex;
    use winapi::shared::minwindef::{BOOL, DWORD, LPARAM};
    use winapi::shared::windef::HWND;
    use winapi::um::winuser::{EnumWindows, GetWindowTextW, GetWindowThreadProcessId};

    static RESULT: std::sync::OnceLock<Mutex<Option<String>>> = std::sync::OnceLock::new();
    let mutex = RESULT.get_or_init(|| Mutex::new(None));
    {
        let mut guard = mutex.lock().unwrap();
        *guard = None;
    }

    unsafe extern "system" fn enum_callback(hwnd: HWND, target_pid: LPARAM) -> BOOL {
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStringExt;
        use winapi::shared::minwindef::DWORD;
        use winapi::um::winuser::{GetWindowTextW, GetWindowThreadProcessId};

        let mut window_pid: DWORD = 0;
        GetWindowThreadProcessId(hwnd, &mut window_pid);
        if window_pid != target_pid as DWORD {
            return 1;
        }

        let mut buf = vec![0u16; 512];
        let len = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
        if len > 0 {
            buf.truncate(len as usize);
            let title = OsString::from_wide(&buf).to_string_lossy().into_owned();
            if !title.is_empty() {
                if let Some(mutex) = RESULT.get() {
                    if let Ok(mut guard) = mutex.lock() {
                        let is_udb_title = title.contains(" - Ultimate Doom Builder");
                        let current_is_udb = guard.as_ref().map_or(false, |t| t.contains(" - Ultimate Doom Builder"));
                        if is_udb_title || (!current_is_udb && guard.is_none()) {
                            *guard = Some(title);
                        }
                    }
                }
            }
        }
        1
    }

    EnumWindows(Some(enum_callback), target_pid as LPARAM);

    let guard = mutex.lock().unwrap();
    guard.clone().unwrap_or_default()
}

// -- parse title --

/* title format (as of udb v3.0.0.4305)
    "doomwad.wad (MAP01: Mapname) - Ultimate Doom Builder R4305 (64-bit)"  -> map + file open
    "doomwad.wad - Ultimate Doom Builder R4305 (64-bit)"                   -> file open, no map
    "Ultimate Doom Builder R4305 (64-bit)"                                 -> startup / no file

   discord output:
    details -> "Editing MAP01: Mapname"
    state   -> "doomwad.wad"
*/
fn parse_title(title: &str) -> (String, String) {
    // if no title found or not a udb window (e.g. .NETBroadcastEventWIndow)
    if title.is_empty() || !title.contains("Ultimate Doom Builder") {
        return ("Idle".to_string(), String::new());
    }

    // collect title preceding '- udb' (if present)
    let stripped = if let Some(pos) = title.find(" - Ultimate Doom Builder") {
        title[..pos].trim()
    } else {
        // if no '-' seperator, just udb title
        return ("Starting up...".to_string(), String::new());
    };

    // defensive check in case trim left an empty string
    if stripped.is_empty() {
        return ("Starting up...".to_string(), String::new());
    }

    // if a '(' is found, there is full title, so split on the '('
    // everything before it is the filename
    // everything after is the mapname
    if let Some(paren_pos) = stripped.find(" (") {
        let file_name = stripped[..paren_pos].trim().trim_end_matches('*').trim();
        let map_name = stripped[paren_pos..].trim().trim_start_matches('(').trim_end_matches(')');
        (format!("Editing {}", map_name), file_name.to_string())
    } else {
        // no '(' found indicates wad file is open but no map loaded)
        let file_name = stripped.trim_end_matches('*').trim();
        (format!("Editing {}", file_name), String::new())
    }
}
