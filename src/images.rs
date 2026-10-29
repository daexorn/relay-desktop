use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use eframe::egui;
use image::{DynamicImage, ImageReader};
use serde_json::{json, Value};
use std::{io::Cursor, path::Path};

#[derive(Clone)]
pub struct Attachment {
    pub name: String,
    pub texture: Option<egui::TextureHandle>,
    pub bytes: usize,
    pub source: Value,
}
pub fn from_file(ctx: &egui::Context, path: &Path) -> Result<Attachment> {
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let header = {
        use std::io::Read;
        let mut f = std::fs::File::open(path)?;
        let mut b = [0u8; 32];
        let n = f.read(&mut b)?;
        b[..n].to_vec()
    };
    if image::guess_format(&header).is_err() {
        anyhow::ensure!(
            path.metadata()?.is_file() && path.metadata()?.len() <= 12 * 1024 * 1024,
            "Размер файла — до 12 МБ."
        );
        let bytes = std::fs::read(path)?;
        let size = bytes.len();
        return Ok(Attachment {
            name: name.clone(),
            texture: None,
            bytes: size,
            source: json!({"type":"file","name":name,"data":STANDARD.encode(bytes)}),
        });
    }
    anyhow::ensure!(
        path.metadata()?.len() <= 24 * 1024 * 1024,
        "Картинка слишком большая: максимум 24 МБ."
    );
    let mut reader = ImageReader::open(path)?.with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(12000);
    limits.max_image_height = Some(12000);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    let img = reader
        .decode()
        .context("Не удалось прочитать картинку. Поддерживаются PNG, JPEG, WebP и GIF.")?;
    prepare(
        ctx,
        path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        img,
    )
}
pub fn from_clipboard(ctx: &egui::Context) -> Result<Option<Attachment>> {
    let mut clipboard = arboard::Clipboard::new()?;
    let data = match clipboard.get_image() {
        Ok(data) => data,
        Err(arboard::Error::ContentNotAvailable) => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    anyhow::ensure!(
        data.width <= 12000 && data.height <= 12000 && data.bytes.len() <= 256 * 1024 * 1024,
        "Картинка в буфере слишком большая."
    );
    let rgba = image::RgbaImage::from_raw(
        data.width as u32,
        data.height as u32,
        data.bytes.into_owned(),
    )
    .context("Не удалось прочитать картинку из буфера.")?;
    Ok(Some(prepare(
        ctx,
        "Из буфера.png".into(),
        DynamicImage::ImageRgba8(rgba),
    )?))
}
fn prepare(ctx: &egui::Context, name: String, img: DynamicImage) -> Result<Attachment> {
    let img = if img.width() > 1800 || img.height() > 1800 {
        img.resize(1800, 1800, image::imageops::FilterType::Lanczos3)
    } else {
        img
    };
    let thumb = img.thumbnail(96, 72).into_rgba8();
    let size = [thumb.width() as usize, thumb.height() as usize];
    let texture = ctx.load_texture(
        uuid::Uuid::new_v4().to_string(),
        egui::ColorImage::from_rgba_unmultiplied(size, thumb.as_raw()),
        egui::TextureOptions::LINEAR,
    );
    let mut bytes = Cursor::new(Vec::new());
    img.write_to(&mut bytes, image::ImageFormat::Png)?;
    let mime = if bytes.get_ref().len() > 4 * 1024 * 1024 {
        bytes = Cursor::new(Vec::new());
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 88)
            .encode_image(&img.to_rgb8())?;
        "image/jpeg"
    } else {
        "image/png"
    };
    anyhow::ensure!(
        bytes.get_ref().len() <= 4 * 1024 * 1024,
        "Не удалось уменьшить картинку до 4 МБ."
    );
    Ok(Attachment {
        name,
        texture: Some(texture),
        bytes: bytes.get_ref().len(),
        source: json!({"type":"base64","media_type":mime,"data":STANDARD.encode(bytes.into_inner())}),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn image_source_is_valid_and_dimensions_bounded() {
        let a = prepare(
            &egui::Context::default(),
            "test.png".into(),
            DynamicImage::new_rgba8(2400, 2),
        )
        .unwrap();
        let bytes = STANDARD.decode(a.source["data"].as_str().unwrap()).unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert!(decoded.width() <= 1800);
        assert_eq!(a.source["media_type"], "image/png");
    }
    #[test]
    fn ordinary_file_is_an_attachment_without_image_decoding() {
        let p = std::env::temp_dir().join(format!("relay-{}.txt", uuid::Uuid::new_v4()));
        std::fs::write(&p, "Текстовый документ").unwrap();
        let file = from_file(&egui::Context::default(), &p).unwrap();
        assert!(file.texture.is_none());
        assert_eq!(file.source["type"], "file");
        assert_eq!(
            String::from_utf8(
                STANDARD
                    .decode(file.source["data"].as_str().unwrap())
                    .unwrap()
            )
            .unwrap(),
            "Текстовый документ"
        );
        std::fs::remove_file(p).unwrap();
    }
}
