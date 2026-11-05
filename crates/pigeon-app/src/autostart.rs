//! "Start with Windows", done the visible way: a normal shortcut to Pigeon in
//! the user's own Startup folder. No registry, no admin rights, and it shows
//! up in Task Manager > Startup apps where it can be turned off too.

use std::path::PathBuf;

use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoTaskMemFree, IPersistFile,
};
use windows::Win32::UI::Shell::{
    FOLDERID_Startup, IShellLinkW, KF_FLAG_DEFAULT, SHGetKnownFolderPath, ShellLink,
};
use windows::core::{HSTRING, Interface};

/// Passed by the Startup shortcut, so Pigeon starts quietly without the
/// welcome panel.
pub const FLAG: &str = "--autostart";

fn shortcut_path() -> Option<PathBuf> {
    unsafe {
        let folder = SHGetKnownFolderPath(&FOLDERID_Startup, KF_FLAG_DEFAULT, None).ok()?;
        let path = folder.to_string().ok();
        CoTaskMemFree(Some(folder.0 as *const _));
        Some(PathBuf::from(path?).join("Pigeon.lnk"))
    }
}

pub fn is_enabled() -> bool {
    shortcut_path().is_some_and(|p| p.exists())
}

/// Create (or update) the shortcut so it points at this copy of Pigeon.
pub fn enable() -> Result<(), String> {
    let path = shortcut_path().ok_or("Couldn't find your Startup folder.")?;
    let exe = std::env::current_exe().map_err(|_| "Couldn't find Pigeon's own file.")?;
    let dir = exe.parent().ok_or("Couldn't find Pigeon's folder.")?;
    unsafe {
        // Fails harmlessly if COM is already set up on this thread.
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
            .map_err(|e| format!("Couldn't create the shortcut ({e})."))?;
        let saved = (|| -> windows::core::Result<()> {
            link.SetPath(&HSTRING::from(exe.as_os_str()))?;
            link.SetArguments(&HSTRING::from(FLAG))?;
            link.SetWorkingDirectory(&HSTRING::from(dir.as_os_str()))?;
            link.SetDescription(&HSTRING::from("Pigeon: press Ctrl+Shift+Space on any post"))?;
            link.cast::<IPersistFile>()?
                .Save(&HSTRING::from(path.as_os_str()), true)
        })();
        saved.map_err(|e| format!("Couldn't save the shortcut ({e})."))
    }
}

pub fn disable() -> Result<(), String> {
    match shortcut_path() {
        Some(p) if p.exists() => {
            std::fs::remove_file(p).map_err(|_| "Couldn't remove the shortcut.".into())
        }
        _ => Ok(()),
    }
}

/// If Pigeon was moved since the shortcut was made, point it at the new place.
/// Also swaps a shortcut from when Pigeon was called Fling for a new one.
pub fn refresh() {
    let old = shortcut_path().map(|p| p.with_file_name("Fling.lnk"));
    if let Some(old) = old.filter(|p| p.exists()) {
        if std::fs::remove_file(&old).is_ok() {
            let _ = enable();
        }
        return;
    }
    if is_enabled() {
        let _ = enable();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Touches the real Startup folder, so it only runs when asked:
    /// `cargo test -p pigeon-app -- --ignored`
    #[test]
    #[ignore]
    fn turns_on_and_off() {
        if is_enabled() {
            eprintln!("Start with Windows is already on here; leaving it alone.");
            return;
        }
        enable().unwrap();
        assert!(is_enabled());
        disable().unwrap();
        assert!(!is_enabled());
    }
}
