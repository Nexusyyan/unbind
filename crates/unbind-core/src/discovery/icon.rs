use crate::discovery::win32::{expand_env_vars, extract_executable_from_command};
use crate::models::InstalledApp;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Thread-safe in-memory icon cache mapping app IDs (or icon specs) to PNG data URIs.
#[derive(Default)]
pub struct IconCache {
    cache: Mutex<HashMap<String, Option<String>>>,
}

impl IconCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_or_extract(&self, app: &InstalledApp) -> Option<String> {
        if let Ok(guard) = self.cache.lock() {
            if let Some(hit) = guard.get(&app.id) {
                return hit.clone();
            }
        }

        let extracted = extract_icon_for_app(app);

        if let Ok(mut guard) = self.cache.lock() {
            guard.insert(app.id.clone(), extracted.clone());
        }
        extracted
    }

    pub fn get_or_extract_for_path(&self, key: &str, exe_path: &Path) -> Option<String> {
        if let Ok(guard) = self.cache.lock() {
            if let Some(hit) = guard.get(key) {
                return hit.clone();
            }
        }

        let extracted = extract_from_path(exe_path, 0);

        if let Ok(mut guard) = self.cache.lock() {
            guard.insert(key.to_string(), extracted.clone());
        }
        extracted
    }

    pub fn clear(&self) {
        if let Ok(mut guard) = self.cache.lock() {
            guard.clear();
        }
    }
}

pub fn extract_icon_for_app(app: &InstalledApp) -> Option<String> {
    // 1. Try explicit DisplayIcon first.
    if !app.display_icon.trim().is_empty() {
        let (path, index) = parse_icon_spec(&app.display_icon);
        if let Some(uri) = extract_from_path(&path, index) {
            return Some(uri);
        }
    }

    // 2. Try executable from UninstallString (if not msiexec).
    if let Some(exe) = extract_executable_from_command(&app.uninstall_string) {
        let lower = exe.to_string_lossy().to_lowercase();
        if !lower.contains("msiexec") && exe.exists() {
            if let Some(uri) = extract_from_path(&exe, 0) {
                return Some(uri);
            }
        }
    }

    // 3. Try finding a primary .exe inside InstallLocation.
    if !app.install_location.trim().is_empty() {
        let dir = Path::new(&app.install_location);
        if dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("exe")).unwrap_or(false) {
                        let fname = p.file_name().and_then(|f| f.to_str()).unwrap_or("").to_lowercase();
                        if !fname.contains("unins") && !fname.contains("crash") {
                            if let Some(uri) = extract_from_path(&p, 0) {
                                return Some(uri);
                            }
                        }
                    }
                }
            }
        }
    }

    None
}

/// Parse `"C:\Path\app.exe",0` or `C:\Path\app.exe,-101` into `(PathBuf, i32)`.
pub fn parse_icon_spec(spec: &str) -> (PathBuf, i32) {
    let trimmed = spec.trim();
    if let Some((left, right)) = trimmed.rsplit_once(',') {
        if let Ok(idx) = right.trim().parse::<i32>() {
            let clean = expand_env_vars(left.trim().trim_matches('"'));
            return (PathBuf::from(clean), idx);
        }
    }
    let clean = expand_env_vars(trimmed.trim_matches('"'));
    (PathBuf::from(clean), 0)
}

fn extract_from_path(path: &Path, icon_index: i32) -> Option<String> {
    if !path.exists() {
        return None;
    }

    // Fast path for raster image files (UWP / MSIX logos).
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        let lower = ext.to_lowercase();
        if lower == "png" {
            let bytes = std::fs::read(path).ok()?;
            if bytes.len() <= 512 * 1024 {
                return Some(format!("data:image/png;base64,{}", BASE64.encode(bytes)));
            }
        } else if lower == "jpg" || lower == "jpeg" {
            let bytes = std::fs::read(path).ok()?;
            if bytes.len() <= 512 * 1024 {
                return Some(format!("data:image/jpeg;base64,{}", BASE64.encode(bytes)));
            }
        } else if lower == "svg" {
            let bytes = std::fs::read(path).ok()?;
            if bytes.len() <= 256 * 1024 {
                return Some(format!("data:image/svg+xml;base64,{}", BASE64.encode(bytes)));
            }
        }
    }

    #[cfg(windows)]
    {
        extract_win32_hicon_as_png_data_uri(path, icon_index)
    }

    #[cfg(not(windows))]
    {
        let _ = icon_index;
        None
    }
}

#[cfg(windows)]
fn extract_win32_hicon_as_png_data_uri(path: &Path, icon_index: i32) -> Option<String> {
    use std::mem::size_of;
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ,
    };
    use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL;
    use windows::Win32::UI::Shell::{
        ExtractIconExW, SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON,
    };
    use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO};

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();

    unsafe {
        let mut hicon = HICON::default();
        let extracted = ExtractIconExW(
            PCWSTR(wide.as_ptr()),
            icon_index,
            Some(&mut hicon),
            None,
            1,
        );

        if extracted == 0 || hicon.is_invalid() {
            // Fallback to Shell icon lookup
            let mut shfi = SHFILEINFOW::default();
            let res = SHGetFileInfoW(
                PCWSTR(wide.as_ptr()),
                FILE_ATTRIBUTE_NORMAL,
                Some(&mut shfi),
                size_of::<SHFILEINFOW>() as u32,
                SHGFI_ICON | SHGFI_LARGEICON,
            );
            if res != 0 && !shfi.hIcon.is_invalid() {
                hicon = shfi.hIcon;
            }
        }

        if hicon.is_invalid() {
            return None;
        }

        let mut icon_info = ICONINFO::default();
        if GetIconInfo(hicon, &mut icon_info).is_err() {
            let _ = DestroyIcon(hicon);
            return None;
        }

        let mut bmp = BITMAP::default();
        let target_bmp = if !icon_info.hbmColor.is_invalid() {
            icon_info.hbmColor
        } else {
            icon_info.hbmMask
        };

        let got_obj = GetObjectW(
            HGDIOBJ(target_bmp.0),
            size_of::<BITMAP>() as i32,
            Some(&mut bmp as *mut _ as *mut _),
        );

        if got_obj == 0 || bmp.bmWidth <= 0 || bmp.bmHeight <= 0 {
            if !icon_info.hbmColor.is_invalid() {
                let _ = DeleteObject(HGDIOBJ(icon_info.hbmColor.0));
            }
            if !icon_info.hbmMask.is_invalid() {
                let _ = DeleteObject(HGDIOBJ(icon_info.hbmMask.0));
            }
            let _ = DestroyIcon(hicon);
            return None;
        }

        let width = bmp.bmWidth as u32;
        let height = if icon_info.hbmColor.is_invalid() {
            (bmp.bmHeight / 2).max(1) as u32
        } else {
            bmp.bmHeight as u32
        };

        let dc = CreateCompatibleDC(None);
        let mut bi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                biHeight: -(height as i32), // top-down DIB
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut pixels = vec![0u8; (width * height * 4) as usize];
        let lines = if !icon_info.hbmColor.is_invalid() {
            GetDIBits(
                dc,
                icon_info.hbmColor,
                0,
                height,
                Some(pixels.as_mut_ptr() as *mut _),
                &mut bi,
                DIB_RGB_COLORS,
            )
        } else {
            0
        };

        // Check whether alpha channel is populated; if all zeros, reconstruct alpha from hbmMask.
        let has_alpha = pixels.as_chunks::<4>().0.iter().any(|px| px[3] != 0);
        if lines > 0 && !has_alpha && !icon_info.hbmMask.is_invalid() {
            let mut mask_pixels = vec![0u8; (width * height * 4) as usize];
            let mask_lines = GetDIBits(
                dc,
                icon_info.hbmMask,
                0,
                height,
                Some(mask_pixels.as_mut_ptr() as *mut _),
                &mut bi,
                DIB_RGB_COLORS,
            );
            if mask_lines > 0 {
                for (px, mpx) in pixels
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .zip(mask_pixels.as_chunks::<4>().0.iter())
                {
                    // In AND mask, white (255) means transparent, black (0) means opaque.
                    px[3] = if mpx[0] > 128 { 0 } else { 255 };
                }
            } else {
                for px in pixels.as_chunks_mut::<4>().0 {
                    px[3] = 255;
                }
            }
        }

        let _ = DeleteDC(dc);
        if !icon_info.hbmColor.is_invalid() {
            let _ = DeleteObject(HGDIOBJ(icon_info.hbmColor.0));
        }
        if !icon_info.hbmMask.is_invalid() {
            let _ = DeleteObject(HGDIOBJ(icon_info.hbmMask.0));
        }
        let _ = DestroyIcon(hicon);

        if lines == 0 {
            return None;
        }

        // Convert BGRA -> RGBA
        for px in pixels.as_chunks_mut::<4>().0 {
            px.swap(0, 2);
        }

        let mut png_bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png_bytes, width, height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().ok()?;
            writer.write_image_data(&pixels).ok()?;
        }

        Some(format!("data:image/png;base64,{}", BASE64.encode(png_bytes)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_icon_spec() {
        let (p, idx) = parse_icon_spec("\"C:\\Program Files\\App\\app.exe\",-101");
        assert_eq!(p, PathBuf::from("C:\\Program Files\\App\\app.exe"));
        assert_eq!(idx, -101);

        let (p2, idx2) = parse_icon_spec("C:\\App\\icon.ico");
        assert_eq!(p2, PathBuf::from("C:\\App\\icon.ico"));
        assert_eq!(idx2, 0);
    }
}
