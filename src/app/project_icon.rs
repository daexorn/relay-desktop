use super::{Color32, Kind, Palette, RelayApp, Vec2, design, egui, json, label};
use crate::model::SessionFolder;
use base64::{Engine, engine::general_purpose::STANDARD};
use image::{DynamicImage, ImageReader, Pixel, Rgba, RgbaImage, imageops};
use std::{io::Cursor, path::Path};

const SIZE: u32 = 96;
const MAX_ICON: usize = 60_000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Fit {
    Contain,
    Cover,
    Original,
}

pub(super) struct IconDialog {
    server: String,
    folder: SessionFolder,
    source: Option<RgbaImage>,
    fit: Fit,
    scale: f32,
    padding: f32,
    radius: f32,
    background: Color32,
    preview: Option<egui::TextureHandle>,
    rendered: Option<RgbaImage>,
    picker: Option<std::sync::mpsc::Receiver<std::path::PathBuf>>,
    error: String,
}

fn decode(encoded: &str) -> Option<RgbaImage> {
    if encoded.is_empty() || encoded.len() > MAX_ICON {
        return None;
    }
    let bytes = STANDARD.decode(encoded).ok()?;
    if bytes.get(16..24)? != [0, 0, 0, 96, 0, 0, 0, 96] {
        return None;
    }
    let image = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png).ok()?;
    Some(image.into_rgba8())
}

fn texture(ctx: &egui::Context, image: &RgbaImage) -> egui::TextureHandle {
    ctx.load_texture(
        "project-icon",
        egui::ColorImage::from_rgba_unmultiplied([SIZE as usize; 2], image.as_raw()),
        egui::TextureOptions::LINEAR,
    )
}

pub(super) fn folder_texture(ctx: &egui::Context, encoded: &str) -> Option<egui::TextureHandle> {
    if encoded.is_empty() || encoded.len() > MAX_ICON {
        return None;
    }
    let key = egui::Id::new(("folder-icon", encoded));
    if let Some(cached) = ctx.data_mut(|data| data.get_temp::<egui::TextureHandle>(key)) {
        return Some(cached);
    }
    let icon = texture(ctx, &decode(encoded)?);
    ctx.data_mut(|data| data.insert_temp(key, icon.clone()));
    Some(icon)
}

fn read_image(path: &Path) -> anyhow::Result<RgbaImage> {
    anyhow::ensure!(path.metadata()?.len() <= 16 * 1024 * 1024, "Файл должен быть меньше 16 МБ.");
    let mut reader = ImageReader::open(path)?.with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    Ok(reader.decode()?.thumbnail(384, 384).into_rgba8())
}

impl IconDialog {
    pub(super) fn new(server: String, folder: &SessionFolder) -> Self {
        Self {
            server,
            folder: folder.clone(),
            source: decode(&folder.icon),
            fit: Fit::Contain,
            scale: 1.,
            padding: 0.,
            radius: 0.,
            background: Color32::TRANSPARENT,
            preview: None,
            rendered: None,
            picker: None,
            error: String::new(),
        }
    }

    fn render(&mut self, ctx: &egui::Context) {
        let Some(source) = &self.source else { return };
        let half = SIZE as f32 / 2.;
        let area = SIZE as f32 * (1. - self.padding * 2.);
        let (w, h) = (source.width() as f32, source.height() as f32);
        let ratio = self.scale * match self.fit {
            Fit::Contain => (area / w).min(area / h),
            Fit::Cover => (area / w).max(area / h),
            Fit::Original => 1.,
        };
        // Limit resampling to the canvas, also for very wide or very tall images.
        let visible_w = ((SIZE as f32 / ratio).ceil() as u32).clamp(1, source.width());
        let visible_h = ((SIZE as f32 / ratio).ceil() as u32).clamp(1, source.height());
        let cropped = imageops::crop_imm(source, (source.width() - visible_w) / 2, (source.height() - visible_h) / 2, visible_w, visible_h);
        let resized = imageops::resize(
            &cropped.to_image(),
            ((visible_w as f32 * ratio).round() as u32).clamp(1, SIZE * 4),
            ((visible_h as f32 * ratio).round() as u32).clamp(1, SIZE * 4),
            imageops::FilterType::Triangle,
        );
        let mut image = RgbaImage::from_pixel(SIZE, SIZE, Rgba(self.background.to_srgba_unmultiplied()));
        let left = (SIZE as i64 - resized.width() as i64) / 2;
        let top = (SIZE as i64 - resized.height() as i64) / 2;
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            let px = x as i64 - left;
            let py = y as i64 - top;
            let dx = (x as f32 + 0.5 - half).abs();
            let dy = (y as f32 + 0.5 - half).abs();
            if dx <= area / 2. && dy <= area / 2. && px >= 0 && py >= 0 {
                if let Some(sample) = resized.get_pixel_checked(px as u32, py as u32) {
                    pixel.blend(sample);
                }
            }
            if self.radius > 0. {
                let edge = half - self.radius;
                let corner = (dx - edge).max(0.).hypot((dy - edge).max(0.));
                let coverage = (self.radius + 0.5 - corner).clamp(0., 1.);
                pixel[3] = (pixel[3] as f32 * coverage).round() as u8;
            }
        }
        self.preview = Some(texture(ctx, &image));
        self.rendered = Some(image);
    }

    fn encoded(&self) -> anyhow::Result<String> {
        let mut out = Cursor::new(Vec::new());
        let image = self.rendered.clone().ok_or_else(|| anyhow::anyhow!("Выбери изображение."))?;
        DynamicImage::ImageRgba8(image).write_to(&mut out, image::ImageFormat::Png)?;
        Ok(STANDARD.encode(out.into_inner()))
    }
}

impl RelayApp {
    pub(super) fn project_icon_modal(&mut self, ctx: &egui::Context, p: Palette) {
        let Some(mut dialog) = self.icon_dialog.take() else { return };
        let mut changed = dialog.preview.is_none() && dialog.source.is_some();
        if let Some(picker) = &dialog.picker {
            match picker.try_recv() {
                Ok(path) => {
                    match read_image(&path) {
                        Ok(source) => {
                            dialog.source = Some(source);
                            dialog.error.clear();
                            changed = true;
                        }
                        Err(error) => dialog.error = error.to_string(),
                    }
                    dialog.picker = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => dialog.picker = None,
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        if changed {
            dialog.render(ctx);
        }
        let mut result = None;
        let mut cancel = false;
        let response = egui::Modal::new(egui::Id::new("project-icon"))
            .frame(egui::Frame::new().fill(p.surface).corner_radius(16.).inner_margin(20.))
            .show(ctx, |ui| {
                ui.set_width(380.);
                label(ui, "Значок проекта", 18., p.text);
                label(ui, &dialog.folder.name, 13., p.muted);
                ui.add_space(8.);
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(96.), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 10., p.base);
                    if let Some(preview) = &dialog.preview {
                        ui.painter().image(preview.id(), rect, egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)), Color32::WHITE);
                    }
                    ui.vertical(|ui| {
                        if ui.add_enabled(dialog.picker.is_none(), egui::Button::new("Выбрать изображение…")).clicked() {
                            let (send, receive) = std::sync::mpsc::channel();
                            dialog.picker = Some(receive);
                            let ctx = ctx.clone();
                            self.controller.runtime.spawn(async move {
                                if let Some(file) = rfd::AsyncFileDialog::new()
                                    .add_filter("Изображения", &["png", "jpg", "jpeg", "webp", "gif"])
                                    .pick_file().await
                                {
                                    let _ = send.send(file.path().to_owned());
                                }
                                drop(send);
                                ctx.request_repaint();
                            });
                        }
                        ui.add_enabled_ui(dialog.source.is_some(), |ui| {
                            ui.horizontal_wrapped(|ui| {
                                changed |= ui.selectable_value(&mut dialog.fit, Fit::Contain, "Вписать").changed();
                                changed |= ui.selectable_value(&mut dialog.fit, Fit::Cover, "Заполнить").changed();
                                changed |= ui.selectable_value(&mut dialog.fit, Fit::Original, "По рисунку").changed();
                            });
                            ui.horizontal(|ui| {
                                if ui.button("Повернуть").clicked() {
                                    dialog.source = dialog.source.as_ref().map(imageops::rotate90);
                                    changed = true;
                                }
                                if ui.button("Отразить").clicked() {
                                    dialog.source = dialog.source.as_ref().map(imageops::flip_horizontal);
                                    changed = true;
                                }
                            });
                        });
                    });
                });
                ui.add_space(8.);
                ui.add_enabled_ui(dialog.source.is_some(), |ui| {
                    changed |= ui.add(egui::Slider::new(&mut dialog.scale, 0.5..=3.).text("Масштаб")).changed();
                    changed |= ui.add(egui::Slider::new(&mut dialog.padding, 0.0..=0.35).text("Отступ")).changed();
                    changed |= ui.add(egui::Slider::new(&mut dialog.radius, 0.0..=48.).text("Скругление")).changed();
                    ui.horizontal(|ui| {
                        label(ui, "Фон", 13., p.muted);
                        changed |= ui.color_edit_button_srgba(&mut dialog.background).changed();
                        if ui.button("Прозрачный").clicked() {
                            dialog.background = Color32::TRANSPARENT;
                            changed = true;
                        }
                    });
                });
                if !dialog.error.is_empty() {
                    label(ui, &dialog.error, 12., design::RED);
                }
                if changed {
                    dialog.render(ctx);
                }
                ui.add_space(10.);
                ui.horizontal_wrapped(|ui| {
                    if !dialog.folder.icon.is_empty() && ui.button("Сбросить значок").clicked() {
                        result = Some(String::new());
                    }
                    if ui.button("Отмена").clicked() {
                        cancel = true;
                    }
                    ui.add_enabled_ui(dialog.rendered.is_some(), |ui| {
                        if design::button(ui, "Сохранить", None, Kind::Primary, p, Vec2::new(100., 34.)).clicked() {
                            match dialog.encoded() {
                                Ok(icon) => result = Some(icon),
                                Err(error) => dialog.error = error.to_string(),
                            }
                        }
                    });
                });
            });
        if let Some(icon) = result {
            self.controller.send(&dialog.server, json!({"op":"session_folder","id":dialog.folder.id,"icon":icon}));
        } else if !cancel && !response.should_close() {
            self.icon_dialog = Some(dialog);
        }
    }
}
