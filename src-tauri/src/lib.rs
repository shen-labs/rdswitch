mod i18n;
mod parse;
mod platform;
mod probe;
mod profiles;
mod settings;
mod tomledit;
mod updater;

use i18n::{tr, trf};
use profiles::Profile;
use serde::Serialize;
use std::sync::Mutex;
use tauri::menu::{
    CheckMenuItem, CheckMenuItemBuilder, IsMenuItem, Menu, MenuBuilder, MenuItemBuilder,
    SubmenuBuilder,
};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, WindowEvent, Wry};
#[cfg(not(windows))]
use tauri_plugin_autostart::ManagerExt as _;

const TRAY_ID: &str = "main";
/// 同一時間只允許一個會動到設定的操作。
static OP_LOCK: Mutex<()> = Mutex::new(());

#[derive(Serialize, Clone)]
struct Toast {
    ok: bool,
    msg: String,
}

fn show_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

fn toast(app: &AppHandle, ok: bool, msg: impl Into<String>) {
    let _ = app.emit(
        "toast",
        Toast {
            ok,
            msg: msg.into(),
        },
    );
    if !ok {
        show_window(app); // 失敗時一定要讓使用者看到
    }
}

fn active_name(ps: &[Profile]) -> Option<String> {
    // Default 的 server 為空，所以目前未設定自訂伺服器時會比對到 Default
    let cur = profiles::current_server();
    ps.iter().find(|p| p.server == cur).map(|p| p.name.clone())
}

/// 目前生效的 profile 名稱；沒有對應的 profile 時顯示伺服器位址。
fn current_label(ps: &[Profile]) -> String {
    active_name(ps).unwrap_or_else(|| {
        let s = profiles::current_server();
        if s.is_empty() {
            tr("tray.official")
        } else {
            s
        }
    })
}

// ---- 開機自動啟動：Windows 用最高權限排程工作，其他平台用 plugin ----

#[allow(unused_variables)]
fn autostart_enabled(app: &AppHandle) -> bool {
    #[cfg(windows)]
    {
        platform::autostart_enabled()
    }
    #[cfg(not(windows))]
    {
        app.autolaunch().is_enabled().unwrap_or(false)
    }
}

#[allow(unused_variables)]
fn set_autostart_inner(app: &AppHandle, enabled: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        platform::set_autostart(enabled)
    }
    #[cfg(not(windows))]
    {
        let al = app.autolaunch();
        (if enabled { al.enable() } else { al.disable() }).map_err(|e| e.to_string())
    }
}

fn profile_item(
    app: &AppHandle,
    p: &Profile,
    active: &Option<String>,
) -> tauri::Result<CheckMenuItem<Wry>> {
    let label = if profiles::is_default_name(&p.name) {
        trf("tray.default_item", &[("name", &p.name)])
    } else {
        p.name.clone()
    };
    CheckMenuItemBuilder::with_id(format!("use:{}", p.name), label)
        .checked(active.as_deref() == Some(p.name.as_str()))
        .build(app)
}

fn build_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let ps = profiles::all().unwrap_or_default();
    let active = active_name(&ps);
    let fav = profiles::load_favorites();
    let (favs, others): (Vec<&Profile>, Vec<&Profile>) =
        ps.iter().partition(|p| fav.contains(&p.name));

    let fav_items = favs
        .iter()
        .map(|p| profile_item(app, p, &active))
        .collect::<tauri::Result<Vec<_>>>()?;
    let other_items = others
        .iter()
        .map(|p| profile_item(app, p, &active))
        .collect::<tauri::Result<Vec<_>>>()?;

    let update_item = match updater::pending_info(app) {
        Some(u) => Some(
            MenuItemBuilder::with_id("update", trf("tray.update", &[("version", &u.version)]))
                .build(app)?,
        ),
        None => None,
    };
    let header = MenuItemBuilder::with_id(
        "current",
        trf("tray.current", &[("name", &current_label(&ps))]),
    )
    .enabled(false)
    .build(app)?;
    let mut mb = MenuBuilder::new(app);
    if let Some(it) = &update_item {
        mb = mb.item(it).separator();
    }
    let mut mb = mb.item(&header).separator();
    if fav_items.is_empty() {
        // 還沒有最愛：全部直接列出
        for it in &other_items {
            mb = mb.item(it);
        }
    } else {
        // 最愛直接列出，其餘收進子選單
        for it in &fav_items {
            mb = mb.item(it);
        }
        if !other_items.is_empty() {
            let refs: Vec<&dyn IsMenuItem<Wry>> = other_items
                .iter()
                .map(|i| i as &dyn IsMenuItem<Wry>)
                .collect();
            let sub = SubmenuBuilder::new(app, tr("tray.more"))
                .items(&refs)
                .build()?;
            mb = mb.separator().item(&sub);
        }
    }

    let autostart = CheckMenuItemBuilder::with_id("autostart", tr("tray.autostart"))
        .checked(autostart_enabled(app))
        .build(app)?;

    mb.separator()
        .text("manage", tr("tray.open"))
        .separator()
        .item(&autostart)
        .separator()
        .text("quit", tr("tray.quit"))
        .build()
}

fn refresh_tray(app: &AppHandle) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        if let Ok(m) = build_menu(app) {
            let _ = tray.set_menu(Some(m));
        }
        let ps = profiles::all().unwrap_or_default();
        let what = current_label(&ps);
        let _ = tray.set_tooltip(Some(trf("tray.tooltip", &[("name", &what)])));
    }
    let _ = app.emit("changed", ());
}

fn do_switch(name: &str) -> Result<(), String> {
    let _g = OP_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    profiles::apply(&profiles::find(name)?)
}

/// 在背景執行緒切換，完成後以 toast 事件通知。
fn switch_async(app: AppHandle, name: String) {
    std::thread::spawn(move || {
        match do_switch(&name) {
            Ok(()) => toast(&app, true, trf("toast.switched", &[("name", &name)])),
            Err(e) => toast(&app, false, e),
        }
        refresh_tray(&app);
    });
}

fn handle_menu(app: &AppHandle, id: &str) {
    match id {
        "quit" => app.exit(0),
        "manage" | "update" => show_window(app),
        "autostart" => {
            if let Err(e) = set_autostart_inner(app, !autostart_enabled(app)) {
                toast(app, false, e);
            }
            refresh_tray(app);
        }
        other => {
            if let Some(name) = other.strip_prefix("use:") {
                switch_async(app.clone(), name.to_string());
            }
        }
    }
}

// ---- 前端呼叫的命令 ----

#[derive(Serialize)]
struct State {
    platform: &'static str,
    elevated: bool,
    /// RustDesk 已安裝為系統服務：切換需要系統管理員/root。
    needs_admin: bool,
    autostart: bool,
    /// 有效語言代碼（已解析 auto）與使用者的設定值（auto 或代碼）
    lang: &'static str,
    lang_setting: String,
    /// 目前執行的版本，以及已找到、等待使用者決定的更新
    version: String,
    update: Option<updater::UpdateInfo>,
    current: String,
    profiles: Vec<ProfileView>,
}

#[derive(Serialize)]
struct ProfileView {
    #[serde(flatten)]
    profile: Profile,
    /// 內建的 Default：不可修改、不可刪除。
    builtin: bool,
    favorite: bool,
}

#[tauri::command]
fn get_state(app: AppHandle) -> Result<State, String> {
    let elevated = platform::is_elevated();
    Ok(State {
        platform: std::env::consts::OS,
        elevated,
        needs_admin: platform::service_installed() && !elevated,
        autostart: autostart_enabled(&app),
        lang: i18n::get().code(),
        lang_setting: settings::load().language,
        version: app.package_info().version.to_string(),
        update: updater::pending_info(&app),
        current: profiles::current_server(),
        profiles: {
            let fav = profiles::load_favorites();
            profiles::all()?
                .into_iter()
                .map(|p| ProfileView {
                    builtin: profiles::is_default_name(&p.name),
                    favorite: fav.contains(&p.name),
                    profile: p,
                })
                .collect()
        },
    })
}

#[tauri::command]
fn save_profile(
    app: AppHandle,
    mut profile: Profile,
    old_name: Option<String>,
) -> Result<(), String> {
    profile.name = profile.name.trim().to_string();
    if profile.name.is_empty() {
        return Err(tr("err.name_required"));
    }
    profiles::save_as(old_name.as_deref(), profile)?;
    refresh_tray(&app);
    Ok(())
}

#[tauri::command]
fn delete_profile(app: AppHandle, name: String) -> Result<(), String> {
    profiles::delete(&name)?;
    refresh_tray(&app);
    Ok(())
}

#[tauri::command]
fn set_favorite(app: AppHandle, name: String, favorite: bool) -> Result<(), String> {
    profiles::set_favorite(&name, favorite)?;
    refresh_tray(&app);
    Ok(())
}

#[tauri::command]
fn use_profile(app: AppHandle, name: String) {
    switch_async(app, name);
}

#[tauri::command]
fn parse_text(text: String) -> Result<Profile, String> {
    let mut p = parse::parse_server_text(&text)?;
    p.name = parse::default_name(&p.server);
    Ok(p)
}

/// 檢查各伺服器的連線狀態（TCP）。空字串（Default）不檢查。
#[tauri::command]
async fn check_servers(servers: Vec<String>) -> Result<Vec<probe::Reach>, String> {
    let mut uniq: Vec<String> = servers
        .into_iter()
        .filter(|s| !s.trim().is_empty())
        .collect();
    uniq.sort();
    uniq.dedup();
    tauri::async_runtime::spawn_blocking(move || {
        probe::probe_all(uniq, std::time::Duration::from_secs(3))
    })
    .await
    .map_err(|e| e.to_string())
}

/// 設定語言：`auto` 跟隨系統，或指定語言代碼。立即生效並重建系統匣選單。
#[tauri::command]
fn set_language(app: AppHandle, lang: String) -> Result<(), String> {
    if lang != "auto" && i18n::Lang::from_code(&lang).is_none() {
        return Err(format!("unsupported language: {lang}"));
    }
    settings::save(&settings::Settings {
        language: lang.clone(),
    })?;
    i18n::set(i18n::resolve(&lang));
    refresh_tray(&app);
    Ok(())
}

/// 手動檢查更新。
#[tauri::command]
async fn check_update(app: AppHandle) -> Result<updater::UpdateCheck, String> {
    let r = updater::check(&app).await?;
    refresh_tray(&app); // 選單最上方的「有新版本」要跟著更新
    Ok(r)
}

/// 使用者按「立即更新」後才會呼叫：下載、驗證簽章、安裝並重新啟動。
#[tauri::command]
async fn install_update(app: AppHandle) -> Result<(), String> {
    updater::install(&app).await
}

#[tauri::command]
fn set_autostart(app: AppHandle, enabled: bool) -> Result<(), String> {
    set_autostart_inner(&app, enabled)?;
    refresh_tray(&app);
    Ok(())
}

pub fn run() {
    i18n::set(i18n::resolve(&settings::load().language));
    // single-instance 必須最先註冊：再次啟動時直接叫出既有視窗
    #[allow(unused_mut)] // Windows 不註冊 autostart plugin，不會重新指派
    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_window(app)
        }))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(updater::Pending::default());
    #[cfg(not(windows))]
    {
        builder = builder.plugin(
            tauri_plugin_autostart::Builder::new()
                .args(["--minimized"])
                .build(),
        );
    }
    builder
        .invoke_handler(tauri::generate_handler![
            get_state,
            save_profile,
            delete_profile,
            use_profile,
            set_favorite,
            check_servers,
            set_language,
            check_update,
            install_update,
            parse_text,
            set_autostart
        ])
        .on_window_event(|window, event| {
            // 關閉視窗 = 縮到系統匣；只有選單的「結束」才會真正離開
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .setup(|app| {
            let handle = app.handle().clone();
            let menu = build_menu(&handle)?;
            TrayIconBuilder::with_id(TRAY_ID)
                .icon(app.default_window_icon().expect("missing app icon").clone())
                .tooltip("rdswitch")
                .menu(&menu)
                .show_menu_on_left_click(false) // 左鍵開主視窗，右鍵才開選單（Linux 不支援，選單仍可用）
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_window(tray.app_handle());
                    }
                })
                .on_menu_event(|app, event| handle_menu(app, event.id().as_ref()))
                .build(app)?;
            refresh_tray(&handle);
            updater::spawn_auto_check(handle.clone(), refresh_tray);
            if !std::env::args().any(|a| a == "--minimized") {
                show_window(&handle);
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
