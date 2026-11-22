use super::*;
use egui::{Sense, TextureHandle};

pub(super) struct Viewer {
    name: String,
    texture: TextureHandle,
    actual_size: bool,
}

impl Viewer {
    fn new(name: &str, texture: TextureHandle) -> Self {
        Self {
            name: name.to_owned(),
            texture,
            actual_size: false,
        }
    }

    pub(super) fn attachment(
        ctx: &egui::Context,
        file: &Attachment,
        history: Option<&Value>,
    ) -> Option<Self> {
        let thumb = file.texture.as_ref()?;
        let texture = file.source["data"]
            .as_str()
            .or_else(|| history.and_then(|v| v["data"].as_str()))
            .and_then(|data| images::preview_texture(ctx, data, 1800))
            .unwrap_or_else(|| thumb.clone());
        Some(Self::new(&file.name, texture))
    }

    pub(super) fn show(&mut self, ctx: &egui::Context, key: &str, p: Palette) -> bool {
        let bounds = (ctx.screen_rect().size() - Vec2::new(48., 100.)).max(Vec2::splat(48.));
        let src = self.texture.size_vec2();
        let fit = (bounds.x / src.x).min(bounds.y / src.y).min(1.);
        let viewport = (src * fit)
            .max(Vec2::new(260_f32.min(bounds.x), 64_f32.min(bounds.y)))
            .min(bounds);
        let modal = egui::Modal::new(egui::Id::new(("photo-viewer", key)))
            .frame(
                egui::Frame::new()
                    .fill(p.surface)
                    .corner_radius(14.)
                    .inner_margin(12.),
            )
            .show(ctx, |ui| {
                ui.set_width(viewport.x);
                let mut close = false;
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        close = design::button(
                            ui,
                            "",
                            Some(Icon::Close),
                            Kind::Ghost,
                            p,
                            Vec2::splat(28.),
                        )
                        .on_hover_text("Закрыть · Esc")
                        .clicked();
                        if fit < 1.
                            && ui
                                .selectable_label(self.actual_size, "1:1")
                                .on_hover_text("Исходный масштаб превью")
                                .clicked()
                        {
                            self.actual_size = !self.actual_size;
                        }
                        ui.add(egui::Label::new(RichText::new(&self.name).strong()).truncate());
                    });
                });
                ui.add_space(6.);
                let (area, _) = ui.allocate_exact_size(viewport, Sense::hover());
                if self.actual_size {
                    let mut scroll_ui = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(area)
                            .layout(Layout::top_down(Align::Min)),
                    );
                    scroll_ui.set_clip_rect(area.intersect(ui.clip_rect()));
                    egui::ScrollArea::both()
                        .id_salt(self.texture.id())
                        .auto_shrink([false, false])
                        .show(&mut scroll_ui, |ui| {
                            let (rect, _) =
                                ui.allocate_exact_size(viewport.max(src), Sense::hover());
                            egui::Image::new(&self.texture)
                                .paint_at(ui, egui::Rect::from_center_size(rect.center(), src));
                        });
                } else {
                    egui::Image::new(&self.texture)
                        .paint_at(ui, egui::Rect::from_center_size(area.center(), src * fit));
                }
                close
            });
        modal.inner || modal.should_close()
    }
}

enum State {
    Waiting(Instant),
    Ready(TextureHandle),
    Missing,
}
struct Entry {
    state: State,
    used: Instant,
}

#[derive(Default)]
pub(super) struct Previews {
    entries: HashMap<String, Entry>,
}

impl Previews {
    pub(super) fn receive(&mut self, ctx: &egui::Context, path: &str, data: &str) {
        let Some(entry) = self.entries.get_mut(path) else {
            return;
        };
        if matches!(entry.state, State::Waiting(_)) {
            entry.state = images::preview_texture(ctx, data, 1024)
                .map(State::Ready)
                .unwrap_or(State::Missing);
        }
    }

    pub(super) fn card(
        &mut self,
        ui: &mut egui::Ui,
        artifact: &crate::artifacts::Artifact,
        connected: bool,
        p: Palette,
        request: impl FnOnce() -> bool,
    ) -> Option<Viewer> {
        let width = ui.available_width().clamp(1., 380.);
        let size = Vec2::new(width, (width * 0.56).clamp(45., 208.));
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        if !ui.is_rect_visible(rect) {
            return None;
        }
        let now = Instant::now();
        let existing = self.entries.get_mut(&artifact.path);
        let mut fetch = existing.is_none();
        let mut caption = if connected {
            "Загрузка превью…"
        } else {
            "Нет связи"
        };
        if let Some(entry) = existing {
            entry.used = now;
            if matches!(entry.state, State::Waiting(at) if now.duration_since(at) >= Duration::from_secs(15))
            {
                entry.state = State::Missing;
            }
            match &entry.state {
                State::Ready(tex) => {
                    let src = tex.size_vec2();
                    let fit = (size.x / src.x).min(size.y / src.y);
                    let dst = egui::Rect::from_center_size(rect.center(), src * fit);
                    ui.painter().rect_filled(rect, 8., p.base);
                    egui::Image::new(tex).corner_radius(6).paint_at(ui, dst);
                    return response
                        .on_hover_cursor(egui::CursorIcon::ZoomIn)
                        .on_hover_text("Открыть изображение")
                        .clicked()
                        .then(|| Viewer::new(&artifact.name, tex.clone()));
                }
                State::Missing => {
                    caption = if connected {
                        "Повторить загрузку превью"
                    } else {
                        "Нет связи"
                    };
                    fetch = response.clicked();
                }
                State::Waiting(_) => ui.ctx().request_repaint_after(Duration::from_secs(15)),
            }
        }
        if fetch && connected {
            if self.entries.len() >= 16 && !self.entries.contains_key(&artifact.path) {
                if let Some(oldest) = self
                    .entries
                    .iter()
                    .min_by_key(|(_, v)| v.used)
                    .map(|(k, _)| k.clone())
                {
                    self.entries.remove(&oldest);
                }
            }
            let state = if request() {
                State::Waiting(now)
            } else {
                State::Missing
            };
            self.entries
                .insert(artifact.path.clone(), Entry { used: now, state });
            ui.ctx().request_repaint();
        }
        ui.painter().rect_filled(rect, 8., p.base);
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            caption,
            FontId::proportional(12.),
            p.muted,
        );
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::epaint::Primitive;

    #[test]
    fn fitted_photo_uses_the_height_of_the_dialog() {
        let ctx = egui::Context::default();
        let texture = ctx.load_texture(
            "large-photo",
            egui::ColorImage::new([1340, 900], Color32::GREEN),
            egui::TextureOptions::LINEAR,
        );
        let id = texture.id();
        let mut viewer = Viewer::new("Фото.png", texture);
        let mut height = 0_f32;
        for _ in 0..4 {
            let frame = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(800., 600.),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    viewer.show(ctx, "chat", design::palette(false));
                },
            );
            for item in ctx.tessellate(frame.shapes, frame.pixels_per_point) {
                if let Primitive::Mesh(mesh) = item.primitive {
                    if mesh.texture_id == id {
                        height = mesh.calc_bounds().intersect(item.clip_rect).height();
                    }
                }
            }
        }
        assert!(
            height >= 490.,
            "expected fitted 500-point image, saw {height}"
        );
    }

    #[test]
    fn preview_fetch_is_deduplicated_and_only_requested_replies_are_decoded() {
        let ctx = egui::Context::default();
        let path = "test/shot.png";
        let artifact = crate::artifacts::Artifact {
            path: path.into(),
            name: "shot.png".into(),
        };
        let mut previews = Previews::default();
        let mut attempts = 0;
        for _ in 0..2 {
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    previews.card(ui, &artifact, true, design::palette(false), || {
                        attempts += 1;
                        true
                    });
                });
            });
        }
        assert_eq!(attempts, 1);
        // A corrupt or omitted preview leaves the original download accessible.
        previews.receive(&ctx, "another.png", "");
        assert_eq!(previews.entries.len(), 1);
        previews.receive(&ctx, path, "");
        assert!(matches!(previews.entries[path].state, State::Missing));
    }
}
