//! Turn "send this link to this person on this app" into what to do: which
//! URL to open and whether to copy the link.
//!
//! Every URL is built here from fixed templates. The panel only says which
//! app and which saved contact, never a URL, so a bug in the page can't make
//! Pigeon open anything else. [`is_allowed`] double-checks before opening.

use crate::contacts::Contact;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    WhatsApp,
    Telegram,
    Instagram,
    Messenger,
    Discord,
    Email,
    X,
    Copy,
}

impl Target {
    pub const ALL: [Target; 8] = [
        Target::WhatsApp,
        Target::Telegram,
        Target::Instagram,
        Target::Messenger,
        Target::Discord,
        Target::Email,
        Target::X,
        Target::Copy,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Target::WhatsApp => "whatsapp",
            Target::Telegram => "telegram",
            Target::Instagram => "instagram",
            Target::Messenger => "messenger",
            Target::Discord => "discord",
            Target::Email => "email",
            Target::X => "x",
            Target::Copy => "copy",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.id() == id)
    }

    /// Whether people can be saved for this app. The others go straight to
    /// a general screen (Discord DMs, posting on X, copying).
    pub fn has_contacts(self) -> bool {
        matches!(
            self,
            Target::WhatsApp
                | Target::Telegram
                | Target::Instagram
                | Target::Messenger
                | Target::Email
        )
    }
}

/// Which apps have their own link type registered on this PC, so we can
/// open the app directly instead of going through the browser.
#[derive(Debug, Clone, Copy, Default)]
pub struct InstalledApps {
    pub whatsapp: bool,
    pub telegram: bool,
    pub discord: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Action {
    pub open: Option<String>,
    pub copy: bool,
    pub hint: &'static str,
    /// After opening, press this button on the page: (part of the page's
    /// address, the button's exact label). Only ever opens a site's own
    /// "New message" or "Share" box; nothing is typed or sent.
    pub then_press: Option<(String, &'static str)>,
    /// When a page whose address contains this opens in front with the
    /// cursor in its message box, paste the link there. Never sends.
    pub then_paste: Option<&'static str>,
}

/// Where an Instagram chat lives once it's open.
const INSTAGRAM_CHAT: &str = "instagram.com/direct/t/";

/// The address of an Instagram post or reel as it shows in the browser
/// ("instagram.com/reel/ABC"), if `link` is one.
fn instagram_post(link: &str) -> Option<String> {
    let parsed = pigeon_core::parse(link)?;
    let is_post = parsed.platform == pigeon_core::Platform::Instagram
        && parsed.kind == pigeon_core::Kind::Post
        && (parsed.url.contains("/p/") || parsed.url.contains("/reel/"));
    is_post.then(|| {
        parsed
            .url
            .trim_start_matches("https://www.")
            .trim_end_matches('/')
            .to_string()
    })
}

/// Only these kinds of URL are ever opened.
const ALLOWED_PREFIXES: &[&str] = &[
    "https://",
    "mailto:",
    "whatsapp://send?",
    "tg://resolve?",
    "tg://msg_url?",
    "discord://-/channels/@me",
];

pub fn is_allowed(url: &str) -> bool {
    ALLOWED_PREFIXES.iter().any(|p| url.starts_with(p))
        && !url
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '"')
}

/// Percent-encode everything except unreserved characters (RFC 3986), so the
/// link can't break out of the parameter it's placed in.
pub fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 3);
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `contact` must already be validated for `target` (see [`crate::contacts`]).
pub fn plan(target: Target, contact: Option<&Contact>, link: &str, apps: InstalledApps) -> Action {
    let text = encode(link);
    let handle = contact.map(|c| c.handle.as_str());
    // Every option also copies the link, so it's never lost if an app
    // ignores the pre-filled text.
    let action = |open: String, hint| Action {
        open: Some(open),
        copy: true,
        hint,
        then_press: None,
        then_paste: None,
    };

    match (target, handle) {
        (Target::WhatsApp, Some(phone)) => action(
            if apps.whatsapp {
                format!("whatsapp://send?phone={phone}&text={text}")
            } else {
                format!("https://wa.me/{phone}?text={text}")
            },
            "Opening WhatsApp. Press Enter to send.",
        ),
        (Target::WhatsApp, None) => action(
            if apps.whatsapp {
                format!("whatsapp://send?text={text}")
            } else {
                format!("https://wa.me/?text={text}")
            },
            "Pick a chat in WhatsApp, then press Enter.",
        ),
        (Target::Telegram, Some(user)) => action(
            if apps.telegram {
                format!("tg://resolve?domain={user}&text={text}")
            } else {
                format!("https://t.me/{user}?text={text}")
            },
            "Opening Telegram. Press Enter to send.",
        ),
        (Target::Telegram, None) => action(
            if apps.telegram {
                format!("tg://msg_url?url={text}")
            } else {
                format!("https://t.me/share/url?url={text}")
            },
            "Pick a chat in Telegram to send it.",
        ),
        // Instagram has no "open a DM with this username" link that works on
        // a computer (ig.me only works on phones), so open their profile.
        (Target::Instagram, Some(user)) => Action {
            then_paste: Some(INSTAGRAM_CHAT),
            ..action(
                format!("https://www.instagram.com/{user}/"),
                "Click Message. Pigeon puts the link in the chat for you.",
            )
        },
        // Sharing an Instagram post: open it and press its own Share button.
        // That box sends to each person you pick in their own chat, where
        // "New message" would put several people in one group chat.
        (Target::Instagram, None) if instagram_post(link).is_some() => Action {
            then_press: instagram_post(link).map(|address| (address, "Share")),
            ..action(
                link.to_string(),
                "Pick everyone you want to send it to, then press Send.",
            )
        },
        // Anything else: instagram.com has no address for its "New message"
        // box, so open the inbox and press its New message button.
        (Target::Instagram, None) => Action {
            then_press: Some(("instagram.com/direct".into(), "New message")),
            then_paste: Some(INSTAGRAM_CHAT),
            ..action(
                "https://www.instagram.com/direct/inbox/".into(),
                "Pick one person (several makes a group). Pigeon adds the link.",
            )
        },
        // The full address rather than the m.me short link, which some
        // networks can't reach.
        (Target::Messenger, Some(user)) => action(
            format!("https://www.facebook.com/messages/t/{user}"),
            "Link copied. Paste it in the chat with Ctrl+V.",
        ),
        (Target::Messenger, None) => action(
            "https://www.facebook.com/messages/".into(),
            "Link copied. Open a chat and paste it with Ctrl+V.",
        ),
        (Target::Discord, _) => action(
            if apps.discord {
                "discord://-/channels/@me".into()
            } else {
                "https://discord.com/channels/@me".into()
            },
            "Link copied. Open a chat and paste it with Ctrl+V.",
        ),
        (Target::Email, to) => action(
            format!(
                "mailto:{}?subject={}&body={text}",
                to.unwrap_or(""),
                encode("Check this out")
            ),
            "Opening your email app.",
        ),
        (Target::X, _) => action(
            format!("https://x.com/intent/post?url={text}"),
            "Opening X. Check the post before you publish it.",
        ),
        (Target::Copy, _) => Action {
            open: None,
            copy: true,
            hint: "Link copied.",
            then_press: None,
            then_paste: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINK: &str = "https://www.instagram.com/reel/C8xYz12AbCd/";
    const ENCODED: &str = "https%3A%2F%2Fwww.instagram.com%2Freel%2FC8xYz12AbCd%2F";

    fn contact(target: Target, handle: &str) -> Contact {
        Contact {
            id: "1".into(),
            target: target.id().into(),
            name: "Sam".into(),
            handle: handle.into(),
        }
    }

    fn url(target: Target, handle: Option<&str>, apps: InstalledApps) -> String {
        let c = handle.map(|h| contact(target, h));
        plan(target, c.as_ref(), LINK, apps)
            .open
            .unwrap_or_default()
    }

    #[test]
    fn builds_app_links() {
        let apps = InstalledApps {
            whatsapp: true,
            telegram: true,
            discord: true,
        };
        assert_eq!(
            url(Target::WhatsApp, Some("97333123456"), apps),
            format!("whatsapp://send?phone=97333123456&text={ENCODED}")
        );
        assert_eq!(
            url(Target::Telegram, Some("sam_k"), apps),
            format!("tg://resolve?domain=sam_k&text={ENCODED}")
        );
        assert_eq!(
            url(Target::Telegram, None, apps),
            format!("tg://msg_url?url={ENCODED}")
        );
        assert_eq!(url(Target::Discord, None, apps), "discord://-/channels/@me");
    }

    #[test]
    fn falls_back_to_web_links() {
        let none = InstalledApps::default();
        assert_eq!(
            url(Target::WhatsApp, Some("97333123456"), none),
            format!("https://wa.me/97333123456?text={ENCODED}")
        );
        assert_eq!(
            url(Target::WhatsApp, None, none),
            format!("https://wa.me/?text={ENCODED}")
        );
        assert_eq!(
            url(Target::Telegram, None, none),
            format!("https://t.me/share/url?url={ENCODED}")
        );
        assert_eq!(
            url(Target::Instagram, Some("sam.k"), none),
            "https://www.instagram.com/sam.k/"
        );
        assert_eq!(
            url(Target::Messenger, Some("sam.khan"), none),
            "https://www.facebook.com/messages/t/sam.khan"
        );
        assert_eq!(
            url(Target::Email, Some("sam@example.com"), none),
            format!("mailto:sam@example.com?subject=Check%20this%20out&body={ENCODED}")
        );
        assert_eq!(
            url(Target::X, None, none),
            format!("https://x.com/intent/post?url={ENCODED}")
        );
    }

    #[test]
    fn instagram_picker_uses_share_for_posts_and_new_message_otherwise() {
        let none = InstalledApps::default();

        // An Instagram reel: open it and press its own Share button.
        let post = plan(Target::Instagram, None, LINK, none);
        assert_eq!(post.open.as_deref(), Some(LINK));
        assert_eq!(
            post.then_press,
            Some(("instagram.com/reel/C8xYz12AbCd".into(), "Share"))
        );

        // Anything else: the inbox and its New message button.
        let video = "https://www.youtube.com/shorts/dQw4w9WgXcQ";
        let other = plan(Target::Instagram, None, video, none);
        assert_eq!(
            other.open.as_deref(),
            Some("https://www.instagram.com/direct/inbox/")
        );
        assert_eq!(
            other.then_press,
            Some(("instagram.com/direct".into(), "New message"))
        );

        // Nothing else ever presses buttons.
        for target in Target::ALL {
            for handle in [None, Some("abc")] {
                if target == Target::Instagram && handle.is_none() {
                    continue;
                }
                let c = handle.map(|h| contact(target, h));
                assert_eq!(
                    plan(target, c.as_ref(), LINK, none).then_press,
                    None,
                    "{target:?}"
                );
            }
        }
    }

    #[test]
    fn only_instagram_chats_get_the_link_pasted() {
        let none = InstalledApps::default();
        let video = "https://www.youtube.com/shorts/dQw4w9WgXcQ";
        let saved = contact(Target::Instagram, "sam.k");
        assert_eq!(
            plan(Target::Instagram, Some(&saved), video, none).then_paste,
            Some(INSTAGRAM_CHAT)
        );
        assert_eq!(
            plan(Target::Instagram, None, video, none).then_paste,
            Some(INSTAGRAM_CHAT)
        );
        // Instagram's own Share box sends the post itself.
        assert_eq!(plan(Target::Instagram, None, LINK, none).then_paste, None);
        for target in Target::ALL.into_iter().filter(|t| *t != Target::Instagram) {
            for handle in [None, Some("abc")] {
                let c = handle.map(|h| contact(target, h));
                assert_eq!(
                    plan(target, c.as_ref(), video, none).then_paste,
                    None,
                    "{target:?}"
                );
            }
        }
    }

    #[test]
    fn every_built_link_is_allowed() {
        for apps in [
            InstalledApps::default(),
            InstalledApps {
                whatsapp: true,
                telegram: true,
                discord: true,
            },
        ] {
            for target in Target::ALL {
                for handle in [None, Some("abc")] {
                    let c = handle.map(|h| contact(target, h));
                    let action = plan(target, c.as_ref(), LINK, apps);
                    assert!(action.copy, "{target:?} should copy the link");
                    if let Some(open) = action.open {
                        assert!(is_allowed(&open), "{open} should be allowed");
                    }
                }
            }
        }
    }

    #[test]
    fn link_text_cannot_escape_its_parameter() {
        let tricky = "https://evil.example/?a=1&phone=999 \"x\"#frag";
        let action = plan(Target::WhatsApp, None, tricky, InstalledApps::default());
        let open = action.open.unwrap();
        assert!(open.starts_with("https://wa.me/?text="));
        assert_eq!(open.matches('&').count(), 0);
        assert_eq!(open.matches('?').count(), 1);
        assert!(is_allowed(&open));
    }

    #[test]
    fn rejects_other_urls() {
        for bad in [
            "file:///C:/Windows/System32/calc.exe",
            "C:\\Windows\\notepad.exe",
            "javascript:alert(1)",
            "ms-settings:privacy",
            "http://example.com",
            "whatsapp://anything-else",
            "https://example.com/a b",
            "https://example.com/\"onload",
        ] {
            assert!(!is_allowed(bad), "{bad} should be rejected");
        }
    }

    #[test]
    fn every_target_round_trips() {
        for t in Target::ALL {
            assert_eq!(Target::from_id(t.id()), Some(t));
        }
        assert_eq!(Target::from_id("calc"), None);
    }
}
