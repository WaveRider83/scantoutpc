use image::DynamicImage;
use oar_ocr::oarocr::OAROCRBuilder;
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
        let engine = OAROCRBuilder::new(det_s.as_str(), rec_s.as_str(), dict_s.as_str())
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
        if h < 120 {
            let scale = (120.0 / h as f32).clamp(1.0, 4.0);
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
            .collect::<Vec<_>>()
            .join("\n");
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
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
