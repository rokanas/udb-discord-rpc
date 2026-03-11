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