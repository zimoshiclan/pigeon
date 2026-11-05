//! Pigeon proof of concept.
//!
//! Press the hotkey while watching a reel, short, TikTok or post in your
//! browser, and the clean link to it is printed here.

use std::time::{Duration, Instant};

use pigeon_core::{Kind, Link};

const HELP: &str = "\
pigeon-cli: Pigeon's link finder on the command line (for testing and debugging)

USAGE:
  pigeon-cli                       Listen for the hotkey (default Ctrl+Shift+Space)
  pigeon-cli detect [--delay N]    Detect once, optionally after N seconds
  pigeon-cli parse <link or text>  Show how a link gets cleaned up
  pigeon-cli press <page> <label>  Press a button on an open page, e.g.
                                  pigeon-cli press instagram.com/direct \"New message\"

OPTIONS:
  --hotkey <keys>   Hotkey to listen for, e.g. ctrl+alt+f or ctrl+shift+f9
  --copy            Also put the clean link on the clipboard
  -h, --help        Show this help
";

struct Options {
    hotkey: String,
    copy: bool,
    delay: f64,
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        print!("{HELP}");
        return;
    }
    if args.first().map(String::as_str) == Some("parse") {
        let text = args[1..].join(" ");
        match pigeon_core::parse(&text) {
            Some(link) => print_link(&link),
            None => println!("No link found in {text:?}"),
        }
        return;
    }
    #[cfg(windows)]
    if args.first().map(String::as_str) == Some("paste") {
        // pigeon-cli paste instagram.com/direct/t/ "https://..."
        let (Some(address), Some(text)) = (args.get(1), args.get(2)) else {
            fail("Usage: pigeon-cli paste <part of page address> <text>");
        };
        let detector = pigeon_core::detect::Detector::new()
            .unwrap_or_else(|e| fail(&format!("UI Automation isn't available: {e}")));
        let pasted = detector.paste_into_page(address, text, Duration::from_secs(15));
        println!("{}", if pasted { "Pasted." } else { "Nothing pasted." });
        std::process::exit(if pasted { 0 } else { 1 });
    }
    #[cfg(windows)]
    if args.first().map(String::as_str) == Some("press") {
        // pigeon-cli press instagram.com/direct "New message"
        let (Some(address), Some(button)) = (args.get(1), args.get(2)) else {
            fail("Usage: pigeon-cli press <part of page address> <button label>");
        };
        let detector = pigeon_core::detect::Detector::new()
            .unwrap_or_else(|e| fail(&format!("UI Automation isn't available: {e}")));
        let pressed = detector.press_button_in_browser(address, button, Duration::from_secs(8));
        println!(
            "{}",
            if pressed {
                "Pressed."
            } else {
                "Button not found."
            }
        );
        std::process::exit(if pressed { 0 } else { 1 });
    }

    let mut opts = Options {
        hotkey: "ctrl+shift+space".into(),
        copy: false,
        delay: 0.0,
    };
    let once = args.first().map(String::as_str) == Some("detect");
    if once {
        args.remove(0);
    }
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--copy" => opts.copy = true,
            "--hotkey" => {
                opts.hotkey = it
                    .next()
                    .unwrap_or_else(|| fail("--hotkey needs keys, e.g. ctrl+alt+f"))
            }
            "--delay" => {
                opts.delay = it
                    .next()
                    .and_then(|n| n.parse().ok())
                    .unwrap_or_else(|| fail("--delay needs a number of seconds"))
            }
            other => fail(&format!(
                "Unknown option {other:?}. Run `pigeon-cli --help`."
            )),
        }
    }

    #[cfg(windows)]
    {
        if once {
            win::detect_once(&opts);
        } else {
            win::listen(&opts);
        }
    }
    #[cfg(not(windows))]
    fail("Detection only runs on Windows for now. `pigeon-cli parse` works everywhere.");
}

fn fail(message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(2);
}

fn print_link(link: &Link) {
    let kind = match link.kind {
        Kind::Post => "post",
        Kind::Feed => "feed (no single post)",
        Kind::Page => "page",
    };
    println!("  Platform : {}", link.platform.name());
    println!("  Type     : {kind}");
    println!("  Link     : {}", link.url);
}

/// "ctrl+shift+space" -> "Ctrl+Shift+Space"
fn pretty_keys(keys: &str) -> String {
    keys.split('+')
        .map(|k| {
            let k = k.trim();
            let mut chars = k.chars();
            match chars.next() {
                Some(first) => {
                    first.to_ascii_uppercase().to_string() + &chars.as_str().to_ascii_lowercase()
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join("+")
}

#[cfg(windows)]
mod win {
    use super::*;
    use pigeon_core::detect::{self, Detector, Miss};
    use windows::Win32::System::SystemInformation::GetLocalTime;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, RegisterHotKey,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetMessageW, MSG, WM_HOTKEY};

    fn timestamp() -> String {
        let t = unsafe { GetLocalTime() };
        format!("{:02}:{:02}:{:02}", t.wHour, t.wMinute, t.wSecond)
    }

    pub fn detect_once(opts: &Options) {
        let detector = new_detector();
        if opts.delay > 0.0 {
            println!("Detecting in {} s. Switch to your browser now.", opts.delay);
            std::thread::sleep(Duration::from_secs_f64(opts.delay));
        }
        let ok = run(&detector, opts);
        std::process::exit(if ok { 0 } else { 1 });
    }

    pub fn listen(opts: &Options) {
        let detector = new_detector();
        let (mods, vk) = parse_hotkey(&opts.hotkey).unwrap_or_else(|| {
            fail(&format!(
                "Couldn't understand hotkey {:?}. Try ctrl+shift+space.",
                opts.hotkey
            ))
        });
        let keys = pretty_keys(&opts.hotkey);
        unsafe {
            if RegisterHotKey(None, 1, mods | MOD_NOREPEAT, vk).is_err() {
                fail(&format!(
                    "{keys} is already taken by another app. Pick another one, e.g. `pigeon-cli --hotkey ctrl+alt+f`."
                ));
            }
        }
        println!("Pigeon is listening. Press {keys} on any reel, short, TikTok or post.");
        println!("Close this window or press Ctrl+C to stop.\n");

        let mut msg = MSG::default();
        while unsafe { GetMessageW(&mut msg, None, 0, 0) }.0 > 0 {
            if msg.message == WM_HOTKEY {
                println!("[{}] {keys}", timestamp());
                run(&detector, opts);
            }
        }
    }

    fn new_detector() -> Detector {
        Detector::new()
            .unwrap_or_else(|e| fail(&format!("Windows UI Automation isn't available: {e}")))
    }

    fn run(detector: &Detector, opts: &Options) -> bool {
        let started = Instant::now();
        match detector.detect() {
            Ok(found) => {
                print_link(&found.link);
                let source = found
                    .app
                    .as_deref()
                    .map(|a| format!(" in {a}"))
                    .unwrap_or_default();
                println!(
                    "  Found via: {}{source}, {} ms",
                    found.method.describe(),
                    found.elapsed.as_millis()
                );
                if opts.copy && detect::set_clipboard_text(&found.link.url) {
                    println!("  Copied to clipboard.");
                }
                println!();
                true
            }
            Err(miss) => {
                match miss {
                    Miss::NoBrowser => {
                        println!("  No browser window found, and no link on the clipboard.")
                    }
                    Miss::Unreadable { app } => {
                        println!(
                            "  Found {app} but couldn't read its address bar. Copy the link and try again."
                        )
                    }
                    Miss::FeedWithoutPost { feed, app } => println!(
                        "  You're on the {} feed in {app}, but no single post could be picked out.\n  \
                         Open the post, or tap its \"Copy link\" and press the hotkey again.",
                        feed.platform.name()
                    ),
                }
                println!("  ({} ms)\n", started.elapsed().as_millis());
                false
            }
        }
    }

    fn parse_hotkey(text: &str) -> Option<(HOT_KEY_MODIFIERS, u32)> {
        let mut mods = HOT_KEY_MODIFIERS(0);
        let mut key = None;
        for part in text.to_ascii_lowercase().split('+').map(str::trim) {
            match part {
                "ctrl" | "control" => mods |= MOD_CONTROL,
                "shift" => mods |= MOD_SHIFT,
                "alt" => mods |= MOD_ALT,
                "win" | "super" => mods |= MOD_WIN,
                "space" => key = Some(0x20),
                k if k.len() == 1 && k.chars().all(|c| c.is_ascii_alphanumeric()) => {
                    key = Some(k.to_ascii_uppercase().as_bytes()[0] as u32)
                }
                k if k.starts_with('f') => {
                    let n: u32 = k[1..].parse().ok().filter(|n| (1..=24).contains(n))?;
                    key = Some(0x70 + n - 1)
                }
                _ => return None,
            }
        }
        // A bare letter as a global hotkey would swallow normal typing.
        (mods.0 != 0).then_some(())?;
        key.map(|k| (mods, k))
    }
}
