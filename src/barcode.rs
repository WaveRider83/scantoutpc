//! Decode QR / DataMatrix / codes-barres 1D / Aztec / PDF417 depuis une capture.
//! rxing (port Rust de ZXing) : aucune dependance native, aucun modele a telecharger.
use rxing::common::HybridBinarizer;
use rxing::{BarcodeFormat, BinaryBitmap, MultiFormatReader, RGBLuminanceSource, Reader};

/// Tente de lire un code dans l'image. Retourne (format court, contenu) si trouve.
pub fn decode(img: &image::DynamicImage) -> Option<(String, String)> {
    let rgb = img.to_rgb8();
    let (w, h) = rgb.dimensions();
    let px: Vec<u32> = rgb
        .pixels()
        .map(|p| {
            0xFF00_0000u32
                | ((p[0] as u32) << 16)
                | ((p[1] as u32) << 8)
                | p[2] as u32
        })
        .collect();
    let luma = RGBLuminanceSource::new_with_width_height_pixels(w as usize, h as usize, &px).ok()?;
    let hybrid = HybridBinarizer::new(luma);
    let mut bitmap = BinaryBitmap::new(hybrid);
    let mut reader = MultiFormatReader::default();
    let res = reader.decode(&mut bitmap).ok()?;
    let text = res.getText().trim().to_string();
    if text.is_empty() {
        return None;
    }
    Some((label(res.getBarcodeFormat()), text))
}

fn label(f: &BarcodeFormat) -> String {
    match *f {
        BarcodeFormat::QR_CODE => "QR".into(),
        BarcodeFormat::MICRO_QR_CODE => "micro-QR".into(),
        BarcodeFormat::DATA_MATRIX => "DataMatrix".into(),
        BarcodeFormat::CODE_128 => "Code128".into(),
        BarcodeFormat::CODE_39 => "Code39".into(),
        BarcodeFormat::CODE_93 => "Code93".into(),
        BarcodeFormat::EAN_13 => "EAN-13".into(),
        BarcodeFormat::EAN_8 => "EAN-8".into(),
        BarcodeFormat::UPC_A => "UPC-A".into(),
        BarcodeFormat::UPC_E => "UPC-E".into(),
        BarcodeFormat::ITF => "ITF".into(),
        BarcodeFormat::CODABAR => "Codabar".into(),
        BarcodeFormat::AZTEC => "Aztec".into(),
        BarcodeFormat::PDF_417 => "PDF417".into(),
        other => format!("{:?}", other),
    }
}
