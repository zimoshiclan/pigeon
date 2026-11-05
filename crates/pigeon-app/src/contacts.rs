//! Saved people for each app, kept in a small JSON file in the user's own
//! app-data folder. Nothing here is sent anywhere.
//!
//! Every name and handle is checked against strict rules before it's saved
//! and again when the file is loaded, because those values end up inside the
//! links built in [`crate::send`].

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::send::Target;

pub const MAX_CONTACTS: usize = 60;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contact {
    pub id: String,
    /// One of the [`Target`] ids, e.g. "whatsapp".
    pub target: String,
    pub name: String,
    pub handle: String,
}

/// Check and tidy a name and handle for `target`. Returns them cleaned up,
/// or a message to show the person.
pub fn validate(target: Target, name: &str, handle: &str) -> Result<(String, String), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Add a name.".into());
    }
    if name.chars().count() > 40 || name.chars().any(char::is_control) {
        return Err("Keep the name under 40 characters.".into());
    }
    let handle = handle.trim();
    let handle = match target {
        Target::WhatsApp => {
            let digits: String = handle
                .chars()
                .filter(|c| !matches!(c, ' ' | '-' | '(' | ')' | '+' | '.'))
                .collect();
            if !(7..=15).contains(&digits.len()) || !digits.chars().all(|c| c.is_ascii_digit()) {
                return Err("Use the full number with country code, like 973 3312 3456.".into());
            }
            digits
        }
        Target::Telegram => {
            let user = handle.trim_start_matches('@');
            let ok = (5..=32).contains(&user.len())
                && user.starts_with(|c: char| c.is_ascii_alphabetic())
                && user.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
            if !ok {
                return Err("Use their Telegram username, like @sam_k.".into());
            }
            user.to_string()
        }
        Target::Instagram => {
            let user = strip_profile_link(handle, &["instagram.com/"]).trim_start_matches('@');
            let ok = (1..=30).contains(&user.len())
                && dots_ok(user)
                && user
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.');
            if !ok {
                return Err("Use their Instagram username, like sam.k.".into());
            }
            user.to_string()
        }
        Target::Messenger => {
            let user = strip_profile_link(handle, &["facebook.com/", "messenger.com/t/", "m.me/"]);
            let ok = (1..=50).contains(&user.len())
                && dots_ok(user)
                && user.chars().all(|c| c.is_ascii_alphanumeric() || c == '.');
            if !ok {
                return Err("Use their Facebook username, like sam.khan.".into());
            }
            user.to_string()
        }
        Target::Email => {
            let ok = handle.len() <= 254
                && handle.matches('@').count() == 1
                && !handle.starts_with('@')
                && handle
                    .rsplit('@')
                    .next()
                    .is_some_and(|d| d.contains('.') && !d.ends_with('.'))
                && handle
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '@' | '.' | '-' | '_' | '+'));
            if !ok {
                return Err("That doesn't look like an email address.".into());
            }
            handle.to_ascii_lowercase()
        }
        Target::Discord | Target::X | Target::Copy => {
            return Err("This app doesn't use saved contacts.".into());
        }
    };
    Ok((name.to_string(), handle))
}

/// Accept a pasted profile link ("https://www.instagram.com/sam.k/?igsh=...")
/// as well as a bare username, and return just the username part.
fn strip_profile_link<'a>(text: &'a str, prefixes: &[&str]) -> &'a str {
    let text = text
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("www.");
    let text = prefixes
        .iter()
        .find_map(|p| text.strip_prefix(p))
        .unwrap_or(text);
    text.split(['/', '?', '#']).next().unwrap_or("")
}

/// Usernames go into a web address, so no "." or ".." on their own, and no
/// dots at the start or end.
fn dots_ok(user: &str) -> bool {
    !user.starts_with('.') && !user.ends_with('.') && !user.contains("..")
}

/// Pigeon used to be called Fling and kept its files in a folder named after
/// that. If there's no contacts file yet, copy the old one across once. The
/// copy is loaded through [`load`], so it's checked like any other.
pub fn bring_over_from_old_name(dir: &Path, path: &Path) {
    if path.exists() {
        return;
    }
    let Some(old) = dir.parent().map(|p| path_in(&p.join("app.fling.desktop"))) else {
        return;
    };
    if old.exists() && std::fs::create_dir_all(dir).is_ok() {
        let _ = std::fs::copy(&old, path);
    }
}

pub fn path_in(dir: &Path) -> PathBuf {
    dir.join("contacts.json")
}

/// Load saved contacts. A missing or damaged file gives an empty list, and
/// entries that fail the checks are skipped.
pub fn load(path: &Path) -> Vec<Contact> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(list) = serde_json::from_str::<Vec<Contact>>(&text) else {
        return Vec::new();
    };
    list.into_iter()
        .filter_map(|c| {
            let target = Target::from_id(&c.target)?;
            let (name, handle) = validate(target, &c.name, &c.handle).ok()?;
            let id_ok = !c.id.is_empty()
                && c.id.len() <= 32
                && c.id.chars().all(|ch| ch.is_ascii_alphanumeric());
            id_ok.then_some(Contact { name, handle, ..c })
        })
        .take(MAX_CONTACTS)
        .collect()
}

/// Write to a temporary file first, then swap it in, so a crash can't leave
/// a half-written file.
pub fn save(path: &Path, contacts: &[Contact]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(contacts).map_err(std::io::Error::other)?;
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

pub fn new_id() -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{:x}{:x}", nanos, COUNTER.fetch_add(1, Ordering::Relaxed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_valid_handles() {
        assert_eq!(
            validate(Target::WhatsApp, " Sam ", "+973 3312-3456").unwrap(),
            ("Sam".into(), "97333123456".into())
        );
        assert_eq!(
            validate(Target::Telegram, "Sam", "@sam_k1").unwrap().1,
            "sam_k1"
        );
        assert_eq!(
            validate(Target::Instagram, "Sam", "@sam.k_").unwrap().1,
            "sam.k_"
        );
        assert_eq!(
            validate(
                Target::Instagram,
                "Sam",
                "https://www.instagram.com/sam.k/?igsh=abc"
            )
            .unwrap()
            .1,
            "sam.k"
        );
        assert_eq!(
            validate(Target::Messenger, "Sam", "facebook.com/sam.khan")
                .unwrap()
                .1,
            "sam.khan"
        );
        assert_eq!(
            validate(Target::Email, "Sam", "Sam@Example.com").unwrap().1,
            "sam@example.com"
        );
    }

    #[test]
    fn rejects_anything_that_could_change_a_link() {
        let bad = [
            (Target::WhatsApp, "123"),
            (Target::WhatsApp, "973331234&text=hi"),
            (Target::Telegram, "sam&text=evil"),
            (Target::Telegram, "ab"),
            (Target::Instagram, ".."),
            (Target::Instagram, ".sam"),
            (Target::Instagram, "sam..k"),
            (Target::Messenger, "."),
            (Target::Email, "sam@example.com?bcc=all@corp.com"),
            (Target::Email, "a@b"),
            (Target::Email, "two@@example.com"),
            (Target::Email, "sp ace@example.com"),
            (Target::X, "anyone"),
        ];
        for (target, handle) in bad {
            assert!(
                validate(target, "Sam", handle).is_err(),
                "{target:?} {handle:?} should be rejected"
            );
        }
        // Anything after the username is dropped, never kept in the link.
        assert_eq!(
            validate(Target::Instagram, "Sam", "sam/../../evil")
                .unwrap()
                .1,
            "sam"
        );
        assert_eq!(
            validate(Target::Instagram, "Sam", "sam?x=1").unwrap().1,
            "sam"
        );
        assert_eq!(
            validate(Target::Messenger, "Sam", "sam/evil?x=1")
                .unwrap()
                .1,
            "sam"
        );
        assert!(validate(Target::WhatsApp, "", "97333123456").is_err());
        assert!(validate(Target::WhatsApp, "a\u{7}b", "97333123456").is_err());
    }

    #[test]
    fn brings_contacts_over_from_the_old_name_once() {
        let base = std::env::temp_dir().join(format!("pigeon-test-{}", new_id()));
        let old_dir = base.join("app.fling.desktop");
        let new_dir = base.join("app.pigeon.desktop");
        let sam = Contact {
            id: "a1".into(),
            target: "telegram".into(),
            name: "Sam".into(),
            handle: "sam_k1".into(),
        };
        save(&path_in(&old_dir), std::slice::from_ref(&sam)).unwrap();

        bring_over_from_old_name(&new_dir, &path_in(&new_dir));
        assert_eq!(load(&path_in(&new_dir)), vec![sam]);

        // Never overwrites a newer list.
        save(&path_in(&new_dir), &[]).unwrap();
        bring_over_from_old_name(&new_dir, &path_in(&new_dir));
        assert!(load(&path_in(&new_dir)).is_empty());
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn load_skips_tampered_entries() {
        let dir = std::env::temp_dir().join(format!("pigeon-test-{}", new_id()));
        let path = path_in(&dir);
        let good = Contact {
            id: "a1".into(),
            target: "telegram".into(),
            name: "Sam".into(),
            handle: "sam_k1".into(),
        };
        let tampered = Contact {
            id: "b2".into(),
            target: "telegram".into(),
            name: "Eve".into(),
            handle: "x&text=evil".into(),
        };
        let unknown = Contact {
            id: "c3".into(),
            target: "calc".into(),
            name: "Eve".into(),
            handle: "x".into(),
        };
        save(&path, &[good.clone(), tampered, unknown]).unwrap();
        assert_eq!(load(&path), vec![good]);
        std::fs::write(&path, "not json").unwrap();
        assert!(load(&path).is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
