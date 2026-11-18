use image::DynamicImage;
use oar_ocr::oarocr::OAROCRBuilder;
use oar_ocr::domain::tasks::text_detection::TextDetectionConfig;
use oar_ocr::processors::types::LimitType;
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum OcrLang {
    Latin,
    Chinese,
    Korean,
}

impl OcrLang {
    pub const ALL: [OcrLang; 3] = [OcrLang::Latin, OcrLang::Chinese, OcrLang::Korean];
    pub fn label(self) -> &'static str {
        match self {
            OcrLang::Latin => "FR / EN / latin",
            OcrLang::Chinese => "中文 (chinois)",
            OcrLang::Korean => "한국어 (coréen)",
        }
    }
    fn rec_files(self) -> (&'static str, &'static str) {
        match self {
            OcrLang::Latin => ("rec_latin.onnx", "rec_latin.dict"),
            OcrLang::Chinese => ("rec_chinese.onnx", "rec_chinese.dict"),
            OcrLang::Korean => ("rec_korean.onnx", "rec_korean.dict"),
        }
    }
}

pub fn models_dir() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let d = dir.join("models");
            if d.exists() {
                return d;
            }
        }
    }
    PathBuf::from("models")
}

pub struct OcrEngine {
    lang: OcrLang,
    engine: oar_ocr::oarocr::OAROCR,
}

impl OcrEngine {
    pub fn load(lang: OcrLang) -> Result<Self, String> {
        let dir = models_dir();
        let det = dir.join("det.onnx");
        let (rec, dict) = lang.rec_files();
        let det_s = det.to_str().ok_or("chemin modele det invalide")?.to_string();
        let rec_s = dir.join(rec).to_str().ok_or("chemin modele rec invalide")?.to_string();
        let dict_s = dir.join(dict).to_str().ok_or("chemin dict invalide")?.to_string();
        if !det.exists() || !dir.join(rec).exists() || !dir.join(dict).exists() {
            return Err(format!("modèles manquants dans {}", dir.display()));
        }
        let det_cfg = TextDetectionConfig {
            score_threshold: 0.2,
            box_threshold: 0.45,
            limit_side_len: Some(2560),
            limit_type: Some(LimitType::Max),
            ..Default::default()
        };
        let engine = OAROCRBuilder::new(det_s.as_str(), rec_s.as_str(), dict_s.as_str())
            .text_detection_config(det_cfg)
            .split_line_gaps(true)
            // Redresse les captures dont les lignes de texte sont inclinées (photo prise de travers)
            .deskew_threshold(Some(2.5))
            .build()
            .map_err(|e| format!("init OCR: {e}"))?;
        Ok(Self { lang, engine })
    }

    pub fn lang(&self) -> OcrLang {
        self.lang
    }

    pub fn recognize(&self, image: &DynamicImage) -> Result<String, String> {
        // Upscale des petites captures : le modele rec lit mieux les lignes >= ~40 px de haut
        let mut rgb = image.to_rgb8();
        let h = rgb.height();
        if h < 340 {
            let scale = (340.0 / h as f32).clamp(1.0, 8.0);
            let (nw, nh) = ((rgb.width() as f32 * scale) as u32, (rgb.height() as f32 * scale) as u32);
            rgb = image::imageops::resize(&rgb, nw, nh, image::imageops::FilterType::Lanczos3);
        }
        let results = self
            .engine
            .predict(vec![rgb])
            .map_err(|e| format!("OCR: {e}"))?;
        let text = results
            .into_iter()
            .flat_map(|r| r.text_regions.into_iter())
            .filter_map(|r| r.text)
            .map(|t| t.to_string())
            .map(|t| postprocess_line(t.to_string()))
            .collect::<Vec<_>>()
            .join("\n");
        Ok(text)
    }
}

/// Complete la post-edition d'une ligne OCR : espaces que le modele ne peut pas deviner.
fn postprocess_line(s: String) -> String {
    space_after_sn_label(&space_before_embedded_date(&s))
}

/// "...W2024-07-18" -> "...W 2024-07-18" (date ISO collee a un alphanum)
fn space_before_embedded_date(s: &str) -> String {
    fn is_iso_date(b: &[u8], i: usize) -> bool {
        if i + 10 > b.len() { return false; }
        let sep = b[i + 4];
        (sep == b'-' || sep == b'/' || sep == b'.')
            && b[i..i + 4].iter().all(|c| c.is_ascii_digit())
            && b[i + 5..i + 7].iter().all(|c| c.is_ascii_digit())
            && b[i + 7] == sep
            && b[i + 8..i + 10].iter().all(|c| c.is_ascii_digit())
    }
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len() + 8);
    let mut last = 0usize;
    let mut i = 1usize;
    while i < b.len() {
        if i + 10 <= b.len() && is_iso_date(b, i) && b[i - 1].is_ascii_alphanumeric() {
            out.push_str(&s[last..i]);
            out.push(' ');
            last = i;
            i += 10;
            continue;
        }
        i += 1;
    }
    out.push_str(&s[last..]);
    out
}

/// "SNK0R9..." -> "SN K0R9..." (label SN colle a la serie, en debut de token)
fn space_after_sn_label(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len() + 4);
    let mut last = 0usize;
    let mut i = 0usize;
    while i + 1 < b.len() {
        if b[i] == b'S' && b[i + 1] == b'N' {
            let boundary = i == 0 || !b[i - 1].is_ascii_alphanumeric();
            let next_alnum = b.get(i + 2).map(|c| c.is_ascii_alphanumeric()).unwrap_or(false);
            if boundary && next_alnum {
                out.push_str(&s[last..i + 2]);
                out.push(' ');
                last = i + 2;
                i += 2;
                continue;
            }
        }
        i += 1;
    }
    out.push_str(&s[last..]);
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn espace_sn_et_date() {
        assert_eq!(super::postprocess_line("SNK0R9M3GX700145W2024-07-18".to_string()), "SN K0R9M3GX700145W 2024-07-18");
        assert_eq!(super::postprocess_line("[SN]K0R9 2024-07-18".to_string()), "[SN] K0R9 2024-07-18");
        assert_eq!(super::postprocess_line("SN K0R9 deja espace".to_string()), "SN K0R9 deja espace");
        assert_eq!(super::postprocess_line("USP-C017F6A/WR".to_string()), "USP-C017F6A/WR");
        assert_eq!(super::postprocess_line("REF2024-07-18 en fin".to_string()), "REF 2024-07-18 en fin");
        assert_eq!(super::postprocess_line("SN".to_string()), "SN");
    }

    #[test]
    #[ignore]
    fn ocr_smoke() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        unsafe {
            std::env::set_var("ORT_DYLIB_PATH", root.join("models").join(if cfg!(windows) { "onnxruntime.dll" } else { "libonnxruntime.so" }));
        }
        let img = image::open("/tmp/ocr_test.png").expect("test image");
        let engine = super::OcrEngine::load(super::OcrLang::Latin).expect("load engine");
        let text = engine.recognize(&img).expect("ocr");
        eprintln!("OCR result: {:?}", text);
        assert!(text.to_lowercase().contains("hello"));
    }
}
