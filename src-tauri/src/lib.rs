mod credential_store;
mod fs_util;
mod overlay_geometry;
mod providers;

use providers::types::UsageSnapshot;
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use std::sync::Mutex;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, RunEvent, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
    WindowEvent,
};

/// Last known "home" position of the top bar (physical pixels). Used while ducked.
static OVERLAY_HOME_PHYSICAL: Mutex<Option<(i32, i32)>> = Mutex::new(None);
/// Pending drag position to flush into settings (physical pixels).
static OVERLAY_PENDING_PHYSICAL: Mutex<Option<(i32, i32)>> = Mutex::new(None);

#[tauri::command]
async fn fetch_provider_usage(provider_id: String) -> UsageSnapshot {
    providers::fetch_usage(&provider_id).await
}

#[tauri::command]
async fn fetch_all_usage(provider_ids: Vec<String>) -> Vec<UsageSnapshot> {
    let mut out = Vec::with_capacity(provider_ids.len());
    for id in provider_ids {
        out.push(providers::fetch_usage(&id).await);
    }
    out
}

#[tauri::command]
fn get_secret(provider_id: String, field_key: String) -> Result<Option<String>, String> {
    credential_store::get_secret(&provider_id, &field_key)
}

#[tauri::command]
fn set_secret(provider_id: String, field_key: String, value: String) -> Result<(), String> {
    if value.trim().is_empty() {
        credential_store::clear_secret(&provider_id, &field_key)
    } else {
        credential_store::set_secret(&provider_id, &field_key, value.trim())
    }
}

#[tauri::command]
fn get_settings() -> Result<Value, String> {
    credential_store::get_settings()
}

#[tauri::command]
fn set_settings(settings: Value) -> Result<(), String> {
    credential_store::set_settings(settings)
}

#[tauri::command]
fn get_last_resets() -> Result<Value, String> {
    credential_store::get_last_resets()
}

#[tauri::command]
fn set_last_resets(state: Value) -> Result<(), String> {
    credential_store::set_last_resets(state)
}

/// True if the app was installed via NSIS (has an uninstall registry key).
#[tauri::command]
fn is_installed() -> bool {
    std::process::Command::new("reg")
        .args([
            "query",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\com.headroom.app",
            "/ve",
        ])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Download the portable exe for `version` and self-replace the running exe.
/// Only called on the portable path; the installed path uses the updater plugin.
#[tauri::command]
async fn download_portable_update(app: AppHandle, version: String) -> Result<(), String> {
    let url = format!(
        "https://github.com/mmartain/HeadRoom/releases/download/v{}/HeadRoom.exe",
        version
    );
    let response = reqwest::get(&url)
        .await
        .map_err(|e| format!("Download failed: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Download error: {e}"))?;

    let current = std::env::current_exe().map_err(|e| e.to_string())?;
    let parent = current.parent().ok_or("No parent directory")?;

    // Write the downloaded exe next to the current one.
    let new_exe = parent.join("HeadRoom_new.exe");
    std::fs::write(&new_exe, &bytes).map_err(|e| e.to_string())?;

    // Spawn a detached batch script that waits for this process to exit,
    // then renames HeadRoom_new.exe → HeadRoom.exe and relaunches.
    let bat = std::env::temp_dir().join("headroom-update.bat");
    let script = format!(
        "@echo off\r\n:wait\r\ntimeout /t 2 /nobreak >nul\r\nmove /y \"{}\" \"{}\"\r\nif exist \"{}\" start \"\" \"{}\"\r\ndel \"%~f0\"",
        new_exe.display(),
        current.display(),
        current.display(),
        current.display()
    );
    std::fs::write(&bat, script).map_err(|e| e.to_string())?;

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        std::process::Command::new("cmd.exe")
            .args(["/C", bat.to_str().unwrap()])
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("sh")
            .arg(bat)
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    // Exit immediately so the batch script can replace the running exe.
    app.exit(0);
    // In case exit is delayed, fall back to a hard exit.
    std::thread::sleep(std::time::Duration::from_millis(500));
    std::process::exit(0);
}

#[tauri::command]
fn show_flyout(app: AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("main") {
        position_flyout_near_tray(&app);
        let _ = win.show();
        let _ = win.set_focus();
        let _ = win.unminimize();
    }
    Ok(())
}

#[tauri::command]
fn hide_flyout(app: AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.hide();
    }
    Ok(())
}

#[tauri::command]
fn set_overlay_visible(app: AppHandle, visible: bool) -> Result<(), String> {
    ensure_overlay(&app)?;
    if let Some(win) = app.get_webview_window("overlay") {
        if visible {
            apply_overlay_layout(&app)?;
            let _ = win.set_always_on_top(true);
            win.unminimize().map_err(|e| e.to_string())?;
            win.show().map_err(|e| e.to_string())?;
        } else {
            persist_overlay_position_from_window(&app);
            let _ = win.hide();
        }
    }
    // Persist preference
    let mut settings = credential_store::get_settings().unwrap_or_else(|_| json!({}));
    if !settings.is_object() {
        settings = json!({});
    }
    settings
        .as_object_mut()
        .unwrap()
        .insert("overlayVisible".into(), json!(visible));
    let _ = credential_store::set_settings(settings);
    Ok(())
}

#[tauri::command]
fn get_overlay_visible(_app: AppHandle) -> Result<bool, String> {
    Ok(overlay_user_wants_visible())
}

#[tauri::command]
fn update_tray_status(app: AppHandle, worst_remaining: Option<f64>) -> Result<(), String> {
    let tooltip = match worst_remaining {
        Some(r) if r <= 5.0 => format!("HeadRoom — CRITICAL {r:.0}% left"),
        Some(r) if r <= 20.0 => format!("HeadRoom — LOW {r:.0}% left"),
        Some(r) => format!("HeadRoom — {r:.0}% left (worst)"),
        None => "HeadRoom".to_string(),
    };

    let (r, g, b) = match worst_remaining {
        Some(v) if v <= 5.0 => (220u8, 70, 70),
        Some(v) if v <= 20.0 => (230, 160, 40),
        Some(_) => (90, 190, 120),
        None => (160, 165, 175),
    };

    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_tooltip(Some(tooltip));
        let icon = status_dot_icon(r, g, b);
        let _ = tray.set_icon(Some(icon));
    }
    Ok(())
}

fn status_dot_icon(r: u8, g: u8, b: u8) -> tauri::image::Image<'static> {
    const SIZE: u32 = 32;
    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];
    let cx = 15i32;
    let cy = 15i32;
    for y in 0..SIZE as i32 {
        for x in 0..SIZE as i32 {
            let dx = x - cx;
            let dy = y - cy;
            let i = ((y as u32 * SIZE + x as u32) * 4) as usize;
            if dx * dx + dy * dy <= 13 * 13 {
                rgba[i] = r;
                rgba[i + 1] = g;
                rgba[i + 2] = b;
                rgba[i + 3] = 255;
            }
        }
    }
    tauri::image::Image::new_owned(rgba, SIZE, SIZE)
}

fn ensure_overlay(app: &AppHandle) -> Result<(), String> {
    if app.get_webview_window("overlay").is_some() {
        // Existing window: keep user-dragged position (do not re-center).
        return Ok(());
    }
    let win = WebviewWindowBuilder::new(app, "overlay", WebviewUrl::App("index.html".into()))
        .title("Usage Overlay")
        .inner_size(713.0 * overlay_zoom_factor(), 40.0 * overlay_zoom_factor())
        .decorations(false)
        // Windows shadows add an invisible frame outside the requested size.
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .transparent(true)
        .visible(false)
        .build()
        .map_err(|e| e.to_string())?;
    attach_overlay_move_listener(&win);
    apply_overlay_layout(app)?;
    Ok(())
}

fn overlay_displays(app: &AppHandle) -> (Vec<overlay_geometry::Display>, usize) {
    let monitors = app.available_monitors().ok().unwrap_or_default();
    let primary = app.primary_monitor().ok().flatten();
    let primary_index = primary
        .and_then(|p| monitors.iter().position(|m| m.position() == p.position()))
        .unwrap_or(0);
    let displays = monitors
        .iter()
        .map(|monitor| {
            let work = monitor.work_area();
            overlay_geometry::Display {
                bounds: overlay_geometry::Rect {
                    x: monitor.position().x,
                    y: monitor.position().y,
                    width: monitor.size().width,
                    height: monitor.size().height,
                },
                work: overlay_geometry::Rect {
                    x: work.position.x,
                    y: work.position.y,
                    width: work.size.width,
                    height: work.size.height,
                },
                scale: monitor.scale_factor(),
            }
        })
        .collect();
    (displays, primary_index)
}

fn overlay_saved_physical(displays: &[overlay_geometry::Display]) -> Option<(f64, f64)> {
    let settings = credential_store::get_settings().ok()?;
    if let Some(point) = settings
        .get("overlayPhysicalX")
        .and_then(Value::as_f64)
        .zip(settings.get("overlayPhysicalY").and_then(Value::as_f64))
    {
        return Some(point);
    }
    overlay_geometry::legacy_position(
        displays,
        settings.get("overlayX")?.as_f64()?,
        settings.get("overlayY")?.as_f64()?,
    )
}

fn persist_overlay_physical(x: i32, y: i32, scale: f64) {
    // Retain the old keys for compatibility with earlier releases.
    let _ = credential_store::set_settings(json!({
        "overlayPhysicalX": x,
        "overlayPhysicalY": y,
        "overlayX": x as f64 / scale,
        "overlayY": y as f64 / scale,
    }));
}

fn place_overlay(app: &AppHandle, position: Option<(f64, f64)>) -> Result<(), String> {
    let win = app
        .get_webview_window("overlay")
        .ok_or("Overlay window missing")?;
    let (displays, primary) = overlay_displays(app);
    let placement = overlay_geometry::place(
        &displays,
        primary,
        position,
        enabled_provider_count(),
        overlay_zoom_factor(),
    )
    .ok_or("No usable display for the overlay")?;
    let size = tauri::PhysicalSize::new(placement.width, placement.height);
    let position = tauri::PhysicalPosition::new(placement.x, placement.y);
    if win.inner_size().ok() != Some(size) {
        win.set_size(size).map_err(|e| e.to_string())?;
    }
    if win.outer_position().ok() != Some(position) {
        win.set_position(position).map_err(|e| e.to_string())?;
    }
    if let Ok(mut home) = OVERLAY_HOME_PHYSICAL.lock() {
        *home = Some((placement.x, placement.y));
    }
    persist_overlay_physical(placement.x, placement.y, placement.scale);
    Ok(())
}

/// Recheck the live rectangle after a drag, DPI change, or display disconnect.
fn keep_overlay_on_screen(app: &AppHandle) {
    if left_mouse_button_held() {
        return;
    }
    let Some(win) = app.get_webview_window("overlay") else {
        return;
    };
    let Ok(position) = win.outer_position() else {
        return;
    };
    let (displays, primary) = overlay_displays(app);
    let point = Some((position.x as f64, position.y as f64));
    let Some(p) = overlay_geometry::place(
        &displays,
        primary,
        point,
        enabled_provider_count(),
        overlay_zoom_factor(),
    ) else {
        return;
    };
    if position != tauri::PhysicalPosition::new(p.x, p.y)
        || win.inner_size().ok() != Some(tauri::PhysicalSize::new(p.width, p.height))
    {
        if let Err(error) = place_overlay(app, point) {
            eprintln!("Could not recover overlay position: {error}");
        }
    }
}

fn persist_overlay_position_from_window(app: &AppHandle) {
    let Some(win) = app.get_webview_window("overlay") else {
        return;
    };
    let Ok(pos) = win.outer_position() else {
        return;
    };
    // Persist a validated placement, including on exit or explicit hiding.
    let _ = place_overlay(app, Some((pos.x as f64, pos.y as f64)));
}

fn enabled_provider_count() -> usize {
    let settings = credential_store::get_settings().unwrap_or_else(|_| json!({}));
    let Some(enabled) = settings.get("enabled").and_then(|v| v.as_object()) else {
        return 3;
    };
    enabled
        .values()
        .filter(|v| v.as_bool() == Some(true))
        .count()
        .max(1)
}

/// Top-bar zoom from settings (75–150%), default 100%.
fn overlay_zoom_factor() -> f64 {
    credential_store::get_settings()
        .ok()
        .and_then(|s| s.get("overlayZoom").and_then(|v| v.as_f64()))
        .unwrap_or(100.0)
        .clamp(75.0, 150.0)
        / 100.0
}

/// Apply saved (or default) placement using physical desktop coordinates.
fn apply_overlay_layout(app: &AppHandle) -> Result<(), String> {
    let (displays, _) = overlay_displays(app);
    place_overlay(app, overlay_saved_physical(&displays))
}

fn restore_overlay_on_startup(app: &AppHandle) {
    if !overlay_user_wants_visible() {
        return;
    }
    let _ = ensure_overlay(app);
    if let Err(error) = apply_overlay_layout(app) {
        eprintln!("Could not restore overlay position: {error}");
        return;
    }
    if let Some(win) = app.get_webview_window("overlay") {
        let _ = win.set_always_on_top(true);
        let _ = win.show();
    }
}

#[tauri::command]
fn refresh_overlay_layout(app: AppHandle, zoom: Option<f64>) -> Result<(), String> {
    if let Some(z) = zoom {
        let mut settings = credential_store::get_settings().unwrap_or_else(|_| json!({}));
        if !settings.is_object() {
            settings = json!({});
        }
        if let Some(obj) = settings.as_object_mut() {
            obj.insert("overlayZoom".into(), json!(z.clamp(75.0, 150.0)));
            let _ = credential_store::set_settings(settings);
        }
    }
    if !overlay_user_wants_visible() {
        return Ok(());
    }
    ensure_overlay(&app)?;
    // Don't force-show: while mouse-ducked the window is intentionally hidden
    // even though overlayVisible stays true.
    let was_visible = app
        .get_webview_window("overlay")
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false);
    apply_overlay_layout(&app)?;
    if let Some(win) = app.get_webview_window("overlay") {
        let _ = win.set_always_on_top(true);
        if was_visible {
            let _ = win.show();
        }
    }
    Ok(())
}

fn attach_overlay_move_listener(win: &WebviewWindow) {
    win.on_window_event(|event| {
        if let WindowEvent::Moved(pos) = event {
            if let Ok(mut home) = OVERLAY_HOME_PHYSICAL.lock() {
                *home = Some((pos.x, pos.y));
            }
            if let Ok(mut pending) = OVERLAY_PENDING_PHYSICAL.lock() {
                *pending = Some((pos.x, pos.y));
            }
        }
    });
}

fn flush_overlay_position_save(app: &AppHandle) {
    // Let native dragging cross monitor boundaries; validate as soon as it ends.
    if left_mouse_button_held() {
        return;
    }
    let pending = OVERLAY_PENDING_PHYSICAL
        .lock()
        .ok()
        .and_then(|mut p| p.take());
    if pending.is_some() {
        persist_overlay_position_from_window(app);
    }
}

/// Last fitted flyout height (logical px). 0 = use estimate from provider count.
static FLYOUT_HEIGHT_PX: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn estimate_flyout_height() -> f64 {
    let n = enabled_provider_count() as f64;
    // Header + footer + chrome ≈ 110; each provider pill ≈ 88.
    (110.0 + n * 88.0).clamp(220.0, 900.0)
}

fn flyout_height_logical() -> f64 {
    let stored = FLYOUT_HEIGHT_PX.load(Ordering::Relaxed);
    if stored > 0 {
        (stored as f64).clamp(200.0, 900.0)
    } else {
        estimate_flyout_height()
    }
}

fn place_flyout(app: &AppHandle, win_h: f64) {
    let Some(win) = app.get_webview_window("main") else {
        return;
    };
    let win_w = 360.0;
    let win_h = win_h.clamp(200.0, 900.0);
    if let Ok(Some(monitor)) = win.current_monitor() {
        let size = monitor.size();
        let scale = monitor.scale_factor();
        let margin = 12.0;
        let x = (size.width as f64 / scale) - win_w - margin;
        let y = (size.height as f64 / scale) - win_h - 48.0;
        let _ = win.set_size(tauri::LogicalSize::new(win_w, win_h));
        let _ = win.set_position(tauri::LogicalPosition::new(x.max(0.0), y.max(0.0)));
    } else {
        let _ = win.set_size(tauri::LogicalSize::new(win_w, win_h));
    }
}

fn position_flyout_near_tray(app: &AppHandle) {
    place_flyout(app, flyout_height_logical());
}

#[tauri::command]
fn fit_flyout_size(app: AppHandle, height: f64) -> Result<(), String> {
    let h = height.round().clamp(200.0, 900.0);
    FLYOUT_HEIGHT_PX.store(h as u32, Ordering::Relaxed);
    place_flyout(&app, h);
    Ok(())
}

#[cfg(windows)]
fn cursor_screen_pos() -> Option<(i32, i32)> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
    let mut pt = POINT { x: 0, y: 0 };
    unsafe {
        GetCursorPos(&mut pt).ok()?;
    }
    Some((pt.x, pt.y))
}

#[cfg(not(windows))]
fn cursor_screen_pos() -> Option<(i32, i32)> {
    None
}

/// Hold Shift to keep the top bar visible while “hide near mouse” is on (so you can drag it).
#[cfg(windows)]
fn shift_key_held() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_SHIFT};
    unsafe { GetAsyncKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000 != 0 }
}

#[cfg(not(windows))]
fn shift_key_held() -> bool {
    false
}

fn overlay_user_wants_visible() -> bool {
    credential_store::get_settings()
        .ok()
        .and_then(|s| s.get("overlayVisible").and_then(|v| v.as_bool()))
        .unwrap_or(true)
}

fn overlay_hide_near_mouse_enabled() -> bool {
    credential_store::get_settings()
        .ok()
        .and_then(|s| s.get("overlayHideNearMouse").and_then(|v| v.as_bool()))
        // Default on so the bar doesn't block title bars / tabs
        .unwrap_or(true)
}

fn cursor_near_overlay(app: &AppHandle) -> bool {
    let Some(win) = app.get_webview_window("overlay") else {
        return false;
    };
    let Ok(size) = win.outer_size() else {
        return false;
    };
    // Prefer remembered home so proximity still works while the window is hidden.
    let (ox, oy) = OVERLAY_HOME_PHYSICAL
        .lock()
        .ok()
        .and_then(|g| *g)
        .or_else(|| win.outer_position().ok().map(|p| (p.x, p.y)))
        .unwrap_or((0, 0));
    let Some((cx, cy)) = cursor_screen_pos() else {
        return false;
    };
    // Proximity pad so the bar ducks before the cursor lands on it
    let pad = 40i32;
    let left = ox - pad;
    let top = oy - pad;
    let right = ox + size.width as i32 + pad;
    let bottom = oy + size.height as i32 + pad + 12;
    cx >= left && cx <= right && cy >= top && cy <= bottom
}

fn show_overlay_at_home(app: &AppHandle) {
    if ensure_overlay(app).is_err() {
        return;
    }
    let home = OVERLAY_HOME_PHYSICAL.lock().ok().and_then(|g| *g);
    let result = match home {
        Some((x, y)) => place_overlay(app, Some((x as f64, y as f64))),
        None => apply_overlay_layout(app),
    };
    if let Err(error) = result {
        eprintln!("Could not show overlay: {error}");
        return;
    }
    if let Some(win) = app.get_webview_window("overlay") {
        let _ = win.set_always_on_top(true);
        let _ = win.unminimize();
        let _ = win.show();
    }
}

#[cfg(windows)]
fn left_mouse_button_held() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
    unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) as u16 & 0x8000 != 0 }
}

#[cfg(not(windows))]
fn left_mouse_button_held() -> bool {
    false
}

fn start_overlay_mouse_dodge(app: AppHandle) {
    std::thread::spawn(move || {
        let mut ducked = false;
        let mut last_screen_check = std::time::Instant::now();
        loop {
            std::thread::sleep(std::time::Duration::from_millis(70));
            flush_overlay_position_save(&app);
            if !overlay_user_wants_visible() {
                ducked = false;
                continue;
            }
            if last_screen_check.elapsed() >= std::time::Duration::from_millis(500) {
                keep_overlay_on_screen(&app);
                last_screen_check = std::time::Instant::now();
            }
            if left_mouse_button_held() {
                continue;
            }
            if !overlay_hide_near_mouse_enabled() {
                if ducked {
                    // Setting turned off while ducked — restore without re-centering
                    show_overlay_at_home(&app);
                    ducked = false;
                }
                continue;
            }
            // Shift temporarily disables ducking so the bar stays grabable / draggable.
            if shift_key_held() {
                if ducked {
                    show_overlay_at_home(&app);
                    ducked = false;
                }
                continue;
            }
            let near = cursor_near_overlay(&app);
            if near && !ducked {
                persist_overlay_position_from_window(&app);
                if let Some(win) = app.get_webview_window("overlay") {
                    let _ = win.hide();
                }
                ducked = true;
            } else if !near && ducked {
                show_overlay_at_home(&app);
                ducked = false;
            }
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Single-instance must register first so a second launch is handed off to this process.
    let mut builder = tauri::Builder::default();
    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // Re-running the exe shows the top status bar instead of starting a second process.
            let _ = set_overlay_visible(app.clone(), true);
            let _ = app.emit("overlay-toggled", true);
        }));
    }

    builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            fetch_provider_usage,
            fetch_all_usage,
            get_secret,
            set_secret,
            get_settings,
            set_settings,
            get_last_resets,
            set_last_resets,
            show_flyout,
            hide_flyout,
            fit_flyout_size,
            set_overlay_visible,
            get_overlay_visible,
            refresh_overlay_layout,
            update_tray_status,
            is_installed,
            download_portable_update,
        ])
        .setup(|app| {
            let show_i = MenuItem::with_id(app, "show", "Show usage", true, None::<&str>)?;
            let settings_i = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
            let refresh_i = MenuItem::with_id(app, "refresh", "Refresh", true, None::<&str>)?;
            let overlay_i =
                MenuItem::with_id(app, "overlay", "Toggle top status bar", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(
                app,
                &[&show_i, &settings_i, &refresh_i, &overlay_i, &quit_i],
            )?;

            let _tray = TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .tooltip("HeadRoom")
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        position_flyout_near_tray(app);
                        let _ = show_flyout(app.clone());
                        let _ = app.emit("open-flyout", ());
                    }
                    "settings" => {
                        position_flyout_near_tray(app);
                        let _ = show_flyout(app.clone());
                        let _ = app.emit("open-settings", ());
                    }
                    "refresh" => {
                        let _ = app.emit("tray-refresh", ());
                        position_flyout_near_tray(app);
                        let _ = show_flyout(app.clone());
                        let _ = app.emit("open-flyout", ());
                    }
                    "overlay" => {
                        let current = overlay_user_wants_visible();
                        let next = !current;
                        let _ = set_overlay_visible(app.clone(), next);
                        if next {
                            let _ = hide_flyout(app.clone());
                        }
                        let _ = app.emit("overlay-toggled", next);
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        position_flyout_near_tray(app);
                        if let Some(win) = app.get_webview_window("main") {
                            if win.is_visible().unwrap_or(false) {
                                let _ = win.hide();
                            } else {
                                let _ = win.show();
                                let _ = win.set_focus();
                            }
                        }
                    }
                })
                .build(app)?;

            // Start hidden; tray is the entry point
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.hide();
            }

            // Restore top-bar visibility + on-screen position from last session
            restore_overlay_on_startup(app.handle());
            start_overlay_mouse_dodge(app.handle().clone());

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building HeadRoom")
        .run(|app_handle, event| {
            if matches!(event, RunEvent::Exit | RunEvent::ExitRequested { .. }) {
                persist_overlay_position_from_window(app_handle);
            }
        });
}
