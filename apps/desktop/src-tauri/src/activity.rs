//! Codex activity tracking backed by lifecycle hooks.
//!
//! The hook process never persists prompts, transcript paths, tool input, or
//! assistant output. Only hashed session/turn identifiers and coarse activity
//! state are written to `activity.json`.

use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// Waiting entries that receive no lifecycle event for this long are stale.
/// A long TTL deliberately preserves a task that is waiting over a weekend.
pub const DEFAULT_ACTIVITY_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Executing turns need a shorter fallback because Codex does not guarantee a
/// Stop hook when a turn is interrupted or its worker is terminated.
const EXECUTING_ACTIVITY_TTL: Duration = Duration::from_secs(5 * 60);

const STATE_VERSION: u32 = 2;
const LOCK_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
enum ActivityStatus {
    Executing,
    WaitingOnApproval,
    WaitingOnUserInput,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct ActivityEntry {
    session_hash: String,
    turn_hash: String,
    status: ActivityStatus,
    updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    host_pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    host_started_at_ms: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ActivityState {
    version: u32,
    updated_at_ms: u64,
    entries: Vec<ActivityEntry>,
}

impl Default for ActivityState {
    fn default() -> Self {
        Self {
            version: STATE_VERSION,
            updated_at_ms: 0,
            entries: Vec::new(),
        }
    }
}

/// Aggregated task counts safe to upload to the synchronization server.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivitySummary {
    pub total: u32,
    pub executing: u32,
    pub waiting_on_approval: u32,
    pub waiting_on_user_input: u32,
    pub state_updated_at_ms: u64,
    pub observed_at_ms: u64,
}

#[derive(Debug, Deserialize)]
struct HookInput {
    session_id: Option<String>,
    turn_id: Option<String>,
    hook_event_name: String,
    tool_name: Option<String>,
    #[serde(default, alias = "process_id", alias = "codex_pid")]
    host_pid: Option<u32>,
    #[serde(
        default,
        alias = "process_started_at_ms",
        alias = "codex_started_at_ms"
    )]
    host_started_at_ms: Option<u64>,
}

/// Returns the activity file shared by the hook process and the desktop app.
///
/// `CODEX_QUOTA_SYNC_ACTIVITY_PATH` is primarily useful for portable builds and
/// tests. The normal Windows location is
/// `%APPDATA%\CodexQuotaSync\activity.json`.
pub fn default_activity_path() -> PathBuf {
    if let Some(path) = std::env::var_os("CODEX_QUOTA_SYNC_ACTIVITY_PATH") {
        if !path.is_empty() {
            return PathBuf::from(path);
        }
    }
    dirs::config_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("CodexQuotaSync")
        .join("activity.json")
}

/// Handles one Codex hook invocation from stdin.
///
/// The empty JSON object printed on success is accepted by every configured
/// event and is required by `Stop`, which does not accept plain-text output.
pub fn run_activity_hook() -> Result<(), String> {
    let path = default_activity_path();
    let result = process_hook_reader(&path, std::io::stdin().lock(), DEFAULT_ACTIVITY_TTL);
    // Stop requires JSON on stdout when the command exits successfully. Emit a
    // neutral response even if local telemetry could not be updated; the main
    // entry point can log `result` while still letting Codex continue.
    println!("{{}}");
    result
}

/// Parses and applies a single hook object without retaining ignored fields.
/// Serde streams over prompt/transcript/tool payloads instead of copying them
/// into the state representation.
pub fn process_hook_reader<R: Read>(path: &Path, reader: R, ttl: Duration) -> Result<(), String> {
    let input: HookInput = serde_json::from_reader(BufReader::new(reader))
        .map_err(|error| format!("invalid Codex hook input: {error}"))?;
    apply_hook(path, &input, now_ms(), ttl)
}

/// Reads, repairs, and aggregates activity from the default file.
pub fn read_activity_summary() -> Result<ActivitySummary, String> {
    read_activity_summary_at(&default_activity_path(), DEFAULT_ACTIVITY_TTL)
}

/// Reads, repairs, and aggregates activity from an explicit file.
pub fn read_activity_summary_at(path: &Path, ttl: Duration) -> Result<ActivitySummary, String> {
    with_activity_lock(path, || {
        if !path.is_file() {
            return Err("activity state is unavailable; no Codex hook has written it yet".into());
        }
        let now = now_ms();
        let (mut state, load_needs_repair) = load_state(path);
        let pruned = prune_stale_entries(&mut state, now, ttl);
        if load_needs_repair || pruned {
            state.updated_at_ms = now;
            persist_state(path, &state)?;
        }
        Ok(summarize(&state, now))
    })
}

fn apply_hook(path: &Path, input: &HookInput, now: u64, ttl: Duration) -> Result<(), String> {
    let Some(session_id) = input
        .session_id
        .as_deref()
        .filter(|value| !value.is_empty())
    else {
        // A malformed or future event shape must never block Codex itself.
        return Ok(());
    };
    let session_hash = hash_identifier(session_id);
    with_activity_lock(path, || {
        let (mut state, _) = load_state(path);
        let pruned = prune_stale_entries(&mut state, now, ttl);

        let event = input.hook_event_name.as_str();
        let changed = match event {
            "SessionStart" => remove_session(&mut state, &session_hash),
            "Stop" => {
                if let Some(turn_id) = input.turn_id.as_deref().filter(|value| !value.is_empty()) {
                    remove_turn(&mut state, &session_hash, &hash_identifier(turn_id))
                } else {
                    remove_session(&mut state, &session_hash)
                }
            }
            "UserPromptSubmit" => upsert_from_input(
                &mut state,
                input,
                &session_hash,
                ActivityStatus::Executing,
                now,
            ),
            "PermissionRequest" => upsert_from_input(
                &mut state,
                input,
                &session_hash,
                ActivityStatus::WaitingOnApproval,
                now,
            ),
            "PreToolUse" if is_request_user_input(input.tool_name.as_deref()) => upsert_from_input(
                &mut state,
                input,
                &session_hash,
                ActivityStatus::WaitingOnUserInput,
                now,
            ),
            // A completed tool means a permission prompt (or request_user_input)
            // has resolved and the turn is executing again.
            "PostToolUse" => upsert_from_input(
                &mut state,
                input,
                &session_hash,
                ActivityStatus::Executing,
                now,
            ),
            _ => false,
        };

        if changed || pruned || event == "SessionStart" {
            state.updated_at_ms = now;
            persist_state(path, &state)?;
        }
        Ok(())
    })
}

fn upsert_from_input(
    state: &mut ActivityState,
    input: &HookInput,
    session_hash: &str,
    status: ActivityStatus,
    now: u64,
) -> bool {
    let Some(turn_id) = input.turn_id.as_deref().filter(|value| !value.is_empty()) else {
        return false;
    };
    let turn_hash = hash_identifier(turn_id);
    let explicit_host_pid = input
        .host_pid
        .or_else(|| env_u32("CODEX_HOST_PID"))
        .or_else(|| env_u32("CODEX_PARENT_PID"));
    let explicit_host_started_at_ms = input
        .host_started_at_ms
        .or_else(|| env_u64("CODEX_HOST_STARTED_AT_MS"));
    let discovered_host = discover_codex_host();
    let host_pid = explicit_host_pid.or(discovered_host.map(|host| host.0));
    let host_started_at_ms = explicit_host_started_at_ms.or_else(|| {
        discovered_host.and_then(|host| (Some(host.0) == host_pid).then_some(host.1).flatten())
    });

    // A Codex session can execute only one turn at a time. If Stop was skipped,
    // the next turn is authoritative and replaces the residual turn.
    state
        .entries
        .retain(|entry| entry.session_hash != session_hash || entry.turn_hash == turn_hash);
    if let Some(entry) = state
        .entries
        .iter_mut()
        .find(|entry| entry.session_hash == session_hash && entry.turn_hash == turn_hash)
    {
        entry.status = status;
        entry.updated_at_ms = now;
        if host_pid.is_some() {
            entry.host_pid = host_pid;
        }
        if host_started_at_ms.is_some() {
            entry.host_started_at_ms = host_started_at_ms;
        }
    } else {
        state.entries.push(ActivityEntry {
            session_hash: session_hash.to_owned(),
            turn_hash,
            status,
            updated_at_ms: now,
            host_pid,
            host_started_at_ms,
        });
    }
    true
}

fn remove_session(state: &mut ActivityState, session_hash: &str) -> bool {
    let before = state.entries.len();
    state
        .entries
        .retain(|entry| entry.session_hash != session_hash);
    state.entries.len() != before
}

fn remove_turn(state: &mut ActivityState, session_hash: &str, turn_hash: &str) -> bool {
    let before = state.entries.len();
    state
        .entries
        .retain(|entry| entry.session_hash != session_hash || entry.turn_hash != turn_hash);
    state.entries.len() != before
}

fn summarize(state: &ActivityState, observed_at_ms: u64) -> ActivitySummary {
    let mut summary = ActivitySummary {
        state_updated_at_ms: state.updated_at_ms,
        observed_at_ms,
        ..ActivitySummary::default()
    };
    for entry in &state.entries {
        summary.total = summary.total.saturating_add(1);
        match entry.status {
            ActivityStatus::Executing => summary.executing = summary.executing.saturating_add(1),
            ActivityStatus::WaitingOnApproval => {
                summary.waiting_on_approval = summary.waiting_on_approval.saturating_add(1)
            }
            ActivityStatus::WaitingOnUserInput => {
                summary.waiting_on_user_input = summary.waiting_on_user_input.saturating_add(1)
            }
        }
    }
    summary
}

fn prune_stale_entries(state: &mut ActivityState, now: u64, ttl: Duration) -> bool {
    let waiting_ttl_ms = duration_ms(ttl);
    let executing_ttl_ms = duration_ms(ttl.min(EXECUTING_ACTIVITY_TTL));
    let before = state.entries.len();
    state.entries.retain(|entry| {
        let entry_ttl_ms = match entry.status {
            ActivityStatus::Executing => executing_ttl_ms,
            ActivityStatus::WaitingOnApproval | ActivityStatus::WaitingOnUserInput => {
                waiting_ttl_ms
            }
        };
        let within_ttl = now.saturating_sub(entry.updated_at_ms) <= entry_ttl_ms;
        let host_is_alive = entry
            .host_pid
            .map_or(true, |pid| process_matches(pid, entry.host_started_at_ms));
        within_ttl && host_is_alive
    });
    state.entries.len() != before
}

fn duration_ms(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

fn load_state(path: &Path) -> (ActivityState, bool) {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return (ActivityState::default(), false)
        }
        Err(_) => return (ActivityState::default(), true),
    };
    match serde_json::from_reader::<_, ActivityState>(BufReader::new(file)) {
        Ok(mut state) => {
            let needs_repair = state.version != STATE_VERSION;
            if state.version < 2 {
                // V1 usually had no host identity, so those entries cannot be
                // distinguished from turns abandoned before this upgrade.
                state.entries.retain(|entry| entry.host_pid.is_some());
            }
            state.version = STATE_VERSION;
            (state, needs_repair)
        }
        Err(_) => (ActivityState::default(), true),
    }
}

fn persist_state(path: &Path, state: &ActivityState) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "activity path has no parent directory".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("failed to create activity directory: {error}"))?;

    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("activity.json");
    let temporary = parent.join(format!(
        ".{file_name}.{}.{}.tmp",
        std::process::id(),
        now_ms()
    ));
    let result = (|| {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| format!("failed to create activity temporary file: {error}"))?;
        let mut writer = BufWriter::new(file);
        serde_json::to_writer_pretty(&mut writer, state)
            .map_err(|error| format!("failed to serialize activity: {error}"))?;
        writer
            .write_all(b"\n")
            .and_then(|_| writer.flush())
            .map_err(|error| format!("failed to write activity: {error}"))?;
        writer
            .get_ref()
            .sync_all()
            .map_err(|error| format!("failed to flush activity: {error}"))?;
        atomic_replace(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(windows)]
fn atomic_replace(from: &Path, to: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;

    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;
    let from_wide: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to_wide: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    let succeeded = unsafe {
        windows_api::MoveFileExW(
            from_wide.as_ptr(),
            to_wide.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if succeeded == 0 {
        Err(format!(
            "failed to atomically replace activity: {}",
            std::io::Error::last_os_error()
        ))
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn atomic_replace(from: &Path, to: &Path) -> Result<(), String> {
    fs::rename(from, to).map_err(|error| format!("failed to replace activity: {error}"))?;
    if let Some(parent) = to.parent() {
        if let Ok(directory) = File::open(parent) {
            let _ = directory.sync_all();
        }
    }
    Ok(())
}

fn is_request_user_input(tool_name: Option<&str>) -> bool {
    let normalized: String = tool_name
        .unwrap_or_default()
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    normalized.ends_with("requestuserinput")
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

fn env_u32(name: &str) -> Option<u32> {
    std::env::var(name).ok()?.parse().ok()
}

fn env_u64(name: &str) -> Option<u64> {
    std::env::var(name).ok()?.parse().ok()
}

fn hash_identifier(value: &str) -> String {
    let digest = sha256(value.as_bytes());
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut encoded, "{byte:02x}");
    }
    encoded
}

// Small dependency-free SHA-256 implementation. The activity module can be
// wired without adding a crypto crate, and raw Codex identifiers never reach
// disk even when activity.json is inspected or uploaded by mistake.
fn sha256(input: &[u8]) -> [u8; 32] {
    const INITIAL: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    let bit_len = (input.len() as u64).wrapping_mul(8);
    let mut padded = Vec::with_capacity((input.len() + 72) & !63);
    padded.extend_from_slice(input);
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    let mut hash = INITIAL;
    for chunk in padded.chunks_exact(64) {
        let mut words = [0_u32; 64];
        for (index, word) in words.iter_mut().take(16).enumerate() {
            let offset = index * 4;
            *word = u32::from_be_bytes([
                chunk[offset],
                chunk[offset + 1],
                chunk[offset + 2],
                chunk[offset + 3],
            ]);
        }
        for index in 16..64 {
            let s0 = words[index - 15].rotate_right(7)
                ^ words[index - 15].rotate_right(18)
                ^ (words[index - 15] >> 3);
            let s1 = words[index - 2].rotate_right(17)
                ^ words[index - 2].rotate_right(19)
                ^ (words[index - 2] >> 10);
            words[index] = words[index - 16]
                .wrapping_add(s0)
                .wrapping_add(words[index - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = hash;
        for index in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choice = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(choice)
                .wrapping_add(K[index])
                .wrapping_add(words[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        for (current, value) in hash.iter_mut().zip([a, b, c, d, e, f, g, h].into_iter()) {
            *current = current.wrapping_add(value);
        }
    }

    let mut result = [0_u8; 32];
    for (index, word) in hash.iter().enumerate() {
        result[index * 4..index * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    result
}

#[cfg(windows)]
fn discover_codex_host() -> Option<(u32, Option<u64>)> {
    use std::mem::{size_of, zeroed};

    const TH32CS_SNAPPROCESS: u32 = 0x2;
    const INVALID_HANDLE_VALUE: *mut std::ffi::c_void = -1_isize as *mut _;
    let snapshot = unsafe { windows_api::CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return None;
    }

    let mut processes = Vec::new();
    let mut entry: windows_api::ProcessEntry32W = unsafe { zeroed() };
    entry.size = size_of::<windows_api::ProcessEntry32W>() as u32;
    let mut has_entry = unsafe { windows_api::Process32FirstW(snapshot, &mut entry) } != 0;
    while has_entry {
        let name_end = entry
            .exe_file
            .iter()
            .position(|character| *character == 0)
            .unwrap_or(entry.exe_file.len());
        processes.push((
            entry.process_id,
            entry.parent_process_id,
            String::from_utf16_lossy(&entry.exe_file[..name_end]),
        ));
        has_entry = unsafe { windows_api::Process32NextW(snapshot, &mut entry) } != 0;
    }
    unsafe { windows_api::CloseHandle(snapshot) };

    let pid = find_codex_ancestor(std::process::id(), &processes)?;
    Some((pid, process_started_at_ms(pid)))
}

#[cfg(not(windows))]
fn discover_codex_host() -> Option<(u32, Option<u64>)> {
    None
}

fn find_codex_ancestor(start_pid: u32, processes: &[(u32, u32, String)]) -> Option<u32> {
    let mut current = start_pid;
    for _ in 0..16 {
        let (_, parent, _) = processes.iter().find(|process| process.0 == current)?;
        let parent_process = processes.iter().find(|process| process.0 == *parent)?;
        if parent_process.2.eq_ignore_ascii_case("codex.exe") {
            return Some(parent_process.0);
        }
        if parent_process.0 == current || parent_process.0 == 0 {
            return None;
        }
        current = parent_process.0;
    }
    None
}

#[cfg(windows)]
fn process_started_at_ms(pid: u32) -> Option<u64> {
    use std::mem::MaybeUninit;

    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    let handle = unsafe { windows_api::OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return None;
    }
    let mut created = MaybeUninit::<windows_api::FileTime>::uninit();
    let mut exited = MaybeUninit::<windows_api::FileTime>::uninit();
    let mut kernel = MaybeUninit::<windows_api::FileTime>::uninit();
    let mut user = MaybeUninit::<windows_api::FileTime>::uninit();
    let ok = unsafe {
        windows_api::GetProcessTimes(
            handle,
            created.as_mut_ptr(),
            exited.as_mut_ptr(),
            kernel.as_mut_ptr(),
            user.as_mut_ptr(),
        )
    };
    unsafe { windows_api::CloseHandle(handle) };
    if ok == 0 {
        return None;
    }
    let created = unsafe { created.assume_init() };
    let ticks = (u64::from(created.high) << 32) | u64::from(created.low);
    const WINDOWS_TO_UNIX_EPOCH_MS: u64 = 11_644_473_600_000;
    Some((ticks / 10_000).saturating_sub(WINDOWS_TO_UNIX_EPOCH_MS))
}

#[cfg(windows)]
fn process_matches(pid: u32, expected_started_at_ms: Option<u64>) -> bool {
    const SYNCHRONIZE: u32 = 0x0010_0000;
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const WAIT_TIMEOUT: u32 = 0x102;
    let handle = unsafe {
        windows_api::OpenProcess(SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, 0, pid)
    };
    if handle.is_null() {
        return false;
    }
    let wait = unsafe { windows_api::WaitForSingleObject(handle, 0) };
    if wait != WAIT_TIMEOUT {
        unsafe { windows_api::CloseHandle(handle) };
        return false;
    }

    let started_matches = expected_started_at_ms.map_or(true, |expected| {
        process_started_at_ms(pid).map_or(true, |actual| actual.abs_diff(expected) <= 1_000)
    });
    unsafe { windows_api::CloseHandle(handle) };
    started_matches
}

#[cfg(not(windows))]
fn process_matches(pid: u32, _expected_started_at_ms: Option<u64>) -> bool {
    Path::new("/proc").join(pid.to_string()).exists()
}

#[cfg(windows)]
fn with_activity_lock<T>(
    _path: &Path,
    operation: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let _guard = WindowsMutexGuard::acquire()?;
    operation()
}

#[cfg(not(windows))]
fn with_activity_lock<T>(
    path: &Path,
    operation: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let _guard = FileLockGuard::acquire(path)?;
    operation()
}

#[cfg(windows)]
struct WindowsMutexGuard(*mut std::ffi::c_void);

#[cfg(windows)]
impl WindowsMutexGuard {
    fn acquire() -> Result<Self, String> {
        use std::{iter, os::windows::ffi::OsStrExt, ptr};

        const WAIT_OBJECT_0: u32 = 0;
        const WAIT_ABANDONED: u32 = 0x80;
        let name: Vec<u16> = std::ffi::OsStr::new("Local\\CodexQuotaSyncActivityV1")
            .encode_wide()
            .chain(iter::once(0))
            .collect();
        let handle = unsafe { windows_api::CreateMutexW(ptr::null_mut(), 0, name.as_ptr()) };
        if handle.is_null() {
            return Err(format!(
                "failed to create activity mutex: {}",
                std::io::Error::last_os_error()
            ));
        }
        let wait =
            unsafe { windows_api::WaitForSingleObject(handle, LOCK_TIMEOUT.as_millis() as u32) };
        if wait == WAIT_OBJECT_0 || wait == WAIT_ABANDONED {
            Ok(Self(handle))
        } else {
            unsafe { windows_api::CloseHandle(handle) };
            Err("timed out waiting for activity mutex".to_string())
        }
    }
}

#[cfg(windows)]
impl Drop for WindowsMutexGuard {
    fn drop(&mut self) {
        unsafe {
            windows_api::ReleaseMutex(self.0);
            windows_api::CloseHandle(self.0);
        }
    }
}

#[cfg(not(windows))]
struct FileLockGuard {
    path: PathBuf,
}

#[cfg(not(windows))]
impl FileLockGuard {
    fn acquire(activity_path: &Path) -> Result<Self, String> {
        use std::{thread, time::Instant};

        let lock_path = activity_path.with_extension("json.lock");
        if let Some(parent) = lock_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("failed to create lock directory: {error}"))?;
        }
        let started = Instant::now();
        loop {
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&lock_path)
            {
                Ok(mut file) => {
                    let _ = writeln!(file, "{}", std::process::id());
                    return Ok(Self { path: lock_path });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    let stale = fs::metadata(&lock_path)
                        .and_then(|metadata| metadata.modified())
                        .ok()
                        .and_then(|modified| modified.elapsed().ok())
                        .map_or(false, |age| age > LOCK_TIMEOUT * 2);
                    if stale {
                        let _ = fs::remove_file(&lock_path);
                        continue;
                    }
                    if started.elapsed() >= LOCK_TIMEOUT {
                        return Err("timed out waiting for activity file lock".to_string());
                    }
                    thread::sleep(Duration::from_millis(20));
                }
                Err(error) => return Err(format!("failed to create activity lock: {error}")),
            }
        }
    }
}

#[cfg(not(windows))]
impl Drop for FileLockGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(windows)]
#[allow(non_snake_case)]
mod windows_api {
    use std::ffi::c_void;

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct FileTime {
        pub low: u32,
        pub high: u32,
    }

    #[repr(C)]
    pub struct ProcessEntry32W {
        pub size: u32,
        pub usage: u32,
        pub process_id: u32,
        pub default_heap_id: usize,
        pub module_id: u32,
        pub threads: u32,
        pub parent_process_id: u32,
        pub priority_class_base: i32,
        pub flags: u32,
        pub exe_file: [u16; 260],
    }

    #[link(name = "kernel32")]
    extern "system" {
        pub fn CreateMutexW(
            attributes: *mut c_void,
            initial_owner: i32,
            name: *const u16,
        ) -> *mut c_void;
        pub fn WaitForSingleObject(handle: *mut c_void, milliseconds: u32) -> u32;
        pub fn ReleaseMutex(handle: *mut c_void) -> i32;
        pub fn CloseHandle(handle: *mut c_void) -> i32;
        pub fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
        pub fn CreateToolhelp32Snapshot(flags: u32, process_id: u32) -> *mut c_void;
        pub fn Process32FirstW(snapshot: *mut c_void, entry: *mut ProcessEntry32W) -> i32;
        pub fn Process32NextW(snapshot: *mut c_void, entry: *mut ProcessEntry32W) -> i32;
        pub fn OpenProcess(access: u32, inherit_handle: i32, process_id: u32) -> *mut c_void;
        pub fn GetProcessTimes(
            process: *mut c_void,
            creation: *mut FileTime,
            exit: *mut FileTime,
            kernel: *mut FileTime,
            user: *mut FileTime,
        ) -> i32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Cursor, sync::Arc, thread};

    fn temp_activity(test_name: &str) -> PathBuf {
        std::env::temp_dir()
            .join("codex-quota-sync-activity-tests")
            .join(format!("{test_name}-{}", std::process::id()))
            .join("activity.json")
    }

    fn clean(path: &Path) {
        if let Some(directory) = path.parent() {
            let _ = fs::remove_dir_all(directory);
        }
    }

    fn hook(path: &Path, json: &str) {
        process_hook_reader(path, Cursor::new(json.as_bytes()), DEFAULT_ACTIVITY_TTL).unwrap();
    }

    #[test]
    fn sha256_matches_published_vector() {
        assert_eq!(
            hash_identifier("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn lifecycle_transitions_and_stop_are_aggregated() {
        let path = temp_activity("lifecycle");
        clean(&path);
        hook(
            &path,
            r#"{"session_id":"s1","turn_id":"t1","hook_event_name":"UserPromptSubmit","prompt":"private"}"#,
        );
        assert_eq!(
            read_activity_summary_at(&path, DEFAULT_ACTIVITY_TTL)
                .unwrap()
                .executing,
            1
        );
        hook(
            &path,
            r#"{"session_id":"s1","turn_id":"t1","hook_event_name":"PermissionRequest","tool_name":"Bash"}"#,
        );
        assert_eq!(
            read_activity_summary_at(&path, DEFAULT_ACTIVITY_TTL)
                .unwrap()
                .waiting_on_approval,
            1
        );
        hook(
            &path,
            r#"{"session_id":"s1","turn_id":"t1","hook_event_name":"PostToolUse","tool_name":"Bash"}"#,
        );
        assert_eq!(
            read_activity_summary_at(&path, DEFAULT_ACTIVITY_TTL)
                .unwrap()
                .executing,
            1
        );
        hook(
            &path,
            r#"{"session_id":"s1","turn_id":"t1","hook_event_name":"PreToolUse","tool_name":"request_user_input"}"#,
        );
        assert_eq!(
            read_activity_summary_at(&path, DEFAULT_ACTIVITY_TTL)
                .unwrap()
                .waiting_on_user_input,
            1
        );
        hook(
            &path,
            r#"{"session_id":"s1","turn_id":"t1","hook_event_name":"Stop","last_assistant_message":"private"}"#,
        );
        assert_eq!(
            read_activity_summary_at(&path, DEFAULT_ACTIVITY_TTL)
                .unwrap()
                .total,
            0
        );
        clean(&path);
    }

    #[test]
    fn state_never_contains_prompt_transcript_or_raw_ids() {
        let path = temp_activity("privacy");
        clean(&path);
        hook(
            &path,
            r#"{"session_id":"raw-session-secret","turn_id":"raw-turn-secret","hook_event_name":"UserPromptSubmit","prompt":"do not persist me","transcript_path":"C:\\secret\\transcript.jsonl"}"#,
        );
        let persisted = fs::read_to_string(&path).unwrap();
        for secret in [
            "raw-session-secret",
            "raw-turn-secret",
            "do not persist me",
            "transcript",
        ] {
            assert!(!persisted.contains(secret));
        }
        assert!(persisted.contains(&hash_identifier("raw-session-secret")));
        clean(&path);
    }

    #[test]
    fn session_start_only_cleans_its_own_residual_entries() {
        let path = temp_activity("session-clean");
        clean(&path);
        for session in ["s1", "s2"] {
            hook(
                &path,
                &format!(
                    r#"{{"session_id":"{session}","turn_id":"t1","hook_event_name":"UserPromptSubmit"}}"#
                ),
            );
        }
        hook(
            &path,
            r#"{"session_id":"s1","hook_event_name":"SessionStart","source":"resume"}"#,
        );
        let state: ActivityState =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(state.entries.len(), 1);
        assert_eq!(state.entries[0].session_hash, hash_identifier("s2"));
        clean(&path);
    }

    #[test]
    fn a_new_turn_replaces_a_residual_turn_in_the_same_session() {
        let path = temp_activity("turn-replacement");
        clean(&path);
        for turn in ["t1", "t2"] {
            hook(
                &path,
                &format!(
                    r#"{{"session_id":"s1","turn_id":"{turn}","hook_event_name":"UserPromptSubmit"}}"#
                ),
            );
        }
        let state: ActivityState =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(state.entries.len(), 1);
        assert_eq!(state.entries[0].turn_hash, hash_identifier("t2"));
        clean(&path);
    }

    #[test]
    fn ttl_and_dead_pid_repair_abandoned_entries() {
        let path = temp_activity("repair");
        clean(&path);
        let now = now_ms();
        let state = ActivityState {
            version: STATE_VERSION,
            updated_at_ms: now,
            entries: vec![
                ActivityEntry {
                    session_hash: hash_identifier("expired"),
                    turn_hash: hash_identifier("turn"),
                    status: ActivityStatus::Executing,
                    updated_at_ms: now.saturating_sub(10_000),
                    host_pid: None,
                    host_started_at_ms: None,
                },
                ActivityEntry {
                    session_hash: hash_identifier("dead"),
                    turn_hash: hash_identifier("turn"),
                    status: ActivityStatus::WaitingOnApproval,
                    updated_at_ms: now,
                    host_pid: Some(u32::MAX),
                    host_started_at_ms: None,
                },
            ],
        };
        persist_state(&path, &state).unwrap();
        let summary = read_activity_summary_at(&path, Duration::from_secs(1)).unwrap();
        assert_eq!(summary.total, 0);
        clean(&path);
    }

    #[test]
    fn executing_entries_expire_before_waiting_entries() {
        let now = now_ms();
        let mut state = ActivityState {
            version: STATE_VERSION,
            updated_at_ms: now,
            entries: vec![
                ActivityEntry {
                    session_hash: hash_identifier("executing"),
                    turn_hash: hash_identifier("turn"),
                    status: ActivityStatus::Executing,
                    updated_at_ms: now.saturating_sub(duration_ms(EXECUTING_ACTIVITY_TTL) + 1),
                    host_pid: None,
                    host_started_at_ms: None,
                },
                ActivityEntry {
                    session_hash: hash_identifier("waiting"),
                    turn_hash: hash_identifier("turn"),
                    status: ActivityStatus::WaitingOnUserInput,
                    updated_at_ms: now.saturating_sub(duration_ms(EXECUTING_ACTIVITY_TTL) + 1),
                    host_pid: None,
                    host_started_at_ms: None,
                },
            ],
        };
        assert!(prune_stale_entries(&mut state, now, DEFAULT_ACTIVITY_TTL));
        assert_eq!(state.entries.len(), 1);
        assert_eq!(state.entries[0].status, ActivityStatus::WaitingOnUserInput);
    }

    #[test]
    fn v1_entries_without_host_identity_are_removed_during_migration() {
        let path = temp_activity("v1-migration");
        clean(&path);
        let now = now_ms();
        let state = ActivityState {
            version: 1,
            updated_at_ms: now,
            entries: vec![ActivityEntry {
                session_hash: hash_identifier("legacy"),
                turn_hash: hash_identifier("turn"),
                status: ActivityStatus::Executing,
                updated_at_ms: now,
                host_pid: None,
                host_started_at_ms: None,
            }],
        };
        persist_state(&path, &state).unwrap();
        let summary = read_activity_summary_at(&path, DEFAULT_ACTIVITY_TTL).unwrap();
        assert_eq!(summary.total, 0);
        let migrated: ActivityState =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(migrated.version, STATE_VERSION);
        clean(&path);
    }

    #[test]
    fn codex_ancestor_is_found_through_command_shells() {
        let processes = vec![
            (10, 20, "codex-quota-sync.exe".to_string()),
            (20, 30, "cmd.exe".to_string()),
            (30, 40, "powershell.exe".to_string()),
            (40, 1, "Codex.exe".to_string()),
            (1, 0, "explorer.exe".to_string()),
        ];
        assert_eq!(find_codex_ancestor(10, &processes), Some(40));
        assert_eq!(find_codex_ancestor(1, &processes), None);
    }

    #[test]
    fn corrupt_state_is_replaced_with_valid_empty_json() {
        let path = temp_activity("corrupt");
        clean(&path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"not json").unwrap();
        let summary = read_activity_summary_at(&path, DEFAULT_ACTIVITY_TTL).unwrap();
        assert_eq!(summary.total, 0);
        serde_json::from_str::<ActivityState>(&fs::read_to_string(&path).unwrap()).unwrap();
        clean(&path);
    }

    #[test]
    fn concurrent_hook_processes_cannot_lose_entries() {
        let path = Arc::new(temp_activity("concurrent"));
        clean(&path);
        let mut workers = Vec::new();
        for index in 0..16 {
            let path = Arc::clone(&path);
            workers.push(thread::spawn(move || {
                hook(
                    &path,
                    &format!(
                        r#"{{"session_id":"s{index}","turn_id":"t{index}","hook_event_name":"UserPromptSubmit"}}"#
                    ),
                );
            }));
        }
        for worker in workers {
            worker.join().unwrap();
        }
        let summary = read_activity_summary_at(&path, DEFAULT_ACTIVITY_TTL).unwrap();
        assert_eq!(summary.total, 16);
        assert_eq!(summary.executing, 16);
        clean(&path);
    }

    #[test]
    fn request_user_input_name_matching_is_tolerant() {
        assert!(is_request_user_input(Some("request_user_input")));
        assert!(is_request_user_input(Some(
            "mcp__codex__request_user_input"
        )));
        assert!(is_request_user_input(Some("requestUserInput")));
        assert!(!is_request_user_input(Some("Bash")));
    }

    #[test]
    fn missing_session_or_turn_is_non_blocking() {
        let path = temp_activity("missing-fields");
        clean(&path);
        hook(&path, r#"{"hook_event_name":"SessionStart"}"#);
        hook(
            &path,
            r#"{"session_id":"s1","hook_event_name":"UserPromptSubmit"}"#,
        );
        assert!(read_activity_summary_at(&path, DEFAULT_ACTIVITY_TTL).is_err());
        clean(&path);
    }

    #[test]
    fn session_start_creates_an_empty_hook_heartbeat() {
        let path = temp_activity("session-heartbeat");
        clean(&path);
        hook(
            &path,
            r#"{"session_id":"s1","hook_event_name":"SessionStart","source":"startup"}"#,
        );
        let summary = read_activity_summary_at(&path, DEFAULT_ACTIVITY_TTL).unwrap();
        assert_eq!(summary.total, 0);
        assert!(summary.state_updated_at_ms > 0);
        clean(&path);
    }
}
