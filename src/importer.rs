//! Import de fichiers externes (images / PDF) pour analyse OCR identique à une capture.

use std::path::Path;

/// Charge un fichier image ou PDF et renvoie une RgbaImage par page
/// (1 page pour une image, N pages pour un PDF, dans l'ordre).
pub fn load_pages(path: &Path) -> Result<Vec<image::RgbaImage>, String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "pdf" => render_pdf(path),
        "png" | "jpg" | "jpeg" | "bmp" | "gif" | "webp" | "tif" | "tiff" | "ico" => {
            let img = image::open(path).map_err(|e| format!("image illisible: {e}"))?;
            Ok(vec![img.to_rgba8()])
        }
        other => Err(format!("format non supporté: .{other} (attendu: image ou PDF)")),
    }
}

fn render_pdf(path: &Path) -> Result<Vec<image::RgbaImage>, String> {
    use hayro::hayro_interpret::InterpreterSettings;
    use hayro::hayro_syntax::Pdf;
    use hayro::vello_cpu::color::palette::css::WHITE;

    let data = std::fs::read(path).map_err(|e| format!("lecture PDF: {e}"))?;
    let pdf = Pdf::new(data).map_err(|e| format!("PDF invalide: {e:?}"))?;
    let pages = pdf.pages();
    if pages.is_empty() {
        return Err("PDF sans page".into());
    }
    if pages.len() > 50 {
        return Err(format!("PDF trop volumineux ({} pages, max 50)", pages.len()));
    }

    let interpreter_settings = InterpreterSettings::default();
    let render_settings = hayro::RenderSettings::default();
    let cache = hayro::RenderCache::new();
    // 2x pour une OCR correcte ; pages très grandes bornées à ~2200px de large
    let pixmap_settings = hayro::PixmapSettings { x_scale: 2.0, y_scale: 2.0, bg_color: WHITE };

    let mut out = Vec::with_capacity(pages.len());
    for (i, page) in pages.iter().enumerate() {
        let pixmap = hayro::render(page, &cache, &interpreter_settings, &render_settings, &pixmap_settings);
        let png = pixmap.into_png().map_err(|e| format!("rendu page {}: {e}", i + 1))?;
        let img = image::load_from_memory(&png)
            .map_err(|e| format!("décodage page {}: {e}", i + 1))?
            .to_rgba8();
        out.push(img);
    }
    Ok(out)
}
