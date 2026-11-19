//! Capture GDI (BitBlt) — chemin CPU, ne réveille pas le GPU NVIDIA.
//! Utilisé quand "Accélération matérielle" est désactivée.
use image::RgbaImage;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::Foundation::HWND;

pub fn gdi_capture(abs_x: i32, abs_y: i32, w: i32, h: i32) -> Result<RgbaImage, String> {
    if w <= 0 || h <= 0 {
        return Err("zone vide".into());
    }
    unsafe {
        let screen: HWND = std::ptr::null_mut();
        let hdc_screen = GetDC(screen);
        if hdc_screen.is_null() {
            return Err("GetDC écran impossible".into());
        }
        let result = (|| {
            let hdc = CreateCompatibleDC(hdc_screen);
            if hdc.is_null() {
                return Err("CreateCompatibleDC impossible".to_string());
            }
            let hbmp = CreateCompatibleBitmap(hdc_screen, w, h);
            if hbmp.is_null() {
                DeleteDC(hdc);
                return Err("CreateCompatibleBitmap impossible".to_string());
            }
            let old = SelectObject(hdc, hbmp as HGDIOBJ);
            let ok = BitBlt(hdc, 0, 0, w, h, hdc_screen, abs_x, abs_y, SRCCOPY | CAPTUREBLT);
            let mut bmi = BITMAPINFO::default();
            bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
            bmi.bmiHeader.biWidth = w;
            bmi.bmiHeader.biHeight = -h; // négatif = top-down
            bmi.bmiHeader.biPlanes = 1;
            bmi.bmiHeader.biBitCount = 32;
            bmi.bmiHeader.biCompression = BI_RGB;
            let mut buf = vec![0u8; (w as usize) * (h as usize) * 4];
            let scan = if ok != 0 {
                GetDIBits(hdc, hbmp, 0, h as u32, buf.as_mut_ptr() as *mut core::ffi::c_void, &mut bmi, DIB_RGB_COLORS)
            } else {
                0
            };
            SelectObject(hdc, old);
            DeleteObject(hbmp as HGDIOBJ);
            DeleteDC(hdc);
            if ok == 0 {
                return Err("BitBlt a échoué".to_string());
            }
            if scan == 0 {
                return Err("GetDIBits a échoué".to_string());
            }
            // BGRA -> RGBA, alpha opaque
            for px in buf.chunks_exact_mut(4) {
                px.swap(0, 2);
                px[3] = 255;
            }
            image::RgbaImage::from_raw(w as u32, h as u32, buf).ok_or_else(|| "buffer image invalide".to_string())
        })();
        ReleaseDC(screen, hdc_screen);
        result
    }
}
