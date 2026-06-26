use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};
use tokio::time::{sleep, Duration};

#[cfg(target_os = "macos")]
use tauri_nspanel::ManagerExt;

use crate::window::show_dashboard_window;

// Global state for always-on-top setting (used by Windows refresh loop)
#[cfg(target_os = "windows")]
static ALWAYS_ON_TOP_ENABLED: AtomicBool = AtomicBool::new(false);

// State for window visibility
pub struct WindowVisibility {
    #[allow(dead_code)]
    pub is_hidden: Mutex<bool>,
}

// State for registered shortcuts
pub struct RegisteredShortcuts {
    pub shortcuts: Mutex<HashMap<String, String>>, // action_id -> shortcut_key
}

impl Default for RegisteredShortcuts {
    fn default() -> Self {
        RegisteredShortcuts {
            shortcuts: Mutex::new(HashMap::new()),
        }
    }
}

pub struct LicenseState {
    has_active_license: AtomicBool,
}

impl Default for LicenseState {
    fn default() -> Self {
        LicenseState {
            has_active_license: AtomicBool::new(false),
        }
    }
}

impl LicenseState {
    #[allow(dead_code)]
    pub fn is_active(&self) -> bool {
        self.has_active_license.load(Ordering::Relaxed)
    }

    pub fn set_active(&self, active: bool) {
        self.has_active_license.store(active, Ordering::Relaxed);
    }
}

pub(crate) type MoveWindowTask = Arc<AtomicBool>;

pub(crate) struct MoveWindowState {
    tasks: Mutex<HashMap<String, MoveWindowTask>>,
}

impl Default for MoveWindowState {
    fn default() -> Self {
        MoveWindowState {
            tasks: Mutex::new(HashMap::new()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShortcutBinding {
    pub action: String,
    pub key: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShortcutsConfig {
    pub bindings: HashMap<String, ShortcutBinding>,
}

/// Initialize global shortcuts for the application
pub fn setup_global_shortcuts<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Let the frontend initialize from localStorage
    let state = app.state::<RegisteredShortcuts>();
    let _registered = match state.shortcuts.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            eprintln!("Mutex poisoned in setup, recovering...");
            poisoned.into_inner()
        }
    };
    eprintln!("Global shortcuts state initialized, waiting for frontend config");

    Ok(())
}

/// Handle shortcut action based on action_id
pub fn handle_shortcut_action<R: Runtime>(app: &AppHandle<R>, action_id: &str) {
    match action_id {
        "toggle_dashboard" => handle_toggle_dashboard(app),
        "toggle_window" => handle_toggle_window(app),
        "focus_input" => handle_focus_input(app),
        "move_window_up" => handle_move_window(app, "up"),
        "move_window_down" => handle_move_window(app, "down"),
        "move_window_left" => handle_move_window(app, "left"),
        "move_window_right" => handle_move_window(app, "right"),
        "audio_recording" => handle_audio_shortcut(app),
        "screenshot" => handle_screenshot_shortcut(app),
        "solve" => handle_solve_shortcut(app),
        "copy_response" => handle_copy_response_shortcut(app),
        "system_audio" => handle_system_audio_shortcut(app),
        custom_action => {
            // Emit custom action event for frontend to handle
            if let Some(window) = app.get_webview_window("main") {
                if let Err(e) = window.emit(
                    "custom-shortcut-triggered",
                    json!({ "action": custom_action }),
                ) {
                    eprintln!("Failed to emit custom shortcut event: {}", e);
                }
            }
        }
    }
}

pub fn start_move_window<R: Runtime>(app: &AppHandle<R>, direction: &str) {
    let state = app.state::<MoveWindowState>();
    let mut tasks = match state.tasks.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };

    if tasks.contains_key(direction) {
        return;
    }

    let stop_flag: MoveWindowTask = Arc::new(AtomicBool::new(false));
    let flag_clone = stop_flag.clone();
    let dir = direction.to_string();
    let app_handle = app.clone();

    tauri::async_runtime::spawn(async move {
        let interval = Duration::from_millis(16);
        while !flag_clone.load(Ordering::Relaxed) {
            handle_move_window(&app_handle, &dir);
            sleep(interval).await;
        }
    });

    tasks.insert(direction.to_string(), stop_flag);
}

pub fn stop_move_window<R: Runtime>(app: &AppHandle<R>, direction: &str) {
    let state = app.state::<MoveWindowState>();
    let mut tasks = match state.tasks.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };

    if let Some(flag) = tasks.remove(direction) {
        flag.store(true, Ordering::Relaxed);
    }
}

pub fn stop_all_move_windows<R: Runtime>(app: &AppHandle<R>) {
    let state = app.state::<MoveWindowState>();
    let mut tasks = match state.tasks.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };

    for (_direction, flag) in tasks.drain() {
        flag.store(true, Ordering::Relaxed);
    }
}

/// Handle app toggle (hide/show) with input focus and app icon management
fn handle_toggle_window<R: Runtime>(app: &AppHandle<R>) {
    // Get the main window
    let Some(window) = app.get_webview_window("main") else {
        return;
    };

    #[cfg(target_os = "windows")]
    {
        let state = app.state::<WindowVisibility>();
        let mut is_hidden = state.is_hidden.lock().unwrap();
        *is_hidden = !*is_hidden;

        // On Windows visibility is handled entirely in the frontend via the
        // `toggle-window-visibility` event (CSS show/hide); the OS window stays
        // shown the whole time. So we only emit the event and never call
        // show()/set_focus(), which would steal focus from the user's current
        // window. (Use the dedicated `focus_input` shortcut to bring Pluely
        // forward and focus the input.)
        if let Err(e) = window.emit("toggle-window-visibility", *is_hidden) {
            eprintln!("Failed to emit toggle-window-visibility event: {}", e);
        }
        return;
    }

    #[cfg(not(target_os = "windows"))]
    match window.is_visible() {
        Ok(true) => {
            #[cfg(target_os = "macos")]
            {
                let panel = app.get_webview_window("main").unwrap();
                let _ = panel.hide();
            }
            // Window is visible, hide it and handle app icon based on user settings
            if let Err(e) = window.hide() {
                eprintln!("Failed to hide window: {}", e);
            }
        }
        Ok(false) => {
            // Window is hidden, show it and handle app icon based on user settings
            if let Err(e) = window.show() {
                eprintln!("Failed to show window: {}", e);
            }

            if let Err(e) = window.set_focus() {
                eprintln!("Failed to focus window: {}", e);
            }

            #[cfg(target_os = "macos")]
            {
                let panel = app.get_webview_panel("main").unwrap();
                panel.show();
            }
            // Emit event to focus text input
            window.emit("focus-text-input", json!({})).unwrap();
        }
        Err(e) => {
            eprintln!("Failed to check window visibility: {}", e);
        }
    }
}

/// Handle audio shortcut
fn handle_audio_shortcut<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        // Ensure window is visible
        if let Ok(false) = window.is_visible() {
            if let Err(_e) = window.show() {
                return;
            }
            if let Err(e) = window.set_focus() {
                eprintln!("Failed to focus window: {}", e);
            }
        }

        // Emit event to start audio recording
        if let Err(e) = window.emit("start-audio-recording", json!({})) {
            eprintln!("Failed to emit audio recording event: {}", e);
        }
    }
}

/// Handle screenshot shortcut
fn handle_screenshot_shortcut<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        // Emit event to trigger screenshot - frontend will determine auto/manual mode
        if let Err(e) = window.emit("trigger-screenshot", json!({})) {
            eprintln!("Failed to emit screenshot event: {}", e);
        }
    }
}

/// Handle solve shortcut - submit all accumulated screenshots to AI
fn handle_solve_shortcut<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        // Ensure window is visible so the user sees the response
        if let Ok(false) = window.is_visible() {
            if let Err(e) = window.show() {
                eprintln!("Failed to show window: {}", e);
                return;
            }
            if let Err(e) = window.set_focus() {
                eprintln!("Failed to focus window: {}", e);
            }
        }

        // Emit event to trigger solve - frontend submits attached screenshots
        if let Err(e) = window.emit("trigger-solve", json!({})) {
            eprintln!("Failed to emit solve event: {}", e);
        }
    }
}

/// Handle copy response shortcut - copy the current AI response to clipboard
fn handle_copy_response_shortcut<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        // Emit event to trigger copy - frontend copies the current response
        if let Err(e) = window.emit("trigger-copy-response", json!({})) {
            eprintln!("Failed to emit copy response event: {}", e);
        }
    }
}

/// Handle system audio shortcut
fn handle_system_audio_shortcut<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        // Ensure window is visible
        if let Ok(false) = window.is_visible() {
            if let Err(e) = window.show() {
                eprintln!("Failed to show window: {}", e);
                return;
            }
            if let Err(e) = window.set_focus() {
                eprintln!("Failed to focus window: {}", e);
            }
        }

        // Emit event to toggle system audio capture - frontend will determine current state
        if let Err(e) = window.emit("toggle-system-audio", json!({})) {
            eprintln!("Failed to emit system audio event: {}", e);
        }
    }
}

/// Tauri command to get all registered shortcuts
#[tauri::command]
pub fn get_registered_shortcuts<R: Runtime>(
    app: AppHandle<R>,
) -> Result<HashMap<String, String>, String> {
    let state = app.state::<RegisteredShortcuts>();
    let registered = match state.shortcuts.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            eprintln!("Mutex poisoned in get_registered_shortcuts, recovering...");
            poisoned.into_inner()
        }
    };
    Ok(registered.clone())
}

/// Tauri command to update shortcuts dynamically
#[tauri::command]
pub fn update_shortcuts<R: Runtime>(
    app: AppHandle<R>,
    config: ShortcutsConfig,
) -> Result<(), String> {
    eprintln!("Updating shortcuts with {} bindings", config.bindings.len());

    let mut shortcuts_to_register = Vec::new();

    for (action_id, binding) in &config.bindings {
        if binding.enabled && !binding.key.is_empty() {
            if action_id == "move_window" {
                let modifiers = binding.key.trim();
                if modifiers.is_empty() {
                    continue;
                }

                let arrow_keys = vec!["up", "down", "left", "right"];
                for arrow in arrow_keys {
                    let full_key = format!("{}+{}", modifiers, arrow);
                    match full_key.parse::<Shortcut>() {
                        Ok(shortcut) => {
                            let direction_action_id = format!("move_window_{}", arrow);
                            shortcuts_to_register.push((direction_action_id, full_key, shortcut));
                        }
                        Err(e) => {
                            eprintln!("Invalid shortcut '{}' for move_window: {}", full_key, e);
                            return Err(format!(
                                "Invalid shortcut '{}' for move_window: {}",
                                full_key, e
                            ));
                        }
                    }
                }

                continue;
            }

            match binding.key.parse::<Shortcut>() {
                Ok(shortcut) => {
                    shortcuts_to_register.push((action_id.clone(), binding.key.clone(), shortcut));
                }
                Err(e) => {
                    eprintln!(
                        "Invalid shortcut '{}' for action '{}': {}",
                        binding.key, action_id, e
                    );
                    return Err(format!(
                        "Invalid shortcut '{}' for action '{}': {}",
                        binding.key, action_id, e
                    ));
                }
            }
        }
    }

    // First, stop any ongoing window movement
    stop_all_move_windows(&app);

    // Then, unregister all existing shortcuts
    unregister_all_shortcuts(&app)?;

    // Now register all new shortcuts
    let mut successfully_registered = HashMap::new();

    let mut registration_failures: Vec<(String, String, String)> = Vec::new();

    for (action_id, shortcut_str, shortcut) in shortcuts_to_register {
        // Defensive: if the plugin still considers this hotkey registered
        // (e.g. left over from a prior partial failure), unregister it first so
        // the register below doesn't fail with "HotKey already registered".
        if app.global_shortcut().is_registered(shortcut) {
            if let Err(e) = app.global_shortcut().unregister(shortcut) {
                eprintln!(
                    "Failed to pre-unregister existing shortcut {}: {}",
                    shortcut_str, e
                );
            }
        }

        match app.global_shortcut().register(shortcut) {
            Ok(_) => {
                eprintln!("Registered shortcut: {} -> {}", action_id, shortcut_str);
                successfully_registered.insert(action_id, shortcut_str);
            }
            Err(e) => {
                eprintln!("Failed to register {} shortcut: {}", action_id, e);
                registration_failures.push((action_id, shortcut_str, e.to_string()));
            }
        }
    }

    // Update state with successfully registered shortcuts
    {
        let state = app.state::<RegisteredShortcuts>();
        let mut registered = match state.shortcuts.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                eprintln!("Mutex poisoned in update_shortcuts, recovering...");
                poisoned.into_inner()
            }
        };

        registered.clear();
        registered.extend(successfully_registered);
    }

    if !registration_failures.is_empty() {
        if let Some(window) = app.get_webview_window("main") {
            if let Err(e) = window.emit("shortcut-registration-error", &registration_failures) {
                eprintln!("Failed to emit shortcut registration error event: {}", e);
            }
        }

        let error_messages: Vec<String> = registration_failures
            .into_iter()
            .map(|(action, key, error)| format!("{} ({}) - {}", action, key, error))
            .collect();

        return Err(format!(
            "Some shortcuts could not be registered: {}",
            error_messages.join("; ")
        ));
    }

    Ok(())
}

/// Unregister all currently registered shortcuts.
///
/// We unregister via the plugin's own `unregister_all()` rather than iterating
/// our `RegisteredShortcuts` state. The state map can drift out of sync with the
/// plugin's actual registry: when a `register()` call fails, the shortcut may
/// remain registered in the plugin but never gets recorded in our state, so a
/// state-driven unregister would leave it behind and the next registration would
/// fail with "HotKey already registered". Clearing the plugin's registry
/// directly guarantees a clean slate.
fn unregister_all_shortcuts<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    if let Err(e) = app.global_shortcut().unregister_all() {
        eprintln!("Failed to unregister all shortcuts: {}", e);
    }

    // Keep our state map in sync with the now-empty plugin registry.
    let state = app.state::<RegisteredShortcuts>();
    let mut registered = match state.shortcuts.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            eprintln!("Mutex poisoned in unregister_all_shortcuts, recovering...");
            poisoned.into_inner()
        }
    };
    registered.clear();

    Ok(())
}

/// Tauri command to check if shortcuts are registered
#[tauri::command]
pub fn check_shortcuts_registered<R: Runtime>(app: AppHandle<R>) -> Result<bool, String> {
    let state = app.state::<RegisteredShortcuts>();
    let registered = match state.shortcuts.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            eprintln!("Mutex poisoned in check_shortcuts_registered, recovering...");
            poisoned.into_inner()
        }
    };
    Ok(!registered.is_empty())
}

/// Tauri command to validate shortcut key
#[tauri::command]
pub fn validate_shortcut_key(key: String) -> Result<bool, String> {
    match key.parse::<Shortcut>() {
        Ok(_) => Ok(true),
        Err(e) => {
            eprintln!("Invalid shortcut '{}': {}", key, e);
            Ok(false)
        }
    }
}

#[tauri::command]
pub fn set_license_status<R: Runtime>(app: AppHandle<R>, has_license: bool) -> Result<(), String> {
    {
        let state = app.state::<LicenseState>();
        state.set_active(has_license);
    }

    if !has_license {
        stop_all_move_windows(&app);
    }

    Ok(())
}

/// Tauri command to set app icon visibility in dock/taskbar
#[tauri::command]
pub fn set_app_icon_visibility<R: Runtime>(app: AppHandle<R>, visible: bool) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        // On macOS, use activation policy to control dock icon
        let policy = if visible {
            tauri::ActivationPolicy::Regular
        } else {
            tauri::ActivationPolicy::Accessory
        };

        app.set_activation_policy(policy).map_err(|e| {
            eprintln!("Failed to set activation policy: {}", e);
            format!("Failed to set activation policy: {}", e)
        })?;
    }

    #[cfg(target_os = "windows")]
    {
        // On Windows, control taskbar icon visibility.
        //
        // `set_skip_taskbar` alone is unreliable: it removes the taskbar button
        // via ITaskbarList::DeleteTab, but Windows re-adds the button whenever
        // the window regains focus (which the overlay does constantly). The
        // reliable fix is to switch the window's extended style to
        // WS_EX_TOOLWINDOW (a tool window is never shown in the taskbar) and
        // clear WS_EX_APPWINDOW. The style change only applies to the taskbar
        // while the window is hidden, so we hide -> change style -> show.
        if let Some(window) = app.get_webview_window("main") {
            window
                .set_skip_taskbar(!visible)
                .map_err(|e| format!("Failed to set taskbar visibility: {}", e))?;

            match window.hwnd() {
                Ok(raw_hwnd) => {
                    use windows_sys::Win32::Foundation::HWND;
                    use windows_sys::Win32::UI::WindowsAndMessaging::{
                        GetAncestor, GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos,
                        ShowWindow, GA_ROOT, GWL_EXSTYLE, SWP_FRAMECHANGED, SWP_NOACTIVATE,
                        SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SW_HIDE, WS_EX_APPWINDOW,
                        WS_EX_TOOLWINDOW,
                    };

                    let was_visible = window.is_visible().unwrap_or(true);

                    // SAFETY: raw_hwnd is a valid top-level window handle owned
                    // by this window; we resolve its root ancestor to be safe.
                    unsafe {
                        let mut hwnd = raw_hwnd.0 as HWND;
                        let root = GetAncestor(hwnd, GA_ROOT);
                        if !root.is_null() {
                            hwnd = root;
                        }

                        let before = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
                        let mut ex_style = before;
                        if visible {
                            ex_style |= WS_EX_APPWINDOW as isize;
                            ex_style &= !(WS_EX_TOOLWINDOW as isize);
                        } else {
                            ex_style |= WS_EX_TOOLWINDOW as isize;
                            ex_style &= !(WS_EX_APPWINDOW as isize);
                        }
                        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex_style);
                        SetWindowPos(
                            hwnd,
                            std::ptr::null_mut(),
                            0,
                            0,
                            0,
                            0,
                            SWP_FRAMECHANGED
                                | SWP_NOACTIVATE
                                | SWP_NOMOVE
                                | SWP_NOSIZE
                                | SWP_NOZORDER,
                        );

                        // Hide now. The shell only re-evaluates taskbar
                        // presence (WS_EX_TOOLWINDOW) across a hide->show that
                        // it actually processes, so we re-show after the message
                        // loop has pumped (deferred below).
                        if was_visible {
                            ShowWindow(hwnd, SW_HIDE);
                        }

                        eprintln!(
                            "set_app_icon_visibility(visible={}): hwnd={:?} ex_style {:#x} -> {:#x} (was_visible={})",
                            visible, hwnd, before, ex_style, was_visible
                        );
                    }

                    // Re-show on the main thread after a short delay so the
                    // taskbar button is dropped/re-added per the new style.
                    if was_visible {
                        let app_handle = app.clone();
                        tauri::async_runtime::spawn(async move {
                            sleep(Duration::from_millis(80)).await;
                            let app_for_main = app_handle.clone();
                            let _ = app_handle.run_on_main_thread(move || {
                                if let Some(w) = app_for_main.get_webview_window("main") {
                                    if let Ok(h) = w.hwnd() {
                                        use windows_sys::Win32::Foundation::HWND;
                                        use windows_sys::Win32::UI::WindowsAndMessaging::{
                                            GetAncestor, ShowWindow, GA_ROOT, SW_SHOWNA,
                                        };
                                        // SAFETY: valid top-level window handle.
                                        unsafe {
                                            let mut hh = h.0 as HWND;
                                            let r = GetAncestor(hh, GA_ROOT);
                                            if !r.is_null() {
                                                hh = r;
                                            }
                                            // SW_SHOWNA = show without activating,
                                            // to avoid stealing focus.
                                            ShowWindow(hh, SW_SHOWNA);
                                        }
                                    }
                                }
                            });
                        });
                    }
                }
                Err(e) => {
                    eprintln!("Failed to get main window hwnd: {}", e);
                }
            }
        } else {
            eprintln!("Main window not found on Windows");
        }
    }

    #[cfg(target_os = "linux")]
    {
        // On Linux, control panel icon visibility
        if let Some(window) = app.get_webview_window("main") {
            window
                .set_skip_taskbar(!visible)
                .map_err(|e| format!("Failed to set panel visibility: {}", e))?;
        } else {
            eprintln!("Main window not found on Linux");
        }
    }

    Ok(())
}

/// Helper function to set window topmost on Windows
#[cfg(target_os = "windows")]
fn set_window_topmost(hwnd: isize, enabled: bool) {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, HWND_NOTOPMOST, HWND_TOPMOST, SWP_NOMOVE, SWP_NOSIZE, SWP_NOACTIVATE,
    };

    let hwnd_value = hwnd as HWND;
    let insert_after = if enabled { HWND_TOPMOST } else { HWND_NOTOPMOST };

    unsafe {
        SetWindowPos(
            hwnd_value,
            insert_after,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}

/// Tauri command to set always on top state
#[tauri::command]
pub fn set_always_on_top<R: Runtime>(app: AppHandle<R>, enabled: bool) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        #[cfg(target_os = "windows")]
        {
            ALWAYS_ON_TOP_ENABLED.store(enabled, Ordering::SeqCst);

            if let Ok(hwnd) = window.hwnd() {
                set_window_topmost(hwnd.0 as isize, enabled);
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            window
                .set_always_on_top(enabled)
                .map_err(|e| format!("Failed to set always on top: {}", e))?;
        }
    } else {
        return Err("Main window not found".to_string());
    }

    Ok(())
}

/// Start a background task that periodically re-asserts topmost status on Windows.
/// This helps maintain always-on-top over fullscreen applications.
#[cfg(target_os = "windows")]
pub fn start_topmost_refresh_task<R: Runtime>(app: AppHandle<R>) {
    use std::thread;
    use std::time::Duration as StdDuration;

    thread::spawn(move || {
        loop {
            thread::sleep(StdDuration::from_millis(500));

            if ALWAYS_ON_TOP_ENABLED.load(Ordering::SeqCst) {
                if let Some(window) = app.get_webview_window("main") {
                    if let Ok(visible) = window.is_visible() {
                        if visible {
                            if let Ok(hwnd) = window.hwnd() {
                                set_window_topmost(hwnd.0 as isize, true);
                            }
                        }
                    }
                }
            }
        }
    });
}

/// Handle toggle dashboard shortcut
fn handle_toggle_dashboard<R: Runtime>(app: &AppHandle<R>) {
    if let Some(dashboard_window) = app.get_webview_window("dashboard") {
        match dashboard_window.is_visible() {
            Ok(true) => {
                // Window is visible, hide it
                if let Err(e) = dashboard_window.hide() {
                    eprintln!("Failed to hide dashboard window: {}", e);
                }
            }
            Ok(false) => {
                // Window is hidden, show and focus it
                if let Err(e) = dashboard_window.show() {
                    eprintln!("Failed to show dashboard window: {}", e);
                }
                if let Err(e) = dashboard_window.set_focus() {
                    eprintln!("Failed to focus dashboard window: {}", e);
                }
            }
            Err(e) => {
                eprintln!("Failed to check dashboard visibility: {}", e);
            }
        }
    } else {
        // Window doesn't exist, create and show it
        match show_dashboard_window(app) {
            Ok(_) => eprintln!("Dashboard window created and shown successfully"),
            Err(e) => eprintln!("Failed to create/show dashboard window: {}", e),
        }
    }
}

/// Handle focus input shortcut
fn handle_focus_input<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        // Ensure window is visible
        if let Ok(false) = window.is_visible() {
            let _ = window.show();
        }

        let _ = window.set_focus();
        let _ = window.emit("focus-text-input", json!({}));
    }
}

fn handle_move_window<R: Runtime>(app: &AppHandle<R>, direction: &str) {
    if let Some(window) = app.get_webview_window("main") {
        match window.outer_position() {
            Ok(current_pos) => {
                let step = 12;
                let (new_x, new_y) = match direction {
                    "up" => (current_pos.x, current_pos.y - step),
                    "down" => (current_pos.x, current_pos.y + step),
                    "left" => (current_pos.x - step, current_pos.y),
                    "right" => (current_pos.x + step, current_pos.y),
                    _ => {
                        eprintln!("Invalid direction: {}", direction);
                        return;
                    }
                };

                if let Err(e) =
                    window.set_position(tauri::Position::Physical(tauri::PhysicalPosition {
                        x: new_x,
                        y: new_y,
                    }))
                {
                    eprintln!("Failed to set window position: {}", e);
                }
            }
            Err(e) => {
                eprintln!("Failed to get window position: {}", e);
            }
        }
    } else {
        eprintln!("Main window not found");
    }
}

/// Tauri command to exit the application
#[tauri::command]
pub fn exit_app(app_handle: tauri::AppHandle) {
    app_handle.exit(0);
}
