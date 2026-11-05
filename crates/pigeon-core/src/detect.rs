//! Find the post on screen in a desktop browser, using Windows UI Automation.
//!
//! UI Automation is the accessibility API screen readers use. Browsers expose
//! their address bar and page content through it, so no browser extension is
//! needed. Nothing is read until [`Detector::detect`] is called, and nothing
//! leaves the machine.
//!
//! Order of attempts:
//! 1. Read the address bar of the browser window in front (or the most recent one).
//! 2. If that's a feed (Instagram home, TikTok For You...), scan the visible
//!    page for the post closest to the middle of the screen.
//! 3. If the address bar can't be read, press Ctrl+L, Ctrl+C in the browser.
//! 4. Last resort: a supported link already on the clipboard.

use std::time::{Duration, Instant};

use windows::Win32::Foundation::{CloseHandle, HANDLE, HGLOBAL, HWND, LPARAM, RECT};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, GetClipboardSequenceNumber, OpenClipboard,
    SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::CF_UNICODETEXT;
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Accessibility::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT,
    KEYEVENTF_KEYUP, SendInput, VIRTUAL_KEY, VK_C, VK_CONTROL, VK_ESCAPE, VK_L, VK_LWIN, VK_MENU,
    VK_RWIN, VK_SHIFT, VK_V,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetForegroundWindow, GetWindowTextLengthW, GetWindowThreadProcessId, IsIconic,
    IsWindowVisible, SetForegroundWindow,
};
use windows::core::{BOOL, BSTR, PWSTR};

use crate::links::{self, Kind, Link};

/// Which browsers we look for, by executable name, and how to find their address bar.
const BROWSERS: &[(&str, Engine)] = &[
    ("chrome.exe", Engine::Chromium),
    ("msedge.exe", Engine::Chromium),
    ("brave.exe", Engine::Chromium),
    ("vivaldi.exe", Engine::Chromium),
    ("opera.exe", Engine::Chromium),
    ("chromium.exe", Engine::Chromium),
    ("arc.exe", Engine::Chromium),
    ("firefox.exe", Engine::Firefox),
    ("zen.exe", Engine::Firefox),
    ("librewolf.exe", Engine::Firefox),
    ("floorp.exe", Engine::Firefox),
    ("waterfox.exe", Engine::Firefox),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Engine {
    Chromium,
    Firefox,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    AddressBar,
    PageScan,
    ShareButton,
    CopyKeys,
    Clipboard,
}

impl Method {
    pub fn describe(self) -> &'static str {
        match self {
            Method::AddressBar => "address bar",
            Method::PageScan => "page scan",
            Method::ShareButton => "the site's own Share > Copy",
            Method::CopyKeys => "Ctrl+L / Ctrl+C",
            Method::Clipboard => "clipboard",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Found {
    pub link: Link,
    pub method: Method,
    /// Executable of the browser it came from, if any.
    pub app: Option<String>,
    pub elapsed: Duration,
}

#[derive(Debug, Clone)]
pub enum Miss {
    /// No browser window open and nothing useful on the clipboard.
    NoBrowser,
    /// On a feed, but no single post could be picked out.
    FeedWithoutPost { feed: Link, app: String },
    /// A browser was found but its URL couldn't be read.
    Unreadable { app: String },
}

struct Browser {
    hwnd: HWND,
    exe: String,
    engine: Engine,
    foreground: bool,
}

pub struct Detector {
    uia: IUIAutomation,
}

impl Detector {
    /// Must be called on the thread that will call [`Detector::detect`].
    pub fn new() -> windows::core::Result<Self> {
        unsafe {
            // Fails harmlessly if COM is already set up on this thread.
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let uia = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
            Ok(Self { uia })
        }
    }

    /// Detect from whatever window is in front right now.
    pub fn detect(&self) -> Result<Found, Miss> {
        self.detect_from(Foreground::now(), true)
    }

    /// Detect from the window that was in front at an earlier moment, such as
    /// when a hotkey was pressed and before a panel of your own took focus.
    ///
    /// `allow_keys` permits the Ctrl+L / Ctrl+C fallback. Turn it off when
    /// your own window has focus, or the keys would go to it.
    pub fn detect_from(&self, front: Foreground, allow_keys: bool) -> Result<Found, Miss> {
        let started = Instant::now();
        let found = |link, method, app: Option<&str>| Found {
            link,
            method,
            app: app.map(str::to_string),
            elapsed: started.elapsed(),
        };

        let candidates = browser_windows(front.hwnd());
        let Some(front) = candidates.first() else {
            return clipboard_link()
                .map(|l| found(l, Method::Clipboard, None))
                .ok_or(Miss::NoBrowser);
        };

        // Menus, popups and bubbles are browser windows too but have no
        // address bar, so walk front to back until one does.
        let mut browser = front;
        let mut link = None;
        let mut method = Method::AddressBar;
        for candidate in &candidates {
            let raw = self.read_address_bar(candidate);
            debug(|| {
                format!(
                    "{} {:?}: address bar = {raw:?} ({} ms)",
                    candidate.exe,
                    candidate.hwnd.0,
                    started.elapsed().as_millis()
                )
            });
            if let Some(l) = raw.and_then(|v| links::parse(&v)) {
                browser = candidate;
                link = Some(l);
                break;
            }
        }
        if link.is_none() && front.foreground && allow_keys {
            link = copy_address_bar().and_then(|v| links::parse(&v));
            method = Method::CopyKeys;
        }
        let app = browser.exe.as_str();

        match link {
            Some(l) if l.kind == Kind::Feed => match self.scan_page(browser, &l) {
                Some((post, how)) => Ok(found(post, how, Some(app))),
                None => clipboard_link()
                    .filter(|c| c.platform == l.platform && c.kind == Kind::Post)
                    .map(|c| found(c, Method::Clipboard, Some(app)))
                    .ok_or(Miss::FeedWithoutPost {
                        feed: l,
                        app: app.to_string(),
                    }),
            },
            Some(l) => Ok(found(l, method, Some(app))),
            None => clipboard_link()
                .map(|c| found(c, Method::Clipboard, Some(app)))
                .ok_or(Miss::Unreadable {
                    app: app.to_string(),
                }),
        }
    }

    fn read_address_bar(&self, browser: &Browser) -> Option<String> {
        unsafe {
            let window = self.uia.ElementFromHandle(browser.hwnd).ok()?;
            let condition = match browser.engine {
                // Chromium names the omnibox's view class the same in every language.
                Engine::Chromium => self.uia.CreatePropertyCondition(
                    UIA_ClassNamePropertyId,
                    &VARIANT::from("OmniboxViewViews"),
                ),
                Engine::Firefox => self.uia.CreatePropertyCondition(
                    UIA_AutomationIdPropertyId,
                    &VARIANT::from("urlbar-input"),
                ),
            }
            .ok()?;
            let bar = window.FindFirst(TreeScope_Descendants, &condition).ok()?;
            let value: IUIAutomationValuePattern =
                bar.GetCurrentPatternAs(UIA_ValuePatternId).ok()?;
            let text = value.CurrentValue().ok()?.to_string();
            (!text.trim().is_empty()).then_some(text)
        }
    }

    /// On a feed page, find links to individual posts on the same platform and
    /// pick the one nearest the middle of the visible page.
    fn scan_page(&self, browser: &Browser, feed: &Link) -> Option<(Link, Method)> {
        // Chrome only builds its accessibility tree for web pages once a tool
        // like this asks for it, so the first look can come back empty.
        for attempt in 0..6 {
            if attempt > 0 {
                std::thread::sleep(Duration::from_millis(250));
            }
            match self.scan_page_once(browser, feed) {
                Scan::Found(link) => return Some((link, Method::PageScan)),
                Scan::NoPosts => break,
                Scan::NotReady => continue,
            }
        }
        // TikTok's For You feed has no link to the video anywhere on the page,
        // so use TikTok's own Share > Copy instead.
        if feed.platform == links::Platform::TikTok {
            return self
                .press_share_copy(browser, feed)
                .map(|l| (l, Method::ShareButton));
        }
        None
    }

    /// Open the Share menu of the post in view, press Copy, and read the
    /// clipboard. The previous clipboard text is put back afterwards.
    ///
    /// Matches the English labels "Share" and "Copy" for now.
    fn press_share_copy(&self, browser: &Browser, feed: &Link) -> Option<Link> {
        unsafe {
            let (page, view) = self.main_document(browser)?;
            let center_y = (view.top + view.bottom) / 2;

            let buttons = self
                .uia
                .CreatePropertyCondition(
                    UIA_ControlTypePropertyId,
                    &VARIANT::from(UIA_ButtonControlTypeId.0),
                )
                .ok()?;
            let cache = self.uia.CreateCacheRequest().ok()?;
            cache.AddProperty(UIA_NamePropertyId).ok()?;
            cache.AddProperty(UIA_BoundingRectanglePropertyId).ok()?;
            let all = page
                .FindAllBuildCache(TreeScope_Descendants, &buttons, &cache)
                .ok()?;
            let mut share: Option<(i32, IUIAutomationElement)> = None;
            for i in 0..all.Length().ok()? {
                let Ok(button) = all.GetElement(i) else {
                    continue;
                };
                let name = button
                    .CachedName()
                    .map(|n| n.to_string())
                    .unwrap_or_default();
                let Ok(r) = button.CachedBoundingRectangle() else {
                    continue;
                };
                if !name.to_ascii_lowercase().starts_with("share")
                    || r.bottom < view.top
                    || r.top > view.bottom
                {
                    continue;
                }
                let distance = ((r.top + r.bottom) / 2 - center_y).abs();
                if share.as_ref().is_none_or(|(d, _)| distance < *d) {
                    share = Some((distance, button));
                }
            }
            let (_, share) = share?;
            debug(|| "share assist: opening Share menu".into());

            let before = clipboard_text();
            let before_seq = GetClipboardSequenceNumber();
            let menu = share
                .GetCurrentPatternAs::<IUIAutomationExpandCollapsePattern>(
                    UIA_ExpandCollapsePatternId,
                )
                .ok();
            let expanded = menu.as_ref().is_some_and(|m| m.Expand().is_ok());
            if !expanded {
                // The button can report a stale "already open" state. Its inner
                // clickable part still opens the menu.
                let inner = self
                    .uia
                    .CreatePropertyCondition(
                        UIA_IsInvokePatternAvailablePropertyId,
                        &VARIANT::from(true),
                    )
                    .and_then(|c| share.FindFirst(TreeScope_Subtree, &c))
                    .and_then(|el| {
                        el.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId)
                    });
                if let Err(e) = inner.and_then(|click| click.Invoke()) {
                    debug(|| format!("share assist: couldn't open the Share menu ({e})"));
                    return None;
                }
            }

            let copy_label = self
                .uia
                .CreatePropertyCondition(UIA_NamePropertyId, &VARIANT::from("Copy"))
                .ok()?;
            let deadline = Instant::now() + Duration::from_millis(1500);
            let copy = loop {
                if let Ok(el) = page.FindFirst(TreeScope_Descendants, &copy_label) {
                    break Some(el);
                }
                if Instant::now() > deadline {
                    break None;
                }
                std::thread::sleep(Duration::from_millis(80));
            };
            let Some(copy) = copy.and_then(|label| self.clickable_ancestor(&label)) else {
                debug(|| "share assist: no Copy option appeared".into());
                if let Some(m) = &menu {
                    let _ = m.Collapse();
                }
                return None;
            };
            let click: IUIAutomationInvokePattern =
                copy.GetCurrentPatternAs(UIA_InvokePatternId).ok()?;
            click.Invoke().ok()?;

            let deadline = Instant::now() + Duration::from_millis(1500);
            while GetClipboardSequenceNumber() == before_seq && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(30));
            }
            let copied = clipboard_text();
            if let Some(text) = &before {
                set_clipboard_text(text);
            }
            debug(|| format!("share assist: copied {copied:?}"));
            copied
                .and_then(|t| links::parse(&t))
                .filter(|l| l.platform == feed.platform && l.kind == Kind::Post)
        }
    }

    /// Wait for a browser window showing a page whose address contains
    /// `address`, then press the button on it labelled exactly `button`.
    /// Gives up after `timeout`. Used to open a site's own "New message"
    /// box; it never types or sends anything.
    pub fn press_button_in_browser(&self, address: &str, button: &str, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            for browser in browser_windows(Foreground::now().hwnd()) {
                let on_page = self
                    .read_address_bar(&browser)
                    .is_some_and(|a| a.contains(address));
                if on_page && self.press_named_button(&browser, button) {
                    debug(|| format!("pressed {button:?} on {address}"));
                    return true;
                }
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        debug(|| format!("gave up looking for {button:?} on {address}"));
        false
    }

    /// Wait until the window in front is a browser on a page whose address
    /// contains `address` and the typing cursor is in one of its text boxes
    /// (not a search box), then paste `text` there once. Never presses Enter,
    /// so nothing is sent. Gives up after `timeout`.
    pub fn paste_into_page(&self, address: &str, text: &str, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if let Some(browser) = browser_from(Foreground::now().hwnd(), true)
                && self
                    .read_address_bar(&browser)
                    .is_some_and(|a| a.contains(address))
                && self.typing_in_page(&browser)
            {
                wait_for_modifiers_released();
                // Checked again: the person may have switched windows while
                // their keys were still down.
                if Foreground::now().hwnd() != browser.hwnd {
                    continue;
                }
                set_clipboard_text(text);
                press(&[VK_CONTROL, VK_V]);
                debug(|| format!("pasted into {address}"));
                return true;
            }
            std::thread::sleep(Duration::from_millis(300));
        }
        debug(|| format!("gave up waiting to paste on {address}"));
        false
    }

    /// The keyboard focus is in a text box that belongs to this browser
    /// window's page, and it isn't a search box.
    fn typing_in_page(&self, browser: &Browser) -> bool {
        unsafe {
            let Ok(focused) = self.uia.GetFocusedElement() else {
                return false;
            };
            let is_text_box = focused
                .CurrentControlType()
                .is_ok_and(|t| t == UIA_EditControlTypeId);
            let name = focused
                .CurrentName()
                .map(|n| n.to_string())
                .unwrap_or_default();
            let is_search = name.to_ascii_lowercase().contains("search");
            // The browser's own address bar is a text box too.
            let is_address_bar = focused
                .CurrentClassName()
                .is_ok_and(|c| c == "OmniboxViewViews");
            let mut pid = 0u32;
            GetWindowThreadProcessId(browser.hwnd, Some(&mut pid));
            let same_app = focused.CurrentProcessId().is_ok_and(|p| p as u32 == pid);
            is_text_box && !is_search && !is_address_bar && same_app
        }
    }

    fn press_named_button(&self, browser: &Browser, name: &str) -> bool {
        unsafe {
            let Some((page, _)) = self.main_document(browser) else {
                return false;
            };
            let found = self
                .uia
                .CreatePropertyCondition(UIA_NamePropertyId, &VARIANT::from(name))
                .and_then(|by_name| {
                    let by_type = self.uia.CreatePropertyCondition(
                        UIA_ControlTypePropertyId,
                        &VARIANT::from(UIA_ButtonControlTypeId.0),
                    )?;
                    let both = self.uia.CreateAndCondition(&by_name, &by_type)?;
                    page.FindFirst(TreeScope_Descendants, &both)
                })
                .and_then(|b| {
                    b.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId)
                });
            found.and_then(|click| click.Invoke()).is_ok()
        }
    }

    /// The label itself usually can't be clicked; its button a level or two up can.
    fn clickable_ancestor(&self, element: &IUIAutomationElement) -> Option<IUIAutomationElement> {
        unsafe {
            let walker = self.uia.ControlViewWalker().ok()?;
            let mut current = element.clone();
            for _ in 0..5 {
                if current
                    .GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId)
                    .is_ok()
                {
                    return Some(current);
                }
                current = walker.GetParentElement(&current).ok()?;
            }
            None
        }
    }

    fn scan_page_once(&self, browser: &Browser, feed: &Link) -> Scan {
        let Some((page, view)) = self.main_document(browser) else {
            debug(|| "page scan: no web document yet".into());
            return Scan::NotReady;
        };
        let Some(anchors) = self.links_in(&page) else {
            debug(|| "page scan: couldn't list links".into());
            return Scan::NotReady;
        };
        debug(|| {
            let posts = anchors
                .iter()
                .filter_map(|(href, _)| links::parse(href))
                .filter(|l| l.platform == feed.platform && l.kind == Kind::Post)
                .count();
            format!(
                "page scan: {} links, {posts} to {} posts, viewport {view:?}",
                anchors.len(),
                feed.platform.name()
            )
        });
        if std::env::var("PIGEON_DEBUG").as_deref() == Ok("2") {
            for (href, r) in &anchors {
                eprintln!(
                    "    {href}  [{},{} {}x{}]",
                    r.left,
                    r.top,
                    r.right - r.left,
                    r.bottom - r.top
                );
            }
        }
        if anchors.is_empty() {
            return Scan::NotReady;
        }

        // Where each post starts on screen: the highest of its links. Posts
        // above and below the screen are included, with their real positions.
        let mut posts: Vec<(i32, Link)> = Vec::new();
        for (href, rect) in anchors {
            let sized = rect.right > rect.left && rect.bottom > rect.top;
            let in_column = rect.right > view.left && rect.left < view.right;
            if !sized || !in_column {
                continue;
            }
            let Some(link) = links::parse(&href) else {
                continue;
            };
            if link.platform != feed.platform || link.kind != Kind::Post {
                continue;
            }
            match posts.iter_mut().find(|(_, l)| l.url == link.url) {
                Some((top, _)) => *top = (*top).min(rect.top),
                None => posts.push((rect.top, link)),
            }
        }
        match pick_post_in_view(&posts, view) {
            Some(link) => Scan::Found(link.clone()),
            None => Scan::NoPosts,
        }
    }

    /// The largest web document in the window, which is the page itself rather
    /// than a side panel or dev tools.
    fn main_document(&self, browser: &Browser) -> Option<(IUIAutomationElement, RECT)> {
        unsafe {
            let window = self.uia.ElementFromHandle(browser.hwnd).ok()?;
            let condition = self
                .uia
                .CreatePropertyCondition(
                    UIA_ControlTypePropertyId,
                    &VARIANT::from(UIA_DocumentControlTypeId.0),
                )
                .ok()?;
            let docs = window.FindAll(TreeScope_Descendants, &condition).ok()?;
            let mut best: Option<(i64, IUIAutomationElement, RECT)> = None;
            for i in 0..docs.Length().ok()? {
                let Ok(doc) = docs.GetElement(i) else {
                    continue;
                };
                let Ok(r) = doc.CurrentBoundingRectangle() else {
                    continue;
                };
                debug(|| format!("document {i}: {r:?}"));
                let area = (r.right - r.left) as i64 * (r.bottom - r.top) as i64;
                if best.as_ref().is_none_or(|(a, _, _)| area > *a) {
                    best = Some((area, doc, r));
                }
            }
            best.filter(|(area, _, _)| *area > 0)
                .map(|(_, d, r)| (d, r))
        }
    }

    /// Every hyperlink in the page with its target and position, fetched in
    /// one round trip to the browser.
    fn links_in(&self, page: &IUIAutomationElement) -> Option<Vec<(String, RECT)>> {
        unsafe {
            let condition = self
                .uia
                .CreatePropertyCondition(
                    UIA_ControlTypePropertyId,
                    &VARIANT::from(UIA_HyperlinkControlTypeId.0),
                )
                .ok()?;
            let cache = self.uia.CreateCacheRequest().ok()?;
            cache.AddProperty(UIA_ValueValuePropertyId).ok()?;
            cache.AddProperty(UIA_BoundingRectanglePropertyId).ok()?;
            let found = page
                .FindAllBuildCache(TreeScope_Descendants, &condition, &cache)
                .ok()?;
            let mut out = Vec::new();
            for i in 0..found.Length().ok()? {
                let Ok(el) = found.GetElement(i) else {
                    continue;
                };
                let Ok(value) = el.GetCachedPropertyValue(UIA_ValueValuePropertyId) else {
                    continue;
                };
                let Ok(href) = BSTR::try_from(&value) else {
                    continue;
                };
                let Ok(rect) = el.CachedBoundingRectangle() else {
                    continue;
                };
                if !href.is_empty() {
                    out.push((href.to_string(), rect));
                }
            }
            Some(out)
        }
    }
}

enum Scan {
    Found(Link),
    NoPosts,
    NotReady,
}

/// The post covering the middle of the screen is the last one that starts
/// above the middle. If every post starts lower down, take the first one
/// that starts on screen.
fn pick_post_in_view(posts: &[(i32, Link)], view: RECT) -> Option<&Link> {
    let middle = (view.top + view.bottom) / 2;
    let above = posts
        .iter()
        .filter(|(top, _)| *top <= middle)
        .max_by_key(|(top, _)| *top);
    let below = || {
        posts
            .iter()
            .filter(|(top, _)| *top > middle && *top < view.bottom)
            .min_by_key(|(top, _)| *top)
    };
    above.or_else(below).map(|(_, link)| link)
}

/// Set PIGEON_DEBUG=1 to see what detection reads, step by step.
fn debug(message: impl FnOnce() -> String) {
    if std::env::var_os("PIGEON_DEBUG").is_some() {
        eprintln!("  [debug] {}", message());
    }
}

/// How many browser windows to try before giving up.
const MAX_WINDOWS: usize = 6;

/// A window that was in front at some moment. Stored as a plain number so it
/// can be passed between threads; it's only ever used as an identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Foreground(isize);

impl Foreground {
    pub fn now() -> Self {
        Self(unsafe { GetForegroundWindow() }.0 as isize)
    }

    /// Give focus back to this window, e.g. after closing a panel. Windows
    /// only allows it while our own window is in front, so it can't be used
    /// to steal focus from other apps.
    pub fn restore(self) {
        if self.0 != 0 {
            unsafe {
                let _ = SetForegroundWindow(self.hwnd());
            }
        }
    }

    fn hwnd(self) -> HWND {
        HWND(self.0 as *mut _)
    }
}

/// Browser windows, most recently used first. The window that was in front
/// comes first if it's a browser.
fn browser_windows(front: HWND) -> Vec<Browser> {
    let mut found: Vec<Browser> = Vec::new();
    unsafe {
        found.extend(browser_from(front, true));

        // EnumWindows walks top-level windows from front to back.
        unsafe extern "system" fn visit(hwnd: HWND, data: LPARAM) -> BOOL {
            unsafe {
                let list = &mut *(data.0 as *mut Vec<Browser>);
                if list.len() >= MAX_WINDOWS {
                    return BOOL(0);
                }
                if IsWindowVisible(hwnd).as_bool()
                    && !IsIconic(hwnd).as_bool()
                    && GetWindowTextLengthW(hwnd) > 0
                    && list.iter().all(|b| b.hwnd != hwnd)
                    && let Some(b) = browser_from(hwnd, false)
                {
                    list.push(b);
                }
                BOOL(1)
            }
        }
        let _ = EnumWindows(Some(visit), LPARAM(&mut found as *mut _ as isize));
    }
    found
}

fn browser_from(hwnd: HWND, foreground: bool) -> Option<Browser> {
    if hwnd.is_invalid() {
        return None;
    }
    let exe = process_exe(hwnd)?;
    let (_, engine) = BROWSERS
        .iter()
        .find(|(name, _)| exe.eq_ignore_ascii_case(name))?;
    Some(Browser {
        hwnd,
        exe,
        engine: *engine,
        foreground,
    })
}

fn process_exe(hwnd: HWND) -> Option<String> {
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return None;
        }
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        );
        let _ = CloseHandle(process);
        ok.ok()?;
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        path.rsplit('\\').next().map(str::to_string)
    }
}

fn clipboard_link() -> Option<Link> {
    clipboard_text()
        .and_then(|t| links::parse(&t))
        .filter(|l| l.platform != links::Platform::Other)
}

/// Press Ctrl+L, Ctrl+C, Esc in the browser in front and read what got copied.
/// Puts the previous clipboard text back afterwards.
fn copy_address_bar() -> Option<String> {
    let before = clipboard_text();
    wait_for_modifiers_released();
    press(&[VK_CONTROL, VK_L]);
    press(&[VK_CONTROL, VK_C]);
    std::thread::sleep(Duration::from_millis(120));
    let copied = clipboard_text();
    press(&[VK_ESCAPE]);
    if let Some(text) = &before {
        set_clipboard_text(text);
    }
    copied.filter(|c| Some(c) != before.as_ref())
}

/// The hotkey's own Ctrl/Shift/Alt may still be held down, which would turn
/// our Ctrl+L into Ctrl+Shift+L.
fn wait_for_modifiers_released() {
    let held = |vk: VIRTUAL_KEY| unsafe { GetAsyncKeyState(vk.0 as i32) as u16 & 0x8000 != 0 };
    let deadline = Instant::now() + Duration::from_millis(1500);
    while [VK_SHIFT, VK_CONTROL, VK_MENU, VK_LWIN, VK_RWIN]
        .into_iter()
        .any(held)
    {
        if Instant::now() > deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(15));
    }
}

fn press(keys: &[VIRTUAL_KEY]) {
    let key = |vk: VIRTUAL_KEY, flags: KEYBD_EVENT_FLAGS| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let mut inputs: Vec<INPUT> = keys.iter().map(|&k| key(k, KEYBD_EVENT_FLAGS(0))).collect();
    inputs.extend(keys.iter().rev().map(|&k| key(k, KEYEVENTF_KEYUP)));
    unsafe {
        SendInput(&inputs, size_of::<INPUT>() as i32);
    }
    std::thread::sleep(Duration::from_millis(40));
}

/// Opening the clipboard fails while another app has it open, so retry briefly.
fn open_clipboard() -> bool {
    for _ in 0..10 {
        if unsafe { OpenClipboard(None) }.is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

pub fn clipboard_text() -> Option<String> {
    if !open_clipboard() {
        return None;
    }
    let text = unsafe {
        GetClipboardData(CF_UNICODETEXT.0 as u32)
            .ok()
            .and_then(|handle| {
                let mem = HGLOBAL(handle.0);
                let ptr = GlobalLock(mem) as *const u16;
                if ptr.is_null() {
                    return None;
                }
                let mut len = 0;
                while *ptr.add(len) != 0 {
                    len += 1;
                }
                let text = String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len));
                let _ = GlobalUnlock(mem);
                Some(text)
            })
    };
    unsafe {
        let _ = CloseClipboard();
    }
    text
}

pub fn set_clipboard_text(text: &str) -> bool {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    if !open_clipboard() {
        return false;
    }
    let ok = unsafe {
        (|| -> Option<()> {
            EmptyClipboard().ok()?;
            let mem = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2).ok()?;
            let ptr = GlobalLock(mem) as *mut u16;
            if ptr.is_null() {
                return None;
            }
            std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
            let _ = GlobalUnlock(mem);
            // The clipboard owns the memory from here on.
            SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(mem.0))).ok()?;
            Some(())
        })()
        .is_some()
    };
    unsafe {
        let _ = CloseClipboard();
    }
    ok
}

#[cfg(test)]
mod tests {
    use super::*;

    fn post(top: i32, code: &str) -> (i32, Link) {
        (
            top,
            links::parse(&format!("https://www.instagram.com/p/{code}/")).unwrap(),
        )
    }

    #[test]
    fn picks_the_post_covering_the_middle() {
        let view = RECT {
            left: 0,
            top: 100,
            right: 1000,
            bottom: 900,
        }; // middle = 500
        let code = |posts: &[(i32, Link)]| pick_post_in_view(posts, view).map(|l| l.url.clone());
        let url = |c: &str| Some(format!("https://www.instagram.com/p/{c}/"));

        // Header of A on screen, B further down.
        assert_eq!(code(&[post(150, "A"), post(1400, "B")]), url("A"));
        // Scrolled halfway through A: its header is above the screen.
        assert_eq!(code(&[post(-300, "A"), post(700, "B")]), url("A"));
        // B's header has passed the middle, so B fills it.
        assert_eq!(code(&[post(-900, "A"), post(300, "B")]), url("B"));
        // Nothing above the middle yet (top of the feed): first post on screen.
        assert_eq!(code(&[post(650, "A"), post(1800, "B")]), url("A"));
        assert_eq!(code(&[post(1800, "A")]), None);
    }
}
