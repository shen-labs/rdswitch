//! 使用者設定（目前只有語言）。存在 app 資料目錄的 settings.json。

use crate::i18n::trf;
use crate::platform;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// "auto"（跟隨系統）或語言代碼（zh-TW / en / zh-CN / ja）
    pub language: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { language: "auto".into() }
    }
}

fn path() -> PathBuf {
    platform::app_data_dir().join("settings.json")
}

pub fn load() -> Settings {
    fs::read_to_string(path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub fn save(s: &Settings) -> Result<(), String> {
    let p = path();
    if let Some(dir) = p.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    fs::write(&p, serde_json::to_string_pretty(s).unwrap())
        .map_err(|e| trf("err.write_failed", &[("path", &p.display().to_string()), ("err", &e.to_string())]))
}
