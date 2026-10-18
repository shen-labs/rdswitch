//! Profile 儲存與套用。
//!
//! 套用方式依 RustDesk 的安裝型態而定：
//! - 已安裝為系統服務：執行中的服務行程把設定放在記憶體，只改檔案不會生效
//!   （實測：檔案已改，服務仍連舊伺服器）。必須用 `rustdesk --option`（需系統管理員/root）通知它。
//! - 免安裝/無服務：關閉 GUI -> 備份 -> 原子寫入設定檔 -> 重啟 GUI。

use crate::i18n::{tr, trf};
use crate::platform::{self, CONFIG_NAME};
use crate::tomledit::{self, Change};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// 欄位留空 = 回到官方預設。
#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
#[serde(default)]
pub struct Profile {
    pub name: String,
    pub server: String, // ID/Rendezvous 伺服器，例如 host:21116
    pub relay: String,
    pub api: String,
    pub key: String, // 公鑰
}

impl Profile {
    fn changes(&self) -> Vec<Change> {
        let opt = |v: &str| (!v.is_empty()).then(|| v.to_string());
        vec![
            Change {
                key: "custom-rendezvous-server",
                value: opt(&self.server),
            },
            Change {
                key: "relay-server",
                value: opt(&self.relay),
            },
            Change {
                key: "api-server",
                value: opt(&self.api),
            },
            Change {
                key: "key",
                value: opt(&self.key),
            },
        ]
    }
}

/// 內建的預設 profile（所有欄位留空 = RustDesk 官方預設），不存檔、不可修改或刪除。
pub const DEFAULT_NAME: &str = "Default";

pub fn is_default_name(name: &str) -> bool {
    name.trim().eq_ignore_ascii_case(DEFAULT_NAME)
}

pub fn default_profile() -> Profile {
    Profile {
        name: DEFAULT_NAME.into(),
        ..Default::default()
    }
}

impl Profile {
    fn is_empty_config(&self) -> bool {
        self.server.is_empty()
            && self.relay.is_empty()
            && self.api.is_empty()
            && self.key.is_empty()
    }
}

/// 儲存前的檢查：保留名稱 Default；欄位全空等同 Default，沒有存在意義。
fn validate(p: &Profile) -> Result<(), String> {
    if is_default_name(&p.name) {
        return Err(trf("err.default_reserved", &[("name", DEFAULT_NAME)]));
    }
    if p.is_empty_config() {
        return Err(trf("err.empty_config", &[("name", DEFAULT_NAME)]));
    }
    Ok(())
}

fn profiles_path() -> PathBuf {
    platform::app_data_dir().join("profiles.json")
}

fn write_private(path: &Path, data: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    fs::write(path, data).map_err(|e| {
        trf(
            "err.write_failed",
            &[
                ("path", &path.display().to_string()),
                ("err", &e.to_string()),
            ],
        )
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600)); // 含公鑰，僅本人可讀
    }
    Ok(())
}

/// 使用者自訂的 profiles（不含內建的 Default）。
/// 舊版留下的「欄位全空」profile（例如 official）等同 Default，載入時略過。
pub fn load() -> Result<Vec<Profile>, String> {
    Ok(load_raw()?
        .into_iter()
        .filter(|p| !p.is_empty_config() && !is_default_name(&p.name))
        .collect())
}

/// Default 加上所有自訂 profiles。
pub fn all() -> Result<Vec<Profile>, String> {
    let mut v = vec![default_profile()];
    v.extend(load()?);
    Ok(v)
}

fn load_raw() -> Result<Vec<Profile>, String> {
    let raw = match fs::read_to_string(profiles_path()) {
        Ok(s) => s,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.to_string()),
    };
    if let Ok(v) = serde_json::from_str::<Vec<Profile>>(&raw) {
        return Ok(v);
    }
    // 相容舊格式 {名稱: {server,relay,api,key}}
    let legacy: HashMap<String, Profile> = serde_json::from_str(&raw)
        .map_err(|e| trf("err.profiles_format", &[("err", &e.to_string())]))?;
    let mut v: Vec<Profile> = legacy
        .into_iter()
        .map(|(name, p)| Profile { name, ..p })
        .collect();
    v.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(v)
}

fn save(mut ps: Vec<Profile>) -> Result<(), String> {
    ps.sort_by(|a, b| a.name.cmp(&b.name));
    write_private(
        &profiles_path(),
        &serde_json::to_string_pretty(&ps).unwrap(),
    )
}

pub fn find(name: &str) -> Result<Profile, String> {
    all()?
        .into_iter()
        .find(|p| p.name == name)
        .ok_or_else(|| trf("err.not_found", &[("name", name)]))
}

/// 儲存 profile。`old` 為 None 代表新增（名稱不可與現有的重複，避免無聲覆蓋）；
/// 為 Some(原名稱) 代表編輯，名稱可以更改，「我的最愛」會跟著改名。
pub fn save_as(old: Option<&str>, p: Profile) -> Result<(), String> {
    validate(&p)?;
    let mut ps = load()?;
    let exists = |n: &str, ps: &[Profile]| ps.iter().any(|x| x.name == n);
    match old {
        None => {
            if exists(&p.name, &ps) {
                return Err(trf("err.name_exists", &[("name", &p.name)]));
            }
            ps.push(p);
            save(ps)
        }
        Some(old) => {
            if is_default_name(old) {
                return Err(trf("err.default_no_edit", &[("name", DEFAULT_NAME)]));
            }
            let idx = ps
                .iter()
                .position(|x| x.name == old)
                .ok_or_else(|| trf("err.not_found", &[("name", old)]))?;
            if p.name != old && exists(&p.name, &ps) {
                return Err(trf("err.name_exists", &[("name", &p.name)]));
            }
            let new_name = p.name.clone();
            ps[idx] = p;
            save(ps)?;
            if new_name != old {
                rename_favorite(old, &new_name)?;
            }
            Ok(())
        }
    }
}

fn rename_favorite(old: &str, new: &str) -> Result<(), String> {
    let mut fav = load_favorites();
    if let Some(slot) = fav.iter_mut().find(|n| n.as_str() == old) {
        *slot = new.to_string();
        write_private(
            &favorites_path(),
            &serde_json::to_string_pretty(&fav).unwrap(),
        )?;
    }
    Ok(())
}

pub fn delete(name: &str) -> Result<(), String> {
    if is_default_name(name) {
        return Err(trf("err.default_no_delete", &[("name", DEFAULT_NAME)]));
    }
    let mut ps = load()?;
    ps.retain(|p| p.name != name);
    save(ps)?;
    set_favorite(name, false)
}

// ---- 我的最愛（以名稱記錄，Default 也可以加入）----

fn favorites_path() -> PathBuf {
    platform::app_data_dir().join("favorites.json")
}

pub fn load_favorites() -> Vec<String> {
    fs::read_to_string(favorites_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn set_favorite(name: &str, on: bool) -> Result<(), String> {
    let mut fav = load_favorites();
    fav.retain(|n| n != name);
    if on {
        if !all()?.iter().any(|p| p.name == name) {
            return Err(trf("err.not_found", &[("name", name)]));
        }
        fav.push(name.to_string());
    }
    write_private(
        &favorites_path(),
        &serde_json::to_string_pretty(&fav).unwrap(),
    )
}

fn user_config() -> PathBuf {
    platform::user_config_dir().join(CONFIG_NAME)
}

/// 目前生效的 ID 伺服器：已安裝服務且有權限時，向執行中的 RustDesk 詢問（設定檔可能與記憶體不一致）。
pub fn current_server() -> String {
    if platform::service_installed() && platform::is_elevated() {
        if let Some(v) = platform::rustdesk_get_option("custom-rendezvous-server") {
            return v;
        }
    }
    fs::read_to_string(user_config())
        .map(|t| tomledit::get_options(&t))
        .unwrap_or_default()
        .remove("custom-rendezvous-server")
        .unwrap_or_default()
}

/// `tag` 區分來源（user/service），避免同一秒的備份檔名相撞。
fn backup(cfg: &Path, tag: &str) -> Result<(), String> {
    let data = match fs::read(cfg) {
        Ok(d) => d,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
        Err(e) => {
            return Err(trf(
                "err.backup_failed",
                &[
                    ("path", &cfg.display().to_string()),
                    ("err", &e.to_string()),
                ],
            ))
        }
    };
    fs::write(backup_dest(tag)?, data).map_err(|e| e.to_string())
}

/// 備份檔的路徑（確保備份資料夾存在）。
fn backup_dest(tag: &str) -> Result<PathBuf, String> {
    let dir = platform::app_data_dir().join("backups");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    Ok(dir.join(format!("{ts}-{tag}-{CONFIG_NAME}")))
}

pub fn apply(p: &Profile) -> Result<(), String> {
    if platform::service_installed() {
        apply_via_cli(p)
    } else {
        apply_via_file(p)
    }
}

/// 已安裝服務：以 `rustdesk --option` 通知執行中的服務，並讀回驗證。
fn apply_via_cli(p: &Profile) -> Result<(), String> {
    let elevated = platform::is_elevated();
    // 不是管理員時：macOS/Linux 可在切換當下跳出系統密碼視窗；Windows 則要求以管理員啟動
    if !elevated && !platform::can_elevate_on_demand() {
        return Err(tr("err.need_admin"));
    }
    let exe = platform::rustdesk_exe().ok_or_else(|| tr("err.no_exe"))?;
    let now = if elevated {
        backup(&user_config(), "user")?;
        backup(&platform::service_config_dir().join(CONFIG_NAME), "service")?;
        for c in p.changes() {
            platform::rustdesk_set_option(&exe, c.key, c.value.as_deref().unwrap_or(""))?;
        }
        platform::rustdesk_get_option("custom-rendezvous-server")
    } else {
        // 整批設定只問一次密碼。備份也交給提升權限的腳本做：以 root 執行 `rustdesk --option`
        // 會把使用者的 RustDesk2.toml 寫成 root 擁有，下次一般權限就讀不到、備份會失敗；
        // 腳本備份之後也會把它的擁有者還原成目前使用者。
        let changes = p.changes();
        let pairs: Vec<(&str, &str)> = changes
            .iter()
            .map(|c| (c.key, c.value.as_deref().unwrap_or("")))
            .collect();
        let path_str = |p: &Path| p.to_string_lossy().to_string();
        let user_cfg = path_str(&user_config());
        let service_cfg = path_str(&platform::service_config_dir().join(CONFIG_NAME));
        let user_bk = path_str(&backup_dest("user")?);
        let service_bk = path_str(&backup_dest("service")?);
        platform::rustdesk_set_options_elevated(
            &exe,
            &pairs,
            "custom-rendezvous-server",
            &[(&user_cfg, &user_bk), (&service_cfg, &service_bk)],
            &[&user_cfg],
        )?
    };
    // 讀回驗證（讀不到就略過，不誤報失敗）
    if let Some(now) = now {
        if now != p.server {
            return Err(trf(
                "err.mismatch",
                &[("now", &now), ("expected", &p.server)],
            ));
        }
    }
    Ok(())
}

/// 免安裝/無服務：關閉 GUI -> 備份 -> 原子寫入 -> 重新啟動 GUI。
fn apply_via_file(p: &Profile) -> Result<(), String> {
    let cfg = user_config();
    let stopped = platform::stop_gui(); // RustDesk 結束時會回寫設定，必須先關閉
    std::thread::sleep(Duration::from_secs(1));
    let old = match fs::read_to_string(&cfg) {
        Ok(s) => s,
        Err(e) if e.kind() == ErrorKind::NotFound => String::new(),
        Err(e) => {
            return Err(trf(
                "err.read_failed",
                &[
                    ("path", &cfg.display().to_string()),
                    ("err", &e.to_string()),
                ],
            ))
        }
    };
    backup(&cfg, "user")?;
    if let Some(dir) = cfg.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = cfg.with_extension("toml.tmp");
    fs::write(&tmp, tomledit::set_options(&old, &p.changes())).map_err(|e| {
        trf(
            "err.write_failed",
            &[
                ("path", &tmp.display().to_string()),
                ("err", &e.to_string()),
            ],
        )
    })?;
    fs::rename(&tmp, &cfg).map_err(|e| {
        trf(
            "err.replace_failed",
            &[
                ("path", &cfg.display().to_string()),
                ("err", &e.to_string()),
            ],
        )
    })?;
    platform::start_gui(&stopped);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(name: &str, server: &str) -> Profile {
        Profile {
            name: name.into(),
            server: server.into(),
            ..Default::default()
        }
    }

    #[test]
    fn default_is_reserved_and_immutable() {
        assert!(validate(&p("Default", "x:1")).is_err());
        assert!(validate(&p("  default ", "x:1")).is_err()); // 不分大小寫、忽略空白
        assert!(delete("Default").is_err());
        assert!(delete("DEFAULT").is_err());
    }

    #[test]
    fn empty_profile_is_rejected() {
        assert!(validate(&p("lab", "")).is_err());
        assert!(validate(&p("lab", "192.0.2.1")).is_ok());
    }

    /// 在暫存資料夾驗證存檔相關行為（不碰真實的使用者資料）。
    #[test]
    fn favorites_and_default_roundtrip() {
        let tmp = std::env::temp_dir().join(format!("rdswitch-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        for k in ["APPDATA", "XDG_CONFIG_HOME", "HOME", "USERPROFILE"] {
            std::env::set_var(k, &tmp);
        }
        let names = || {
            all()
                .unwrap()
                .into_iter()
                .map(|p| p.name)
                .collect::<Vec<_>>()
        };

        assert_eq!(names(), ["Default"]); // 一開始只有內建的 Default

        save_as(None, p("lab", "192.0.2.1:21116")).unwrap();
        assert!(save_as(None, p("Default", "x:1")).is_err());
        assert!(save_as(None, p("lab", "192.0.2.9:1")).is_err()); // 新增不可無聲覆蓋同名項目
        assert_eq!(names(), ["Default", "lab"]);

        set_favorite("Default", true).unwrap();
        set_favorite("lab", true).unwrap();
        assert_eq!(load_favorites(), ["Default", "lab"]);
        assert!(set_favorite("nope", true).is_err());

        set_favorite("Default", false).unwrap();
        assert_eq!(load_favorites(), ["lab"]);

        // 改名：最愛跟著改名、內容保留；不可改成 Default 或與其他項目重名
        save_as(None, p("other", "192.0.2.5:1")).unwrap();
        assert!(save_as(Some("lab"), p("other", "192.0.2.1:21116")).is_err());
        assert!(save_as(Some("lab"), p("default", "192.0.2.1:21116")).is_err());
        assert!(save_as(Some("Default"), p("x", "192.0.2.1:21116")).is_err());
        save_as(Some("lab"), p("實驗室", "192.0.2.1:21116")).unwrap();
        assert_eq!(names(), ["Default", "other", "實驗室"]);
        assert_eq!(load_favorites(), ["實驗室"]);
        assert_eq!(find("實驗室").unwrap().server, "192.0.2.1:21116");
        save_as(Some("實驗室"), p("實驗室", "192.0.2.7:1")).unwrap(); // 名稱不變只改內容
        assert_eq!(find("實驗室").unwrap().server, "192.0.2.7:1");
        delete("other").unwrap();
        assert!(find("lab").is_err());

        delete("實驗室").unwrap(); // 刪除 profile 時一併移出最愛
        assert!(load_favorites().is_empty());
        assert_eq!(names(), ["Default"]);

        // 舊版遺留的「欄位全空」profile（例如 official）等同 Default，載入時略過
        fs::write(profiles_path(), r#"{"official":{}, "x":{"server":"a:1"}}"#).unwrap();
        assert_eq!(names(), ["Default", "x"]);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn default_profile_is_empty_config() {
        let d = default_profile();
        assert_eq!(d.name, "Default");
        assert!(d.is_empty_config());
    }
}

#[cfg(test)]
mod e2e {
    use super::*;

    /// 真實驗證（需以系統管理員/root 執行，且會暫時改動真實的 RustDesk 設定）：
    /// 切到 RDS_E2E_PROFILE 指定的 profile，確認 RustDesk 回報已採用，最後切回原設定。
    #[test]
    #[ignore]
    fn switch_real_profile_and_restore() {
        let name = std::env::var("RDS_E2E_PROFILE").expect("set RDS_E2E_PROFILE");
        let target = find(&name).expect("profile");
        let opts = fs::read_to_string(user_config())
            .map(|t| tomledit::get_options(&t))
            .unwrap_or_default();
        let get = |k: &str| opts.get(k).cloned().unwrap_or_default();
        let orig = Profile {
            name: "orig".into(),
            server: get("custom-rendezvous-server"),
            relay: get("relay-server"),
            api: get("api-server"),
            key: get("key"),
        };

        let r = apply(&target);
        let during = current_server();
        println!("切換後 RustDesk 回報的伺服器：{during}");
        std::thread::sleep(Duration::from_secs(10));
        let back = apply(&orig);
        let after = current_server();
        println!("還原後 RustDesk 回報的伺服器：{after}");

        r.expect("apply target");
        back.expect("restore");
        assert_eq!(during, target.server);
        assert_eq!(after, orig.server);
    }

    /// 需以系統管理員執行：開機自動啟動排程工作的建立/查詢/刪除。
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn autostart_task_roundtrip() {
        assert!(!platform::autostart_enabled(), "測試前不應已存在排程工作");
        platform::set_autostart(true).expect("create");
        assert!(platform::autostart_enabled());
        platform::set_autostart(false).expect("delete");
        assert!(!platform::autostart_enabled());
    }
}
