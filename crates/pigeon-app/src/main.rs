//! Pigeon: press Ctrl+Shift+Space on any reel, short, TikTok or post, and a
//! round glass panel lets you send it to someone on another app.
//!
//! The panel (ui/) only displays things and reports clicks. This file holds
//! the link that was found, the saved contacts, and decides what to open.

// No console window behind the app in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(windows)]
mod autostart;
mod contacts;
mod send;
#[cfg(windows)]
mod win;

#[cfg(not(windows))]
fn main() {
    eprintln!("Pigeon only runs on Windows for now.");
}

#[cfg(windows)]
fn main() {
    app::run();
}

#[cfg(windows)]
mod app {
    use std::path::PathBuf;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::time::Duration;

    use pigeon_core::detect::{Detector, Foreground, Miss};
    use pigeon_core::{Kind, Link, Platform};
    use serde::Serialize;
    use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
    use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, State, WindowEvent};
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, RegisterHotKey, VK_SPACE,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetMessageW, MSG, PostThreadMessageW, WM_APP, WM_HOTKEY,
    };

    use crate::autostart;
    use crate::contacts::{self, Contact, MAX_CONTACTS};
    use crate::send::{self, InstalledApps, Target};
    use crate::win;

    const HOTKEY: &str = "Ctrl+Shift+Space";
    const PANEL: &str = "panel";
    /// Sent to the hotkey thread to open the panel without the hotkey (tray icon).
    const WM_OPEN: u32 = WM_APP + 1;
    const WM_WELCOME: u32 = WM_APP + 2;

    #[derive(Default)]
    struct Session {
        /// The post found for the panel that's open now. Cleared on close.
        link: Option<Link>,
        /// The window to give focus back to if the panel is cancelled.
        previous: Option<Foreground>,
        open: bool,
        /// "welcome" or "share".
        mode: &'static str,
        contacts: Vec<Contact>,
    }

    struct AppState {
        session: Mutex<Session>,
        contacts_path: PathBuf,
        apps: InstalledApps,
        hotkey_thread: AtomicU32,
        /// The page has attached its event listeners. Until then the window
        /// stays hidden, so an empty, invisible window can never sit on screen.
        panel_ready: AtomicBool,
        welcomed: AtomicBool,
    }

    #[derive(Serialize, Clone)]
    struct OpenEvent {
        mode: &'static str,
        background: Option<String>,
        hotkey: &'static str,
    }

    #[derive(Serialize, Clone)]
    struct LinkEvent {
        found: bool,
        platform: String,
        label: String,
        short: String,
        message: String,
    }

    pub fn run() {
        tauri::Builder::default()
            .setup(|app| {
                let dir = app.path().app_config_dir()?;
                let contacts_path = contacts::path_in(&dir);
                contacts::bring_over_from_old_name(&dir, &contacts_path);
                let session = Session {
                    contacts: contacts::load(&contacts_path),
                    ..Default::default()
                };
                app.manage(AppState {
                    session: Mutex::new(session),
                    contacts_path,
                    apps: win::installed_apps(),
                    hotkey_thread: AtomicU32::new(0),
                    panel_ready: AtomicBool::new(false),
                    // Started with Windows: stay quiet, no welcome panel.
                    welcomed: AtomicBool::new(std::env::args().any(|a| a == autostart::FLAG)),
                });
                autostart::refresh();
                build_tray(app.handle())?;
                let handle = app.handle().clone();
                std::thread::Builder::new()
                    .name("pigeon-hotkey".into())
                    .spawn(move || hotkey_loop(handle))?;
                Ok(())
            })
            .on_window_event(|window, event| {
                // Safety net: whatever the page does, the panel never stays
                // on screen once you click somewhere else.
                if let WindowEvent::Focused(focused) = event {
                    log(&format!("focused: {focused}"));
                }
                if let WindowEvent::Focused(false) = event {
                    let state = window.state::<AppState>();
                    let mut session = lock(&state);
                    if session.open {
                        session.open = false;
                        session.link = None;
                        let _ = window.hide();
                    }
                }
            })
            .invoke_handler(tauri::generate_handler![
                panel_ready,
                list_contacts,
                add_contact,
                remove_contact,
                send_link,
                close_panel
            ])
            .run(tauri::generate_context!())
            .expect("Pigeon failed to start");
    }

    fn lock(state: &AppState) -> std::sync::MutexGuard<'_, Session> {
        state
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn build_tray(app: &AppHandle) -> tauri::Result<()> {
        let info = MenuItem::with_id(
            app,
            "info",
            format!("Press {HOTKEY} on any post"),
            false,
            None::<&str>,
        )?;
        let open = MenuItem::with_id(app, "open", "Open Pigeon", true, None::<&str>)?;
        let startup = CheckMenuItem::with_id(
            app,
            "autostart",
            "Start with Windows",
            true,
            autostart::is_enabled(),
            None::<&str>,
        )?;
        let quit = MenuItem::with_id(app, "quit", "Quit Pigeon", true, None::<&str>)?;
        let line = || PredefinedMenuItem::separator(app);
        let menu = Menu::with_items(app, &[&info, &line()?, &open, &startup, &line()?, &quit])?;
        let mut tray = TrayIconBuilder::with_id("pigeon")
            .tooltip(format!("Pigeon ({HOTKEY})"))
            .menu(&menu)
            .show_menu_on_left_click(false)
            .on_menu_event(move |app, event| match event.id.as_ref() {
                "open" => post_to_hotkey_thread(app, WM_OPEN),
                "autostart" => {
                    // Do what was asked, then show what's really true.
                    let result = if autostart::is_enabled() {
                        autostart::disable()
                    } else {
                        autostart::enable()
                    };
                    if let Err(e) = result {
                        win::message(&e);
                    }
                    let _ = startup.set_checked(autostart::is_enabled());
                }
                "quit" => app.exit(0),
                _ => {}
            })
            .on_tray_icon_event(|tray, event| {
                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } = event
                {
                    post_to_hotkey_thread(tray.app_handle(), WM_OPEN);
                }
            });
        if let Some(icon) = app.default_window_icon() {
            tray = tray.icon(icon.clone());
        }
        tray.build(app)?;
        Ok(())
    }

    fn post_to_hotkey_thread(app: &AppHandle, msg: u32) {
        let thread = app.state::<AppState>().hotkey_thread.load(Ordering::SeqCst);
        if thread != 0 {
            unsafe {
                let _ = PostThreadMessageW(thread, msg, Default::default(), Default::default());
            }
        }
    }

    /// Owns the hotkey and the detector (which needs its own COM thread).
    fn hotkey_loop(app: AppHandle) {
        let detector = match Detector::new() {
            Ok(d) => d,
            Err(e) => {
                win::message(&format!(
                    "Pigeon can't start: Windows UI Automation isn't available ({e})."
                ));
                app.exit(1);
                return;
            }
        };
        let state = app.state::<AppState>();
        state
            .hotkey_thread
            .store(unsafe { GetCurrentThreadId() }, Ordering::SeqCst);

        let registered = unsafe {
            RegisterHotKey(
                None,
                1,
                MOD_CONTROL | MOD_SHIFT | MOD_NOREPEAT,
                VK_SPACE.0 as u32,
            )
        };
        if registered.is_err() {
            win::message(&format!(
                "Pigeon is already running (look for its icon near the clock), \
                 or another app is using {HOTKEY}."
            ));
            app.exit(1);
            return;
        }

        log("hotkey registered");
        maybe_welcome(&app);

        let mut msg = MSG::default();
        while unsafe { GetMessageW(&mut msg, None, 0, 0) }.0 > 0 {
            match msg.message {
                WM_HOTKEY | WM_OPEN => toggle(&app, &detector),
                WM_WELCOME => show_panel(&app, "welcome", Foreground::now()),
                _ => {}
            }
        }
    }

    /// Show the welcome panel once, as soon as both the page and the hotkey
    /// thread are ready (whichever finishes last triggers it).
    fn maybe_welcome(app: &AppHandle) {
        let state = app.state::<AppState>();
        let ready = state.panel_ready.load(Ordering::SeqCst)
            && state.hotkey_thread.load(Ordering::SeqCst) != 0;
        if ready && !state.welcomed.swap(true, Ordering::SeqCst) {
            post_to_hotkey_thread(app, WM_WELCOME);
        }
    }

    /// Set PIGEON_DEBUG=1 to see what the app is doing (debug builds show a console).
    fn log(message: &str) {
        if std::env::var_os("PIGEON_DEBUG").is_some() {
            eprintln!("[pigeon] {message}");
        }
    }

    fn toggle(app: &AppHandle, detector: &Detector) {
        let state = app.state::<AppState>();
        log(&format!(
            "hotkey (page ready: {})",
            state.panel_ready.load(Ordering::SeqCst)
        ));
        if !state.panel_ready.load(Ordering::SeqCst) {
            return;
        }
        // Pressing the hotkey again closes the share panel. During the
        // welcome screen it opens the share panel instead.
        {
            let session = lock(&state);
            if session.open && session.mode == "share" {
                let _ = app.emit_to(PANEL, "panel:close", ());
                return;
            }
        }
        // Remember the browser before our panel takes focus.
        let front = Foreground::now();
        show_panel(app, "share", front);

        // The panel is in front now, so no Ctrl+L / Ctrl+C fallback: those
        // keys would land in our own window.
        let event = match detector.detect_from(front, false) {
            Ok(found) => {
                let event = LinkEvent {
                    found: true,
                    platform: platform_id(found.link.platform).into(),
                    label: label(&found.link),
                    short: short(&found.link.url),
                    message: String::new(),
                };
                let mut session = lock(&state);
                if !session.open {
                    return;
                }
                session.link = Some(found.link);
                event
            }
            Err(miss) => LinkEvent {
                found: false,
                platform: String::new(),
                label: String::new(),
                short: String::new(),
                message: miss_message(&miss).into(),
            },
        };
        let _ = app.emit_to(PANEL, "panel:link", event);
    }

    fn show_panel(app: &AppHandle, mode: &'static str, front: Foreground) {
        let Some(window) = app.get_webview_window(PANEL) else {
            log("no panel window");
            return;
        };
        let at = win::placement();
        log(&format!("show {mode} at {at:?}"));
        // Grab the backdrop while the panel is still hidden.
        let background = win::backdrop(at);
        {
            let state = app.state::<AppState>();
            let mut session = lock(&state);
            // Opening over the welcome screen: keep the window that was in
            // front before it, not the welcome panel itself.
            if !(session.open && session.mode == "welcome") {
                session.previous = Some(front);
            }
            session.open = true;
            session.mode = mode;
            session.link = None;
        }
        let _ = window.set_size(PhysicalSize::new(at.size, at.size));
        let _ = window.set_position(PhysicalPosition::new(at.x, at.y));
        let _ = app.emit_to(
            PANEL,
            "panel:open",
            OpenEvent {
                mode,
                background,
                hotkey: HOTKEY,
            },
        );
        let shown = window.show();
        let focused = window.set_focus();
        log(&format!("shown: {shown:?}, focused: {focused:?}"));
    }

    fn miss_message(miss: &Miss) -> &'static str {
        match miss {
            Miss::NoBrowser => "No browser open. Copy a link, then try again.",
            Miss::FeedWithoutPost { .. } => {
                "Couldn't pick out one post. Open it, or copy its link."
            }
            Miss::Unreadable { .. } => "Couldn't read the browser. Copy the link, then try again.",
        }
    }

    fn platform_id(p: Platform) -> &'static str {
        match p {
            Platform::YouTube => "youtube",
            Platform::Instagram => "instagram",
            Platform::TikTok => "tiktok",
            Platform::X => "x",
            Platform::Facebook => "facebook",
            Platform::Reddit => "reddit",
            Platform::Threads => "threads",
            Platform::Snapchat => "snapchat",
            Platform::Other => "web",
        }
    }

    /// "Instagram reel", "YouTube short", "TikTok video"...
    fn label(link: &Link) -> String {
        let url = link.url.as_str();
        let noun = match link.platform {
            _ if link.kind == Kind::Page && link.platform == Platform::Other => {
                return "Web page".into();
            }
            Platform::YouTube if url.contains("/shorts/") => "short",
            Platform::YouTube => "video",
            Platform::Instagram | Platform::Facebook if url.contains("/reel/") => "reel",
            Platform::TikTok if url.contains("/photo/") => "photo",
            Platform::TikTok => "video",
            Platform::Snapchat => "Spotlight",
            _ if link.kind == Kind::Page => "page",
            _ => "post",
        };
        format!("{} {noun}", link.platform.name())
    }

    /// The link without "https://www." and shortened in the middle if long.
    fn short(url: &str) -> String {
        let s = url
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_start_matches("www.");
        let s = s.trim_end_matches('/');
        let chars: Vec<char> = s.chars().collect();
        if chars.len() <= 40 {
            return s.to_string();
        }
        let head: String = chars[..26].iter().collect();
        let tail: String = chars[chars.len() - 10..].iter().collect();
        format!("{head}…{tail}")
    }

    // ----- Commands the panel can call -----

    #[tauri::command]
    fn panel_ready(app: AppHandle, state: State<AppState>) {
        log("page ready");
        state.panel_ready.store(true, Ordering::SeqCst);
        maybe_welcome(&app);
    }

    #[tauri::command]
    fn list_contacts(state: State<AppState>) -> Vec<Contact> {
        lock(&state).contacts.clone()
    }

    #[tauri::command]
    fn add_contact(
        state: State<AppState>,
        target: String,
        name: String,
        handle: String,
    ) -> Result<Contact, String> {
        let target = Target::from_id(&target).ok_or("Unknown app.")?;
        if !target.has_contacts() {
            return Err("This app doesn't use saved contacts.".into());
        }
        let (name, handle) = contacts::validate(target, &name, &handle)?;
        let mut session = lock(&state);
        if session.contacts.len() >= MAX_CONTACTS {
            return Err("That's the most people Pigeon can save. Remove someone first.".into());
        }
        if session
            .contacts
            .iter()
            .any(|c| c.target == target.id() && c.handle == handle)
        {
            return Err("You've already saved them.".into());
        }
        let contact = Contact {
            id: contacts::new_id(),
            target: target.id().into(),
            name,
            handle,
        };
        session.contacts.insert(0, contact.clone());
        contacts::save(&state.contacts_path, &session.contacts)
            .map_err(|_| "Couldn't save. Try again.")?;
        Ok(contact)
    }

    #[tauri::command]
    fn remove_contact(state: State<AppState>, id: String) -> Result<(), String> {
        let mut session = lock(&state);
        session.contacts.retain(|c| c.id != id);
        contacts::save(&state.contacts_path, &session.contacts)
            .map_err(|_| "Couldn't save. Try again.".into())
    }

    /// Send the post that was found to `target`, optionally to a saved
    /// contact. Returns a short hint to show, like "Press Enter to send".
    #[tauri::command]
    fn send_link(
        state: State<AppState>,
        target: String,
        contact_id: Option<String>,
    ) -> Result<String, String> {
        let target = Target::from_id(&target).ok_or("Unknown app.")?;
        let mut session = lock(&state);
        let link = session.link.clone().ok_or("There's no post to send yet.")?;
        let contact = match &contact_id {
            Some(id) => Some(
                session
                    .contacts
                    .iter()
                    .find(|c| &c.id == id && c.target == target.id())
                    .cloned()
                    .ok_or("That contact isn't saved any more.")?,
            ),
            None => None,
        };

        let action = send::plan(target, contact.as_ref(), &link.url, state.apps);
        if action.copy
            && !pigeon_core::detect::set_clipboard_text(&link.url)
            && action.open.is_none()
        {
            return Err("Couldn't copy the link. Try again.".into());
        }
        if let Some(url) = &action.open {
            win::open_url(url)?;
        }
        if action.then_press.is_some() || action.then_paste.is_some() {
            // The page takes a few seconds to load, and choosing a person
            // takes longer; wait off the UI thread, with its own detector
            // (UI Automation needs COM set up on the thread that uses it).
            let (press, paste, text) = (action.then_press, action.then_paste, link.url.clone());
            std::thread::spawn(move || {
                let Ok(detector) = Detector::new() else {
                    return;
                };
                if let Some((address, button)) = press {
                    detector.press_button_in_browser(&address, button, Duration::from_secs(12));
                }
                if let Some(address) = paste {
                    detector.paste_into_page(address, &text, Duration::from_secs(90));
                }
            });
        }

        // Most recently used people first.
        if let Some(c) = contact
            && let Some(pos) = session.contacts.iter().position(|x| x.id == c.id)
        {
            let c = session.contacts.remove(pos);
            session.contacts.insert(0, c);
            let _ = contacts::save(&state.contacts_path, &session.contacts);
        }
        Ok(action.hint.into())
    }

    #[tauri::command]
    fn close_panel(app: AppHandle, state: State<AppState>, restore_focus: bool) {
        let previous = {
            let mut session = lock(&state);
            session.open = false;
            session.link = None;
            session.previous.take()
        };
        // Hand focus back while our window is still in front; Windows
        // ignores the request otherwise.
        if restore_focus && let Some(p) = previous {
            p.restore();
        }
        if let Some(window) = app.get_webview_window(PANEL) {
            let _ = window.hide();
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn labels_and_short_links() {
            let l = pigeon_core::parse("https://www.instagram.com/reel/C8xYz12AbCd/").unwrap();
            assert_eq!(label(&l), "Instagram reel");
            assert_eq!(short(&l.url), "instagram.com/reel/C8xYz12AbCd");
            let y = pigeon_core::parse("youtube.com/shorts/dQw4w9WgXcQ").unwrap();
            assert_eq!(label(&y), "YouTube short");
            let w = pigeon_core::parse("https://example.com/a").unwrap();
            assert_eq!(label(&w), "Web page");
            let long =
                short("https://www.tiktok.com/@someone_with_a_long_name/video/7301234567890123456");
            assert!(long.chars().count() <= 37 && long.contains('…'), "{long}");
        }
    }
}
