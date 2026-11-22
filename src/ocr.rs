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

    pub fn recognize(&self, image: &DynamicImage, layout: bool) -> Result<String, String> {
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
        // Le moteur rend des *régions*, pas des lignes : une même ligne visuelle
        // peut être découpée en plusieurs boîtes. On regroupe par chevauchement
        // vertical (> 50 % de la hauteur min, comme le détecteur), triées par x.
        let mut regs: Vec<(f32, f32, f32, f32, String)> = results
            .into_iter()
            .flat_map(|r| r.text_regions.into_iter())
            .filter_map(|r| {
                r.text.map(|t| {
                    (
                        r.bounding_box.y_min(),
                        r.bounding_box.y_max(),
                        r.bounding_box.x_min(),
                        r.bounding_box.x_max(),
                        t.to_string(),
                    )
                })
            })
            .collect();
        regs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let mut lines: Vec<OcrLine> = Vec::new();
        for (y0, y1, x0, x1, text) in regs {
            let mut placement = None;
            for (i, line) in lines.iter().enumerate() {
                let overlap = line.y1.min(y1) - line.y0.max(y0);
                let min_h = (line.y1 - line.y0).min(y1 - y0);
                if min_h > 0.0 && overlap / min_h > 0.5 {
                    placement = Some(i);
                    break;
                }
            }
            match placement {
                Some(i) => {
                    let l = &mut lines[i];
                    l.frags.push((x0, x1, text));
                    l.y0 = l.y0.min(y0);
                    l.y1 = l.y1.max(y1);
                }
                None => lines.push(OcrLine { y0, y1, frags: vec![(x0, x1, text)] }),
            }
        }
        for l in lines.iter_mut() {
            l.frags.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        }
        lines.sort_by(|a, b| a.y0.partial_cmp(&b.y0).unwrap_or(std::cmp::Ordering::Equal));
        let text = if layout {
            layout_text(&lines)
        } else {
            lines
                .into_iter()
                .map(|l| {
                    postprocess_line(
                        l.frags.into_iter().map(|(_, _, t)| t).collect::<Vec<_>>().join(" "),
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        Ok(text)
    }
}

struct OcrLine {
    y0: f32,
    y1: f32,
    /// (x_min, x_max, texte)
    frags: Vec<(f32, f32, String)>,
}

/// Reconstruit la disposition : grille monospace alignee, ou tableau TSV
/// si des colonnes recurrentes sont detectees.
fn layout_text(lines: &[OcrLine]) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let total_w: f32 = lines.iter().flat_map(|l| l.frags.iter()).map(|f| (f.1 - f.0).max(0.0)).sum();
    let total_c: usize = lines.iter().flat_map(|l| l.frags.iter()).map(|f| f.2.chars().count()).filter(|&c| c > 0).sum();
    let cw = if total_c > 0 { (total_w / total_c as f32).max(1.0) } else { 8.0 };
    let min_x = lines.iter().flat_map(|l| l.frags.iter().map(|f| f.0)).fold(f32::INFINITY, f32::min);

    let mut xs: Vec<f32> = lines.iter().flat_map(|l| l.frags.iter().map(|f| f.0)).collect();
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut clusters: Vec<f32> = Vec::new();
    for x in xs {
        match clusters.last_mut() {
            Some(c) if x - *c <= cw * 1.5 => *c = (*c + x) / 2.0,
            _ => clusters.push(x),
        }
    }
    let assign = |x: f32| -> Option<usize> {
        let mut best = None;
        let mut bd = f32::INFINITY;
        for (i, c) in clusters.iter().enumerate() {
            let d = (x - *c).abs();
            if d < bd {
                bd = d;
                best = Some(i);
            }
        }
        if bd <= cw * 1.5 { best } else { None }
    };
    let is_table = clusters.len() >= 2
        && lines.len() >= 3
        && lines.iter().all(|l| {
            if l.frags.len() < 2 {
                return false;
            }
            let mut used = std::collections::HashSet::new();
            l.frags.iter().all(|f| assign(f.0).map(|i| used.insert(i)).unwrap_or(false))
        });
    if is_table {
        return lines
            .iter()
            .map(|l| {
                let mut cells = vec![String::new(); clusters.len()];
                for (x, _, t) in &l.frags {
                    let i = assign(*x).unwrap_or(0);
                    let cell = &mut cells[i];
                    if !cell.is_empty() {
                        cell.push(' ');
                    }
                    cell.push_str(postprocess_line(t.clone()).trim());
                }
                cells.join("\t").trim_end_matches(|c: char| c == '\t' || c == ' ').to_string()
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    lines
        .iter()
        .map(|l| {
            let mut out = String::new();
            let mut col = 0usize;
            for (x0, _, t) in &l.frags {
                let start = (((*x0 - min_x) / cw).round() as i64).max(0) as usize;
                if start > col {
                    for _ in col..start {
                        out.push(' ');
                    }
                } else if !out.is_empty() {
                    out.push(' ');
                }
                let t = postprocess_line(t.clone());
                col = start.max(col) + t.chars().count();
                out.push_str(&t);
            }
            out.trim_end().to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
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
