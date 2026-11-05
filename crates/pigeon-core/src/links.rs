//! Recognise links to social posts and cut them down to a clean, shareable form.
//!
//! Everything here is plain string work with no OS calls, so it runs and is
//! tested on every platform.

use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Platform {
    YouTube,
    Instagram,
    TikTok,
    X,
    Facebook,
    Reddit,
    Threads,
    Snapchat,
    Other,
}

impl Platform {
    pub fn name(self) -> &'static str {
        match self {
            Platform::YouTube => "YouTube",
            Platform::Instagram => "Instagram",
            Platform::TikTok => "TikTok",
            Platform::X => "X",
            Platform::Facebook => "Facebook",
            Platform::Reddit => "Reddit",
            Platform::Threads => "Threads",
            Platform::Snapchat => "Snapchat",
            Platform::Other => "Web",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A single post, reel, short or video.
    Post,
    /// A scrolling feed whose URL doesn't say which post is on screen.
    Feed,
    /// Any other page (a profile, an article, a search...).
    Page,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub platform: Platform,
    pub kind: Kind,
    pub url: String,
}

/// Query parameters that only exist to track who shared what. Dropped everywhere.
const TRACKING_PARAMS: &[&str] = &[
    "igsh",
    "igshid",
    "si",
    "pp",
    "feature",
    "fbclid",
    "gclid",
    "mibextid",
    "rdid",
    "share_url",
    "is_from_webapp",
    "sender_device",
    "sender_web_id",
    "web_id",
    "_r",
    "_t",
    "ref",
    "ref_src",
    "ref_url",
    "share_id",
    "share_app_id",
    "tt_from",
    "u_code",
    "utm_source",
    "utm_medium",
    "utm_campaign",
    "utm_content",
    "utm_term",
    "utm_name",
];

/// Find a link in `text` and classify it.
///
/// `text` can be a full URL, an address-bar value without `https://`
/// (`youtube.com/shorts/abc`), or a sentence with a link in it
/// ("look at this https://...").
pub fn parse(text: &str) -> Option<Link> {
    let url = extract_url(text)?;
    let host = url.host_str()?.to_ascii_lowercase();
    let host = host
        .strip_prefix("www.")
        .or_else(|| host.strip_prefix("m."))
        .or_else(|| host.strip_prefix("mobile."))
        .or_else(|| host.strip_prefix("web."))
        .unwrap_or(&host)
        .to_string();
    let segs: Vec<&str> = url
        .path_segments()
        .map(|s| s.filter(|p| !p.is_empty()).collect())
        .unwrap_or_default();

    let link = match host.as_str() {
        "youtube.com" | "youtu.be" | "music.youtube.com" => youtube(&host, &segs, &url),
        "instagram.com" | "instagr.am" => instagram(&segs),
        "tiktok.com" | "vm.tiktok.com" | "vt.tiktok.com" => tiktok(&host, &segs),
        "x.com" | "twitter.com" => x(&segs),
        "facebook.com" | "fb.watch" | "fb.com" => facebook(&host, &segs, &url),
        "reddit.com" | "old.reddit.com" | "new.reddit.com" | "redd.it" => reddit(&host, &segs),
        "threads.net" | "threads.com" => threads(&host, &segs),
        "snapchat.com" => snapchat(&segs),
        _ => None,
    };
    Some(link.unwrap_or_else(|| generic(&url)))
}

fn extract_url(text: &str) -> Option<Url> {
    let text = text.trim();
    let candidate = match text.find("https://").or_else(|| text.find("http://")) {
        Some(start) => text[start..].split_whitespace().next()?,
        // Address bars often hide the scheme. Only accept a lone token that
        // looks like a domain, so a typed search like "cats" isn't a link.
        None if !text.contains(char::is_whitespace) && looks_like_domain(text) => text,
        None => return None,
    };
    let candidate = candidate.trim_end_matches([')', ']', '>', '"', '\'', ',', '.']);
    let with_scheme = if candidate.starts_with("http://") || candidate.starts_with("https://") {
        candidate.to_string()
    } else {
        format!("https://{candidate}")
    };
    let url = Url::parse(&with_scheme).ok()?;
    matches!(url.scheme(), "http" | "https").then_some(url)
}

fn looks_like_domain(text: &str) -> bool {
    let host = text.split(['/', '?', '#']).next().unwrap_or("");
    let host = host.split(':').next().unwrap_or("");
    let Some((_, tld)) = host.rsplit_once('.') else {
        return false;
    };
    tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic())
}

fn is_id(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn query(url: &Url, key: &str) -> Option<String> {
    url.query_pairs()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.into_owned())
}

fn post(platform: Platform, url: String) -> Option<Link> {
    Some(Link {
        platform,
        kind: Kind::Post,
        url,
    })
}

fn feed(platform: Platform, url: &str) -> Option<Link> {
    Some(Link {
        platform,
        kind: Kind::Feed,
        url: url.to_string(),
    })
}

fn youtube(host: &str, segs: &[&str], url: &Url) -> Option<Link> {
    let p = Platform::YouTube;
    let with_time = |base: String| match query(url, "t") {
        Some(t) => format!("{base}&t={t}"),
        None => base,
    };
    if host == "youtu.be" {
        let id = segs.first().filter(|s| is_id(s))?;
        return post(
            p,
            with_time(format!("https://www.youtube.com/watch?v={id}")),
        );
    }
    match segs {
        ["shorts", id, ..] if is_id(id) => post(p, format!("https://www.youtube.com/shorts/{id}")),
        ["live", id, ..] if is_id(id) => post(p, format!("https://www.youtube.com/live/{id}")),
        ["embed", id, ..] if is_id(id) => post(p, format!("https://www.youtube.com/watch?v={id}")),
        ["watch", ..] => {
            let id = query(url, "v").filter(|id| is_id(id))?;
            post(
                p,
                with_time(format!("https://www.youtube.com/watch?v={id}")),
            )
        }
        ["shorts"] => feed(p, "https://www.youtube.com/shorts"),
        _ => None,
    }
}

fn instagram(segs: &[&str]) -> Option<Link> {
    let p = Platform::Instagram;
    // Newer links put the username first: /<user>/reel/<code>/
    let without_user: Vec<&str>;
    let segs = match segs {
        [_, kind @ ("p" | "reel" | "reels" | "tv"), rest @ ..] if !rest.is_empty() => {
            without_user = std::iter::once(*kind).chain(rest.iter().copied()).collect();
            &without_user[..]
        }
        _ => segs,
    };
    match segs {
        ["p", code, ..] if is_id(code) => post(p, format!("https://www.instagram.com/p/{code}/")),
        ["reel" | "reels" | "tv", code, ..] if is_id(code) => {
            post(p, format!("https://www.instagram.com/reel/{code}/"))
        }
        ["stories", user, id, ..] if is_id(user) && is_id(id) => {
            post(p, format!("https://www.instagram.com/stories/{user}/{id}/"))
        }
        [] => feed(p, "https://www.instagram.com/"),
        ["explore"] => feed(p, "https://www.instagram.com/explore/"),
        ["reels"] => feed(p, "https://www.instagram.com/reels/"),
        _ => None,
    }
}

fn tiktok(host: &str, segs: &[&str]) -> Option<Link> {
    let p = Platform::TikTok;
    if host != "tiktok.com" {
        // vm.tiktok.com/<code>/ share links. They redirect, so keep them whole.
        let code = segs.first().filter(|s| is_id(s))?;
        return post(p, format!("https://{host}/{code}/"));
    }
    match segs {
        [user, kind @ ("video" | "photo"), id, ..] if user.starts_with('@') && is_id(id) => {
            post(p, format!("https://www.tiktok.com/{user}/{kind}/{id}"))
        }
        ["t", code, ..] if is_id(code) => post(p, format!("https://www.tiktok.com/t/{code}/")),
        [] | ["foryou"] | ["following"] | ["friends"] | ["explore"] => {
            feed(p, "https://www.tiktok.com/foryou")
        }
        _ => None,
    }
}

fn x(segs: &[&str]) -> Option<Link> {
    let p = Platform::X;
    match segs {
        [user, "status", id, ..] if is_id(user) && id.chars().all(|c| c.is_ascii_digit()) => {
            post(p, format!("https://x.com/{user}/status/{id}"))
        }
        ["i", "web", "status", id, ..] if id.chars().all(|c| c.is_ascii_digit()) => {
            post(p, format!("https://x.com/i/status/{id}"))
        }
        [] | ["home"] => feed(p, "https://x.com/home"),
        _ => None,
    }
}

fn facebook(host: &str, segs: &[&str], url: &Url) -> Option<Link> {
    let p = Platform::Facebook;
    if host == "fb.watch" {
        let code = segs.first().filter(|s| is_id(s))?;
        return post(p, format!("https://fb.watch/{code}/"));
    }
    match segs {
        ["reel", id, ..] if is_id(id) => post(p, format!("https://www.facebook.com/reel/{id}")),
        ["share", kind, code, ..] if is_id(kind) && is_id(code) => {
            post(p, format!("https://www.facebook.com/share/{kind}/{code}/"))
        }
        ["watch", ..] if query(url, "v").is_some() => {
            let v = query(url, "v")?;
            post(p, format!("https://www.facebook.com/watch/?v={v}"))
        }
        [page, kind @ ("posts" | "videos"), id, ..] if is_id(id) => {
            post(p, format!("https://www.facebook.com/{page}/{kind}/{id}"))
        }
        ["permalink.php" | "story.php"] => {
            let story = query(url, "story_fbid")?;
            let id = query(url, "id")?;
            post(
                p,
                format!("https://www.facebook.com/permalink.php?story_fbid={story}&id={id}"),
            )
        }
        ["photo" | "photo.php"] => {
            let fbid = query(url, "fbid")?;
            post(p, format!("https://www.facebook.com/photo/?fbid={fbid}"))
        }
        [] | ["watch"] | ["reel"] => feed(p, "https://www.facebook.com/"),
        _ => None,
    }
}

fn reddit(host: &str, segs: &[&str]) -> Option<Link> {
    let p = Platform::Reddit;
    if host == "redd.it" {
        let id = segs.first().filter(|s| is_id(s))?;
        return post(p, format!("https://redd.it/{id}"));
    }
    match segs {
        ["r", sub, "comments", id, slug, ..] if is_id(id) => post(
            p,
            format!("https://www.reddit.com/r/{sub}/comments/{id}/{slug}/"),
        ),
        ["r", sub, "comments", id] if is_id(id) => {
            post(p, format!("https://www.reddit.com/r/{sub}/comments/{id}/"))
        }
        ["r", sub, "s", code, ..] if is_id(code) => {
            post(p, format!("https://www.reddit.com/r/{sub}/s/{code}"))
        }
        [] | ["best" | "hot" | "new" | "top" | "rising"] => feed(p, "https://www.reddit.com/"),
        ["r", sub] | ["r", sub, "hot" | "new" | "top" | "rising"] => Some(Link {
            platform: p,
            kind: Kind::Feed,
            url: format!("https://www.reddit.com/r/{sub}/"),
        }),
        _ => None,
    }
}

fn threads(host: &str, segs: &[&str]) -> Option<Link> {
    let p = Platform::Threads;
    match segs {
        [user, "post", code, ..] if user.starts_with('@') && is_id(code) => {
            post(p, format!("https://www.{host}/{user}/post/{code}"))
        }
        [] => Some(Link {
            platform: p,
            kind: Kind::Feed,
            url: format!("https://www.{host}/"),
        }),
        _ => None,
    }
}

fn snapchat(segs: &[&str]) -> Option<Link> {
    let i = segs.iter().position(|s| *s == "spotlight")?;
    let id = segs.get(i + 1).filter(|s| is_id(s))?;
    post(
        Platform::Snapchat,
        format!("https://www.snapchat.com/spotlight/{id}"),
    )
}

/// Unknown site, or a known site on a page we don't special-case: keep the
/// URL but drop tracking parameters and the #fragment.
fn generic(url: &Url) -> Link {
    let mut clean = url.clone();
    clean.set_fragment(None);
    let kept: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| !TRACKING_PARAMS.contains(&k.as_ref()) && !k.starts_with("utm_"))
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    if kept.is_empty() {
        clean.set_query(None);
    } else {
        clean.query_pairs_mut().clear().extend_pairs(kept);
    }
    let platform = parse_platform_only(url).unwrap_or(Platform::Other);
    Link {
        platform,
        kind: Kind::Page,
        url: clean.to_string(),
    }
}

fn parse_platform_only(url: &Url) -> Option<Platform> {
    let host = url.host_str()?.to_ascii_lowercase();
    let has = |d: &str| host == d || host.ends_with(&format!(".{d}"));
    Some(if has("youtube.com") || has("youtu.be") {
        Platform::YouTube
    } else if has("instagram.com") {
        Platform::Instagram
    } else if has("tiktok.com") {
        Platform::TikTok
    } else if has("x.com") || has("twitter.com") {
        Platform::X
    } else if has("facebook.com") || has("fb.watch") {
        Platform::Facebook
    } else if has("reddit.com") || has("redd.it") {
        Platform::Reddit
    } else if has("threads.net") || has("threads.com") {
        Platform::Threads
    } else if has("snapchat.com") {
        Platform::Snapchat
    } else {
        return None;
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(input: &str, platform: Platform, kind: Kind, url: &str) {
        let link = parse(input).unwrap_or_else(|| panic!("no link found in {input:?}"));
        assert_eq!(
            link,
            Link {
                platform,
                kind,
                url: url.into()
            },
            "input: {input}"
        );
    }

    #[test]
    fn youtube() {
        use Platform::YouTube as Y;
        check(
            "https://www.youtube.com/shorts/dQw4w9WgXcQ?si=abc123",
            Y,
            Kind::Post,
            "https://www.youtube.com/shorts/dQw4w9WgXcQ",
        );
        check(
            "youtube.com/shorts/dQw4w9WgXcQ",
            Y,
            Kind::Post,
            "https://www.youtube.com/shorts/dQw4w9WgXcQ",
        );
        check(
            "https://youtu.be/dQw4w9WgXcQ?si=xyz&t=42",
            Y,
            Kind::Post,
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=42",
        );
        check(
            "https://m.youtube.com/watch?v=dQw4w9WgXcQ&pp=ygU&feature=share",
            Y,
            Kind::Post,
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
        );
        check(
            "https://www.youtube.com/",
            Y,
            Kind::Page,
            "https://www.youtube.com/",
        );
    }

    #[test]
    fn instagram() {
        use Platform::Instagram as I;
        check(
            "https://www.instagram.com/reel/C8xYz12AbCd/?igsh=MWQ1ZGUxMzBkMA==",
            I,
            Kind::Post,
            "https://www.instagram.com/reel/C8xYz12AbCd/",
        );
        check(
            "https://www.instagram.com/reels/C8xYz12AbCd/",
            I,
            Kind::Post,
            "https://www.instagram.com/reel/C8xYz12AbCd/",
        );
        check(
            "instagram.com/p/C8xYz12AbCd/?img_index=2",
            I,
            Kind::Post,
            "https://www.instagram.com/p/C8xYz12AbCd/",
        );
        check(
            "https://www.instagram.com/someuser/reel/C8xYz12AbCd/",
            I,
            Kind::Post,
            "https://www.instagram.com/reel/C8xYz12AbCd/",
        );
        check(
            "https://www.instagram.com/",
            I,
            Kind::Feed,
            "https://www.instagram.com/",
        );
        check(
            "instagram.com/explore/",
            I,
            Kind::Feed,
            "https://www.instagram.com/explore/",
        );
        check(
            "https://www.instagram.com/natgeo/",
            I,
            Kind::Page,
            "https://www.instagram.com/natgeo/",
        );
    }

    #[test]
    fn tiktok() {
        use Platform::TikTok as T;
        check(
            "https://www.tiktok.com/@cooluser/video/7301234567890123456?is_from_webapp=1&sender_device=pc",
            T,
            Kind::Post,
            "https://www.tiktok.com/@cooluser/video/7301234567890123456",
        );
        check(
            "https://vm.tiktok.com/ZMabc123/",
            T,
            Kind::Post,
            "https://vm.tiktok.com/ZMabc123/",
        );
        check(
            "tiktok.com/foryou",
            T,
            Kind::Feed,
            "https://www.tiktok.com/foryou",
        );
        check(
            "https://www.tiktok.com/",
            T,
            Kind::Feed,
            "https://www.tiktok.com/foryou",
        );
    }

    #[test]
    fn x_and_others() {
        check(
            "https://twitter.com/jack/status/20?s=20&t=abc",
            Platform::X,
            Kind::Post,
            "https://x.com/jack/status/20",
        );
        check("x.com/home", Platform::X, Kind::Feed, "https://x.com/home");
        check(
            "https://www.facebook.com/reel/1234567890?mibextid=xyz",
            Platform::Facebook,
            Kind::Post,
            "https://www.facebook.com/reel/1234567890",
        );
        check(
            "https://old.reddit.com/r/rust/comments/abc123/some_title/?utm_source=share",
            Platform::Reddit,
            Kind::Post,
            "https://www.reddit.com/r/rust/comments/abc123/some_title/",
        );
        check(
            "https://www.threads.net/@zuck/post/C1a2b3",
            Platform::Threads,
            Kind::Post,
            "https://www.threads.net/@zuck/post/C1a2b3",
        );
    }

    #[test]
    fn generic_and_text() {
        check(
            "Look at this!! https://example.com/a?utm_source=x&id=7#top",
            Platform::Other,
            Kind::Page,
            "https://example.com/a?id=7",
        );
        check(
            "(https://www.youtube.com/shorts/dQw4w9WgXcQ)",
            Platform::YouTube,
            Kind::Post,
            "https://www.youtube.com/shorts/dQw4w9WgXcQ",
        );
        assert_eq!(parse("funny cat videos"), None);
        assert_eq!(parse("cats"), None);
        assert_eq!(parse("chrome://settings"), None);
        assert_eq!(parse(""), None);
    }
}
