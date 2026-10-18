//! 後端（系統匣選單、錯誤訊息）的多國語言。前端另有自己的字典（src/i18n.js）。
//!
//! 語言設定以後端為準：存在 settings.json，前端透過 get_state 取得有效語言。
//! 新增語言：在 `Lang`、`ALL`、`code`、`from_locale` 與 `TABLE` 每個項目補一欄。

use crate::platform;
use std::sync::RwLock;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    ZhTw,
    En,
    ZhCn,
    Ja,
}

pub const ALL: [Lang; 4] = [Lang::ZhTw, Lang::En, Lang::ZhCn, Lang::Ja];

impl Lang {
    pub fn code(self) -> &'static str {
        match self {
            Lang::ZhTw => "zh-TW",
            Lang::En => "en",
            Lang::ZhCn => "zh-CN",
            Lang::Ja => "ja",
        }
    }

    pub fn from_code(code: &str) -> Option<Lang> {
        ALL.into_iter()
            .find(|l| l.code().eq_ignore_ascii_case(code))
    }

    /// 由系統語系字串（如 `zh-TW`、`zh_CN.UTF-8`、`ja-JP`、`en_US`）對應到支援的語言；不支援的一律用英文。
    pub fn from_locale(locale: &str) -> Lang {
        let l = locale.to_ascii_lowercase().replace('_', "-");
        if l.starts_with("zh") {
            // 繁體：台灣、香港、澳門，或明確標示 Hant
            if ["tw", "hk", "mo", "hant"]
                .iter()
                .any(|t| l.split(['-', '.']).any(|p| p == *t))
            {
                Lang::ZhTw
            } else {
                Lang::ZhCn
            }
        } else if l.starts_with("ja") {
            Lang::Ja
        } else {
            Lang::En
        }
    }

    fn idx(self) -> usize {
        ALL.iter().position(|l| *l == self).unwrap()
    }
}

static CURRENT: RwLock<Lang> = RwLock::new(Lang::ZhTw);

pub fn set(lang: Lang) {
    *CURRENT.write().unwrap_or_else(|e| e.into_inner()) = lang;
}

pub fn get() -> Lang {
    *CURRENT.read().unwrap_or_else(|e| e.into_inner())
}

/// 由設定值（"auto" 或語言代碼）解析出有效語言。
pub fn resolve(setting: &str) -> Lang {
    Lang::from_code(setting).unwrap_or_else(|| Lang::from_locale(&platform::system_locale()))
}

/// 查表；找不到鍵時回傳鍵本身，讓缺漏在畫面上一眼就看得出來。
pub fn tr(key: &str) -> String {
    TABLE
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| v[get().idx()].to_string())
        .unwrap_or_else(|| key.to_string())
}

/// 查表並代入 `{名稱}` 變數。
pub fn trf(key: &str, vars: &[(&str, &str)]) -> String {
    let mut s = tr(key);
    for (k, v) in vars {
        s = s.replace(&format!("{{{k}}}"), v);
    }
    s
}

/// 欄位順序必須與 `ALL` 相同：[zh-TW, en, zh-CN, ja]
/// 這是對照表，每個鍵的四種語言要上下對齊才容易檢查，所以不交給 rustfmt 重排。
#[rustfmt::skip]
const TABLE: &[(&str, [&str; 4])] = &[
    ("tray.current", ["目前：{name}", "Current: {name}", "当前：{name}", "現在：{name}"]),
    ("tray.default_item", ["{name}（官方預設）", "{name} (RustDesk default)", "{name}（官方默认）", "{name}（公式デフォルト）"]),
    ("tray.official", ["官方預設", "RustDesk default", "官方默认", "公式デフォルト"]),
    ("tray.more", ["其他", "More", "其他", "その他"]),
    ("tray.autostart", ["開機自動啟動", "Start at login", "开机自动启动", "ログイン時に起動"]),
    ("tray.open", ["開啟主視窗", "Open window", "打开主窗口", "ウィンドウを開く"]),
    ("tray.quit", ["結束", "Quit", "退出", "終了"]),
    ("tray.update", ["有新版本 {version}（點此開啟）", "Update available: {version} (click to open)", "有新版本 {version}（点此打开）", "新しいバージョン {version} があります（クリックで開く）"]),
    ("tray.tooltip", ["RustDesk：{name}", "RustDesk: {name}", "RustDesk：{name}", "RustDesk：{name}"]),
    ("toast.switched", ["已切換到「{name}」", "Switched to “{name}”", "已切换到“{name}”", "「{name}」に切り替えました"]),
    ("err.name_required", ["請輸入名稱", "Please enter a name", "请输入名称", "名前を入力してください"]),
    (
        "err.default_reserved",
        [
            "「{name}」是內建的預設 profile，名稱保留，請換一個名稱",
            "“{name}” is the built-in default profile and its name is reserved. Please choose another name.",
            "“{name}”是内置的默认配置，名称为保留名称，请换一个名称",
            "「{name}」は組み込みのデフォルトプロファイルで、この名前は予約されています。別の名前を指定してください",
        ],
    ),
    (
        "err.default_no_edit",
        [
            "「{name}」是內建的預設 profile，不能修改",
            "“{name}” is the built-in default profile and cannot be modified",
            "“{name}”是内置的默认配置，不能修改",
            "「{name}」は組み込みのデフォルトプロファイルのため変更できません",
        ],
    ),
    (
        "err.default_no_delete",
        [
            "「{name}」是內建的預設 profile，不能刪除",
            "“{name}” is the built-in default profile and cannot be deleted",
            "“{name}”是内置的默认配置，不能删除",
            "「{name}」は組み込みのデフォルトプロファイルのため削除できません",
        ],
    ),
    (
        "err.empty_config",
        [
            "至少要填寫一個欄位（全部留空就是「{name}」）",
            "Fill in at least one field (leaving everything empty is the same as “{name}”)",
            "至少要填写一个字段（全部留空就是“{name}”）",
            "少なくとも1つの項目を入力してください（すべて空欄は「{name}」と同じです）",
        ],
    ),
    (
        "err.name_exists",
        [
            "已有名為「{name}」的 profile，請換一個名稱",
            "A profile named “{name}” already exists. Please choose another name.",
            "已有名为“{name}”的配置，请换一个名称",
            "「{name}」というプロファイルは既にあります。別の名前を指定してください",
        ],
    ),
    ("err.not_found", ["找不到 profile：{name}", "Profile not found: {name}", "找不到配置：{name}", "プロファイルが見つかりません：{name}"]),
    ("err.write_failed", ["寫入 {path} 失敗：{err}", "Failed to write {path}: {err}", "写入 {path} 失败：{err}", "{path} への書き込みに失敗しました：{err}"]),
    ("err.read_failed", ["讀取 {path} 失敗：{err}", "Failed to read {path}: {err}", "读取 {path} 失败：{err}", "{path} の読み込みに失敗しました：{err}"]),
    ("err.backup_failed", ["備份 {path} 失敗：{err}", "Failed to back up {path}: {err}", "备份 {path} 失败：{err}", "{path} のバックアップに失敗しました：{err}"]),
    ("err.replace_failed", ["取代 {path} 失敗：{err}", "Failed to replace {path}: {err}", "替换 {path} 失败：{err}", "{path} の置き換えに失敗しました：{err}"]),
    (
        "err.profiles_format",
        ["profiles.json 格式錯誤：{err}", "profiles.json is malformed: {err}", "profiles.json 格式错误：{err}", "profiles.json の形式が正しくありません：{err}"],
    ),
    (
        "err.need_admin",
        [
            "RustDesk 已安裝為系統服務，切換伺服器需要系統管理員/root 權限，請以提升權限重新啟動本程式",
            "RustDesk is installed as a system service. Switching servers requires administrator/root privileges; please restart this app elevated.",
            "RustDesk 已安装为系统服务，切换服务器需要管理员/root 权限，请以提升权限重新启动本程序",
            "RustDesk はシステムサービスとしてインストールされています。サーバーの切り替えには管理者/root 権限が必要です。権限を昇格してこのアプリを再起動してください",
        ],
    ),
    (
        "err.elevate_cancelled",
        ["已取消授權，未做任何變更", "Authorization was cancelled; nothing was changed", "已取消授权，未做任何更改", "認証がキャンセルされました。変更は行われていません"],
    ),
    (
        "err.elevate_failed",
        [
            "提升權限執行失敗（{status}）：{err}",
            "Running with elevated privileges failed ({status}): {err}",
            "提升权限运行失败（{status}）：{err}",
            "権限を昇格した実行に失敗しました（{status}）：{err}",
        ],
    ),
    (
        "err.elevate_bad_input",
        ["設定值含有不允許的字元，已拒絕執行", "The settings contain characters that are not allowed; refusing to run", "设置值含有不允许的字符，已拒绝执行", "設定値に使用できない文字が含まれているため、実行を拒否しました"],
    ),
    ("err.no_exe", ["找不到 RustDesk 執行檔", "RustDesk executable not found", "找不到 RustDesk 可执行文件", "RustDesk の実行ファイルが見つかりません"]),
    (
        "err.mismatch",
        [
            "RustDesk 回報目前伺服器為「{now}」，與預期「{expected}」不符",
            "RustDesk reports the current server as “{now}”, which does not match the expected “{expected}”",
            "RustDesk 报告当前服务器为“{now}”，与预期“{expected}”不符",
            "RustDesk が報告した現在のサーバーは「{now}」で、期待値「{expected}」と一致しません",
        ],
    ),
    ("err.run_rustdesk", ["無法執行 RustDesk：{err}", "Cannot run RustDesk: {err}", "无法运行 RustDesk：{err}", "RustDesk を実行できません：{err}"]),
    (
        "err.rustdesk_needs_admin",
        [
            "RustDesk 要求系統管理員權限，請以提升權限重新啟動本程式",
            "RustDesk requires administrator privileges; please restart this app elevated",
            "RustDesk 要求管理员权限，请以提升权限重新启动本程序",
            "RustDesk は管理者権限を要求しています。権限を昇格してこのアプリを再起動してください",
        ],
    ),
    (
        "err.option_failed",
        ["rustdesk --option {key} 失敗（{status}）", "rustdesk --option {key} failed ({status})", "rustdesk --option {key} 失败（{status}）", "rustdesk --option {key} が失敗しました（{status}）"],
    ),
    ("err.task_failed", ["排程工作操作失敗：{err}", "Scheduled task operation failed: {err}", "计划任务操作失败：{err}", "タスクスケジューラの操作に失敗しました：{err}"]),
    ("err.update_check", ["無法檢查更新：{err}", "Cannot check for updates: {err}", "无法检查更新：{err}", "更新を確認できません：{err}"]),
    ("err.update_install", ["更新失敗：{err}", "Update failed: {err}", "更新失败：{err}", "更新に失敗しました：{err}"]),
    (
        "err.update_missing",
        ["沒有可安裝的更新，請先檢查更新", "There is no update to install. Check for updates first.", "没有可安装的更新，请先检查更新", "インストールできる更新がありません。先に更新を確認してください"],
    ),
    ("err.empty_input", ["內容是空的", "The input is empty", "内容为空", "入力が空です"]),
    (
        "err.unrecognized",
        [
            "無法辨識格式；支援 RustDesk 設定字串、JSON、或「標籤: 值」文字",
            "Unrecognized format. Supported: RustDesk config string, JSON, or “label: value” text.",
            "无法识别格式；支持 RustDesk 配置字符串、JSON，或“标签: 值”文本",
            "形式を認識できません。RustDesk の設定文字列、JSON、または「ラベル: 値」のテキストに対応しています",
        ],
    ),
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn placeholders(s: &str) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        let mut rest = s;
        while let Some(a) = rest.find('{') {
            let Some(b) = rest[a..].find('}') else { break };
            out.insert(rest[a + 1..a + b].to_string());
            rest = &rest[a + b + 1..];
        }
        out
    }

    #[test]
    fn every_key_is_translated_in_every_language_with_the_same_placeholders() {
        let mut seen = BTreeSet::new();
        for (key, vals) in TABLE {
            assert!(seen.insert(*key), "重複的鍵：{key}");
            let base = placeholders(vals[0]);
            for (lang, v) in ALL.iter().zip(vals.iter()) {
                assert!(!v.trim().is_empty(), "{key} 缺少 {} 翻譯", lang.code());
                assert_eq!(
                    placeholders(v),
                    base,
                    "{key} 的 {} 變數與繁中不一致：{v}",
                    lang.code()
                );
            }
        }
    }

    #[test]
    fn locale_mapping() {
        for (loc, want) in [
            ("zh-TW", Lang::ZhTw),
            ("zh_TW.UTF-8", Lang::ZhTw),
            ("zh-Hant-TW", Lang::ZhTw),
            ("zh-HK", Lang::ZhTw),
            ("zh-Hant", Lang::ZhTw),
            ("zh-CN", Lang::ZhCn),
            ("zh_CN.UTF-8", Lang::ZhCn),
            ("zh-Hans-CN", Lang::ZhCn),
            ("zh-SG", Lang::ZhCn),
            ("zh", Lang::ZhCn),
            ("ja-JP", Lang::Ja),
            ("ja_JP.UTF-8", Lang::Ja),
            ("en-US", Lang::En),
            ("en_GB", Lang::En),
            ("de-DE", Lang::En),
            ("", Lang::En),
            ("C", Lang::En),
        ] {
            assert_eq!(Lang::from_locale(loc), want, "{loc}");
        }
    }

    #[test]
    fn codes_roundtrip() {
        for l in ALL {
            assert_eq!(Lang::from_code(l.code()), Some(l));
        }
        assert_eq!(Lang::from_code("ZH-tw"), Some(Lang::ZhTw)); // 不分大小寫
        assert_eq!(Lang::from_code("auto"), None);
        assert_eq!(Lang::from_code("fr"), None);
    }

    #[test]
    fn missing_key_is_visible_and_vars_are_substituted() {
        assert_eq!(tr("no.such.key"), "no.such.key");
        set(Lang::En);
        assert_eq!(
            trf("toast.switched", &[("name", "Lab")]),
            "Switched to “Lab”"
        );
        set(Lang::Ja);
        assert_eq!(
            trf("toast.switched", &[("name", "Lab")]),
            "「Lab」に切り替えました"
        );
        set(Lang::ZhTw); // 還原預設，避免影響其他測試
    }
}
