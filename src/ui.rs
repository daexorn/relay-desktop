use eframe::egui::{self, Color32, FontId, Pos2, Rect, Response, Sense, Stroke, Vec2};

pub const ACCENT: Color32 = Color32::from_rgb(217, 119, 87);
pub const GREEN: Color32 = Color32::from_rgb(92, 184, 132);
#[derive(Clone, Copy)]
pub struct Palette {
    pub base: Color32,
    pub side: Color32,
    pub rail: Color32,
    pub surface: Color32,
    pub line: Color32,
    pub text: Color32,
    pub muted: Color32,
}
pub fn palette(light: bool) -> Palette {
    if light {
        Palette {
            base: Color32::from_rgb(252, 252, 251),
            side: Color32::from_rgb(245, 244, 242),
            rail: Color32::from_rgb(237, 236, 233),
            surface: Color32::WHITE,
            line: Color32::from_rgb(228, 226, 222),
            text: Color32::from_rgb(30, 30, 33),
            muted: Color32::from_rgb(120, 119, 126),
        }
    } else {
        Palette {
            base: Color32::from_rgb(25, 25, 27),
            side: Color32::from_rgb(19, 19, 21),
            rail: Color32::from_rgb(14, 14, 16),
            surface: Color32::from_rgb(34, 34, 37),
            line: Color32::from_rgb(46, 46, 50),
            text: Color32::from_rgb(236, 236, 238),
            muted: Color32::from_rgb(142, 142, 150),
        }
    }
}
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let a = a.to_array();
    let b = b.to_array();
    Color32::from_rgba_unmultiplied(
        (a[0] as f32 + (b[0] as f32 - a[0] as f32) * t) as u8,
        (a[1] as f32 + (b[1] as f32 - a[1] as f32) * t) as u8,
        (a[2] as f32 + (b[2] as f32 - a[2] as f32) * t) as u8,
        (a[3] as f32 + (b[3] as f32 - a[3] as f32) * t) as u8,
    )
}
pub fn animated_palette(ctx: &egui::Context, light: bool) -> Palette {
    let t = ctx.animate_bool_with_time_and_easing(
        egui::Id::new("theme-transition"),
        light,
        0.36,
        egui::emath::easing::cubic_in_out,
    );
    let a = palette(false);
    let b = palette(true);
    Palette {
        base: mix(a.base, b.base, t),
        side: mix(a.side, b.side, t),
        rail: mix(a.rail, b.rail, t),
        surface: mix(a.surface, b.surface, t),
        line: mix(a.line, b.line, t),
        text: mix(a.text, b.text, t),
        muted: mix(a.muted, b.muted, t),
    }
}
#[derive(Clone, Copy)]
pub enum Icon {
    Plus,
    Settings,
    Search,
    Folder,
    Sun,
    Moon,
    Attach,
    Send,
    Stop,
    Copy,
    Refresh,
    File,
    Close,
    Check,
    Arrow,
    Server,
    Computer,
    Board,
}
pub fn icon(p: &egui::Painter, r: Rect, which: Icon, color: Color32) {
    let c = r.center();
    let z = r.width().min(r.height()) / 24.0;
    let v = |x: f32, y: f32| c + Vec2::new(x * z, y * z);
    let s = Stroke::new(1.65 * z, color);
    let line = |a: (f32, f32), b: (f32, f32)| {
        p.line_segment([v(a.0, a.1), v(b.0, b.1)], s);
    };
    let path = |points: &[(f32, f32)]| {
        p.add(egui::Shape::line(
            points.iter().map(|&(x, y)| v(x, y)).collect(),
            s,
        ));
    };
    match which {
        Icon::Plus => {
            line((-6., 0.), (6., 0.));
            line((0., -6.), (0., 6.));
        }
        Icon::Close => {
            line((-4., -4.), (4., 4.));
            line((-4., 4.), (4., -4.));
        }
        Icon::Search => {
            p.circle_stroke(v(-1.5, -1.5), 5.5 * z, s);
            line((3., 3.), (7., 7.));
        }
        Icon::Send => {
            path(&[(-5., -1.), (0., -6.), (5., -1.)]);
            line((0., -6.), (0., 6.));
        }
        Icon::Stop => {
            p.rect_filled(Rect::from_center_size(c, Vec2::splat(10. * z)), 2., color);
        }
        Icon::Check => path(&[(-6., 0.), (-2., 4.), (6., -5.)]),
        Icon::Arrow => {
            line((-6., 0.), (6., 0.));
            path(&[(1., -5.), (6., 0.), (1., 5.)]);
        }
        Icon::Attach => {
            path(&[
                (-2., 5.),
                (5., -2.),
                (5., -5.),
                (2., -7.),
                (-1., -6.),
                (-7., 0.),
                (-7., 4.),
                (-4., 7.),
                (0., 7.),
                (7., 0.),
            ]);
            path(&[(-4., 2.), (2., -4.), (3., -3.), (-3., 3.)]);
        }
        Icon::Folder => path(&[
            (-8., -5.),
            (-3., -5.),
            (-1., -2.),
            (8., -2.),
            (7., 6.),
            (-7., 6.),
            (-8., -5.),
        ]),
        Icon::File => {
            path(&[
                (-6., -8.),
                (2., -8.),
                (7., -3.),
                (7., 8.),
                (-6., 8.),
                (-6., -8.),
            ]);
            path(&[(2., -8.), (2., -3.), (7., -3.)]);
            line((-3., 1.), (3., 1.));
            line((-3., 4.), (2., 4.));
        }
        Icon::Copy => {
            p.rect_stroke(
                Rect::from_min_max(v(-3., -3.), v(7., 8.)),
                3.,
                s,
                egui::StrokeKind::Inside,
            );
            path(&[(-7., 4.), (-7., -7.), (3., -7.)]);
        }
        Icon::Server => {
            for y in [-4., 4.] {
                p.rect_stroke(
                    Rect::from_center_size(v(0., y), Vec2::new(16., 6.) * z),
                    2.,
                    s,
                    egui::StrokeKind::Inside,
                );
                p.circle_filled(v(-4., y), 0.8 * z, color);
            }
        }
        Icon::Computer => {
            p.rect_stroke(
                Rect::from_min_max(v(-8., -7.), v(8., 4.)),
                2.,
                s,
                egui::StrokeKind::Inside,
            );
            line((0., 4.), (0., 8.));
            line((-5., 8.), (5., 8.));
        }
        Icon::Board => {
            for (x, h) in [(-6., 12.), (0., 8.), (6., 10.)] {
                p.rect_stroke(
                    Rect::from_min_size(v(x - 2., -7.), Vec2::new(4., h) * z),
                    1.,
                    s,
                    egui::StrokeKind::Inside,
                );
            }
        }
        Icon::Settings => {
            for x in [-5., 0., 5.] {
                p.circle_filled(v(x, 0.), 1.3 * z, color);
            }
        }
        Icon::Sun => {
            p.circle_stroke(c, 4. * z, s);
            for i in 0..8 {
                let t = i as f32 * std::f32::consts::TAU / 8.;
                let d = Vec2::angled(t);
                p.line_segment([c + d * 7. * z, c + d * 9. * z], s);
            }
        }
        Icon::Moon => {
            let pts: Vec<_> = (0..=20)
                .map(|i| {
                    let t = 0.4 + i as f32 / 20. * 5.5;
                    v(t.cos() * 7., t.sin() * 7.)
                })
                .collect();
            p.add(egui::Shape::line(pts, s));
            path(&[(6., -3.), (1., -1.), (-1., 3.), (2., 7.)]);
        }
        Icon::Refresh => {
            let pts: Vec<_> = (0..=20)
                .map(|i| {
                    let t = 0.3 + i as f32 / 20. * 5.;
                    v(t.cos() * 6., t.sin() * 6.)
                })
                .collect();
            p.add(egui::Shape::line(pts, s));
            path(&[(1., -7.), (5., -6.), (5., -2.)]);
        }
    }
}
#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Primary,
    Secondary,
    Ghost,
}
pub fn chip(ui: &mut egui::Ui, text: &str, which: Icon, p: Palette, active: bool) -> Response {
    let font = FontId::proportional(12.5);
    let galley = ui.painter().layout_no_wrap(text.into(), font, p.text);
    let width = if text.is_empty() {
        28.
    } else {
        galley.size().x + 38.
    };
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(width.min(ui.available_width()), 28.),
        Sense::click(),
    );
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), text));
    let hover = ui.ctx().animate_bool_with_time_and_easing(
        response.id.with("hover"),
        response.hovered(),
        0.20,
        egui::emath::easing::cubic_in_out,
    );
    let press = ui.ctx().animate_bool_with_time(
        response.id.with("press"),
        response.is_pointer_button_down_on(),
        0.10,
    );
    let painted = rect.shrink(press * 0.7);
    ui.painter()
        .rect_filled(painted, 14., mix(Color32::TRANSPARENT, p.line, 0.35 + hover * 0.45));
    ui.painter().rect_stroke(
        painted,
        14.,
        Stroke::new(1.0_f32, mix(p.line, ACCENT, hover * 0.4)),
        egui::StrokeKind::Inside,
    );
    let icon_color = if active {
        GREEN
    } else {
        mix(p.muted, p.text, hover * 0.5)
    };
    let center = if text.is_empty() {
        rect.center()
    } else {
        egui::pos2(rect.left() + 16., rect.center().y)
    };
    icon(
        ui.painter(),
        Rect::from_center_size(center, Vec2::splat(15.)),
        which,
        icon_color,
    );
    if !text.is_empty() {
        ui.painter().galley(
            egui::pos2(rect.left() + 28., rect.center().y - galley.size().y / 2.),
            galley,
            if active { p.text } else { p.muted },
        );
    }
    if response.has_focus() {
        ui.painter().rect_stroke(
            painted.expand(2.),
            16.,
            Stroke::new(1.0_f32, ACCENT),
            egui::StrokeKind::Outside,
        );
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
pub fn button(
    ui: &mut egui::Ui,
    text: &str,
    which: Option<Icon>,
    kind: Kind,
    p: Palette,
    size: Vec2,
) -> Response {
    let font = FontId::proportional(13.5);
    let galley = ui.painter().layout_no_wrap(text.into(), font, p.text);
    let icon_size = if text.is_empty() { 17. } else { 15. };
    let gap = if which.is_some() && !text.is_empty() { 7. } else { 0. };
    let inner = galley.size().x + gap + if which.is_some() { icon_size } else { 0. };
    let width = if size.x > 0. { size.x } else { inner + 24. };
    let (r, response) = ui.allocate_exact_size(Vec2::new(width, size.y.max(28.)), Sense::click());
    let hovered = ui.ctx().animate_bool_with_time_and_easing(
        response.id.with("hover"),
        response.hovered(),
        0.16,
        egui::emath::easing::cubic_in_out,
    );
    let pressed = ui.ctx().animate_bool_with_time(
        response.id.with("press"),
        response.is_pointer_button_down_on(),
        0.10,
    );
    let active = ui.is_enabled();
    let (base, over) = match kind {
        Kind::Primary if active => (ACCENT, mix(ACCENT, Color32::WHITE, 0.14)),
        Kind::Primary => (p.line, p.line),
        Kind::Secondary => (p.surface, mix(p.surface, p.line, 0.8)),
        Kind::Ghost => (Color32::TRANSPARENT, mix(Color32::TRANSPARENT, p.line, 0.75)),
    };
    let painted = r.shrink(pressed * 1.);
    let radius = (r.height() / 2.).min(9.);
    ui.painter().rect_filled(painted, radius, mix(base, over, hovered));
    if kind == Kind::Secondary {
        ui.painter().rect_stroke(
            painted,
            radius,
            Stroke::new(1.0_f32, p.line),
            egui::StrokeKind::Inside,
        );
    }
    if response.has_focus() {
        ui.painter().rect_stroke(
            painted.expand(1.5),
            radius + 1.5,
            Stroke::new(1.2_f32, ACCENT.gamma_multiply(0.7)),
            egui::StrokeKind::Outside,
        );
    }
    let color = if !active {
        p.muted
    } else if kind == Kind::Primary {
        Color32::WHITE
    } else if kind == Kind::Ghost && text.is_empty() {
        mix(p.muted, p.text, hovered)
    } else {
        p.text
    };
    let mut x = r.center().x - inner.min(r.width() - 16.) / 2.;
    if let Some(which) = which {
        icon(
            ui.painter(),
            Rect::from_center_size(Pos2::new(x + icon_size / 2., r.center().y), Vec2::splat(icon_size)),
            which,
            color,
        );
        x += icon_size + gap;
    }
    if !text.is_empty() {
        ui.painter()
            .galley(Pos2::new(x, r.center().y - galley.size().y / 2.), galley, color);
    }
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), text));
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
pub fn mark(ui: &mut egui::Ui, size: f32) {
    let (r, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    icon(ui.painter(), r, Icon::Server, ACCENT);
}
pub fn field(ui: &mut egui::Ui, value: &mut String, hint: &str, password: bool) -> Response {
    let p = animated_palette(ui.ctx(), !ui.visuals().dark_mode);
    let id = ui.next_auto_id();
    let focused = ui.memory(|m| m.focused() == Some(id));
    let t = ui
        .ctx()
        .animate_bool_with_time(id.with("field-focus"), focused, 0.24);
    egui::Frame::new()
        .fill(p.base)
        .corner_radius(8.)
        .stroke(Stroke::new(1.0_f32, mix(p.line, ACCENT, t)))
        .inner_margin(egui::Margin::symmetric(11, 7))
        .show(ui, |ui| {
            ui.add(
                egui::TextEdit::singleline(value)
                    .id(id)
                    .frame(false)
                    .hint_text(egui::RichText::new(hint).color(p.muted))
                    .password(password)
                    .desired_width(f32::INFINITY)
                    .font(FontId::proportional(14.0)),
            )
        })
        .inner
}
pub fn dots(ui: &mut egui::Ui, color: Color32) {
    let (r, _) = ui.allocate_exact_size(Vec2::new(30., 16.), Sense::hover());
    let time = ui.input(|i| i.time) as f32;
    for i in 0..3 {
        let t = (time * 4. - i as f32 * 0.7).sin() * 0.5 + 0.5;
        ui.painter().circle_filled(
            egui::pos2(r.left() + 5. + i as f32 * 9., r.center().y),
            2.3,
            color.gamma_multiply(0.35 + t * 0.65),
        );
    }
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(16));
}
pub fn toggle(ui: &mut egui::Ui, value: &mut bool, text: &str, p: Palette) -> Response {
    let width = ui
        .painter()
        .layout_no_wrap(text.into(), FontId::proportional(13.), p.text)
        .size()
        .x
        + 48.;
    let (r, response) = ui.allocate_exact_size(Vec2::new(width, 30.), Sense::click());
    if response.clicked() {
        *value = !*value;
    }
    let t = ui.ctx().animate_bool_with_time(response.id, *value, 0.24);
    let track = Rect::from_center_size(
        egui::pos2(r.left() + 16., r.center().y),
        Vec2::new(32., 19.),
    );
    ui.painter().rect_filled(track, 10., mix(p.line, ACCENT, t));
    ui.painter().circle_filled(
        egui::pos2(track.left() + 9.5 + t * 13., track.center().y),
        6.5,
        if *value { Color32::WHITE } else { p.muted },
    );
    ui.painter().text(
        egui::pos2(r.left() + 43., r.center().y),
        egui::Align2::LEFT_CENTER,
        text,
        FontId::proportional(13.),
        p.muted,
    );
    response
}
pub fn content(ui: &mut egui::Ui, max_width: f32, add: impl FnOnce(&mut egui::Ui)) {
    let w = ui.available_width().min(max_width);
    let gap = ((ui.available_width() - w) * 0.5).max(0.);
    ui.horizontal(|ui| {
        ui.add_space(gap);
        ui.vertical(|ui| {
            ui.set_width(w);
            add(ui);
        });
    });
}

pub fn session_title(ui: &mut egui::Ui, text: &str, color: Color32) {
    let width = ui.available_width();
    let mut job = egui::text::LayoutJob::simple(text.into(), FontId::proportional(13.5), color, width);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('…');
    let galley = ui.painter().layout_job(job);
    let (r, response) = ui.allocate_exact_size(Vec2::new(width, galley.size().y), Sense::hover());
    ui.painter().galley(r.min, galley, color);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, text));
}
#[derive(Clone)]
struct Motion {
    from: f32,
    to: f32,
    start: f64,
}
pub fn glide(ctx: &egui::Context, id: egui::Id, target: f32) -> f32 {
    let now = ctx.input(|i| i.time);
    let mut motion = ctx
        .data_mut(|d| d.get_temp::<Motion>(id))
        .unwrap_or(Motion {
            from: target,
            to: target,
            start: now,
        });
    let position = |m: &Motion| {
        let t = ((now - m.start) / 0.30).clamp(0., 1.) as f32;
        m.from + (m.to - m.from) * egui::emath::easing::cubic_in_out(t)
    };
    let current = position(&motion);
    if (motion.to - target).abs() > 0.01 {
        motion = Motion {
            from: current,
            to: target,
            start: now,
        };
    }
    let current = position(&motion);
    if now - motion.start < 0.30 && (motion.from - motion.to).abs() > 0.01 {
        ctx.request_repaint();
    }
    ctx.data_mut(|d| d.insert_temp(id, motion));
    current
}
