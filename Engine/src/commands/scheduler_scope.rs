use sha2::{Digest, Sha256};
use std::path::Path;
use tauri::{AppHandle, Manager};

#[derive(Clone, Copy)]
pub(super) enum TaskKind {
    Reminders,
    Cron,
}

pub(super) struct SchedulerScope {
    folder: String,
}

impl SchedulerScope {
    pub(super) fn from_app(app: &AppHandle) -> Result<Self, String> {
        let data_dir = app
            .path()
            .app_data_dir()
            .map_err(|e| format!("Cannot resolve scheduler scope: {e}"))?;
        Self::from_data_dir(&data_dir)
    }

    fn from_data_dir(data_dir: &Path) -> Result<Self, String> {
        if data_dir.as_os_str().is_empty() {
            return Err("Cannot resolve scheduler scope: empty data directory".to_string());
        }
        let absolute = dunce::canonicalize(data_dir)
            .map_err(|e| format!("Cannot resolve scheduler scope: {e}"))?;
        let normalized = if cfg!(windows) {
            absolute.to_string_lossy().replace('/', "\\").to_lowercase()
        } else {
            absolute.to_string_lossy().to_string()
        };
        let digest = Sha256::digest(normalized.as_bytes());
        Ok(Self {
            folder: format!("\\Search\\{}\\", hex::encode(digest)),
        })
    }

    pub(super) fn task_path(&self, kind: TaskKind) -> String {
        let leaf = match kind {
            TaskKind::Reminders => "Reminders",
            TaskKind::Cron => "Cron",
        };
        format!("{}{}\\", self.folder, leaf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheduler_paths_isolate_data_roots_and_task_kinds() {
        let fixture =
            std::env::temp_dir().join(format!("search-scheduler-test-{}", std::process::id()));
        let world_a = fixture.join("world-a");
        let world_b = fixture.join("world-b");
        std::fs::create_dir_all(&world_a).unwrap();
        std::fs::create_dir_all(&world_b).unwrap();
        let a = SchedulerScope::from_data_dir(&world_a).unwrap();
        let b = SchedulerScope::from_data_dir(&world_b).unwrap();
        let a_cron = a.task_path(TaskKind::Cron);
        let a_reminders = a.task_path(TaskKind::Reminders);
        let b_cron = b.task_path(TaskKind::Cron);
        let equivalent = SchedulerScope::from_data_dir(&world_a.join(".")).unwrap();
        assert!(a_cron.starts_with("\\Search\\"));
        assert!(a_cron.ends_with("\\Cron\\"));
        assert!(a_reminders.ends_with("\\Reminders\\"));
        assert_ne!(a_cron, a_reminders);
        assert_ne!(a_cron, b_cron);
        assert_eq!(a_cron, equivalent.task_path(TaskKind::Cron));
        assert!(SchedulerScope::from_data_dir(Path::new("")).is_err());
        std::fs::remove_dir_all(fixture).unwrap();
    }
}
