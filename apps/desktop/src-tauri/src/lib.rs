mod activity;
mod codex;
mod models;
mod shutdown;
mod sync;

use std::{
    fs,
    io::Write,
    path::PathBuf,
    sync::{atomic::AtomicU64, Mutex},
    time::{Duration, Instant},
};

#[cfg(debug_assertions)]
use models::UsageWindow;
use models::{
    ActivitySnapshot, DesktopSnapshot, ProviderSnapshot, SyncAttempt, SyncEnvelope, SyncView,
    WidgetPreferences,
};
use serde::{Deserialize, Serialize};
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, State, WindowEvent,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_window_state::Builder as WindowStateBuilder;

const COLLAPSED_LOGICAL_SIZE: f64 = 80.0;
const EXPANDED_LOGICAL_SIZE: f64 = 320.0;
const EDGE_SAFE_INSET_LOGICAL: f64 = 4.0;
const SNAP_THRESHOLD_LOGICAL: f64 = 24.0;
const POSITION_EPSILON: u32 = 2;

#[derive(Clone, Copy)]
enum HorizontalDock {
    Left,
    Right,
}

#[derive(Clone, Copy)]
enum VerticalDock {
    Top,
    Bottom,
}

#[derive(Clone, Copy, Default)]
struct DockState {
    horizontal: Option<HorizontalDock>,
    vertical: Option<VerticalDock>,
}

impl DockState {
    fn is_docked(self) -> bool {
        self.horizontal.is_some() || self.vertical.is_some()
    }
}

#[derive(Clone, Copy)]
struct WidgetRect {
    position: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
}

#[derive(Clone, Copy, Deserialize)]
struct WorkAreaPoint {
    x: i32,
    y: i32,
}

#[derive(Clone, Copy, Deserialize)]
struct WorkAreaSize {
    width: u32,
    height: u32,
}

#[derive(Clone, Copy, Deserialize)]
struct WorkAreaPayload {
    position: WorkAreaPoint,
    size: WorkAreaSize,
}

#[derive(Clone, Copy)]
enum WidgetMode {
    Collapsed,
    Expanded,
}

#[derive(Clone, Copy)]
struct WidgetGeometryState {
    mode: WidgetMode,
    dock: DockState,
    collapsed_rect: WidgetRect,
    expanded_rect: Option<WidgetRect>,
    user_moved_expanded: bool,
}

struct AppState {
    app_handle: AppHandle,
    client: reqwest::Client,
    preferences: Mutex<WidgetPreferences>,
    preferences_path: PathBuf,
    fetch_lock: tokio::sync::Mutex<()>,
    dashboard_cache: Mutex<Option<(Instant, Vec<DesktopSnapshot>)>>,
    quota_cache: Mutex<Option<(Instant, ProviderSnapshot)>>,
    last_good_snapshot: Mutex<Option<ProviderSnapshot>>,
    remote_cache: Mutex<Option<SyncEnvelope>>,
    revision: AtomicU64,
    completion_shutdown: Mutex<shutdown::ShutdownArm>,
    #[cfg(debug_assertions)]
    simulate_short_window_for_testing: Mutex<bool>,
    geometry: Mutex<Option<WidgetGeometryState>>,
    drag_mode: Mutex<Option<WidgetMode>>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CompletionShutdownView {
    armed: bool,
}

#[cfg(debug_assertions)]
fn apply_short_window_test_override(
    state: &AppState,
    mut snapshots: Vec<ProviderSnapshot>,
) -> Vec<ProviderSnapshot> {
    if state
        .simulate_short_window_for_testing
        .lock()
        .map(|value| *value)
        .unwrap_or(false)
    {
        for snapshot in &mut snapshots {
            if snapshot.status == "ok" {
                snapshot.short_window = Some(UsageWindow {
                    remaining_percent: 88.0,
                    resets_at: Some((chrono::Utc::now() + chrono::Duration::hours(3)).to_rfc3339()),
                    window_seconds: Some(18_000),
                });
            }
        }
    }
    snapshots
}

const DASHBOARD_CACHE_TTL: Duration = Duration::from_secs(30);
const LOCAL_QUOTA_TTL: Duration = Duration::from_secs(5 * 60);
const LOCAL_QUOTA_FAST_TTL: Duration = Duration::from_secs(60);
const REMOTE_STALE_AFTER: chrono::Duration = chrono::Duration::minutes(15);

fn preferences(state: &AppState) -> WidgetPreferences {
    state
        .preferences
        .lock()
        .map(|value| value.clone())
        .unwrap_or_default()
}

/// Collector 保存普通设置时保留未回显的写入密钥；Viewer 从不保留密钥。
fn preserve_or_clear_write_secret(
    mut next: WidgetPreferences,
    previous: &WidgetPreferences,
) -> WidgetPreferences {
    if next.sync_role == "viewer" {
        next.write_secret.clear();
    } else if next.write_secret.is_empty() {
        next.write_secret = previous.write_secret.clone();
    }
    next
}

fn completion_shutdown_view(state: &AppState) -> CompletionShutdownView {
    CompletionShutdownView {
        armed: state
            .completion_shutdown
            .lock()
            .map(|value| value.enabled())
            .unwrap_or(false),
    }
}

fn emit_completion_shutdown_changed(state: &AppState) {
    let value = completion_shutdown_view(state);
    let _ = state
        .app_handle
        .emit_to("widget", "completion-shutdown-changed", value.clone());
    let _ = state
        .app_handle
        .emit_to("settings", "completion-shutdown-changed", value);
}

fn emit_completion_shutdown_notice(state: &AppState, message: &str) {
    let _ = state
        .app_handle
        .emit_to("widget", "completion-shutdown-notice", message);
    let _ = state
        .app_handle
        .emit_to("settings", "completion-shutdown-notice", message);
}

fn emit_preferences_changed(state: &AppState, preferences: &WidgetPreferences) {
    let _ = state
        .app_handle
        .emit_to("widget", "preferences-changed", preferences);
    let _ = state
        .app_handle
        .emit_to("settings", "preferences-changed", preferences);
}

fn is_near_reset(snapshot: &ProviderSnapshot) -> bool {
    snapshot.next_reset_at.as_deref().is_some_and(|value| {
        chrono::DateTime::parse_from_rfc3339(value)
            .ok()
            .map(|reset| {
                let remaining = reset.with_timezone(&chrono::Utc) - chrono::Utc::now();
                remaining > chrono::Duration::minutes(-5)
                    && remaining <= chrono::Duration::minutes(15)
            })
            .unwrap_or(false)
    })
}

fn quota_cache_ttl(snapshot: &ProviderSnapshot) -> Duration {
    if is_near_reset(snapshot) {
        LOCAL_QUOTA_FAST_TTL
    } else {
        LOCAL_QUOTA_TTL
    }
}

async fn local_quota(state: &AppState, force: bool) -> ProviderSnapshot {
    if !force {
        if let Ok(cache) = state.quota_cache.lock() {
            if let Some((time, snapshot)) = &*cache {
                if time.elapsed() < quota_cache_ttl(snapshot) {
                    return snapshot.clone();
                }
            }
        }
    }

    let snapshot = codex::fetch_snapshot(&state.client)
        .await
        .with_derived_next_reset();
    if snapshot.status == "ok" {
        if let Ok(mut last_good) = state.last_good_snapshot.lock() {
            *last_good = Some(snapshot.clone());
        }
    }
    if let Ok(mut cache) = state.quota_cache.lock() {
        *cache = Some((Instant::now(), snapshot.clone()));
    }
    snapshot
}

fn current_activity(preferences: &WidgetPreferences) -> ActivitySnapshot {
    let summary = if preferences.activity_state_path.is_empty() {
        activity::read_activity_summary()
    } else {
        activity::read_activity_summary_at(
            std::path::Path::new(&preferences.activity_state_path),
            activity::DEFAULT_ACTIVITY_TTL,
        )
    };
    match summary {
        Ok(summary) => {
            let activity_ttl_ms = activity::DEFAULT_ACTIVITY_TTL
                .as_millis()
                .min(u128::from(u64::MAX)) as u64;
            let stale = summary.state_updated_at_ms == 0
                || summary
                    .observed_at_ms
                    .saturating_sub(summary.state_updated_at_ms)
                    > activity_ttl_ms;
            let observed_at_ms = if summary.state_updated_at_ms == 0 {
                summary.observed_at_ms
            } else {
                summary.state_updated_at_ms
            };
            let observed_at =
                chrono::DateTime::<chrono::Utc>::from_timestamp_millis(observed_at_ms as i64)
                    .unwrap_or_else(chrono::Utc::now)
                    .to_rfc3339();
            ActivitySnapshot {
                executing: summary.executing.into(),
                waiting_on_approval: summary.waiting_on_approval.into(),
                waiting_on_user_input: summary.waiting_on_user_input.into(),
                source: "hooks".into(),
                observed_at,
                stale,
            }
        }
        Err(_) => ActivitySnapshot::unavailable(),
    }
}

pub fn run_activity_hook() -> Result<(), String> {
    activity::run_activity_hook()
}

fn stale_copy(last_good: &ProviderSnapshot, attempt: &ProviderSnapshot) -> ProviderSnapshot {
    let mut snapshot = last_good.clone();
    snapshot.status = "stale".into();
    snapshot.message = attempt.message.clone();
    snapshot
}

fn remote_is_stale(envelope: &SyncEnvelope) -> bool {
    chrono::DateTime::parse_from_rfc3339(&envelope.collected_at)
        .ok()
        .map(|value| chrono::Utc::now() - value.with_timezone(&chrono::Utc) > REMOTE_STALE_AFTER)
        .unwrap_or(true)
}

fn desktop_from_envelope(
    envelope: &SyncEnvelope,
    connection_state: &str,
    connection_message: Option<String>,
) -> DesktopSnapshot {
    let age_stale = remote_is_stale(envelope);
    let attempt_failed = envelope.latest_attempt.status != "ok";
    let mut activity = envelope.activity.clone();
    activity.stale |= age_stale;
    let quota = match envelope.last_good_snapshot.clone() {
        Some(mut snapshot) => {
            if age_stale || attempt_failed || connection_state == "offline" {
                snapshot.status = "stale".into();
                snapshot.message = connection_message
                    .clone()
                    .or_else(|| envelope.latest_attempt.message.clone())
                    .or_else(|| Some("The synchronized snapshot is stale.".into()));
            }
            snapshot.with_derived_next_reset()
        }
        None => ProviderSnapshot::failure(
            &envelope.latest_attempt.status,
            envelope
                .latest_attempt
                .message
                .as_deref()
                .unwrap_or("The collector has no successful quota snapshot yet."),
        ),
    };
    let state = if connection_state == "offline" {
        "offline"
    } else if age_stale || attempt_failed || activity.stale {
        "stale"
    } else {
        "synced"
    };
    DesktopSnapshot {
        quota,
        activity,
        sync: SyncView {
            role: "viewer".into(),
            state: state.into(),
            source_id: envelope.source_id.clone(),
            collected_at: Some(envelope.collected_at.clone()),
            received_at: envelope.received_at.clone(),
            message: connection_message.or_else(|| envelope.latest_attempt.message.clone()),
        },
    }
}

async fn collector_dashboard(
    state: &AppState,
    preferences: &WidgetPreferences,
    force_quota: bool,
) -> DesktopSnapshot {
    let attempt = local_quota(state, force_quota).await;
    let activity = current_activity(preferences);
    let should_shutdown_after_completion = state
        .completion_shutdown
        .lock()
        .map(|mut value| value.observe(&preferences.sync_role, &activity))
        .unwrap_or(false);
    let last_good = state
        .last_good_snapshot
        .lock()
        .ok()
        .and_then(|value| value.clone());
    let collected_at = chrono::Utc::now().to_rfc3339();
    let envelope = SyncEnvelope {
        schema_version: 1,
        source_id: preferences.source_id.clone(),
        revision: 0,
        collector_version: env!("CARGO_PKG_VERSION").into(),
        collected_at: collected_at.clone(),
        received_at: None,
        activity: activity.clone(),
        latest_attempt: SyncAttempt::from_snapshot(&attempt),
        last_good_snapshot: last_good.clone(),
    };

    let (sync_state, sync_message, received_at) = if preferences.server_url.is_empty() {
        ("local", None, None)
    } else if preferences.write_secret.is_empty() {
        (
            "configuration",
            Some("Write secret is not configured.".into()),
            None,
        )
    } else {
        match sync::upload_status(&state.client, preferences, &state.revision, envelope).await {
            Ok(saved) => {
                if let Ok(mut cache) = state.remote_cache.lock() {
                    *cache = Some(saved.clone());
                }
                ("synced", None, saved.received_at)
            }
            Err(message) => ("offline", Some(message), None),
        }
    };

    if should_shutdown_after_completion {
        // 先同步“执行中 = 0”的最终状态；无论服务器是否可用，随后都执行用户已武装的本机动作。
        emit_completion_shutdown_changed(state);
        match shutdown::launch_script(&preferences.shutdown_script_path) {
            Ok(()) => eprintln!("completion shutdown script started"),
            Err(error) => {
                eprintln!("completion shutdown script failed: {error}");
                emit_completion_shutdown_notice(state, &error);
            }
        }
    }

    let quota = if attempt.status == "ok" || attempt.status == "signed_out" {
        attempt
    } else if let Some(snapshot) = last_good {
        stale_copy(&snapshot, &attempt)
    } else {
        attempt
    };
    DesktopSnapshot {
        quota,
        activity,
        sync: SyncView {
            role: "collector".into(),
            state: sync_state.into(),
            source_id: preferences.source_id.clone(),
            collected_at: Some(collected_at),
            received_at,
            message: sync_message,
        },
    }
}

async fn viewer_dashboard(state: &AppState, preferences: &WidgetPreferences) -> DesktopSnapshot {
    match sync::download_status(&state.client, preferences).await {
        Ok(envelope) => {
            if let Ok(mut cache) = state.remote_cache.lock() {
                *cache = Some(envelope.clone());
            }
            desktop_from_envelope(&envelope, "synced", None)
        }
        Err(message) => {
            let cached = state
                .remote_cache
                .lock()
                .ok()
                .and_then(|value| value.clone());
            if let Some(envelope) = cached {
                desktop_from_envelope(&envelope, "offline", Some(message))
            } else {
                DesktopSnapshot {
                    quota: ProviderSnapshot::failure("unavailable", &message),
                    activity: ActivitySnapshot::unavailable(),
                    sync: SyncView {
                        role: "viewer".into(),
                        state: if preferences.server_url.is_empty() {
                            "configuration".into()
                        } else {
                            "offline".into()
                        },
                        source_id: preferences.source_id.clone(),
                        collected_at: None,
                        received_at: None,
                        message: Some(message),
                    },
                }
            }
        }
    }
}

async fn fetch_dashboard(state: &AppState, force_quota: bool) -> Vec<DesktopSnapshot> {
    let preferences = preferences(state);
    let snapshot = if preferences.sync_role == "viewer" {
        viewer_dashboard(state, &preferences).await
    } else {
        collector_dashboard(state, &preferences, force_quota).await
    };
    let values = vec![snapshot];
    #[cfg(debug_assertions)]
    let values = {
        let mut values = values;
        let quotas = apply_short_window_test_override(
            state,
            values.iter().map(|item| item.quota.clone()).collect(),
        );
        for (item, quota) in values.iter_mut().zip(quotas) {
            item.quota = quota.with_derived_next_reset();
        }
        values
    };
    values
}

fn load_preferences(path: &PathBuf) -> WidgetPreferences {
    let parse = |candidate: &PathBuf| {
        fs::read_to_string(candidate)
            .ok()
            .and_then(|raw| serde_json::from_str::<WidgetPreferences>(&raw).ok())
    };
    if let Some(value) = parse(path) {
        return value.normalized();
    }
    let backup = path.with_extension("json.bak");
    if let Some(value) = parse(&backup) {
        eprintln!("preferences recovered from backup");
        return value.normalized();
    }
    WidgetPreferences::default()
}

fn persist_preferences(path: &PathBuf, value: &WidgetPreferences) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|_| "failed to create settings directory".to_string())?;
    }
    let serialized = serialize_preferences(value)?;
    let temporary = path.with_extension("json.tmp");
    let backup = path.with_extension("json.bak");
    let mut file = fs::File::create(&temporary)
        .map_err(|_| "failed to create temporary settings file".to_string())?;
    file.write_all(&serialized)
        .and_then(|_| file.sync_all())
        .map_err(|_| "failed to write settings".to_string())?;
    if path.exists() {
        let _ = fs::remove_file(&backup);
        fs::rename(path, &backup).map_err(|_| "failed to back up settings".to_string())?;
    }
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::rename(&backup, path);
        return Err(format!("failed to commit settings: {error}"));
    }
    Ok(())
}

fn serialize_preferences(value: &WidgetPreferences) -> Result<Vec<u8>, String> {
    let mut persisted =
        serde_json::to_value(value).map_err(|_| "failed to serialize settings".to_string())?;
    if let serde_json::Value::Object(fields) = &mut persisted {
        fields.insert(
            "writeSecret".into(),
            serde_json::Value::String(value.write_secret.clone()),
        );
    }
    serde_json::to_vec_pretty(&persisted).map_err(|_| "failed to serialize settings".to_string())
}

#[cfg(test)]
mod preference_tests {
    use super::*;

    #[test]
    fn persisted_preferences_keep_secret_without_exposing_it_to_webview_serialization() {
        let mut preferences = WidgetPreferences::default();
        preferences.write_secret = "collector-secret".into();

        let persisted = serialize_preferences(&preferences).unwrap();
        let reloaded: WidgetPreferences = serde_json::from_slice(&persisted).unwrap();
        assert_eq!(reloaded.write_secret, "collector-secret");

        let webview_value = serde_json::to_value(&preferences).unwrap();
        assert!(webview_value.get("writeSecret").is_none());
    }

    #[test]
    fn collector_keeps_an_omitted_secret_but_viewer_clears_it() {
        let previous = WidgetPreferences {
            write_secret: "existing-secret".into(),
            ..WidgetPreferences::default()
        };
        let collector = preserve_or_clear_write_secret(WidgetPreferences::default(), &previous);
        assert_eq!(collector.write_secret, "existing-secret");

        let viewer = preserve_or_clear_write_secret(
            WidgetPreferences {
                sync_role: "viewer".into(),
                write_secret: "should-not-remain".into(),
                ..WidgetPreferences::default()
            },
            &previous,
        );
        assert!(viewer.write_secret.is_empty());
    }
}

#[tauri::command]
async fn get_snapshots(state: State<'_, AppState>) -> Result<Vec<DesktopSnapshot>, String> {
    if let Ok(cache) = state.dashboard_cache.lock() {
        if let Some((time, values)) = &*cache {
            if time.elapsed() < DASHBOARD_CACHE_TTL {
                return Ok(values.clone());
            }
        }
    }
    let _guard = match state.fetch_lock.try_lock() {
        Ok(guard) => guard,
        Err(_) => {
            if let Ok(cache) = state.dashboard_cache.lock() {
                if let Some((_, values)) = &*cache {
                    return Ok(values.clone());
                }
            }
            let preferences = preferences(state.inner());
            return Ok(vec![DesktopSnapshot {
                quota: ProviderSnapshot::failure(
                    "unavailable",
                    "Quota refresh is already running.",
                ),
                activity: ActivitySnapshot::unavailable(),
                sync: SyncView {
                    role: preferences.sync_role,
                    state: "offline".into(),
                    source_id: preferences.source_id,
                    collected_at: None,
                    received_at: None,
                    message: Some("Quota refresh is already running.".into()),
                },
            }]);
        }
    };
    if let Ok(cache) = state.dashboard_cache.lock() {
        if let Some((time, values)) = &*cache {
            if time.elapsed() < DASHBOARD_CACHE_TTL {
                return Ok(values.clone());
            }
        }
    }
    let values = fetch_dashboard(state.inner(), false).await;
    if let Ok(mut cache) = state.dashboard_cache.lock() {
        *cache = Some((Instant::now(), values.clone()));
    }
    Ok(values)
}

#[tauri::command]
async fn refresh_snapshots(state: State<'_, AppState>) -> Result<Vec<DesktopSnapshot>, String> {
    let _guard = state.fetch_lock.lock().await;
    let values = fetch_dashboard(state.inner(), true).await;
    if let Ok(mut cache) = state.dashboard_cache.lock() {
        *cache = Some((Instant::now(), values.clone()));
    }
    Ok(values)
}

fn clamp_position_to_monitor(
    position: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
    monitor: &tauri::Monitor,
    safe_inset: i32,
) -> PhysicalPosition<i32> {
    let monitor_position = monitor.position();
    let monitor_size = monitor.size();
    let left = monitor_position.x;
    let top = monitor_position.y;
    let right = left + monitor_size.width as i32;
    let bottom = top + monitor_size.height as i32;
    PhysicalPosition::new(
        position
            .x
            .clamp(left - safe_inset, right - size.width as i32 + safe_inset),
        position
            .y
            .clamp(top - safe_inset, bottom - size.height as i32 + safe_inset),
    )
}

fn logical_to_physical(value: f64, scale_factor: f64) -> u32 {
    (value * scale_factor).round().max(1.0) as u32
}

fn window_size_for_visual_size(visual_size: u32, safe_inset: u32) -> u32 {
    visual_size + safe_inset * 2
}

fn widget_window_size(logical_visual_size: f64, scale_factor: f64, safe_inset: u32) -> u32 {
    window_size_for_visual_size(
        logical_to_physical(logical_visual_size, scale_factor),
        safe_inset,
    )
}

fn detect_dock(
    position: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
    monitor: &tauri::Monitor,
    threshold: i32,
    safe_inset: i32,
) -> DockState {
    let monitor_position = monitor.position();
    let monitor_size = monitor.size();
    let visible_left = position.x + safe_inset;
    let visible_top = position.y + safe_inset;
    let visible_right = position.x + size.width as i32 - safe_inset;
    let visible_bottom = position.y + size.height as i32 - safe_inset;
    let left_distance = (visible_left - monitor_position.x).abs();
    let top_distance = (visible_top - monitor_position.y).abs();
    let right_distance = (monitor_position.x + monitor_size.width as i32 - visible_right).abs();
    let bottom_distance = (monitor_position.y + monitor_size.height as i32 - visible_bottom).abs();
    let horizontal = if left_distance <= threshold || right_distance <= threshold {
        if left_distance <= right_distance {
            Some(HorizontalDock::Left)
        } else {
            Some(HorizontalDock::Right)
        }
    } else {
        None
    };
    let vertical = if top_distance <= threshold || bottom_distance <= threshold {
        if top_distance <= bottom_distance {
            Some(VerticalDock::Top)
        } else {
            Some(VerticalDock::Bottom)
        }
    } else {
        None
    };
    DockState {
        horizontal,
        vertical,
    }
}

fn snap_position(
    position: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
    dock: DockState,
    monitor: &tauri::Monitor,
    safe_inset: i32,
) -> PhysicalPosition<i32> {
    let monitor_position = monitor.position();
    let monitor_size = monitor.size();
    let mut next = clamp_position_to_monitor(position, size, monitor, safe_inset);
    match dock.horizontal {
        Some(HorizontalDock::Left) => next.x = monitor_position.x - safe_inset,
        Some(HorizontalDock::Right) => {
            next.x = monitor_position.x + monitor_size.width as i32 - size.width as i32 + safe_inset
        }
        None => {}
    }
    match dock.vertical {
        Some(VerticalDock::Top) => next.y = monitor_position.y - safe_inset,
        Some(VerticalDock::Bottom) => {
            next.y =
                monitor_position.y + monitor_size.height as i32 - size.height as i32 + safe_inset
        }
        None => {}
    }
    next
}

fn expanded_position_in_bounds(
    collapsed: WidgetRect,
    expanded_size: PhysicalSize<u32>,
    dock: DockState,
    bounds_position: PhysicalPosition<i32>,
    bounds_size: PhysicalSize<u32>,
    safe_inset: i32,
) -> PhysicalPosition<i32> {
    let monitor_right = bounds_position.x + bounds_size.width as i32;
    let monitor_bottom = bounds_position.y + bounds_size.height as i32;
    let collapsed_left = collapsed.position.x + safe_inset;
    let collapsed_top = collapsed.position.y + safe_inset;
    let collapsed_right = collapsed.position.x + collapsed.size.width as i32 - safe_inset;
    let collapsed_bottom = collapsed.position.y + collapsed.size.height as i32 - safe_inset;
    let x = match dock.horizontal {
        Some(HorizontalDock::Left) => collapsed_left - safe_inset,
        Some(HorizontalDock::Right) => collapsed_right - expanded_size.width as i32 + safe_inset,
        None if collapsed_left + expanded_size.width as i32 - safe_inset > monitor_right => {
            collapsed_right - expanded_size.width as i32 + safe_inset
        }
        None => collapsed_left - safe_inset,
    };
    let y = match dock.vertical {
        Some(VerticalDock::Top) => collapsed_top - safe_inset,
        Some(VerticalDock::Bottom) => collapsed_bottom - expanded_size.height as i32 + safe_inset,
        None if collapsed_top + expanded_size.height as i32 - safe_inset > monitor_bottom => {
            collapsed_bottom - expanded_size.height as i32 + safe_inset
        }
        None => collapsed_top - safe_inset,
    };
    let min_x = bounds_position.x - safe_inset;
    let min_y = bounds_position.y - safe_inset;
    let max_x = (monitor_right - expanded_size.width as i32 + safe_inset).max(min_x);
    let max_y = (monitor_bottom - expanded_size.height as i32 + safe_inset).max(min_y);
    PhysicalPosition::new(x.clamp(min_x, max_x), y.clamp(min_y, max_y))
}

fn expanded_position(
    collapsed: WidgetRect,
    expanded_size: PhysicalSize<u32>,
    dock: DockState,
    monitor: &tauri::Monitor,
    work_area: Option<WorkAreaPayload>,
    safe_inset: i32,
) -> PhysicalPosition<i32> {
    let (bounds_position, bounds_size) = work_area
        .map(|area| {
            (
                PhysicalPosition::new(area.position.x, area.position.y),
                PhysicalSize::new(area.size.width, area.size.height),
            )
        })
        .unwrap_or_else(|| (*monitor.position(), *monitor.size()));
    expanded_position_in_bounds(
        collapsed,
        expanded_size,
        dock,
        bounds_position,
        bounds_size,
        safe_inset,
    )
}

fn collapsed_geometry_for_expand(
    current_position: PhysicalPosition<i32>,
    collapsed_size: PhysicalSize<u32>,
    monitor: &tauri::Monitor,
    threshold: i32,
    safe_inset: i32,
    previous: Option<WidgetGeometryState>,
) -> (WidgetRect, DockState) {
    if let Some(previous) = previous {
        let can_reuse_anchor = matches!(previous.mode, WidgetMode::Collapsed)
            || (matches!(previous.mode, WidgetMode::Expanded) && !previous.user_moved_expanded);
        if can_reuse_anchor {
            let position = if previous.dock.is_docked() {
                snap_position(
                    previous.collapsed_rect.position,
                    collapsed_size,
                    previous.dock,
                    monitor,
                    safe_inset,
                )
            } else {
                clamp_position_to_monitor(
                    previous.collapsed_rect.position,
                    collapsed_size,
                    monitor,
                    safe_inset,
                )
            };
            return (
                WidgetRect {
                    position,
                    size: collapsed_size,
                },
                previous.dock,
            );
        }
    }

    let current_collapsed = WidgetRect {
        position: clamp_position_to_monitor(current_position, collapsed_size, monitor, safe_inset),
        size: collapsed_size,
    };
    let dock = detect_dock(
        current_collapsed.position,
        collapsed_size,
        monitor,
        threshold,
        safe_inset,
    );
    let position = if dock.is_docked() {
        snap_position(
            current_collapsed.position,
            collapsed_size,
            dock,
            monitor,
            safe_inset,
        )
    } else {
        current_collapsed.position
    };
    (
        WidgetRect {
            position,
            size: collapsed_size,
        },
        dock,
    )
}

fn current_widget_rect(window: &tauri::WebviewWindow) -> Result<WidgetRect, String> {
    Ok(WidgetRect {
        position: window
            .outer_position()
            .map_err(|_| "failed to read widget position".to_string())?,
        size: window
            .outer_size()
            .map_err(|_| "failed to read widget size".to_string())?,
    })
}

fn monitor_and_scale(
    window: &tauri::WebviewWindow,
) -> Result<(Option<tauri::Monitor>, f64), String> {
    let monitor = window
        .current_monitor()
        .map_err(|_| "failed to read monitor".to_string())?;
    let scale_factor = monitor
        .as_ref()
        .map(|item| item.scale_factor())
        .unwrap_or(1.0);
    Ok((monitor, scale_factor))
}

fn infer_mode(rect: WidgetRect, collapsed_size: PhysicalSize<u32>) -> WidgetMode {
    if rect.size.width <= collapsed_size.width + POSITION_EPSILON
        && rect.size.height <= collapsed_size.height + POSITION_EPSILON
    {
        WidgetMode::Collapsed
    } else {
        WidgetMode::Expanded
    }
}

#[tauri::command]
fn expand_widget(
    work_area: Option<WorkAreaPayload>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let window = app
        .get_webview_window("widget")
        .ok_or_else(|| "widget window missing".to_string())?;
    let current = current_widget_rect(&window)?;
    let (monitor, scale_factor) = monitor_and_scale(&window)?;
    let safe_inset = logical_to_physical(EDGE_SAFE_INSET_LOGICAL, scale_factor);
    let collapsed_size = PhysicalSize::new(
        widget_window_size(COLLAPSED_LOGICAL_SIZE, scale_factor, safe_inset),
        widget_window_size(COLLAPSED_LOGICAL_SIZE, scale_factor, safe_inset),
    );
    let expanded_size = PhysicalSize::new(
        widget_window_size(EXPANDED_LOGICAL_SIZE, scale_factor, safe_inset),
        widget_window_size(EXPANDED_LOGICAL_SIZE, scale_factor, safe_inset),
    );
    let Some(monitor) = monitor else {
        window
            .set_size(expanded_size)
            .map_err(|_| "failed to resize widget".to_string())?;
        return Ok(());
    };
    let threshold = logical_to_physical(SNAP_THRESHOLD_LOGICAL, scale_factor) as i32;
    let previous = state.geometry.lock().ok().and_then(|value| *value);
    let (collapsed_rect, dock) = collapsed_geometry_for_expand(
        current.position,
        collapsed_size,
        &monitor,
        threshold,
        safe_inset as i32,
        previous,
    );
    let expanded_rect = WidgetRect {
        position: expanded_position(
            collapsed_rect,
            expanded_size,
            dock,
            &monitor,
            work_area,
            safe_inset as i32,
        ),
        size: expanded_size,
    };

    if let Ok(mut geometry) = state.geometry.lock() {
        *geometry = Some(WidgetGeometryState {
            mode: WidgetMode::Expanded,
            dock,
            collapsed_rect,
            expanded_rect: Some(expanded_rect),
            user_moved_expanded: false,
        });
    }

    window
        .set_position(expanded_rect.position)
        .map_err(|_| "failed to position widget".to_string())?;
    window
        .set_size(expanded_size)
        .map_err(|_| "failed to resize widget".to_string())
}

#[cfg(test)]
mod geometry_tests {
    use super::*;

    fn rect(x: i32, y: i32, size: u32) -> WidgetRect {
        WidgetRect {
            position: PhysicalPosition::new(x, y),
            size: PhysicalSize::new(size, size),
        }
    }

    #[test]
    fn window_size_includes_the_transparent_safe_inset() {
        assert_eq!(window_size_for_visual_size(80, 4), 88);
        assert_eq!(widget_window_size(320.0, 1.5, 6), 492);
    }

    #[test]
    fn expansion_stays_above_a_bottom_taskbar() {
        let position = expanded_position_in_bounds(
            rect(1812, 952, 88),
            PhysicalSize::new(328, 328),
            DockState {
                horizontal: Some(HorizontalDock::Right),
                vertical: Some(VerticalDock::Bottom),
            },
            PhysicalPosition::new(0, 0),
            PhysicalSize::new(1920, 1040),
            4,
        );
        assert_eq!(position, PhysicalPosition::new(1572, 712));
    }

    #[test]
    fn expansion_handles_negative_origin_work_areas() {
        let position = expanded_position_in_bounds(
            rect(-1284, -4, 88),
            PhysicalSize::new(328, 328),
            DockState {
                horizontal: Some(HorizontalDock::Left),
                vertical: Some(VerticalDock::Top),
            },
            PhysicalPosition::new(-1280, 0),
            PhysicalSize::new(1280, 984),
            4,
        );
        assert_eq!(position, PhysicalPosition::new(-1284, -4));
    }

    #[test]
    fn undocked_expansion_flips_inward_near_work_area_edges() {
        let position = expanded_position_in_bounds(
            rect(1750, 900, 88),
            PhysicalSize::new(328, 328),
            DockState::default(),
            PhysicalPosition::new(0, 0),
            PhysicalSize::new(1920, 1040),
            4,
        );
        assert_eq!(position, PhysicalPosition::new(1510, 660));
    }
}

#[tauri::command]
fn collapse_widget(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let window = app
        .get_webview_window("widget")
        .ok_or_else(|| "widget window missing".to_string())?;
    let current = current_widget_rect(&window)?;
    let (monitor, scale_factor) = monitor_and_scale(&window)?;
    let safe_inset = logical_to_physical(EDGE_SAFE_INSET_LOGICAL, scale_factor);
    let collapsed_size = PhysicalSize::new(
        widget_window_size(COLLAPSED_LOGICAL_SIZE, scale_factor, safe_inset),
        widget_window_size(COLLAPSED_LOGICAL_SIZE, scale_factor, safe_inset),
    );
    let Some(monitor) = monitor else {
        window
            .set_size(collapsed_size)
            .map_err(|_| "failed to resize widget".to_string())?;
        return Ok(());
    };
    let threshold = logical_to_physical(SNAP_THRESHOLD_LOGICAL, scale_factor) as i32;
    let previous = state.geometry.lock().ok().and_then(|value| *value);
    let user_moved_expanded = previous
        .map(|value| value.user_moved_expanded)
        .unwrap_or(false);
    let candidate = if user_moved_expanded {
        current.position
    } else {
        previous
            .map(|value| value.collapsed_rect.position)
            .unwrap_or(current.position)
    };
    let dock = detect_dock(
        candidate,
        collapsed_size,
        &monitor,
        threshold,
        safe_inset as i32,
    );
    let next_position = if dock.is_docked() {
        snap_position(candidate, collapsed_size, dock, &monitor, safe_inset as i32)
    } else {
        clamp_position_to_monitor(candidate, collapsed_size, &monitor, safe_inset as i32)
    };
    let collapsed_rect = WidgetRect {
        position: next_position,
        size: collapsed_size,
    };
    if let Ok(mut geometry) = state.geometry.lock() {
        *geometry = Some(WidgetGeometryState {
            mode: WidgetMode::Collapsed,
            dock,
            collapsed_rect,
            expanded_rect: None,
            user_moved_expanded: false,
        });
    }
    window
        .set_size(collapsed_size)
        .map_err(|_| "failed to resize widget".to_string())?;
    window
        .set_position(next_position)
        .map_err(|_| "failed to position widget".to_string())
}

#[tauri::command]
fn begin_widget_drag(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let window = app
        .get_webview_window("widget")
        .ok_or_else(|| "widget window missing".to_string())?;
    let current = current_widget_rect(&window)?;
    let (_, scale_factor) = monitor_and_scale(&window)?;
    let safe_inset = logical_to_physical(EDGE_SAFE_INSET_LOGICAL, scale_factor);
    let collapsed_size = PhysicalSize::new(
        widget_window_size(COLLAPSED_LOGICAL_SIZE, scale_factor, safe_inset),
        widget_window_size(COLLAPSED_LOGICAL_SIZE, scale_factor, safe_inset),
    );
    let mode = state
        .geometry
        .lock()
        .ok()
        .and_then(|value| *value)
        .map(|value| value.mode)
        .unwrap_or_else(|| infer_mode(current, collapsed_size));
    if let Ok(mut drag_mode) = state.drag_mode.lock() {
        *drag_mode = Some(mode);
    }
    Ok(())
}

#[tauri::command]
fn finish_widget_drag(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let window = app
        .get_webview_window("widget")
        .ok_or_else(|| "widget window missing".to_string())?;
    let current = current_widget_rect(&window)?;
    let (monitor, scale_factor) = monitor_and_scale(&window)?;
    let Some(monitor) = monitor else {
        return Ok(());
    };
    let threshold = logical_to_physical(SNAP_THRESHOLD_LOGICAL, scale_factor) as i32;
    let safe_inset = logical_to_physical(EDGE_SAFE_INSET_LOGICAL, scale_factor);
    let collapsed_size = PhysicalSize::new(
        widget_window_size(COLLAPSED_LOGICAL_SIZE, scale_factor, safe_inset),
        widget_window_size(COLLAPSED_LOGICAL_SIZE, scale_factor, safe_inset),
    );
    let expanded_size = PhysicalSize::new(
        widget_window_size(EXPANDED_LOGICAL_SIZE, scale_factor, safe_inset),
        widget_window_size(EXPANDED_LOGICAL_SIZE, scale_factor, safe_inset),
    );
    let mode = state
        .drag_mode
        .lock()
        .ok()
        .and_then(|mut value| value.take())
        .or_else(|| {
            state
                .geometry
                .lock()
                .ok()
                .and_then(|value| *value)
                .map(|value| value.mode)
        })
        .unwrap_or_else(|| infer_mode(current, collapsed_size));

    match mode {
        WidgetMode::Collapsed => {
            let dock = detect_dock(
                current.position,
                collapsed_size,
                &monitor,
                threshold,
                safe_inset as i32,
            );
            let next_position = if dock.is_docked() {
                snap_position(
                    current.position,
                    collapsed_size,
                    dock,
                    &monitor,
                    safe_inset as i32,
                )
            } else {
                clamp_position_to_monitor(
                    current.position,
                    collapsed_size,
                    &monitor,
                    safe_inset as i32,
                )
            };
            let collapsed_rect = WidgetRect {
                position: next_position,
                size: collapsed_size,
            };
            window
                .set_position(next_position)
                .map_err(|_| "failed to position widget".to_string())?;
            if let Ok(mut geometry) = state.geometry.lock() {
                *geometry = Some(WidgetGeometryState {
                    mode: WidgetMode::Collapsed,
                    dock,
                    collapsed_rect,
                    expanded_rect: None,
                    user_moved_expanded: false,
                });
            }
        }
        WidgetMode::Expanded => {
            let current_position = clamp_position_to_monitor(
                current.position,
                expanded_size,
                &monitor,
                safe_inset as i32,
            );
            let updated_rect = WidgetRect {
                position: current_position,
                size: expanded_size,
            };
            window
                .set_position(current_position)
                .map_err(|_| "failed to position widget".to_string())?;
            if let Ok(mut geometry) = state.geometry.lock() {
                if let Some(mut value) = *geometry {
                    value.mode = WidgetMode::Expanded;
                    value.expanded_rect = Some(updated_rect);
                    value.user_moved_expanded = true;
                    *geometry = Some(value);
                }
            }
        }
    }
    Ok(())
}

#[tauri::command]
fn get_preferences(state: State<'_, AppState>) -> Result<WidgetPreferences, String> {
    state
        .preferences
        .lock()
        .map(|value| value.clone())
        .map_err(|_| "settings unavailable".into())
}

#[tauri::command]
fn get_completion_shutdown_state(
    state: State<'_, AppState>,
) -> Result<CompletionShutdownView, String> {
    state
        .completion_shutdown
        .lock()
        .map(|value| CompletionShutdownView {
            armed: value.enabled(),
        })
        .map_err(|_| "完成后关机状态不可用。".into())
}

#[tauri::command]
fn set_completion_shutdown_armed(
    armed: bool,
    state: State<'_, AppState>,
) -> Result<CompletionShutdownView, String> {
    let preferences = preferences(state.inner());
    if armed {
        if preferences.sync_role != "collector" {
            return Err("仅 Collector 可以启用完成后关机。".into());
        }
        shutdown::validate_script_path(&preferences.shutdown_script_path)?;
    }

    let value = state
        .completion_shutdown
        .lock()
        .map_err(|_| "完成后关机状态不可用。".to_string())
        .map(|mut completion_shutdown| {
            completion_shutdown.set_enabled(armed);
            CompletionShutdownView { armed }
        })?;
    emit_completion_shutdown_changed(state.inner());
    Ok(value)
}

/// 单独接受 Collector 的写入密钥，避免把密钥序列化回 WebView。
/// 空值是“保持现有密钥”的无操作，绝不用于清空已保存的密钥。
#[tauri::command]
fn set_collector_write_secret(secret: String, state: State<'_, AppState>) -> Result<(), String> {
    if secret.is_empty() {
        return Ok(());
    }

    let mut next = state
        .preferences
        .lock()
        .map_err(|_| "settings unavailable".to_string())?
        .clone();
    if next.sync_role != "collector" {
        return Err("仅 Collector 可以保存写入密钥。".into());
    }
    next.write_secret = secret;
    persist_preferences(&state.preferences_path, &next)?;
    *state
        .preferences
        .lock()
        .map_err(|_| "settings unavailable".to_string())? = next;
    // 仅修改密钥时，preferences-changed 的依赖项不会变化；主动刷新可让
    // 新密钥立即用于下一次同步，而不是等待定时器。
    let _ = state.app_handle.emit_to("widget", "refresh-requested", ());
    Ok(())
}

#[tauri::command]
fn set_preferences(
    preferences: WidgetPreferences,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let previous = state
        .preferences
        .lock()
        .map_err(|_| "settings unavailable".to_string())?
        .clone();
    let next = preserve_or_clear_write_secret(preferences.normalized(), &previous);
    persist_preferences(&state.preferences_path, &next)?;
    *state
        .preferences
        .lock()
        .map_err(|_| "settings unavailable".to_string())? = next.clone();

    // 设置变更后不能继续把旧服务器或旧角色的快照展示为当前结果。
    if let Ok(mut cache) = state.dashboard_cache.lock() {
        *cache = None;
    }
    if previous.server_url != next.server_url
        || previous.source_id != next.source_id
        || previous.sync_role != next.sync_role
    {
        if let Ok(mut cache) = state.remote_cache.lock() {
            *cache = None;
        }
    }

    // Hooks 文件、角色或脚本被切换时，旧任务基线不再可信。要求用户重新武装，
    // 避免新配置首次读到 0 时触发错误的本机脚本。
    let should_disarm_completion_shutdown = previous.sync_role != next.sync_role
        || previous.activity_state_path != next.activity_state_path
        || previous.shutdown_script_path != next.shutdown_script_path;
    if should_disarm_completion_shutdown {
        if let Ok(mut completion_shutdown) = state.completion_shutdown.lock() {
            completion_shutdown.disable();
        }
    }
    emit_preferences_changed(state.inner(), &next);
    if should_disarm_completion_shutdown {
        emit_completion_shutdown_changed(state.inner());
    }
    Ok(())
}

#[tauri::command]
fn show_settings(app: AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("settings")
        .ok_or_else(|| "settings window missing".to_string())?;
    window
        .show()
        .map_err(|_| "failed to show settings".to_string())?;
    window
        .set_focus()
        .map_err(|_| "failed to focus settings".to_string())
}

fn apply_lock(app: &AppHandle, locked: bool) -> Result<(), String> {
    let window = app
        .get_webview_window("widget")
        .ok_or_else(|| "widget window missing".to_string())?;
    window
        .set_ignore_cursor_events(locked)
        .map_err(|_| "failed to toggle click-through".to_string())
}

#[tauri::command]
fn set_widget_locked(
    locked: bool,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<WidgetPreferences, String> {
    let previous = state
        .preferences
        .lock()
        .map_err(|_| "settings unavailable".to_string())?
        .clone();
    let mut next = previous.clone();
    next.locked = locked;
    persist_preferences(&state.preferences_path, &next)?;
    if let Err(error) = apply_lock(&app, locked) {
        let _ = persist_preferences(&state.preferences_path, &previous);
        return Err(error);
    }
    *state
        .preferences
        .lock()
        .map_err(|_| "settings unavailable".to_string())? = next.clone();
    Ok(next)
}

#[tauri::command]
fn set_widget_always_on_top(
    always_on_top: bool,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<WidgetPreferences, String> {
    let previous = state
        .preferences
        .lock()
        .map_err(|_| "settings unavailable".to_string())?
        .clone();
    let mut next = previous.clone();
    next.always_on_top = always_on_top;
    persist_preferences(&state.preferences_path, &next)?;
    let window = app
        .get_webview_window("widget")
        .ok_or_else(|| "widget window missing".to_string())?;
    if let Err(error) = window.set_always_on_top(always_on_top) {
        let _ = persist_preferences(&state.preferences_path, &previous);
        return Err(format!("failed to toggle always-on-top: {error}"));
    }
    *state
        .preferences
        .lock()
        .map_err(|_| "settings unavailable".to_string())? = next.clone();
    let _ = app.emit_to("widget", "preferences-changed", next.clone());
    Ok(next)
}

fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show / Hide", true, None::<&str>)?;
    let refresh = MenuItem::with_id(app, "refresh", "Refresh now", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
    let unlock = MenuItem::with_id(app, "unlock", "Unlock widget", true, None::<&str>)?;
    let pin = MenuItem::with_id(app, "pin", "Pin / Unpin Codex", true, None::<&str>)?;
    let language = MenuItem::with_id(
        app,
        "language",
        "Switch Language / 切换语言",
        true,
        None::<&str>,
    )?;
    let autostart_enabled = app.autolaunch().is_enabled().unwrap_or(false);
    let autostart = CheckMenuItem::with_id(
        app,
        "autostart",
        "Start at login",
        true,
        autostart_enabled,
        None::<&str>,
    )?;
    #[cfg(debug_assertions)]
    let test_short_window = CheckMenuItem::with_id(
        app,
        "debug-short-window",
        "Test: simulate 5-hour quota",
        true,
        false,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let initial_language = app
        .try_state::<AppState>()
        .and_then(|state| {
            state
                .preferences
                .lock()
                .ok()
                .map(|prefs| prefs.language.clone())
        })
        .unwrap_or_else(|| "zh-CN".into());
    if initial_language != "en" {
        let _ = show.set_text("显示 / 隐藏");
        let _ = refresh.set_text("立即刷新");
        let _ = settings.set_text("设置");
        let _ = unlock.set_text("解锁悬浮窗");
        let _ = pin.set_text("固定 / 取消固定 Codex");
        let _ = language.set_text("Switch to English");
        let _ = autostart.set_text("开机启动");
        let _ = quit.set_text("退出");
    }
    #[cfg(debug_assertions)]
    let menu = Menu::with_items(
        app,
        &[
            &show,
            &refresh,
            &settings,
            &unlock,
            &pin,
            &language,
            &autostart,
            &test_short_window,
            &quit,
        ],
    )?;
    #[cfg(not(debug_assertions))]
    let menu = Menu::with_items(
        app,
        &[
            &show, &refresh, &settings, &unlock, &pin, &language, &autostart, &quit,
        ],
    )?;
    let mut builder = TrayIconBuilder::with_id("main")
        .menu(&menu)
        .tooltip("Codex Quota Sync");
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    let autostart_menu = autostart.clone();
    let show_menu = show.clone();
    let refresh_menu = refresh.clone();
    let settings_menu = settings.clone();
    let unlock_menu = unlock.clone();
    let pin_menu = pin.clone();
    let language_menu = language.clone();
    let quit_menu = quit.clone();
    #[cfg(debug_assertions)]
    let test_short_window_menu = test_short_window.clone();
    builder
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "show" => {
                if let Some(window) = app.get_webview_window("widget") {
                    if window.is_visible().unwrap_or(false) {
                        let _ = window.hide();
                    } else {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
            }
            "refresh" => {
                let _ = app.emit_to("widget", "refresh-requested", ());
            }
            "settings" => {
                if let Some(window) = app.get_webview_window("settings") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "debug-short-window" =>
            {
                #[cfg(debug_assertions)]
                if let Some(state) = app.try_state::<AppState>() {
                    if let Ok(mut enabled) = state.simulate_short_window_for_testing.lock() {
                        *enabled = !*enabled;
                        let _ = test_short_window_menu.set_checked(*enabled);
                        let _ = app.emit_to("widget", "refresh-requested", ());
                    }
                }
            }
            "unlock" => {
                let _ = apply_lock(app, false);
                if let Some(state) = app.try_state::<AppState>() {
                    if let Ok(mut prefs) = state.preferences.lock() {
                        prefs.locked = false;
                        let _ = persist_preferences(&state.preferences_path, &prefs);
                        let _ = app.emit_to("widget", "preferences-changed", prefs.clone());
                    }
                }
            }
            "pin" => {
                if let Some(state) = app.try_state::<AppState>() {
                    if let Ok(mut prefs) = state.preferences.lock() {
                        prefs.pinned_provider = if prefs.pinned_provider.is_some() {
                            None
                        } else {
                            Some("codex".into())
                        };
                        let _ = persist_preferences(&state.preferences_path, &prefs);
                        let _ = app.emit_to("widget", "preferences-changed", prefs.clone());
                    }
                }
            }
            "language" => {
                if let Some(state) = app.try_state::<AppState>() {
                    if let Ok(mut prefs) = state.preferences.lock() {
                        prefs.language = if prefs.language == "en" {
                            "zh-CN".into()
                        } else {
                            "en".into()
                        };
                        let normalized = prefs.clone().normalized();
                        *prefs = normalized.clone();
                        let _ = persist_preferences(&state.preferences_path, &normalized);
                        let english = normalized.language == "en";
                        let _ = show_menu.set_text(if english {
                            "Show / Hide"
                        } else {
                            "显示 / 隐藏"
                        });
                        let _ = refresh_menu.set_text(if english {
                            "Refresh now"
                        } else {
                            "立即刷新"
                        });
                        let _ = settings_menu.set_text(if english { "Settings" } else { "设置" });
                        let _ = unlock_menu.set_text(if english {
                            "Unlock widget"
                        } else {
                            "解锁悬浮窗"
                        });
                        let _ = pin_menu.set_text(if english {
                            "Pin / Unpin Codex"
                        } else {
                            "固定 / 取消固定 Codex"
                        });
                        let _ = language_menu.set_text(if english {
                            "切换到中文"
                        } else {
                            "Switch to English"
                        });
                        let _ = autostart_menu.set_text(if english {
                            "Start at login"
                        } else {
                            "开机启动"
                        });
                        let _ = quit_menu.set_text(if english { "Quit" } else { "退出" });
                        let _ = app.emit_to("widget", "preferences-changed", normalized);
                    }
                }
            }
            "autostart" => {
                let manager = app.autolaunch();
                let enabled = manager.is_enabled().unwrap_or(false);
                let result = if enabled {
                    manager.disable()
                } else {
                    manager.enable()
                };
                match result {
                    Ok(()) => {
                        let _ = autostart_menu.set_checked(!enabled);
                    }
                    Err(_) => eprintln!("autostart update failed"),
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(window) = app.get_webview_window("widget") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(WindowStateBuilder::default().build())
        .setup(|app| {
            let data_dir = app.path().app_config_dir()?;
            let preferences_path = data_dir.join("preferences.json");
            let preferences = load_preferences(&preferences_path);
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(12))
                .redirect(reqwest::redirect::Policy::none())
                .user_agent(concat!("CodexQuotaSync/", env!("CARGO_PKG_VERSION")))
                .build()
                .expect("static HTTP client configuration must be valid");
            app.manage(AppState {
                app_handle: app.handle().clone(),
                client,
                preferences: Mutex::new(preferences.clone()),
                preferences_path,
                fetch_lock: tokio::sync::Mutex::new(()),
                dashboard_cache: Mutex::new(None),
                quota_cache: Mutex::new(None),
                last_good_snapshot: Mutex::new(None),
                remote_cache: Mutex::new(None),
                revision: AtomicU64::new(0),
                completion_shutdown: Mutex::new(shutdown::ShutdownArm::default()),
                #[cfg(debug_assertions)]
                simulate_short_window_for_testing: Mutex::new(false),
                geometry: Mutex::new(None),
                drag_mode: Mutex::new(None),
            });
            if setup_tray(app).is_err() {
                eprintln!("tray setup failed; enabling taskbar fallback");
                if let Some(window) = app.get_webview_window("widget") {
                    let _ = window.set_skip_taskbar(false);
                }
            }
            if preferences.locked {
                let _ = apply_lock(app.handle(), true);
            }
            if let Some(window) = app.get_webview_window("widget") {
                let _ = window.set_always_on_top(preferences.always_on_top);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_snapshots,
            refresh_snapshots,
            expand_widget,
            collapse_widget,
            begin_widget_drag,
            finish_widget_drag,
            get_preferences,
            set_preferences,
            get_completion_shutdown_state,
            set_completion_shutdown_armed,
            set_collector_write_secret,
            show_settings,
            set_widget_locked,
            set_widget_always_on_top
        ])
        .on_tray_icon_event(|app, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                if let Some(window) = app.get_webview_window("widget") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("failed to build Codex Quota Sync");
    app.run(|app_handle, event| {
        if matches!(event, tauri::RunEvent::Resumed) {
            let _ = app_handle.emit_to("widget", "refresh-requested", ());
        }
    });
}
