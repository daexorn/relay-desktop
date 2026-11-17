use eframe::egui::{self, Color32, FontId, Pos2, Rect, Response, Sense, Stroke, Vec2};

/// The accent belongs to the theme and glides with it; read it with [`accent`].
static ACCENT_RGB: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x8ba6f2);
pub fn accent() -> Color32 {
    let n = ACCENT_RGB.load(std::sync::atomic::Ordering::Relaxed);
    Color32::from_rgb((n >> 16) as u8, (n >> 8) as u8, n as u8)
}
pub const GREEN: Color32 = Color32::from_rgb(112, 199, 165);
pub const RED: Color32 = Color32::from_rgb(218, 116, 123);
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
#[cfg(test)]
pub fn palette(light: bool) -> Palette {
    theme_palette(if light { "paper" } else { "graphite" })
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
    let name = ctx
        .data(|d| d.get_temp::<String>(egui::Id::new("relay-theme")))
        .unwrap_or_else(|| if light { "paper" } else { "graphite" }.into());
    let b = theme_palette(&name);
    let channels = |id: &str, color: Color32| {
        let c = color.to_array();
        Color32::from_rgb(
            ctx.animate_value_with_time(
                egui::Id::new(("theme", id, 0)),
                c[0] as f32,
                duration(ctx, 0.32),
            ) as u8,
            ctx.animate_value_with_time(
                egui::Id::new(("theme", id, 1)),
                c[1] as f32,
                duration(ctx, 0.32),
            ) as u8,
            ctx.animate_value_with_time(
                egui::Id::new(("theme", id, 2)),
                c[2] as f32,
                duration(ctx, 0.32),
            ) as u8,
        )
    };
    let a = channels("accent", theme_accent(&name));
    ACCENT_RGB.store(
        u32::from_be_bytes([0, a.r(), a.g(), a.b()]),
        std::sync::atomic::Ordering::Relaxed,
    );
    Palette {
        base: channels("base", b.base),
        side: channels("side", b.side),
        rail: channels("rail", b.rail),
        surface: channels("surface", b.surface),
        line: channels("line", b.line),
        text: channels("text", b.text),
        muted: channels("muted", b.muted),
    }
}
/// Theme id, name, light or dark, and colours: base, side, rail, surface, line, text,
/// muted, accent.
pub const THEME_TABLE: [(&str, &str, bool, [u32; 8]); 13] = [
    (
        "graphite",
        "Графит",
        false,
        [
            0x10141a, 0x151b23, 0x0c1016, 0x1b242e, 0x2a3643, 0xe6edf3, 0x8d9cab, 0x83c4b3,
        ],
    ),
    (
        "midnight",
        "Полночь",
        false,
        [
            0x0f1520, 0x141d2b, 0x0b111b, 0x1b2739, 0x2b3b52, 0xe5edf9, 0x95a5bd, 0x89aef1,
        ],
    ),
    (
        "forest",
        "Хвоя",
        false,
        [
            0x111916, 0x17211c, 0x0c120f, 0x202e27, 0x2e4136, 0xe4eee8, 0x9aad9f, 0x83c6a9,
        ],
    ),
    (
        "plum",
        "Слива",
        false,
        [
            0x1b1721, 0x16121c, 0x120e18, 0x292331, 0x3b3246, 0xeee7f5, 0xab9bb8, 0xc39bff,
        ],
    ),
    (
        "ember",
        "Угли",
        false,
        [
            0x1d1816, 0x171311, 0x120f0d, 0x2a2320, 0x3d322d, 0xf3e9e4, 0xb0a097, 0xff8a5b,
        ],
    ),
    (
        "ocean",
        "Океан",
        false,
        [
            0x0f1b1e, 0x0c1619, 0x091214, 0x172a2e, 0x24393e, 0xe2f2f2, 0x8fadaf, 0x4fd1c5,
        ],
    ),
    (
        "nord",
        "Север",
        false,
        [
            0x2e3440, 0x292e39, 0x242933, 0x3b4252, 0x434c5e, 0xeceff4, 0x9aa5b8, 0x88c0d0,
        ],
    ),
    (
        "rose",
        "Роза",
        false,
        [
            0x1e1619, 0x181114, 0x130d10, 0x2c2024, 0x402e34, 0xf6e8ec, 0xb79aa3, 0xf28ba8,
        ],
    ),
    (
        "dracula",
        "Дракула",
        false,
        [
            0x282a36, 0x21222c, 0x1b1c24, 0x343746, 0x44475a, 0xf8f8f2, 0xa4a9c6, 0xbd93f9,
        ],
    ),
    (
        "oled",
        "OLED",
        false,
        [
            0x000000, 0x050506, 0x000000, 0x121214, 0x232327, 0xf2f2f4, 0x8a8a93, 0xa78bfa,
        ],
    ),
    (
        "paper",
        "Бумага",
        true,
        [
            0xfcfcfb, 0xf5f4f2, 0xedece9, 0xffffff, 0xe4e2de, 0x1e1e21, 0x78777e, 0x4a6fe3,
        ],
    ),
    (
        "sand",
        "Песок",
        true,
        [
            0xfbf7f0, 0xf4eee3, 0xece4d6, 0xfffdf9, 0xe3d8c6, 0x2b241c, 0x857865, 0xc2703d,
        ],
    ),
    (
        "mint",
        "Мята",
        true,
        [
            0xf6fbf8, 0xedf6f1, 0xe2efe8, 0xffffff, 0xd5e7dc, 0x17261e, 0x6c8478, 0x2f9e77,
        ],
    ),
];
pub const THEMES: [(&str, &str); 13] = {
    let mut out = [("", ""); 13];
    let mut i = 0;
    while i < 13 {
        out[i] = (THEME_TABLE[i].0, THEME_TABLE[i].1);
        i += 1;
    }
    out
};
fn theme_row(name: &str) -> &'static (&'static str, &'static str, bool, [u32; 8]) {
    THEME_TABLE
        .iter()
        .find(|t| t.0 == name)
        .unwrap_or(&THEME_TABLE[0])
}
fn rgb(n: u32) -> Color32 {
    Color32::from_rgb((n >> 16) as u8, (n >> 8) as u8, n as u8)
}
pub fn is_light(name: &str) -> bool {
    theme_row(name).2
}
pub fn theme_accent(name: &str) -> Color32 {
    rgb(theme_row(name).3[7])
}
pub fn theme_palette(name: &str) -> Palette {
    let c = theme_row(name).3;
    Palette {
        base: rgb(c[0]),
        side: rgb(c[1]),
        rail: rgb(c[2]),
        surface: rgb(c[3]),
        line: rgb(c[4]),
        text: rgb(c[5]),
        muted: rgb(c[6]),
    }
}
pub fn duration(ctx: &egui::Context, seconds: f32) -> f32 {
    if ctx
        .data(|d| d.get_temp::<bool>(egui::Id::new("relay-animations")))
        .unwrap_or(true)
    {
        seconds
    } else {
        0.
    }
}
pub fn chevron(p: &egui::Painter, rect: Rect, open: f32, color: Color32) {
    let rotation = egui::emath::Rot2::from_angle(open * std::f32::consts::FRAC_PI_2);
    let pts = [
        Vec2::new(-2.5, -4.),
        Vec2::new(1.5, 0.),
        Vec2::new(-2.5, 4.),
    ]
    .map(|v| rect.center() + rotation * v)
    .to_vec();
    p.add(egui::Shape::line(pts, Stroke::new(1.5_f32, color)));
}
pub fn combo_icon(
    ui: &egui::Ui,
    rect: Rect,
    visuals: &egui::style::WidgetVisuals,
    open: bool,
    _: egui::AboveOrBelow,
) {
    let t = ui
        .ctx()
        .animate_bool_with_time(rect_id(rect), open, duration(ui.ctx(), 0.22));
    chevron(ui.painter(), rect, 1. + 2. * t, visuals.fg_stroke.color);
}
fn rect_id(rect: Rect) -> egui::Id {
    egui::Id::new(("dropdown-chevron", rect.min.x as i32, rect.min.y as i32))
}
#[derive(Clone, Copy)]
pub enum Icon {
    Browser,
    Key,
    Back,
    Forward,
    Expand,
    Collapse,
    Chat,
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
    Trash,
    Check,
    Arrow,
    Server,
    Computer,
    Board,
    Cluster,
    Terminal,
    Pencil,
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
        Icon::Browser => {
            p.circle_stroke(c, 8. * z, s);
            line((-8., 0.), (8., 0.));
            path(&[(0., -8.), (-3., -4.), (-4., 0.), (-3., 4.), (0., 8.)]);
            path(&[(0., -8.), (3., -4.), (4., 0.), (3., 4.), (0., 8.)]);
        }
        Icon::Key => {
            p.circle_stroke(v(-3., -3.), 4.5 * z, s);
            path(&[(0., 0.), (7., 7.), (9., 5.)]);
            line((4., 4.), (6., 2.));
        }
        Icon::Back => {
            path(&[(1., -6.), (-5., 0.), (1., 6.)]);
            line((-5., 0.), (8., 0.));
        }
        Icon::Forward => {
            path(&[(-1., -6.), (5., 0.), (-1., 6.)]);
            line((-8., 0.), (5., 0.));
        }
        Icon::Expand => {
            path(&[(-8., -2.), (-8., -8.), (-2., -8.)]);
            path(&[(2., 8.), (8., 8.), (8., 2.)]);
            line((-8., -8.), (-2., -2.));
            line((8., 8.), (2., 2.));
        }
        Icon::Collapse => {
            path(&[(-8., -2.), (-2., -2.), (-2., -8.)]);
            path(&[(2., 8.), (2., 2.), (8., 2.)]);
        }
        Icon::Chat => {
            path(&[
                (-8., -6.),
                (8., -6.),
                (8., 5.),
                (-2., 5.),
                (-7., 9.),
                (-7., 5.),
                (-8., 5.),
                (-8., -6.),
            ]);
            line((-4., -2.), (4., -2.));
            line((-4., 1.), (1., 1.));
        }
        Icon::Cluster => {
            for (x, y) in [(0., -6.), (-6., 5.), (6., 5.)] {
                p.circle_stroke(v(x, y), 3. * z, s);
                p.line_segment([v(x * 0.55, y * 0.55), v(0., 0.)], s);
            }
        }
        Icon::Plus => {
            line((-6., 0.), (6., 0.));
            line((0., -6.), (0., 6.));
        }
        Icon::Close => {
            line((-4., -4.), (4., 4.));
            line((-4., 4.), (4., -4.));
        }
        Icon::Trash => {
            line((-7., -5.), (7., -5.));
            path(&[(-3., -5.), (-3., -8.), (3., -8.), (3., -5.)]);
            path(&[(-5., -5.), (-4., 8.), (4., 8.), (5., -5.)]);
            line((-2., -2.), (-2., 5.));
            line((2., -2.), (2., 5.));
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
        Icon::Terminal => {
            p.rect_stroke(
                Rect::from_center_size(c, Vec2::new(18. * z, 15. * z)),
                3. * z,
                s,
                egui::StrokeKind::Middle,
            );
            path(&[(-5., -2.5), (-2., 0.5), (-5., 3.5)]);
            line((0., 3.5), (5., 3.5));
        }
        Icon::Pencil => {
            path(&[
                (-7., 7.),
                (-6., 2.5),
                (4., -7.5),
                (7.5, -4.),
                (-2.5, 6.),
                (-7., 7.),
            ]);
            line((1.5, -5.), (5., -1.5));
        }
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
            for (y, x) in [(-5., 2.), (0., -3.), (5., 3.)] {
                line((-8., y), (8., y));
                p.circle_filled(v(x, y), 2.6 * z, color);
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
pub fn action_button(ui: &mut egui::Ui, which: Icon, p: Palette, primary: bool) -> Response {
    let response = button(
        ui,
        "",
        Some(which),
        if primary {
            Kind::Primary
        } else {
            Kind::Secondary
        },
        p,
        Vec2::splat(34.),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            ui.is_enabled(),
            if primary {
                "Отправить"
            } else {
                "Остановить"
            },
        )
    });
    response
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
        duration(ui.ctx(), 0.20),
        egui::emath::easing::cubic_in_out,
    );
    let press = ui.ctx().animate_bool_with_time(
        response.id.with("press"),
        response.is_pointer_button_down_on(),
        duration(ui.ctx(), 0.10),
    );
    let painted = rect.shrink(press * 0.7);
    ui.painter()
        .rect_filled(painted, 7., mix(p.surface, p.line, 0.3 + hover * 0.45));
    ui.painter().rect_stroke(
        painted,
        7.,
        Stroke::new(1.0_f32, mix(p.line, accent(), hover * 0.3)),
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
            Stroke::new(1.0_f32, accent()),
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
    let font = FontId::proportional(13.0);
    let galley = ui.painter().layout_no_wrap(text.into(), font, p.text);
    let icon_size = if text.is_empty() { 17. } else { 15. };
    let gap = if which.is_some() && !text.is_empty() {
        7.
    } else {
        0.
    };
    let inner = galley.size().x + gap + if which.is_some() { icon_size } else { 0. };
    let width = if size.x > 0. { size.x } else { inner + 26. };
    let (r, response) = ui.allocate_exact_size(Vec2::new(width, size.y.max(28.)), Sense::click());
    let hovered = ui.ctx().animate_bool_with_time_and_easing(
        response.id.with("hover"),
        response.hovered(),
        duration(ui.ctx(), 0.16),
        egui::emath::easing::cubic_in_out,
    );
    let pressed = ui.ctx().animate_bool_with_time(
        response.id.with("press"),
        response.is_pointer_button_down_on(),
        duration(ui.ctx(), 0.10),
    );
    let active = ui.is_enabled();
    let (base, over) = match kind {
        Kind::Primary if active => (accent(), mix(accent(), Color32::WHITE, 0.1)),
        Kind::Primary => (p.line, p.line),
        Kind::Secondary => (p.surface, mix(p.surface, p.line, 0.8)),
        Kind::Ghost => (
            Color32::TRANSPARENT,
            mix(Color32::TRANSPARENT, p.line, 0.75),
        ),
    };
    let painted = r.shrink(pressed * 1.);
    let radius = (r.height() / 2.).min(8.);
    ui.painter()
        .rect_filled(painted, radius, mix(base, over, hovered));
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
            Stroke::new(1.2_f32, accent().gamma_multiply(0.7)),
            egui::StrokeKind::Outside,
        );
    }
    let color = if !active {
        p.muted
    } else if kind == Kind::Primary {
        on_accent()
    } else if kind == Kind::Ghost && text.is_empty() {
        mix(p.muted, p.text, hovered)
    } else {
        p.text
    };
    let mut x = r.center().x - inner.min(r.width() - 16.) / 2.;
    if let Some(which) = which {
        icon(
            ui.painter(),
            Rect::from_center_size(
                Pos2::new(x + icon_size / 2., r.center().y),
                Vec2::splat(icon_size),
            ),
            which,
            color,
        );
        x += icon_size + gap;
    }
    if !text.is_empty() {
        ui.painter()
            .with_clip_rect(painted.shrink2(Vec2::new(8., 0.)))
            .galley_with_override_text_color(
                Pos2::new(x, r.center().y - galley.size().y / 2.),
                galley,
                color,
            );
    }
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), text));
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn on_accent() -> Color32 {
    let c = accent();
    if c.r() as u32 * 3 + c.g() as u32 * 6 + c.b() as u32 > 1350 {
        Color32::from_rgb(15, 30, 28)
    } else {
        Color32::WHITE
    }
}

/// ComboBox builds a child row before allocating its final width. Start a new
/// row explicitly so its selected value never spills out of a wrapped toolbar.
pub fn wrap_control(ui: &mut egui::Ui, text: &str) {
    let width = ui
        .painter()
        .layout_no_wrap(
            text.into(),
            FontId::proportional(12.5),
            Color32::PLACEHOLDER,
        )
        .size()
        .x
        + ui.spacing().icon_width
        + ui.spacing().icon_spacing
        + 2. * ui.spacing().button_padding.x;
    if ui.layout().is_horizontal()
        && ui.layout().main_wrap
        && width > ui.available_rect_before_wrap().width()
    {
        ui.end_row();
    }
}

pub fn service_action(
    ui: &mut egui::Ui,
    text: &str,
    which: Icon,
    active: bool,
    p: Palette,
) -> Response {
    let (r, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 36.), Sense::click());
    let hover = ui.ctx().animate_bool_with_time(
        response.id.with("hover"),
        response.hovered(),
        duration(ui.ctx(), 0.16),
    );
    let fill = if active {
        mix(p.surface, accent(), 0.13)
    } else {
        mix(p.surface, p.line, 0.55 * hover)
    };
    ui.painter().rect_filled(r, 8., fill);
    icon(
        ui.painter(),
        Rect::from_center_size(egui::pos2(r.left() + 18., r.center().y), Vec2::splat(18.)),
        which,
        if active {
            accent()
        } else {
            mix(p.muted, p.text, hover)
        },
    );
    ui.painter().text(
        egui::pos2(r.left() + 36., r.center().y),
        egui::Align2::LEFT_CENTER,
        text,
        FontId::proportional(13.),
        p.text,
    );
    chevron(
        ui.painter(),
        Rect::from_center_size(r.right_center() - Vec2::new(16., 0.), Vec2::splat(14.)),
        0.,
        p.muted,
    );
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), text));
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn badge(ui: &mut egui::Ui, text: &str, color: Color32, p: Palette) {
    let galley = ui
        .painter()
        .layout_no_wrap(text.into(), FontId::proportional(11.), color);
    let (r, _) = ui.allocate_exact_size(Vec2::new(galley.size().x + 18., 24.), Sense::hover());
    ui.painter().rect_filled(r, 6., mix(p.surface, color, 0.1));
    ui.painter().galley(
        egui::pos2(r.left() + 9., r.center().y - galley.size().y * 0.5),
        galley,
        color,
    );
}

pub fn starter_card(
    ui: &mut egui::Ui,
    title: &str,
    detail: &str,
    which: Icon,
    p: Palette,
) -> Response {
    let (r, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 64.), Sense::click());
    let t = ui.ctx().animate_bool_with_time(
        response.id.with("hover"),
        response.hovered(),
        duration(ui.ctx(), 0.16),
    );
    ui.painter().rect_filled(r, 10., mix(p.side, p.surface, t));
    ui.painter().rect_stroke(
        r,
        10.,
        Stroke::new(1.0_f32, mix(p.line, accent(), t * 0.35)),
        egui::StrokeKind::Inside,
    );
    let tile = Rect::from_center_size(egui::pos2(r.left() + 30., r.center().y), Vec2::splat(34.));
    ui.painter().rect_filled(tile, 8., p.surface);
    icon(
        ui.painter(),
        tile.shrink(8.),
        which,
        mix(p.muted, accent(), t),
    );
    let painter = ui.painter().with_clip_rect(r.shrink2(Vec2::new(12., 0.)));
    painter.text(
        egui::pos2(r.left() + 58., r.top() + 21.),
        egui::Align2::LEFT_CENTER,
        title,
        FontId::proportional(13.5),
        p.text,
    );
    painter.text(
        egui::pos2(r.left() + 58., r.top() + 43.),
        egui::Align2::LEFT_CENTER,
        detail,
        FontId::proportional(11.5),
        p.muted,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn preference(
    ui: &mut egui::Ui,
    value: &mut bool,
    title: &str,
    detail: &str,
    p: Palette,
) -> Response {
    let (r, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 54.), Sense::click());
    if response.clicked() {
        *value = !*value;
    }
    let t = ui.ctx().animate_bool_with_time(
        response.id.with("switch"),
        *value,
        duration(ui.ctx(), 0.18),
    );
    let painter = ui
        .painter()
        .with_clip_rect(Rect::from_min_max(r.min, r.max - Vec2::new(60., 0.)));
    painter.text(
        egui::pos2(r.left(), r.top() + 17.),
        egui::Align2::LEFT_CENTER,
        title,
        FontId::proportional(13.),
        p.text,
    );
    painter.text(
        egui::pos2(r.left(), r.top() + 37.),
        egui::Align2::LEFT_CENTER,
        detail,
        FontId::proportional(11.5),
        p.muted,
    );
    let track = Rect::from_center_size(r.right_center() - Vec2::new(20., 0.), Vec2::new(34., 20.));
    ui.painter()
        .rect_filled(track, 10., mix(p.line, accent(), t));
    ui.painter().circle_filled(
        egui::pos2(track.left() + 10. + 14. * t, track.center().y),
        7.,
        mix(p.muted, on_accent(), t),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Checkbox, ui.is_enabled(), title)
    });
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn card(p: Palette) -> egui::Frame {
    egui::Frame::new()
        .fill(p.surface)
        .corner_radius(12.)
        .stroke(Stroke::new(1.0_f32, mix(p.side, p.line, 0.7)))
        .inner_margin(16)
}
pub fn mark(ui: &mut egui::Ui, size: f32) {
    let (r, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    icon(ui.painter(), r, Icon::Server, accent());
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
        .corner_radius(9.)
        .stroke(Stroke::new(1.0_f32, mix(p.line, accent(), t)))
        .inner_margin(egui::Margin::symmetric(12, 9))
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
    ui.painter()
        .rect_filled(track, 10., mix(p.line, accent(), t));
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
/// A centred column whose width comes from the window, not from what is inside: a wide
/// line of output can never make the whole chat wider than the screen.
pub fn column(ui: &mut egui::Ui, visible: f32, max_width: f32, add: impl FnOnce(&mut egui::Ui)) {
    let w = (visible - 40.).min(max_width).max(120.);
    let gap = ((visible - w) * 0.5).max(0.);
    ui.horizontal(|ui| {
        ui.add_space(gap);
        ui.vertical(|ui| {
            ui.set_width(w);
            ui.set_max_width(w);
            let clip = ui.clip_rect().intersect(egui::Rect::from_min_size(
                ui.min_rect().min - egui::vec2(8., 10000.),
                egui::vec2(w + 16., f32::INFINITY),
            ));
            ui.set_clip_rect(clip);
            add(ui);
        });
    });
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
    let mut job =
        egui::text::LayoutJob::simple(text.into(), FontId::proportional(13.5), color, width);
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

/// Inter for text, the full Noto Emoji set behind it: egui's bundled emoji font stops at
/// older Unicode versions, so newer emoji used to come out as boxes.
pub fn fonts() -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "inter".into(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/InterVariable.ttf"
        ))),
    );
    fonts.font_data.insert(
        "noto-emoji".into(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/NotoEmoji.ttf"
        ))),
    );
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        let list = fonts.families.entry(family.clone()).or_default();
        if family == egui::FontFamily::Proportional {
            list.insert(0, "inter".into());
        }
        // Before egui's own, older emoji font, after the text fonts.
        let at = list
            .iter()
            .position(|f| f.contains("Emoji") || f.contains("emoji"))
            .unwrap_or(list.len());
        list.insert(at, "noto-emoji".into());
    }
    fonts
}

#[cfg(test)]
mod font_tests {
    use super::*;

    #[test]
    fn modern_emoji_have_glyphs() {
        let ctx = egui::Context::default();
        ctx.set_fonts(fonts());
        let _ = ctx.run(Default::default(), |_| {});
        let font = egui::FontId::proportional(14.);
        // Unicode 6 to 15: classics, skin tones are separate, newer faces and objects.
        for emoji in [
            "😀", "🚀", "🔥", "✅", "❤", "🤖", "🥲", "🫠", "🫡", "🪄", "🧪", "🦀", "🛠", "📌", "🗂",
            "⚓", "🐉",
        ] {
            assert!(
                ctx.fonts(|f| f.has_glyphs(&font, emoji)),
                "no glyph for {emoji}"
            );
        }
        assert!(ctx.fonts(|f| f.has_glyphs(&egui::FontId::monospace(13.), "🦀✅")));
    }
}
