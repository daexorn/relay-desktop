use crate::ui::{Palette, ACCENT};
use eframe::egui::{self, Color32, RichText, Vec2};
use serde::Deserialize;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Clone, Default, Deserialize, Debug)]
#[serde(default)]
pub struct Usage {
    pub available: bool,
    pub windows: Vec<Window>,
    pub status: Option<String>,
    pub message: String,
    pub updated_at: f64,
}

#[derive(Clone, Default, Deserialize, Debug)]
#[serde(default)]
pub struct Window {
    pub id: String,
    pub label: String,
    pub utilization: Option<f32>,
    pub resets_at: Option<f64>,
}

fn percent(value: Option<f32>) -> Option<f32> {
    value
        .filter(|value| value.is_finite())
        .map(|value| value.clamp(0., 100.))
}

fn countdown(resets_at: Option<f64>, now: f64) -> Option<String> {
    let resets_at = resets_at.filter(|value| value.is_finite() && *value > 0.)?;
    if resets_at <= now {
        return Some("Ожидаем обновления".into());
    }
    let minutes = ((resets_at - now) / 60.).ceil() as u64;
    let days = minutes / (24 * 60);
    let hours = (minutes / 60) % 24;
    let minutes = minutes % 60;
    Some(if days > 0 {
        format!("Сброс через {days} д {hours} ч")
    } else if hours > 0 {
        format!("Сброс через {hours} ч {minutes} мин")
    } else {
        format!("Сброс через {minutes} мин")
    })
}

fn tint(value: f32) -> Color32 {
    if value >= 95. {
        Color32::from_rgb(204, 133, 138)
    } else if value >= 80. {
        Color32::from_rgb(194, 164, 109)
    } else {
        ACCENT
    }
}

fn title(window: &Window) -> String {
    match window.id.as_str() {
        "five_hour" => "5 часов".into(),
        "seven_day" => "Неделя".into(),
        "seven_day_opus" => "Неделя · Opus".into(),
        "seven_day_sonnet" => "Неделя · Sonnet".into(),
        _ if !window.label.is_empty() => window
            .label
            .chars()
            .filter(|value| !value.is_control())
            .take(24)
            .collect(),
        _ => "Лимит".into(),
    }
}

/// Shows account limits without deriving a percentage from token counts.
pub fn show(ui: &mut egui::Ui, usage: &Usage, palette: Palette, connected: bool) {
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 4.;
        ui.spacing_mut().interact_size.y = 18.;
        ui.label(RichText::new("Лимиты Claude").size(12.).color(palette.muted));
        if !connected {
            ui.label(RichText::new("Нет подключения").size(12.).color(palette.muted));
            return;
        }
        if !usage.available {
            ui.label(RichText::new("Лимиты недоступны").size(12.).color(palette.muted))
                .on_hover_text("Claude на этом сервере пока не передал данные о лимитах. Проценты появятся после их получения.");
            return;
        }

        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs_f64();
        let mut windows: Vec<_> = usage.windows.iter().collect();
        windows.sort_by_key(|window| match window.id.as_str() {
            "five_hour" => 0,
            "seven_day" => 1,
            _ => 2,
        });
        if windows.is_empty() {
            let (text, color) = match usage.status.as_deref() {
                Some("rejected") => ("Лимит достигнут", tint(100.)),
                Some("allowed_warning") => ("Лимит близко", tint(85.)),
                Some("allowed") => ("Доступно · без данных о процентах", palette.muted),
                _ => ("Ожидаем данные", palette.muted),
            };
            ui.label(RichText::new(text).size(12.).color(color));
            return;
        }

        let mut needs_countdown = false;
        for (index, window) in windows.iter().take(3).enumerate() {
            if index > 0 {
                ui.add_space(7.);
            }
            let value = percent(window.utilization);
            ui.horizontal(|ui| {
                ui.label(RichText::new(title(window)).size(12.).color(palette.text));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(value.map_or_else(|| "—".into(), |value| format!("{value:.0}%")))
                            .size(12.)
                            .color(value.map_or(palette.muted, tint)),
                    )
                    .on_hover_text("Использовано от общего лимита");
                });
            });
            let (rect, response) = ui.allocate_exact_size(
                Vec2::new(ui.available_width(), 5.),
                egui::Sense::hover(),
            );
            ui.painter().rect_filled(rect, 3., palette.line);
            if let Some(value) = value {
                let fill = egui::Rect::from_min_size(rect.min, Vec2::new(rect.width() * value / 100., rect.height()));
                if fill.width() > 0. {
                    ui.painter().rect_filled(fill, 3., tint(value));
                }
                response.on_hover_text(format!("Использовано {value:.0}% · осталось {:.0}%", 100. - value));
            } else {
                response.on_hover_text("Claude не сообщил процент использования этого лимита.");
            }
            if let Some(reset) = countdown(window.resets_at, now) {
                ui.label(RichText::new(reset).size(11.).color(palette.muted));
                needs_countdown = true;
            }
        }
        if usage.updated_at.is_finite() && usage.updated_at > 0. && now - usage.updated_at > 300. {
            ui.add_space(3.);
            ui.label(RichText::new("Данные требуют обновления").size(11.).color(palette.muted));
        }
        if needs_countdown {
            ui.ctx().request_repaint_after(Duration::from_secs(60));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentage_stays_unknown_or_clamped() {
        assert_eq!(percent(None), None);
        assert_eq!(percent(Some(f32::NAN)), None);
        assert_eq!(percent(Some(f32::INFINITY)), None);
        assert_eq!(percent(Some(-2.)), Some(0.));
        assert_eq!(percent(Some(104.)), Some(100.));
        assert_eq!(percent(Some(82.5)), Some(82.5));
    }

    #[test]
    fn reset_time_never_claims_a_reset_happened() {
        assert_eq!(countdown(None, 1000.), None);
        assert_eq!(countdown(Some(f64::NAN), 1000.), None);
        assert_eq!(
            countdown(Some(999.), 1000.).as_deref(),
            Some("Ожидаем обновления")
        );
        assert_eq!(
            countdown(Some(1001.), 1000.).as_deref(),
            Some("Сброс через 1 мин")
        );
        assert_eq!(
            countdown(Some(5500.), 1000.).as_deref(),
            Some("Сброс через 1 ч 15 мин")
        );
        assert_eq!(
            countdown(Some(181000.), 1000.).as_deref(),
            Some("Сброс через 2 д 2 ч")
        );
    }
}
