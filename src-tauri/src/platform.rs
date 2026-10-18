//! 平台設定適配層：所有「作業系統不同」的知識都集中在這個檔案，
//! 其餘模組不得自行判斷作業系統。

use crate::elevate;
use crate::i18n::{tr, trf};
use std::env;
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

pub const CONFIG_NAME: &str = "RustDesk2.toml";

/// 建立子行程指令；Windows 下不彈出主控台視窗。
#[allow(unused_mut)]
fn command(program: &str) -> Command {
    let mut c = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    c
}

fn env_path(key: &str) -> PathBuf {
    env::var_os(key).map(PathBuf::from).unwrap_or_default()
}

fn home() -> PathBuf {
    let key = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    env_path(key)
}

fn xdg_config() -> PathBuf {
    match env::var_os("XDG_CONFIG_HOME") {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => home().join(".config"),
    }
}

/// 目前使用者的 RustDesk 設定目錄（GUI 使用）。
pub fn user_config_dir() -> PathBuf {
    if cfg!(windows) {
        env_path("APPDATA").join("RustDesk").join("config")
    } else if cfg!(target_os = "macos") {
        home().join("Library/Preferences/com.carriez.RustDesk")
    } else {
        xdg_config().join("rustdesk")
    }
}

/// RustDesk 背景服務的設定目錄（需系統管理員/root 才能寫入）。
pub fn service_config_dir() -> PathBuf {
    if cfg!(windows) {
        let win = env::var_os("SystemRoot")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        win.join("ServiceProfiles/LocalService/AppData/Roaming/RustDesk/config")
    } else if cfg!(target_os = "macos") {
        PathBuf::from("/var/root/Library/Preferences/com.carriez.RustDesk")
    } else {
        PathBuf::from("/root/.config/rustdesk")
    }
}

/// 本工具自己的資料目錄（profiles、設定、備份）。
pub fn app_data_dir() -> PathBuf {
    if cfg!(windows) {
        env_path("APPDATA").join("rdswitch")
    } else if cfg!(target_os = "macos") {
        home().join("Library/Application Support/rdswitch")
    } else {
        xdg_config().join("rdswitch")
    }
}

/// 系統的顯示語言（語系字串，如 `zh-TW`、`ja-JP`、`en_US.UTF-8`）；偵測不到回傳空字串。
pub fn system_locale() -> String {
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        extern "system" {
            fn GetUserDefaultUILanguage() -> u16;
            fn LCIDToLocaleName(locale: u32, name: *mut u16, cch: i32, flags: u32) -> i32;
        }
        let mut buf = [0u16; 85];
        // SAFETY: 兩個都是無副作用的唯讀系統呼叫；buf 長度與傳入的 cch 一致。
        let n = unsafe {
            LCIDToLocaleName(
                GetUserDefaultUILanguage() as u32,
                buf.as_mut_ptr(),
                buf.len() as i32,
                0,
            )
        };
        if n > 1 {
            return String::from_utf16_lossy(&buf[..n as usize - 1]);
        }
        String::new()
    }
    #[cfg(not(windows))]
    {
        #[cfg(target_os = "macos")]
        if let Ok(o) = command("defaults")
            .args(["read", "-g", "AppleLanguages"])
            .output()
        {
            // 輸出形如 ( "zh-Hant-TW", "en-TW" )，取第一個引號內的字串
            let text = String::from_utf8_lossy(&o.stdout).to_string();
            if let Some(first) = text.split('"').nth(1) {
                return first.to_string();
            }
        }
        for k in ["LC_ALL", "LC_MESSAGES", "LANG"] {
            if let Ok(v) = env::var(k) {
                if !v.is_empty() {
                    return v;
                }
            }
        }
        String::new()
    }
}

pub fn is_elevated() -> bool {
    static CACHE: OnceLock<bool> = OnceLock::new();
    *CACHE.get_or_init(|| {
        if cfg!(windows) {
            command("net")
                .arg("session")
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        } else {
            command("id")
                .arg("-u")
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "0")
                .unwrap_or(false)
        }
    })
}

/// RustDesk 在 Windows 的實際安裝路徑：優先讀服務登錄的 ImagePath（使用者可能裝在 D: 等非預設位置）。
#[cfg(windows)]
fn windows_rustdesk_exe() -> Option<PathBuf> {
    let out = command("reg")
        .args([
            "query",
            r"HKLM\SYSTEM\CurrentControlSet\Services\RustDesk",
            "/v",
            "ImagePath",
        ])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().find(|l| l.contains("ImagePath"))?;
    let value = line
        .split("REG_EXPAND_SZ")
        .nth(1)
        .or_else(|| line.split("REG_SZ").nth(1))?
        .trim();
    let path = match value.strip_prefix('"') {
        Some(rest) => rest.split('"').next()?.to_string(),
        None => value.split(" --").next()?.trim().to_string(),
    };
    let p = PathBuf::from(path);
    p.exists().then_some(p)
}

/// 關閉 RustDesk GUI（不得動到 --service / --server 背景行程）。
/// 回傳被關閉的 GUI 的命令列；空陣列代表原本就沒有開著（之後不需要重啟）。
pub fn stop_gui() -> Vec<String> {
    if cfg!(windows) {
        // 只關閉「本人 session 內、命令列可讀、且不是 --service/--server」的行程。
        // 背景服務跑在 session 0 且命令列對非管理員不可讀，必須明確排除，
        // 否則以管理員身分執行時會誤殺 RustDesk 服務。
        let ps = "$sid = (Get-Process -Id $PID).SessionId;                   Get-CimInstance Win32_Process -Filter \"Name='rustdesk.exe'\" |                   Where-Object { $_.SessionId -eq $sid -and $_.CommandLine -and $_.CommandLine -notmatch '--(service|server)' } |                   ForEach-Object { Stop-Process -Id $_.ProcessId -Force; $_.CommandLine }";
        match command("powershell")
            .args(["-NoProfile", "-Command", ps])
            .output()
        {
            Ok(o) => String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect(),
            Err(_) => vec![],
        }
    } else if cfg!(target_os = "macos") {
        let running = command("pgrep")
            .args(["-x", "RustDesk"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if running {
            let _ = command("osascript")
                .args(["-e", "tell application \"RustDesk\" to quit"])
                .status();
            vec![String::new()]
        } else {
            vec![]
        }
    } else {
        let mut stopped = vec![];
        if let Ok(out) = command("pgrep").args(["-xa", "rustdesk"]).output() {
            for line in String::from_utf8_lossy(&out.stdout).lines() {
                let (pid, cmd) = line.trim().split_once(' ').unwrap_or((line.trim(), ""));
                if !pid.is_empty() && !cmd.contains("--service") && !cmd.contains("--server") {
                    let _ = command("kill").arg(pid).status();
                    stopped.push(cmd.to_string());
                }
            }
        }
        stopped
    }
}

/// 重新啟動先前被 stop_gui 關閉的 GUI（沿用 --tray 等啟動方式）。
pub fn start_gui(stopped: &[String]) {
    if stopped.is_empty() {
        return;
    }
    let tray = stopped.iter().any(|c| c.contains("--tray"));
    if cfg!(windows) {
        #[cfg(windows)]
        {
            let exe = windows_rustdesk_exe().or_else(|| {
                Some(
                    env_path("ProgramFiles")
                        .join("RustDesk")
                        .join("RustDesk.exe"),
                )
                .filter(|p| p.exists())
            });
            if let Some(exe) = exe {
                let mut c = command(&exe.to_string_lossy());
                if tray {
                    c.arg("--tray");
                }
                let _ = c.spawn();
            }
        }
    } else if cfg!(target_os = "macos") {
        let _ = command("open").args(["-a", "RustDesk"]).status();
    } else {
        let mut c = command("rustdesk");
        if tray {
            c.arg("--tray");
        }
        let _ = c.spawn();
    }
}

/// RustDesk 可執行檔（含 `--option` 命令列介面）。
pub fn rustdesk_exe() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        windows_rustdesk_exe().or_else(|| {
            Some(
                env_path("ProgramFiles")
                    .join("RustDesk")
                    .join("RustDesk.exe"),
            )
            .filter(|p| p.exists())
        })
    }
    #[cfg(target_os = "macos")]
    {
        Some(PathBuf::from(
            "/Applications/RustDesk.app/Contents/MacOS/RustDesk",
        ))
        .filter(|p| p.exists())
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        env::var_os("PATH").and_then(|paths| {
            env::split_paths(&paths)
                .map(|d| d.join("rustdesk"))
                .find(|p| p.is_file())
        })
    }
}

/// RustDesk 是否安裝為系統服務。已安裝服務時，執行中的服務行程把設定放在記憶體，
/// 只改設定檔不會生效，必須透過 `rustdesk --option`（需系統管理員/root）通知它。
pub fn service_installed() -> bool {
    if cfg!(windows) {
        command("reg")
            .args(["query", r"HKLM\SYSTEM\CurrentControlSet\Services\RustDesk"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    } else if cfg!(target_os = "macos") {
        std::path::Path::new("/Library/LaunchDaemons/com.carriez.RustDesk_service.plist").exists()
    } else {
        [
            "/usr/lib/systemd/system/rustdesk.service",
            "/etc/systemd/system/rustdesk.service",
            "/lib/systemd/system/rustdesk.service",
        ]
        .iter()
        .any(|p| std::path::Path::new(p).exists())
    }
}

const NEED_ADMIN_MSG: &str = "privileges required";

/// 透過 `rustdesk --option <key> <value>` 設定執行中的 RustDesk（空字串 = 清除）。
pub fn rustdesk_set_option(exe: &std::path::Path, key: &str, value: &str) -> Result<(), String> {
    let out = command(&exe.to_string_lossy())
        .args(["--option", key, value])
        .output()
        .map_err(|e| trf("err.run_rustdesk", &[("err", &e.to_string())]))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if text.to_lowercase().contains(NEED_ADMIN_MSG) {
        return Err(tr("err.rustdesk_needs_admin"));
    }
    if !out.status.success() {
        return Err(trf(
            "err.option_failed",
            &[("key", key), ("status", &out.status.to_string())],
        ));
    }
    Ok(())
}

fn in_path(name: &str) -> bool {
    env::var_os("PATH")
        .map(|paths| env::split_paths(&paths).any(|d| d.join(name).is_file()))
        .unwrap_or(false)
}

/// 目前不是管理員/root 時，能否在「切換當下」才要求提升權限（跳出系統密碼視窗）。
/// Windows 沒有對應機制（啟動時由 manifest 要求管理員）；macOS 用 osascript，Linux 需要 pkexec。
pub fn can_elevate_on_demand() -> bool {
    if cfg!(windows) {
        false
    } else if cfg!(target_os = "macos") {
        in_path("osascript")
    } else {
        in_path("pkexec")
    }
}

/// 以系統密碼視窗提升權限，一次執行所有 `--option` 設定並讀回 `readback_key` 的值。
/// 整批只問一次密碼；使用者取消會回傳明確的錯誤。回傳 None 代表讀不到值（略過驗證）。
pub fn rustdesk_set_options_elevated(
    exe: &std::path::Path,
    changes: &[(&str, &str)],
    readback_key: &str,
) -> Result<Option<String>, String> {
    if !can_elevate_on_demand() {
        return Err(tr("err.need_admin"));
    }
    let script = elevate::build_script(&exe.to_string_lossy(), changes, readback_key)?;
    let out = if cfg!(target_os = "macos") {
        command("osascript")
            .args(["-e", &elevate::osascript_arg(&script)])
            .output()
    } else {
        command("pkexec").args(["sh", "-c", &script]).output()
    }
    .map_err(|e| trf("err.run_rustdesk", &[("err", &e.to_string())]))?;

    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    if !out.status.success() {
        if elevate::is_cancelled(out.status.code(), &stderr) {
            return Err(tr("err.elevate_cancelled"));
        }
        return Err(trf(
            "err.elevate_failed",
            &[("err", stderr.trim()), ("status", &out.status.to_string())],
        ));
    }
    Ok(elevate::parse_readback(&String::from_utf8_lossy(
        &out.stdout,
    )))
}

/// 向執行中的 RustDesk 詢問某個 option 目前的值；讀不到（權限或輸出被吞）回傳 None。
pub fn rustdesk_get_option(key: &str) -> Option<String> {
    let exe = rustdesk_exe()?;
    let out = command(&exe.to_string_lossy())
        .args(["--option", key])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!text.to_lowercase().contains(NEED_ADMIN_MSG) && !text.contains('\n')).then_some(text)
}

// ---- 開機自動啟動 ----
// Windows：本程式要求系統管理員權限，Run 登錄值無法在登入時啟動需提升權限的程式，
// 因此改用「最高權限」的排程工作。其他平台使用 tauri-plugin-autostart。

#[cfg(windows)]
const TASK_NAME: &str = "rdswitch";

#[cfg(windows)]
pub fn autostart_enabled() -> bool {
    command("schtasks")
        .args(["/Query", "/TN", TASK_NAME])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(windows)]
pub fn set_autostart(on: bool) -> Result<(), String> {
    let out = if on {
        let exe = env::current_exe().map_err(|e| e.to_string())?;
        let user = env::var("USERNAME").unwrap_or_default();
        command("schtasks")
            .args([
                "/Create",
                "/TN",
                TASK_NAME,
                "/TR",
                &format!("\"{}\" --minimized", exe.display()),
                "/SC",
                "ONLOGON",
                "/RL",
                "HIGHEST",
                "/RU",
                &user,
                "/IT",
                "/F",
            ])
            .output()
    } else {
        command("schtasks")
            .args(["/Delete", "/TN", TASK_NAME, "/F"])
            .output()
    }
    .map_err(|e| e.to_string())?;
    if out.status.success() || (!on && !autostart_enabled()) {
        Ok(())
    } else {
        Err(trf(
            "err.task_failed",
            &[("err", String::from_utf8_lossy(&out.stderr).trim())],
        ))
    }
}
