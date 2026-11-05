# Pigeon

Send any reel, short, TikTok or post to anyone on any app, straight from your PC.

Press **Ctrl + Shift + Space** while you're watching something in your browser. A round glass panel opens with the post already picked out. Choose WhatsApp, Telegram, Instagram or another app, choose a person, and Pigeon opens that chat with the link ready to go.

> **Status: early preview (v0.2).** Works on Windows 10 and 11.

## Try it

1. Download `Pigeon.exe` from the [Releases](../../releases) page. It's a single file with nothing to install.
2. Double-click it. A welcome panel shows for a few seconds, then Pigeon waits quietly with a small paper-plane icon near the clock.
3. Open a reel, short or post in your browser and press **Ctrl + Shift + Space**.
4. Pick an app, then a person.

Windows may show a SmartScreen warning the first time because the app isn't code-signed yet. Click **More info**, then **Run anyway**.

### In the panel

| Do this | To |
|---|---|
| Click an app, or press its number (1 to 8) | Choose where to send it |
| Click a person, or press their number | Open the chat with them |
| **+** | Save someone new for that app |
| Right-click a person | Remove them |
| **Esc** or **Backspace** | Go back, or close the panel |
| Click outside the circle | Close the panel |

To quit Pigeon, right-click its icon near the clock and choose **Quit Pigeon**.

### Start with Windows

Right-click the icon near the clock and tick **Start with Windows**. Pigeon then starts quietly each time you log in (no welcome panel), so the hotkey always works. Untick it to stop.

This puts a normal shortcut called `Pigeon` in your Startup folder (press Win+R and type `shell:startup` to see it). It also shows in Task Manager under Startup apps.

### What happens when you send

Pigeon opens the right chat with the link filled in, and **also copies the link**, so it's never lost. You press Enter in the app to actually send it. Pigeon never sends a message by itself.

| App | What Pigeon does |
|---|---|
| WhatsApp | Opens the chat with the person (by phone number) with the link typed in |
| Telegram | Opens the chat with the person (by username) with the link typed in |
| Instagram | Opens the person's profile so you can tap Message; once the chat box is open, Pigeon pastes the link into it |
| Messenger | Opens the chat with the person; paste the copied link with Ctrl+V |
| Discord | Opens your DMs; paste the copied link |
| Email | Opens a new email to the person with the link in it |
| X | Opens a new post with the link, for you to check and publish |
| Copy link | Copies the clean link |

For WhatsApp, Telegram, Instagram and Email you can also pick **Choose in WhatsApp** (and so on) to choose the chat inside the app instead of a saved person. On an Instagram post or reel, this opens the post's own Share box, so you can send it to several people separately.

## What it understands

| Where you are | How Pigeon finds the post |
|---|---|
| YouTube Shorts, any YouTube video | Address bar |
| Instagram Reels tab, opened posts and reels | Address bar |
| **Instagram home feed** | Finds the post in the middle of your screen |
| TikTok video pages | Address bar |
| **TikTok For You feed** | Presses TikTok's own Share > Copy for the video in view |
| X posts, Facebook reels and posts, Reddit, Threads, Snapchat Spotlight | Address bar |
| X timeline, Reddit front page | Finds the post in the middle of your screen |
| Any other website | Shares the page itself |

It strips tracking junk such as `?igsh=`, `?si=`, `utm_*` and `is_from_webapp`, so you share `instagram.com/reel/ABC/`, not a link that reports who shared it.

Works with Chrome, Edge, Brave, Vivaldi, Opera, Arc and Firefox-based browsers. If the browser isn't the window in front, Pigeon uses the one you used most recently.

## Privacy and safety

- **Nothing leaves your PC.** There is no server, no account and no analytics.
- **Pigeon only looks when you press the hotkey.** It never watches in the background.
- **It never sends a message for you.** It opens the chat; you press Enter.
- **It can only open a fixed set of link types** (web links, email, and the WhatsApp, Telegram and Discord apps). It can't run programs or open files, even if something goes wrong in the panel.
- **The glass effect** uses a small, blurred snapshot of the screen behind the panel. It's kept in memory only and thrown away each time; it's never saved or sent.
- **Saved people** are kept in `%APPDATA%\app.pigeon.desktop\contacts.json` on your PC. Delete that file to forget everyone.
- No admin rights, no registry changes, no installer. Starting with Windows is off until you turn it on.

## How it works

No browser extension is needed. Pigeon uses **Windows UI Automation**, the accessibility API that screen readers use to see what's on screen.

1. **Address bar.** Pigeon reads the URL from the browser's address bar. This takes about 30 to 60 ms.
2. **Page scan.** If the URL is a feed (like `instagram.com/`), Pigeon lists the links on the page with their positions and picks the post covering the middle of your screen.
3. **Share button.** TikTok's For You feed has no link to the video anywhere on the page, so Pigeon opens the video's Share menu, presses Copy, reads the link and puts your old clipboard text back.
4. **Clipboard.** As a last resort, a supported link you've already copied.

The panel is a [Tauri](https://tauri.app) window drawn with WebView2, which is built into Windows 10 and 11. It's plain HTML, CSS and JavaScript with no npm packages.

## Known limits

- Windows only for now. macOS is planned.
- The hotkey is fixed to Ctrl+Shift+Space for now.
- The TikTok share-button step matches the English labels "Share" and "Copy". Other languages fall back to asking you to copy the link.
- Messenger and Discord don't let other apps fill in a message, so for those you paste the link yourself.
- Instagram's paste step and the "Share" and "New message" buttons match English labels.
- Up to 6 saved people show per app.

## Building from source

You need [Rust](https://rustup.rs) and the Visual Studio C++ Build Tools. No Node.js is needed.

```
git clone https://github.com/zimoshiclan/pigeon
cd pigeon
cargo build --release
```

This builds two programs in `target/release`:

- `Pigeon.exe`: the app.
- `pigeon-cli.exe`: the link finder on its own, for testing (`pigeon-cli --help`).

Run the tests with `cargo test`.

### Debugging

Set `PIGEON_DEBUG=1` to log what the app and the link finder are doing, or `PIGEON_DEBUG=2` to also list every link found on the page:

```
set PIGEON_DEBUG=2
pigeon-cli detect --delay 3
```

Please include this output when reporting a site that doesn't work.

To work on the panel's design without running the app, serve `crates/pigeon-app/ui` with any static web server and open `index.html`. It runs in a demo mode with sample data. Add `?mode=welcome` or `?link=missing` to see the other screens.

## Project layout

```
crates/
  pigeon-core/   Finding and cleaning links
    links.rs      Recognises and cleans links (works on every OS, unit tested)
    detect.rs     Finds the post on screen (Windows UI Automation)
  pigeon-app/    The app
    src/main.rs   Hotkey, tray icon, and the commands the panel can call
    src/send.rs   Builds the link to open for each app (unit tested)
    src/contacts.rs  Saved people and their validation (unit tested)
    src/win.rs    Panel placement, glass snapshot, opening links
    src/autostart.rs  The optional Start with Windows shortcut
    ui/           The panel: index.html, style.css, app.js, icons.js
  pigeon-cli/    pigeon-cli, the command-line link finder
```

### Adding a site

Add a function for it in `crates/pigeon-core/src/links.rs` that returns `Kind::Post` for a single post and `Kind::Feed` for pages where the URL doesn't say which post you're on. Add a test with a few real links at the bottom of the same file. If the site has a feed, the page scan picks it up automatically as long as each post has a link to itself somewhere on the page.

## Roadmap

- [x] Find the post you're watching in any browser
- [x] The round glass share panel
- [x] Saved people per app, with WhatsApp, Telegram and email links filled in
- [x] Start with Windows (optional)
- [ ] Choose your own hotkey
- [ ] Fully automatic sending where apps allow it (Telegram, Discord, email)
- [ ] Appear in the Windows share menu
- [ ] macOS

## License

MIT. See [LICENSE](LICENSE).
