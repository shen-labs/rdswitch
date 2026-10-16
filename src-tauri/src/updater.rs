//! 應用程式內更新（Tauri updater 外掛）。
//!
//! 流程：定期檢查 → 有新版本就記下並通知前端 → 使用者按「立即更新」才下載、驗證簽章、安裝並重啟。
//! 簽章驗證由外掛使用內嵌公鑰（tauri.conf.json 的 plugins.updater.pubkey）完成；簽章不符一律拒絕。
//! 維護者尚未設定公鑰與更新來源時，整個功能視為停用（不會對外連線）。

use crate::i18n::trf;
use serde::Serialize;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

/// 啟動後多久做第一次檢查（讓視窗與系統匣先就緒）。
const FIRST_CHECK_DELAY: Duration = Duration::from_secs(5);
/// 之後的檢查間隔。
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

/// 找到、尚未安裝的更新。
#[derive(Default)]
pub struct Pending(pub Mutex<Option<Update>>);

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct UpdateInfo {
    pub version: String,
    pub current: String,
    pub notes: Option<String>,
}

#[derive(Serialize)]
pub struct UpdateCheck {
    /// "available" | "uptodate" | "unconfigured"
    pub state: &'static str,
    pub info: Option<UpdateInfo>,
}

#[derive(Serialize, Clone)]
struct Progress {
    downloaded: u64,
    total: Option<u64>,
}

/// 範本裡的更新網址佔位字串；還沒換成真實的 `<owner>/<repo>` 就視為未設定。
const PLACEHOLDER: &str = "OWNER/REPO";

/// 設定是否完整：公鑰不是空的，且每個更新網址都已不是範本佔位字串。
pub fn config_complete(cfg: &serde_json::Value) -> bool {
    let pubkey_ok = cfg
        .get("pubkey")
        .and_then(|v| v.as_str())
        .is_some_and(|s| !s.trim().is_empty());
    let endpoints_ok = cfg
        .get("endpoints")
        .and_then(|v| v.as_array())
        .is_some_and(|a| {
            !a.is_empty()
                && a.iter().all(|e| {
                    e.as_str()
                        .is_some_and(|u| !u.trim().is_empty() && !u.contains(PLACEHOLDER))
                })
        });
    pubkey_ok && endpoints_ok
}

pub fn configured(app: &AppHandle) -> bool {
    app.config()
        .plugins
        .0
        .get("updater")
        .is_some_and(config_complete)
}

fn info_of(u: &Update) -> UpdateInfo {
    UpdateInfo {
        version: u.version.clone(),
        current: u.current_version.clone(),
        notes: u.body.clone().filter(|b| !b.trim().is_empty()),
    }
}

/// 目前已找到、等待使用者決定的更新。
pub fn pending_info(app: &AppHandle) -> Option<UpdateInfo> {
    app.state::<Pending>().0.lock().ok()?.as_ref().map(info_of)
}

/// 檢查一次；有新版本就記到 `Pending`。
pub async fn check(app: &AppHandle) -> Result<UpdateCheck, String> {
    if !configured(app) {
        return Ok(UpdateCheck {
            state: "unconfigured",
            info: None,
        });
    }
    let err = |e: tauri_plugin_updater::Error| trf("err.update_check", &[("err", &e.to_string())]);
    let found = app.updater().map_err(err)?.check().await.map_err(err)?;
    let pending = app.state::<Pending>();
    let mut slot = pending.0.lock().map_err(|e| e.to_string())?;
    match found {
        Some(u) => {
            let info = info_of(&u);
            *slot = Some(u);
            Ok(UpdateCheck {
                state: "available",
                info: Some(info),
            })
        }
        None => {
            *slot = None;
            Ok(UpdateCheck {
                state: "uptodate",
                info: None,
            })
        }
    }
}

/// 下載、驗證簽章並安裝先前找到的更新，然後重新啟動。Windows 的安裝程式會自行結束並重新啟動本程式。
pub async fn install(app: &AppHandle) -> Result<(), String> {
    let update = app
        .state::<Pending>()
        .0
        .lock()
        .map_err(|e| e.to_string())?
        .take()
        .ok_or_else(|| crate::i18n::tr("err.update_missing"))?;
    let mut downloaded: u64 = 0;
    let (a, b) = (app.clone(), app.clone());
    let res = update
        .download_and_install(
            move |chunk, total| {
                downloaded += chunk as u64;
                let _ = a.emit("update-progress", Progress { downloaded, total });
            },
            move || {
                let _ = b.emit("update-installing", ());
            },
        )
        .await;
    if let Err(e) = res {
        // 失敗時把更新放回去，讓使用者可以重試
        if let Ok(mut slot) = app.state::<Pending>().0.lock() {
            *slot = Some(update);
        }
        return Err(trf("err.update_install", &[("err", &e.to_string())]));
    }
    app.restart()
}

/// 背景定期檢查；找到新版本就通知前端並更新系統匣。失敗（例如離線）靜默略過，下次再試。
pub fn spawn_auto_check(app: AppHandle, on_found: impl Fn(&AppHandle) + Send + 'static) {
    std::thread::spawn(move || {
        std::thread::sleep(FIRST_CHECK_DELAY);
        loop {
            if let Ok(UpdateCheck {
                state: "available",
                info: Some(info),
            }) = tauri::async_runtime::block_on(check(&app))
            {
                let _ = app.emit("update-available", info);
                on_found(&app);
            }
            std::thread::sleep(CHECK_INTERVAL);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn config_needs_pubkey_and_a_real_endpoint() {
        let good = json!({"pubkey": "dW50cnVzdGVk...", "endpoints": ["https://github.com/acme/tool/releases/latest/download/latest.json"]});
        assert!(config_complete(&good));

        // 範本狀態：公鑰空白、網址是佔位字串 → 停用
        let template = json!({"pubkey": "", "endpoints": ["https://github.com/OWNER/REPO/releases/latest/download/latest.json"]});
        assert!(!config_complete(&template));

        // 只填了其中一項也不算完整
        assert!(!config_complete(
            &json!({"pubkey": "key", "endpoints": ["https://github.com/OWNER/REPO/releases/latest/download/latest.json"]})
        ));
        assert!(!config_complete(
            &json!({"pubkey": "  ", "endpoints": ["https://example.com/latest.json"]})
        ));
        assert!(!config_complete(&json!({"pubkey": "key", "endpoints": []})));
        assert!(!config_complete(&json!({"pubkey": "key"})));
        assert!(!config_complete(
            &json!({"endpoints": ["https://example.com/latest.json"]})
        ));
        // 多個網址時，只要有一個還是佔位字串就視為未設定
        assert!(!config_complete(
            &json!({"pubkey": "key", "endpoints": ["https://example.com/latest.json", "https://github.com/OWNER/REPO/x"]})
        ));
    }

    /// 專案實際發佈用的設定檔：確認 JSON 結構是外掛看得懂的（欄位名稱、型別），並反映目前是否已設定。
    #[test]
    fn shipped_config_has_the_updater_section() {
        let raw = include_str!("../tauri.conf.json");
        let conf: serde_json::Value = serde_json::from_str(raw).unwrap();
        let up = &conf["plugins"]["updater"];
        assert!(
            up["pubkey"].is_string(),
            "plugins.updater.pubkey 必須存在（可先留空）"
        );
        assert!(
            up["endpoints"].as_array().is_some_and(|a| !a.is_empty()),
            "plugins.updater.endpoints 必須存在"
        );
        for e in up["endpoints"].as_array().unwrap() {
            assert!(
                e.as_str().unwrap().starts_with("https://"),
                "更新網址必須是 https"
            );
        }
        // 清單本身沒有簽章。簽章帶有版本資訊時，外掛一律檢查版本是否與清單一致；
        // 但沒有版本資訊的簽章（舊式簽章）在旗標關閉時會直接略過這項檢查，所以必須開啟 requireSignedVersion，
        // 讓這類簽章一律被拒絕（見 tests/updater_e2e.rs 的實測）。
        assert_eq!(
            up["requireSignedVersion"], true,
            "plugins.updater.requireSignedVersion 必須開啟"
        );
        assert!(
            up.get("dangerousInsecureTransportProtocol").is_none(),
            "不可允許不安全的傳輸協定"
        );
        assert!(up.get("allowDowngrades").is_none(), "不可允許降級");
    }

    /// 出貨的設定必須是「更新已啟用」（公鑰與網址都已填寫）。
    /// 否則發佈出去的版本會悄悄停用更新，使用者永遠收不到修正。
    #[test]
    fn shipped_config_enables_the_updater() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert!(
            config_complete(&conf["plugins"]["updater"]),
            "plugins.updater 設定不完整（公鑰或更新網址），更新功能會被停用"
        );
    }

    /// 公鑰必須是格式正確的 minisign 公鑰：能擋下貼上時被截斷、或誤貼成私鑰。
    /// （這只驗證格式；它是否真的對應到簽章用的私鑰，要靠真實發佈後的更新測試確認。）
    #[test]
    fn shipped_pubkey_is_a_valid_minisign_public_key() {
        use base64::Engine;
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let pubkey = conf["plugins"]["updater"]["pubkey"]
            .as_str()
            .expect("pubkey 必須是字串");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(pubkey.trim())
            .expect("pubkey 必須是 base64");
        let text = String::from_utf8(bytes).expect("pubkey 解碼後必須是文字");
        assert!(
            text.starts_with("untrusted comment: minisign public key"),
            "這不是公鑰（是不是貼成私鑰了？）：{}",
            text.lines().next().unwrap_or("")
        );
        minisign_verify::PublicKey::decode(&text).expect("公鑰格式錯誤（可能被截斷）");
    }
}
