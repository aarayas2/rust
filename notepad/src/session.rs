use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct SessionBounds {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SessionTab {
    pub file: Option<PathBuf>,
    pub is_dirty: bool,
    pub cursor: (u32, u32),
    pub scroll: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_from_backup: Option<bool>,
    #[serde(default)]
    pub undo_history: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct SessionWindow {
    pub folders: Vec<PathBuf>,
    pub bounds: SessionBounds,
    pub tabs: Vec<SessionTab>,
    pub active_tab_index: usize,
    #[serde(default)]
    pub layout: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Session {
    pub version: u32,
    pub windows: Vec<SessionWindow>,
    pub last_saved_at: String,
}

impl Default for Session {
    fn default() -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let time_str = format!("1970-01-01T00:00:{:02}Z", now % 60);

        Self {
            version: 1,
            windows: vec![],
            last_saved_at: time_str,
        }
    }
}

pub fn get_session_dir() -> PathBuf {
    let mut path = if cfg!(target_os = "windows") {
        PathBuf::from(std::env::var("APPDATA").unwrap_or_else(|_| ".".into()))
    } else {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        let mut p = PathBuf::from(home);
        p.push(".config");
        p
    };
    path.push("TextEditor");
    path
}

pub fn get_session_file() -> PathBuf {
    let mut path = get_session_dir();
    path.push("Session.sublime_session");
    path
}

pub fn get_hot_exit_setting() -> bool {
    true
}
