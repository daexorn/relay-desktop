//! File preparation shared by headless clients; no texture or clipboard needed.
use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{io::Cursor, path::Path};

pub const MAX_ATTACHMENTS: usize = 8;
pub const MAX_ATTACHMENT_BYTES: usize = 512 * 1024 * 1024;
pub const MAX_INLINE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub name: String,
    pub bytes: usize,
    pub source: Value,
}

pub fn from_file(path: &Path) -> Result<Attachment> {
    use std::io::Read;
    let metadata = path.metadata().context("Файл не найден")?;
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
    let mut header = [0u8; 32];
    let length = std::fs::File::open(path)?.read(&mut header)?;
    if image::guess_format(&header[..length]).is_err() || metadata.len() > 24 * 1024 * 1024 {
        return Ok(Attachment {
            name: name.clone(),
            bytes: metadata.len() as usize,
            source: json!({"type":"file","name":name,"path":path.canonicalize()?,"bytes":metadata.len()}),
        });
    }
    let mut reader = image::ImageReader::open(path)?.with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(12000);
    limits.max_image_height = Some(12000);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    let image = reader
        .decode()
        .context("Не удалось прочитать изображение")?;
    let image = if image.width() > 1800 || image.height() > 1800 {
        image.resize(1800, 1800, image::imageops::FilterType::Lanczos3)
    } else {
        image
    };
    let mut body = Cursor::new(Vec::new());
    image.write_to(&mut body, image::ImageFormat::Png)?;
    let media_type = if body.get_ref().len() > 4 * 1024 * 1024 {
        body = Cursor::new(Vec::new());
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut body, 88)
            .encode_image(&image.to_rgb8())?;
        "image/jpeg"
    } else {
        "image/png"
    };
    anyhow::ensure!(
        body.get_ref().len() <= 4 * 1024 * 1024,
        "Не удалось уменьшить изображение до 4 МБ."
    );
    Ok(Attachment {
        name,
        bytes: body.get_ref().len(),
        source: json!({"type":"base64","media_type":media_type,"data":STANDARD.encode(body.into_inner())}),
    })
}

pub fn validate(files: &[Attachment]) -> Result<()> {
    anyhow::ensure!(
        files.len() <= MAX_ATTACHMENTS
            && files.iter().map(|f| f.bytes).sum::<usize>() <= MAX_ATTACHMENT_BYTES,
        "До восьми вложений общим размером 512 МБ."
    );
    anyhow::ensure!(
        files
            .iter()
            .filter(|f| f.source["type"] != "file")
            .map(|f| f.bytes)
            .sum::<usize>()
            <= MAX_INLINE_BYTES,
        "Изображения в сообщении — до 16 МБ."
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_file_path_with_spaces_is_lazy_and_has_no_shell_interpretation() {
        let path =
            std::env::temp_dir().join(format!("relay файл $literal {}.bin", uuid::Uuid::new_v4()));
        std::fs::write(&path, b"ordinary file").unwrap();
        let file = from_file(&path).unwrap();
        assert_eq!(file.source["type"], "file");
        assert_eq!(file.source["path"], json!(path.canonicalize().unwrap()));
        assert_eq!(file.bytes, 13);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn images_are_real_bounded_payloads_without_a_window_system() {
        let path =
            std::env::temp_dir().join(format!("relay-tui-image-{}.png", uuid::Uuid::new_v4()));
        image::RgbaImage::from_pixel(8, 8, image::Rgba([1, 2, 3, 255]))
            .save(&path)
            .unwrap();
        let file = from_file(&path).unwrap();
        assert_eq!(file.source["type"], "base64");
        let bytes = STANDARD
            .decode(file.source["data"].as_str().unwrap())
            .unwrap();
        assert_eq!(image::load_from_memory(&bytes).unwrap().width(), 8);
        assert!(file.bytes < 4 * 1024 * 1024);
        assert!(validate(&vec![file; 9]).is_err());
        std::fs::remove_file(path).unwrap();
    }
}
