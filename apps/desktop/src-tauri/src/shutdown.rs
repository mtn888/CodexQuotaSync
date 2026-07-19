use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use crate::models::ActivitySnapshot;

/// 一次性的“任务完成后关机”运行期状态。
///
/// 该状态绝不写入 preferences.json：应用重启后必须重新由用户武装，
/// 这样启动时的空任务快照不会触发关机。
#[derive(Debug, Default)]
pub struct ShutdownArm {
    enabled: bool,
    armed: bool,
    last_fresh_executing: Option<u64>,
}

impl ShutdownArm {
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        self.armed = enabled && self.last_fresh_executing.is_some_and(|value| value > 0);
    }

    pub fn disable(&mut self) {
        self.enabled = false;
        self.armed = false;
        self.last_fresh_executing = None;
    }

    /// 仅在可信的 Collector Hooks 快照里检测“执行中 > 0 → 0”。
    /// 返回 true 表示本次应立即消耗武装并启动脚本。
    pub fn observe(&mut self, role: &str, activity: &ActivitySnapshot) -> bool {
        if role != "collector" {
            self.disable();
            return false;
        }
        if activity.stale || activity.source != "hooks" {
            // 断联或过期数据不能作为完成依据，也不要保留旧基线。
            self.armed = false;
            self.last_fresh_executing = None;
            return false;
        }

        let previous = self.last_fresh_executing;
        self.last_fresh_executing = Some(activity.executing);

        if !self.enabled {
            return false;
        }

        let completed =
            self.armed && previous.is_some_and(|value| value > 0) && activity.executing == 0;
        if completed {
            // 一次性消耗，脚本启动失败时也不在下一轮重复尝试。
            self.set_enabled(false);
            return true;
        }

        if activity.executing > 0 {
            self.armed = true;
        }
        false
    }
}

pub fn validate_script_path(value: &str) -> Result<PathBuf, String> {
    let raw = value.trim();
    if raw.is_empty() {
        return Err("未配置完成后关机脚本。".into());
    }

    let path = Path::new(raw);
    if !path.is_absolute() {
        return Err("完成后关机脚本必须使用绝对路径。".into());
    }

    let canonical = path
        .canonicalize()
        .map_err(|_| "完成后关机脚本不存在或无法访问。".to_string())?;
    if !canonical.is_file() {
        return Err("完成后关机脚本必须是普通文件。".into());
    }
    let extension = canonical
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if !extension.eq_ignore_ascii_case("cmd") && !extension.eq_ignore_ascii_case("bat") {
        return Err("完成后关机脚本仅支持 .cmd 或 .bat 文件。".into());
    }
    Ok(canonical)
}

pub fn launch_script(value: &str) -> Result<(), String> {
    let path = validate_script_path(value)?;

    #[cfg(windows)]
    {
        Command::new(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|_| "无法启动完成后关机脚本。".to_string())
    }

    #[cfg(not(windows))]
    {
        let _ = path;
        Err("完成后关机仅支持 Windows。".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    fn fresh_activity(executing: u64) -> ActivitySnapshot {
        ActivitySnapshot {
            executing,
            waiting_on_approval: 0,
            waiting_on_user_input: 0,
            source: "hooks".into(),
            observed_at: "2026-07-19T00:00:00Z".into(),
            stale: false,
        }
    }

    #[test]
    fn defaults_to_disabled_and_does_not_fire_for_an_initial_idle_snapshot() {
        let mut arm = ShutdownArm::default();
        assert!(!arm.enabled());
        assert!(!arm.observe("collector", &fresh_activity(0)));
        arm.set_enabled(true);
        assert!(!arm.observe("collector", &fresh_activity(0)));
    }

    #[test]
    fn fires_once_after_a_fresh_running_to_idle_transition() {
        let mut arm = ShutdownArm::default();
        assert!(!arm.observe("collector", &fresh_activity(3)));
        arm.set_enabled(true);
        assert!(!arm.observe("collector", &fresh_activity(2)));
        assert!(arm.observe("collector", &fresh_activity(0)));
        assert!(!arm.enabled());
        assert!(!arm.observe("collector", &fresh_activity(0)));
        assert!(!arm.observe("collector", &fresh_activity(1)));
        assert!(!arm.observe("collector", &fresh_activity(0)));
    }

    #[test]
    fn waits_for_a_task_started_after_being_enabled_from_idle() {
        let mut arm = ShutdownArm::default();
        assert!(!arm.observe("collector", &fresh_activity(0)));
        arm.set_enabled(true);
        assert!(!arm.observe("collector", &fresh_activity(0)));
        assert!(!arm.observe("collector", &fresh_activity(2)));
        assert!(arm.observe("collector", &fresh_activity(0)));
    }

    #[test]
    fn stale_or_viewer_activity_clears_the_baseline_without_firing() {
        let mut arm = ShutdownArm::default();
        assert!(!arm.observe("collector", &fresh_activity(3)));
        arm.set_enabled(true);
        let mut stale = fresh_activity(0);
        stale.stale = true;
        assert!(!arm.observe("collector", &stale));
        assert!(!arm.observe("collector", &fresh_activity(0)));

        arm.set_enabled(true);
        assert!(!arm.observe("viewer", &fresh_activity(3)));
        assert!(!arm.enabled());
    }

    #[test]
    fn validates_an_existing_batch_file_without_launching_it() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("codex-quota-sync-{nonce}.cmd"));
        fs::write(&path, "@echo off\r\n").unwrap();
        let validated = validate_script_path(path.to_string_lossy().as_ref()).unwrap();
        assert!(validated.is_file());
        assert!(validated
            .extension()
            .is_some_and(|value| value.to_string_lossy().eq_ignore_ascii_case("cmd")));
        fs::remove_file(path).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn launches_a_cmd_file_without_using_the_configured_shutdown_script() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("codex-quota-sync-launch-{nonce}"));
        fs::create_dir(&root).unwrap();
        let script = root.join("write marker.cmd");
        let marker = root.join("marker.txt");
        fs::write(
            &script,
            format!("@echo off\r\n>\"{}\" echo launched\r\n", marker.display()),
        )
        .unwrap();

        launch_script(script.to_string_lossy().as_ref()).unwrap();
        for _ in 0..50 {
            if marker.exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(fs::read_to_string(&marker).unwrap().trim(), "launched");

        fs::remove_file(marker).unwrap();
        fs::remove_file(script).unwrap();
        fs::remove_dir(root).unwrap();
    }
}
