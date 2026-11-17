//! Decode and map the server browser's pixels. This module makes no website requests.
use anyhow::{Context, Result};
use base64::Engine;
use eframe::egui::{self, Pos2, Rect};
use serde_json::Value;
use std::io::Cursor;

pub const MAX_FRAME: usize = 2 * 1024 * 1024;
pub struct Frame {
    pub sequence: u64,
    pub image: egui::ColorImage,
    pub width: u32,
    pub height: u32,
}
impl Frame {
    pub fn decode(value: &Value) -> Result<Self> {
        let sequence = value["frame"]
            .as_u64()
            .filter(|n| *n > 0)
            .context("Некорректный номер кадра.")?;
        let width = value["width"]
            .as_u64()
            .filter(|n| *n > 0 && *n <= 1920)
            .context("Некорректная ширина кадра.")? as u32;
        let height = value["height"]
            .as_u64()
            .filter(|n| *n > 0 && *n <= 1200)
            .context("Некорректная высота кадра.")? as u32;
        let data = value["data"]
            .as_str()
            .context("Нет изображения браузера.")?;
        anyhow::ensure!(data.len() <= MAX_FRAME, "Кадр браузера слишком большой.");
        let bytes = base64::engine::general_purpose::STANDARD.decode(data)?;
        let mut reader =
            image::ImageReader::with_format(Cursor::new(bytes), image::ImageFormat::Jpeg);
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(1920);
        limits.max_image_height = Some(1200);
        limits.max_alloc = Some(16 * 1024 * 1024);
        reader.limits(limits);
        let decoded = reader.decode()?.into_rgba8();
        anyhow::ensure!(
            decoded.width() == width && decoded.height() == height,
            "Размер изображения браузера не совпадает с кадром."
        );
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [width as usize, height as usize],
            decoded.as_raw(),
        );
        Ok(Self {
            sequence,
            image,
            width,
            height,
        })
    }
}
pub fn point(rect: Rect, p: Pos2, width: u32, height: u32) -> Option<(f32, f32)> {
    if !rect.is_positive() || !rect.contains(p) || width == 0 || height == 0 {
        return None;
    }
    Some((
        ((p.x - rect.left()) / rect.width() * width as f32).clamp(0., width as f32),
        ((p.y - rect.top()) / rect.height() * height as f32).clamp(0., height as f32),
    ))
}
pub fn modifiers(m: egui::Modifiers) -> u8 {
    u8::from(m.alt) + u8::from(m.ctrl) * 2 + u8::from(m.mac_cmd) * 4 + u8::from(m.shift) * 8
}
pub fn key(value: egui::Key, command: bool) -> Option<&'static str> {
    use egui::Key::*;
    Some(match value {
        Enter => "Enter",
        Tab => "Tab",
        Escape => "Escape",
        Backspace => "Backspace",
        Delete => "Delete",
        ArrowLeft => "ArrowLeft",
        ArrowRight => "ArrowRight",
        ArrowUp => "ArrowUp",
        ArrowDown => "ArrowDown",
        Home => "Home",
        End => "End",
        PageUp => "PageUp",
        PageDown => "PageDown",
        A if command => "a",
        C if command => "c",
        V if command => "v",
        X if command => "x",
        Z if command => "z",
        Y if command => "y",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn maps_letterboxed_and_scaled_images_to_server_coordinates() {
        let rect = Rect::from_min_size(Pos2::new(50., 70.), egui::vec2(600., 400.));
        assert_eq!(
            point(rect, Pos2::new(350., 270.), 1200, 800),
            Some((600., 400.))
        );
        assert_eq!(point(rect, Pos2::new(49., 100.), 1200, 800), None);
        assert_eq!(point(Rect::NOTHING, Pos2::ZERO, 1200, 800), None);
    }
    #[test]
    fn frames_have_a_real_bounded_jpeg_and_consistent_geometry() {
        let mut jpeg = vec![];
        image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
            .encode(&[20, 40, 60], 1, 1, image::ExtendedColorType::Rgb8)
            .unwrap();
        let value = json!({"frame":1,"width":1,"height":1,"data":base64::engine::general_purpose::STANDARD.encode(&jpeg)});
        assert_eq!(Frame::decode(&value).unwrap().image.size, [1, 1]);
        for bad in [
            json!({"frame":1,"width":999999,"height":1,"data":""}),
            json!({"frame":1,"width":1,"height":1,"data":"not-jpeg"}),
            json!({"frame":0,"width":1,"height":1,"data":""}),
        ] {
            assert!(Frame::decode(&bad).is_err());
        }
        let mut wrong = value.clone();
        wrong["width"] = json!(2);
        assert!(Frame::decode(&wrong).is_err());
    }
    #[test]
    fn keyboard_modifiers_and_navigation_are_sent_without_local_navigation() {
        assert_eq!(
            modifiers(egui::Modifiers {
                ctrl: true,
                shift: true,
                ..Default::default()
            }),
            10
        );
        assert_eq!(key(egui::Key::ArrowLeft, false), Some("ArrowLeft"));
        assert_eq!(key(egui::Key::A, true), Some("a"));
        assert_eq!(key(egui::Key::A, false), None);
    }
}
