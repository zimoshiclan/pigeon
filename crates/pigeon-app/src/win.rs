//! The Windows-only pieces: where to put the panel, the frosted background,
//! opening links, and checking which apps are installed.

use base64::Engine;
use windows::Win32::Foundation::{POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleBitmap, CreateCompatibleDC,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, GetMonitorInfoW, HALFTONE,
    MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint, ReleaseDC, SRCCOPY, SelectObject,
    SetStretchBltMode, StretchBlt,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Shell::{
    ASSOCF_IS_PROTOCOL, ASSOCSTR_FRIENDLYAPPNAME, AssocQueryStringW, ShellExecuteW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, MB_ICONINFORMATION, MB_OK, MB_SETFOREGROUND, MB_TOPMOST, MessageBoxW,
    SW_SHOWNORMAL,
};
use windows::core::{HSTRING, PCWSTR, PWSTR, w};

use crate::send::{self, InstalledApps};

/// The panel's size in logical pixels (scaled up on high-DPI screens).
pub const PANEL_SIZE: f64 = 480.0;

/// Where the panel goes, in physical screen pixels.
#[derive(Debug, Clone, Copy)]
pub struct Placement {
    pub x: i32,
    pub y: i32,
    pub size: u32,
}

/// Centre the panel on the screen the mouse is on.
pub fn placement() -> Placement {
    unsafe {
        let mut cursor = POINT::default();
        let _ = GetCursorPos(&mut cursor);
        let monitor = MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let work = if GetMonitorInfoW(monitor, &mut info).as_bool() {
            info.rcWork
        } else {
            RECT {
                left: 0,
                top: 0,
                right: 1280,
                bottom: 720,
            }
        };
        let (mut dpi_x, mut dpi_y) = (96u32, 96u32);
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
        let size = (PANEL_SIZE * dpi_x.max(96) as f64 / 96.0).round() as u32;
        Placement {
            x: (work.left + work.right) / 2 - size as i32 / 2,
            y: (work.top + work.bottom) / 2 - size as i32 / 2,
            size,
        }
    }
}

/// A small, blurry-anyway copy of the screen behind the panel, as an image
/// the page can use for its frosted-glass background. It stays in memory and
/// is replaced every time the panel opens. It's never saved or sent.
///
/// It covers a quarter of the panel's size more on every side, so the blur
/// has real pixels to work with right up to the panel's edge. The page
/// positions it to match (see `.glass` in style.css).
pub fn backdrop(at: Placement) -> Option<String> {
    const SHRINK: u32 = 4;
    let margin = (at.size / 4) as i32;
    let (src_x, src_y, src_size) = (at.x - margin, at.y - margin, at.size as i32 + 2 * margin);
    let side = (src_size / SHRINK as i32).max(1);
    let (w, h) = (side, side);
    unsafe {
        let screen = GetDC(None);
        if screen.is_invalid() {
            return None;
        }
        let memory = CreateCompatibleDC(Some(screen));
        let bitmap = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(memory, bitmap.into());
        SetStretchBltMode(memory, HALFTONE);
        let copied = StretchBlt(
            memory,
            0,
            0,
            w,
            h,
            Some(screen),
            src_x,
            src_y,
            src_size,
            src_size,
            SRCCOPY,
        )
        .as_bool();

        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // top-down rows
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut pixels = vec![0u8; (w * h * 4) as usize];
        let rows = GetDIBits(
            memory,
            bitmap,
            0,
            h as u32,
            Some(pixels.as_mut_ptr().cast()),
            &mut info,
            DIB_RGB_COLORS,
        );

        SelectObject(memory, old);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(memory);
        ReleaseDC(None, screen);

        if !copied || rows == 0 {
            return None;
        }
        Some(format!(
            "data:image/bmp;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bmp(w, h, &mut pixels))
        ))
    }
}

/// Wrap 32-bit top-down BGRA pixels in a .bmp file.
fn bmp(w: i32, h: i32, pixels: &mut [u8]) -> Vec<u8> {
    // GDI leaves the alpha byte at 0; make every pixel opaque.
    for px in pixels.as_chunks_mut::<4>().0 {
        px[3] = 255;
    }
    let header_len = 14 + 40;
    let mut out = Vec::with_capacity(header_len + pixels.len());
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&((header_len + pixels.len()) as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(header_len as u32).to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&w.to_le_bytes());
    out.extend_from_slice(&(-h).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&[0u8; 24]);
    out.extend_from_slice(pixels);
    out
}

/// Open a link built by [`send::plan`] with whatever app handles it. Anything
/// not on the allowlist is refused, so this can never run a program or open
/// a file.
pub fn open_url(url: &str) -> Result<(), String> {
    if !send::is_allowed(url) {
        return Err("Pigeon refused to open that link.".into());
    }
    let result = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &HSTRING::from(url),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    // ShellExecute reports success as a value above 32.
    if result.0 as usize > 32 {
        Ok(())
    } else {
        Err("Couldn't open the app.".into())
    }
}

/// Read-only check of which link types have an app registered.
pub fn installed_apps() -> InstalledApps {
    InstalledApps {
        whatsapp: has_protocol("whatsapp"),
        telegram: has_protocol("tg"),
        discord: has_protocol("discord"),
    }
}

fn has_protocol(scheme: &str) -> bool {
    let mut buf = [0u16; 260];
    let mut len = buf.len() as u32;
    unsafe {
        AssocQueryStringW(
            ASSOCF_IS_PROTOCOL,
            ASSOCSTR_FRIENDLYAPPNAME,
            &HSTRING::from(scheme),
            PCWSTR::null(),
            Some(PWSTR(buf.as_mut_ptr())),
            &mut len,
        )
        .is_ok()
    }
}

pub fn message(text: &str) {
    unsafe {
        // On top of everything, so it can't end up hidden behind other windows.
        MessageBoxW(
            None,
            &HSTRING::from(text),
            w!("Pigeon"),
            MB_OK | MB_ICONINFORMATION | MB_TOPMOST | MB_SETFOREGROUND,
        );
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn bmp_header_is_well_formed() {
        let mut px = vec![1u8; 2 * 3 * 4];
        let file = super::bmp(2, 3, &mut px);
        assert_eq!(&file[..2], b"BM");
        assert_eq!(
            u32::from_le_bytes(file[2..6].try_into().unwrap()) as usize,
            file.len()
        );
        assert_eq!(i32::from_le_bytes(file[22..26].try_into().unwrap()), -3);
        assert!(file[54..].chunks(4).all(|p| p[3] == 255));
    }
}
