mod credential_store;
mod providers;

use providers::types::UsageSnapshot;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent,
};

/// Last known "home" position of the top bar (physical pixels). Used while ducked.
static OVERLAY_HOME_PHYSICAL: Mutex<Option<(i32, i32)>> = Mutex::new(None);
/// Pending drag position to flush into settings (physical pixels).
static OVERLAY_PENDING_PHYSICAL: Mutex<Option<(i32, i32)>> = Mutex::new(None);
/// Skip persisting programmatic moves (layout apply).
static OVERLAY_SKIP_MOVE_SAVE: AtomicBool = AtomicBool::new(false);

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
            apply_overlay_layout(&app);
            let _ = win.show();
        } else {
            capture_overlay_home(&app);
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
fn get_overlay_visible(app: AppHandle) -> Result<bool, String> {
    let settings = credential_store::get_settings().unwrap_or_else(|_| json!({}));
    let preferred = settings
        .get("overlayVisible")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if preferred {
        ensure_overlay(&app)?;
        apply_overlay_layout(&app);
        if let Some(win) = app.get_webview_window("overlay") {
            let _ = win.show();
        }
    }
    Ok(preferred)
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
        .inner_size(620.0, 32.0)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .transparent(true)
        .visible(false)
        .build()
        .map_err(|e| e.to_string())?;
    attach_overlay_move_listener(&win);
    apply_overlay_layout(app);
    Ok(())
}

fn overlay_saved_logical() -> Option<(f64, f64)> {
    let settings = credential_store::get_settings().ok()?;
    let x = settings.get("overlayX")?.as_f64()?;
    let y = settings.get("overlayY")?.as_f64()?;
    if !x.is_finite() || !y.is_finite() {
        return None;
    }
    Some((x, y))
}

fn persist_overlay_logical(x: f64, y: f64) {
    let mut settings = credential_store::get_settings().unwrap_or_else(|_| json!({}));
    if !settings.is_object() {
        settings = json!({});
    }
    if let Some(obj) = settings.as_object_mut() {
        obj.insert("overlayX".into(), json!(x));
        obj.insert("overlayY".into(), json!(y));
        let _ = credential_store::set_settings(settings);
    }
}

fn capture_overlay_home(app: &AppHandle) {
    let Some(win) = app.get_webview_window("overlay") else {
        return;
    };
    if let Ok(pos) = win.outer_position() {
        if let Ok(mut home) = OVERLAY_HOME_PHYSICAL.lock() {
            *home = Some((pos.x, pos.y));
        }
    }
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

fn default_overlay_logical(win: &WebviewWindow) -> (f64, f64, f64, f64) {
    let monitor = win
        .primary_monitor()
        .ok()
        .flatten()
        .or_else(|| win.current_monitor().ok().flatten());
    let Some(monitor) = monitor else {
        return (520.0, 32.0, 24.0, 8.0);
    };
    let size = monitor.size();
    let position = monitor.position();
    let scale = monitor.scale_factor();
    let screen_w = size.width as f64 / scale;
    let origin_x = position.x as f64 / scale;
    let origin_y = position.y as f64 / scale;
    let n = enabled_provider_count() as f64;
    // Prefer ~200px per enabled provider, never wider than the screen,
    // and at least 360px unless the display itself is narrower.
    let available = (screen_w - 24.0).max(1.0);
    let preferred = 200.0 * n;
    let min_w = 360.0_f64.min(available);
    let bar_w = preferred.clamp(min_w, available);
    let bar_h = 32.0;
    let x = origin_x + (screen_w - bar_w) / 2.0;
    let y = origin_y + 8.0;
    (bar_w, bar_h, x, y)
}

/// Apply size + saved (or default) position. Used on create / show — not on mouse unduck.
fn apply_overlay_layout(app: &AppHandle) {
    let Some(win) = app.get_webview_window("overlay") else {
        return;
    };
    let (bar_w, bar_h, default_x, default_y) = default_overlay_logical(&win);
    let (x, y) = overlay_saved_logical().unwrap_or((default_x, default_y));

    OVERLAY_SKIP_MOVE_SAVE.store(true, Ordering::SeqCst);
    let _ = win.set_size(tauri::LogicalSize::new(bar_w, bar_h));
    let _ = win.set_position(tauri::LogicalPosition::new(x, y));
    capture_overlay_home(app);
    // Allow move events again after Windows finishes the programmatic move.
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_millis(250));
        OVERLAY_SKIP_MOVE_SAVE.store(false, Ordering::SeqCst);
    });
}

fn attach_overlay_move_listener(win: &WebviewWindow) {
    win.on_window_event(|event| {
        if OVERLAY_SKIP_MOVE_SAVE.load(Ordering::SeqCst) {
            return;
        }
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
    let pending = {
        let Ok(mut guard) = OVERLAY_PENDING_PHYSICAL.lock() else {
            return;
        };
        guard.take()
    };
    let Some((px, py)) = pending else {
        return;
    };
    let Some(win) = app.get_webview_window("overlay") else {
        return;
    };
    let scale = win.scale_factor().unwrap_or(1.0);
    if scale <= 0.0 {
        return;
    }
    persist_overlay_logical(px as f64 / scale, py as f64 / scale);
}

fn position_flyout_near_tray(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        // Place near bottom-right of primary monitor (typical tray corner on Win11)
        if let Ok(Some(monitor)) = win.current_monitor() {
            let size = monitor.size();
            let scale = monitor.scale_factor();
            let win_w = 360.0;
            let win_h = 520.0;
            let margin = 12.0;
            let x = (size.width as f64 / scale) - win_w - margin;
            let y = (size.height as f64 / scale) - win_h - 48.0;
            let _ = win.set_size(tauri::LogicalSize::new(win_w, win_h));
            let _ = win.set_position(tauri::LogicalPosition::new(x.max(0.0), y.max(0.0)));
        }
    }
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

fn overlay_user_wants_visible() -> bool {
    credential_store::get_settings()
        .ok()
        .and_then(|s| s.get("overlayVisible").and_then(|v| v.as_bool()))
        .unwrap_or(false)
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
    let _ = ensure_overlay(app);
    if let Some(win) = app.get_webview_window("overlay") {
        // Restore last dragged spot if Windows moved it while hidden; otherwise leave as-is.
        if let Ok(home) = OVERLAY_HOME_PHYSICAL.lock() {
            if let Some((x, y)) = *home {
                OVERLAY_SKIP_MOVE_SAVE.store(true, Ordering::SeqCst);
                let _ = win.set_position(tauri::PhysicalPosition::new(x, y));
                std::thread::spawn(|| {
                    std::thread::sleep(std::time::Duration::from_millis(250));
                    OVERLAY_SKIP_MOVE_SAVE.store(false, Ordering::SeqCst);
                });
            }
        }
        let _ = win.show();
    }
}

fn start_overlay_mouse_dodge(app: AppHandle) {
    std::thread::spawn(move || {
        let mut ducked = false;
        loop {
            std::thread::sleep(std::time::Duration::from_millis(70));
            flush_overlay_position_save(&app);
            if !overlay_user_wants_visible() {
                ducked = false;
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
            let near = cursor_near_overlay(&app);
            if near && !ducked {
                capture_overlay_home(&app);
                flush_overlay_position_save(&app);
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
        .invoke_handler(tauri::generate_handler![
            fetch_provider_usage,
            fetch_all_usage,
            get_secret,
            set_secret,
            get_settings,
            set_settings,
            show_flyout,
            hide_flyout,
            set_overlay_visible,
            get_overlay_visible,
            update_tray_status,
        ])
        .setup(|app| {
            let show_i = MenuItem::with_id(app, "show", "Show usage", true, None::<&str>)?;
            let settings_i = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
            let refresh_i = MenuItem::with_id(app, "refresh", "Refresh", true, None::<&str>)?;
            let overlay_i =
                MenuItem::with_id(app, "overlay", "Toggle top status bar", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu =
                Menu::with_items(app, &[&show_i, &settings_i, &refresh_i, &overlay_i, &quit_i])?;

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
                        let current = get_overlay_visible(app.clone()).unwrap_or(false);
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

            // Restore overlay preference
            let _ = get_overlay_visible(app.handle().clone());
            start_overlay_mouse_dodge(app.handle().clone());

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running HeadRoom");
}
