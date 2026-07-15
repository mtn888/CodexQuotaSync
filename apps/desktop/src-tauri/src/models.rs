use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageWindow {
    pub remaining_percent: f64,
    pub resets_at: Option<String>,
    pub window_seconds: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSnapshot {
    pub provider: String,
    pub display_name: String,
    pub plan: Option<String>,
    pub short_window: Option<UsageWindow>,
    pub weekly_window: Option<UsageWindow>,
    pub reset_credits: Option<u64>,
    #[serde(default)]
    pub reset_credit_expires_at: Vec<String>,
    pub updated_at: String,
    pub status: String,
    pub message: Option<String>,
    #[serde(default)]
    pub next_reset_at: Option<String>,
    #[serde(default)]
    pub next_reset_window: Option<String>,
}

impl ProviderSnapshot {
    pub fn failure(status: &str, message: &str) -> Self {
        Self {
            provider: "codex".into(),
            display_name: "CODEX".into(),
            plan: None,
            short_window: None,
            weekly_window: None,
            reset_credits: None,
            reset_credit_expires_at: Vec::new(),
            updated_at: Utc::now().to_rfc3339(),
            status: status.into(),
            message: Some(message.into()),
            next_reset_at: None,
            next_reset_window: None,
        }
    }

    pub fn with_derived_next_reset(mut self) -> Self {
        let now = Utc::now();
        let candidates = [
            (
                "5h",
                self.short_window
                    .as_ref()
                    .and_then(|window| window.resets_at.as_deref()),
            ),
            (
                "weekly",
                self.weekly_window
                    .as_ref()
                    .and_then(|window| window.resets_at.as_deref()),
            ),
        ];
        let next = candidates
            .into_iter()
            .filter_map(|(label, value)| {
                let raw = value?;
                let parsed = DateTime::parse_from_rfc3339(raw).ok()?.with_timezone(&Utc);
                (parsed > now).then_some((label, raw.to_string(), parsed))
            })
            .min_by_key(|(_, _, parsed)| *parsed);
        if let Some((label, raw, _)) = next {
            self.next_reset_at = Some(raw);
            self.next_reset_window = Some(label.into());
        } else {
            self.next_reset_at = None;
            self.next_reset_window = None;
        }
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivitySnapshot {
    pub executing: u64,
    pub waiting_on_approval: u64,
    pub waiting_on_user_input: u64,
    pub source: String,
    pub observed_at: String,
    pub stale: bool,
}

impl ActivitySnapshot {
    pub fn unavailable() -> Self {
        Self {
            executing: 0,
            waiting_on_approval: 0,
            waiting_on_user_input: 0,
            source: "unavailable".into(),
            observed_at: Utc::now().to_rfc3339(),
            stale: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncAttempt {
    pub status: String,
    pub message: Option<String>,
    pub attempted_at: String,
}

impl SyncAttempt {
    pub fn from_snapshot(snapshot: &ProviderSnapshot) -> Self {
        Self {
            status: snapshot.status.clone(),
            message: snapshot.message.clone(),
            attempted_at: snapshot.updated_at.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncEnvelope {
    pub schema_version: u32,
    pub source_id: String,
    pub revision: u64,
    pub collector_version: String,
    pub collected_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub received_at: Option<String>,
    pub activity: ActivitySnapshot,
    pub latest_attempt: SyncAttempt,
    pub last_good_snapshot: Option<ProviderSnapshot>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncView {
    pub role: String,
    pub state: String,
    pub source_id: String,
    pub collected_at: Option<String>,
    pub received_at: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopSnapshot {
    #[serde(flatten)]
    pub quota: ProviderSnapshot,
    pub activity: ActivitySnapshot,
    pub sync: SyncView,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetPreferences {
    pub locked: bool,
    #[serde(default = "default_always_on_top")]
    pub always_on_top: bool,
    #[serde(default)]
    pub stay_expanded: bool,
    pub pinned_provider: Option<String>,
    pub auto_rotate_seconds: u64,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default = "default_sync_role")]
    pub sync_role: String,
    #[serde(default)]
    pub server_url: String,
    #[serde(default = "default_source_id")]
    pub source_id: String,
    #[serde(default, skip_serializing)]
    pub write_secret: String,
    #[serde(default)]
    pub activity_state_path: String,
}

fn default_always_on_top() -> bool {
    true
}
fn default_language() -> String {
    "zh-CN".into()
}
fn default_sync_role() -> String {
    "collector".into()
}
fn default_source_id() -> String {
    "windows-main".into()
}

impl Default for WidgetPreferences {
    fn default() -> Self {
        Self {
            locked: false,
            always_on_top: true,
            stay_expanded: false,
            pinned_provider: None,
            auto_rotate_seconds: 12,
            language: default_language(),
            sync_role: default_sync_role(),
            server_url: String::new(),
            source_id: default_source_id(),
            write_secret: String::new(),
            activity_state_path: String::new(),
        }
    }
}

impl WidgetPreferences {
    pub fn normalized(mut self) -> Self {
        self.auto_rotate_seconds = self.auto_rotate_seconds.clamp(5, 300);
        if self.pinned_provider.as_deref() != Some("codex") {
            self.pinned_provider = None;
        }
        if self.language != "en" && self.language != "zh-CN" {
            self.language = default_language();
        }
        if self.sync_role != "viewer" {
            self.sync_role = default_sync_role();
        }
        self.server_url = self.server_url.trim().trim_end_matches('/').to_string();
        self.source_id = self.source_id.trim().to_string();
        if self.source_id.is_empty() {
            self.source_id = default_source_id();
        }
        self.activity_state_path = self.activity_state_path.trim().to_string();
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_the_nearest_future_reset() {
        let now = Utc::now();
        let snapshot = ProviderSnapshot {
            provider: "codex".into(),
            display_name: "CODEX".into(),
            plan: None,
            short_window: Some(UsageWindow {
                remaining_percent: 50.0,
                resets_at: Some((now + chrono::Duration::hours(4)).to_rfc3339()),
                window_seconds: Some(18_000),
            }),
            weekly_window: Some(UsageWindow {
                remaining_percent: 50.0,
                resets_at: Some((now + chrono::Duration::hours(2)).to_rfc3339()),
                window_seconds: Some(604_800),
            }),
            reset_credits: None,
            reset_credit_expires_at: Vec::new(),
            updated_at: now.to_rfc3339(),
            status: "ok".into(),
            message: None,
            next_reset_at: None,
            next_reset_window: None,
        }
        .with_derived_next_reset();
        assert_eq!(snapshot.next_reset_window.as_deref(), Some("weekly"));
    }

    #[test]
    fn normalizes_viewer_and_http_settings_without_exposing_defaults() {
        let prefs = WidgetPreferences {
            sync_role: "viewer".into(),
            server_url: " http://nas.example:8787/ ".into(),
            source_id: " ".into(),
            ..WidgetPreferences::default()
        }
        .normalized();
        assert_eq!(prefs.sync_role, "viewer");
        assert_eq!(prefs.server_url, "http://nas.example:8787");
        assert_eq!(prefs.source_id, "windows-main");
    }

    #[test]
    fn accepts_a_null_window_duration_from_the_v1_schema() {
        let window: UsageWindow =
            serde_json::from_str(r#"{"remainingPercent":50,"resetsAt":null,"windowSeconds":null}"#)
                .unwrap();
        assert_eq!(window.window_seconds, None);
    }
}
