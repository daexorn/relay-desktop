use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use eframe::egui;
use image::{DynamicImage, ImageReader};
use serde_json::{json, Value};
use std::{io::Cursor, path::Path};

pub const MAX_ATTACHMENTS: usize = 8;
pub const MAX_ATTACHMENT_BYTES: usize = 512 * 1024 * 1024;
pub const MAX_INLINE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone)]
pub struct Attachment {
    pub name: String,
    pub texture: Option<egui::TextureHandle>,
    pub bytes: usize,
    pub source: Value,
}
pub fn from_file(ctx: &egui::Context, path: &Path) -> Result<Attachment> {
    let metadata = path.metadata()?;
    anyhow::ensure!(metadata.is_file(), "Выбери обычный файл.");
    anyhow::ensure!(
        metadata.len() <= MAX_ATTACHMENT_BYTES as u64,
        "Размер файла — до 512 МБ."
    );
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
    if image::guess_format(&header).is_err() || metadata.len() > 24 * 1024 * 1024 {
        return Ok(Attachment {
            name: name.clone(),
            texture: None,
            bytes: metadata.len() as usize,
            source: json!({"type":"file","name":name,"path":path.canonicalize()?,"bytes":metadata.len()}),
        });
    }
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
    // Big enough for the preview in a message bubble; tiles scale it down.
    let thumb = img.thumbnail(440, 320).into_rgba8();
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

/// A picture from a saved conversation: preview only, it is never sent again.
pub fn from_history(ctx: &egui::Context, image: &Value) -> Attachment {
    let data = image["data"].as_str().unwrap_or("");
    let decoded = STANDARD.decode(data).ok().and_then(|bytes| {
        let img = image::load_from_memory(&bytes).ok()?;
        Some((bytes.len(), img.thumbnail(440, 320).into_rgba8()))
    });
    let (bytes, texture) = match decoded {
        Some((bytes, thumb)) => {
            let size = [thumb.width() as usize, thumb.height() as usize];
            let texture = ctx.load_texture(
                uuid::Uuid::new_v4().to_string(),
                egui::ColorImage::from_rgba_unmultiplied(size, thumb.as_raw()),
                egui::TextureOptions::LINEAR,
            );
            (bytes, Some(texture))
        }
        None => (0, None),
    };
    Attachment { name: "Изображение".into(), texture, bytes, source: Value::Null }
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
        assert_eq!(file.source["bytes"], "Текстовый документ".len());
        assert_eq!(
            std::fs::read_to_string(file.source["path"].as_str().unwrap()).unwrap(),
            "Текстовый документ"
        );
        assert!(file.source.get("data").is_none());
        std::fs::remove_file(p).unwrap();
    }
    #[test]
    fn large_files_are_lazy_and_over_limit_is_rejected_before_reading() {
        let p = std::env::temp_dir().join(format!("relay-{}.zip", uuid::Uuid::new_v4()));
        let file = std::fs::File::create(&p).unwrap();
        file.set_len(32 * 1024 * 1024).unwrap();
        let attachment = from_file(&egui::Context::default(), &p).unwrap();
        assert_eq!(attachment.bytes, 32 * 1024 * 1024);
        assert!(attachment.source.to_string().len() < 1024);
        assert!(attachment.source.get("data").is_none());
        file.set_len(MAX_ATTACHMENT_BYTES as u64 + 1).unwrap();
        assert!(from_file(&egui::Context::default(), &p)
            .err()
            .unwrap()
            .to_string()
            .contains("512"));
        drop(file);
        std::fs::remove_file(p).unwrap();
    }
}
