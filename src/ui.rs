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
pub use crate::model::THEME_SPECS as THEME_TABLE;

pub const THEMES: [(&str, &str); THEME_TABLE.len()] = {
    let mut out = [("", ""); THEME_TABLE.len()];
    let mut i = 0;
    while i < THEME_TABLE.len() {
        out[i] = (THEME_TABLE[i].0, THEME_TABLE[i].1);
        i += 1;
    }
    out
};
fn theme_row(name: &str) -> &'static (&'static str, &'static str, bool, [u32; 8]) {
    crate::model::theme_spec(name)
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
#[derive(Clone, Copy, Debug)]
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
    Lock,
    Unlock,
    Download,
    Orbit,
    Spark,
    Pulse,
    Bolt,
    Radar,
    Layers,
}
pub fn icon(p: &egui::Painter, r: Rect, which: Icon, color: Color32) {
    let c = r.center();
    let z = r.width().min(r.height()) / 24.0;
    let v = |x: f32, y: f32| c + Vec2::new(x * z, y * z);
    let s = Stroke::new(1.95 * z, color);
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
            for (y, knob_x) in [(-3.8_f32, 2.2_f32), (3.8_f32, -2.2_f32)] {
                line((-6.8, y), (knob_x - 2.3, y));
                line((knob_x + 2.3, y), (6.8, y));
                p.circle_stroke(v(knob_x, y), 2.1 * z, s);
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
            let outer: Vec<_> = (0..=18)
                .map(|i| {
                    let t = 0.55 + i as f32 / 18. * 4.95;
                    v(t.cos() * 6.8, t.sin() * 6.8)
                })
                .collect();
            let inner: Vec<_> = (0..=14)
                .map(|i| {
                    let t = -0.85 - i as f32 / 14. * 2.95;
                    v(2.7 + t.cos() * 5.4, -1.6 + t.sin() * 5.4)
                })
                .collect();
            p.add(egui::Shape::line(outer, s));
            p.add(egui::Shape::line(inner, s));
        }
        Icon::Refresh => {
            let start = -0.65_f32;
            let end = 4.55_f32;
            let r_arc = 5.8_f32;
            let pts: Vec<_> = (0..=20)
                .map(|i| {
                    let t = start + i as f32 / 20. * (end - start);
                    v(t.cos() * r_arc, t.sin() * r_arc)
                })
                .collect();
            p.add(egui::Shape::line(pts, s));
            let tip = (start.cos() * r_arc, start.sin() * r_arc);
            path(&[(tip.0 - 3.6, tip.1 - 0.6), tip, (tip.0 + 0.4, tip.1 - 3.6)]);
        }
        Icon::Lock => {
            p.rect_stroke(
                Rect::from_min_max(v(-6.2, -0.8), v(6.2, 7.6)),
                2.2 * z,
                s,
                egui::StrokeKind::Inside,
            );
            let arch: Vec<_> = (0..=12)
                .map(|i| {
                    let a = std::f32::consts::PI + i as f32 / 12. * std::f32::consts::PI;
                    v(a.cos() * 3.8, -3.4 + a.sin() * 3.8)
                })
                .collect();
            p.add(egui::Shape::line(arch, s));
            line((-3.8, -3.4), (-3.8, -0.8));
            line((3.8, -3.4), (3.8, -0.8));
            p.circle_filled(v(0., 3.3), 1.35 * z, color);
        }
        Icon::Unlock => {
            p.rect_stroke(
                Rect::from_min_max(v(-6.2, -0.8), v(6.2, 7.6)),
                2.2 * z,
                s,
                egui::StrokeKind::Inside,
            );
            let arch: Vec<_> = (0..=10)
                .map(|i| {
                    let a = std::f32::consts::PI + i as f32 / 10. * (std::f32::consts::PI * 0.78);
                    v(a.cos() * 3.8, -3.6 + a.sin() * 3.8)
                })
                .collect();
            p.add(egui::Shape::line(arch, s));
            line((-3.8, -3.6), (-3.8, -0.8));
            p.circle_filled(v(0., 3.3), 1.35 * z, color);
        }
        Icon::Download => {
            line((0., -6.5), (0., 2.2));
            path(&[(-3.8, -1.3), (0., 2.5), (3.8, -1.3)]);
            path(&[(-6.5, 3.8), (-6.5, 6.4), (6.5, 6.4), (6.5, 3.8)]);
        }
        Icon::Orbit => {
            p.circle_filled(c, 2.3 * z, color);
            let ring_a: Vec<_> = (0..=24)
                .map(|i| {
                    let a = i as f32 * std::f32::consts::TAU / 24.;
                    let (sx, sy) = (a.cos() * 8.4, a.sin() * 3.8);
                    let rot = egui::emath::Rot2::from_angle(-0.52) * Vec2::new(sx, sy);
                    c + rot * z
                })
                .collect();
            let ring_b: Vec<_> = (0..=24)
                .map(|i| {
                    let a = i as f32 * std::f32::consts::TAU / 24.;
                    let (sx, sy) = (a.cos() * 8.4, a.sin() * 3.8);
                    let rot = egui::emath::Rot2::from_angle(0.52) * Vec2::new(sx, sy);
                    c + rot * z
                })
                .collect();
            p.add(egui::Shape::line(ring_a, Stroke::new(1.35 * z, color)));
            p.add(egui::Shape::line(ring_b, Stroke::new(1.35 * z, color.gamma_multiply(0.75))));
        }
        Icon::Spark => {
            let star = [
                (0., -8.5),
                (2.1, -2.1),
                (8.5, 0.),
                (2.1, 2.1),
                (0., 8.5),
                (-2.1, 2.1),
                (-8.5, 0.),
                (-2.1, -2.1),
                (0., -8.5),
            ];
            path(&star);
            p.circle_filled(c, 1.5 * z, color);
        }
        Icon::Pulse => {
            path(&[
                (-8.5, 0.5),
                (-4.5, 0.5),
                (-2.2, -6.5),
                (1.2, 6.8),
                (3.8, -2.5),
                (5.6, 0.5),
                (8.5, 0.5),
            ]);
        }
        Icon::Bolt => {
            p.add(egui::Shape::convex_polygon(
                vec![v(1.4, -7.2), v(-5.2, 0.7), v(0.6, 0.7)],
                color,
                Stroke::NONE,
            ));
            p.add(egui::Shape::convex_polygon(
                vec![v(-1.4, 7.2), v(5.2, -0.7), v(-0.6, -0.7)],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Radar => {
            p.circle_stroke(c, 8.2 * z, s);
            p.circle_stroke(c, 4.4 * z, Stroke::new(1.25 * z, color.gamma_multiply(0.7)));
            p.circle_filled(c, 1.6 * z, color);
            line((0., 0.), (5.8, -5.8));
        }
        Icon::Layers => {
            path(&[(0., -7.5), (8., -3.5), (0., 0.5), (-8., -3.5), (0., -7.5)]);
            path(&[(-8., 0.8), (0., 4.8), (8., 0.8)]);
            path(&[(-8., 4.6), (0., 8.6), (8., 4.6)]);
        }
    }
}

pub fn backend_color(backend: &str, light: bool) -> Color32 {
    let (r, g, b) = crate::model::backend_rgb(backend, light);
    Color32::from_rgb(r, g, b)
}

pub fn backend_icon(backend: &str) -> Icon {
    match backend {
        "codex" => Icon::Terminal,
        "agy" => Icon::Orbit,
        "api" => Icon::Bolt,
        _ => Icon::Spark,
    }
}

pub fn phase_color(phase: crate::model::ToolPhase, light: bool) -> Color32 {
    let (r, g, b) = phase.rgb(light);
    Color32::from_rgb(r, g, b)
}

pub fn phase_icon(phase: crate::model::ToolPhase) -> Icon {
    match phase {
        crate::model::ToolPhase::Research => Icon::Search,
        crate::model::ToolPhase::Edit => Icon::Pencil,
        crate::model::ToolPhase::Terminal => Icon::Terminal,
        crate::model::ToolPhase::Subagent => Icon::Orbit,
        crate::model::ToolPhase::Desktop => Icon::Computer,
        crate::model::ToolPhase::Other => Icon::Layers,
    }
}

/// Top stripes intentionally removed for a clean, band-free matte header.
pub fn ambient_horizon(_painter: &egui::Painter, _rect: Rect, _color: Color32, _light: bool) {}

/// Claude mark: the orange spark of uneven rays.
fn paint_claude_mark(painter: &egui::Painter, rect: Rect) {
    const LENGTHS: [f32; 12] = [1.0, 0.80, 0.94, 0.78, 0.98, 0.84, 0.90, 0.76, 0.97, 0.82, 0.92, 0.79];
    let color = Color32::from_rgb(217, 119, 87);
    let c = rect.center();
    let r = rect.width().min(rect.height()) * 0.5;
    for (i, l) in LENGTHS.iter().enumerate() {
        let a = -std::f32::consts::FRAC_PI_2
            + i as f32 * std::f32::consts::TAU / LENGTHS.len() as f32
            + if i % 2 == 1 { 0.05 } else { -0.03 };
        let u = Vec2::new(a.cos(), a.sin());
        let n = Vec2::new(-u.y, u.x);
        let (base, tip) = (r * 0.135, (r * 0.06).max(0.5));
        let end = c + u * (r * l - tip);
        painter.add(egui::Shape::convex_polygon(
            vec![c + n * base, end + n * tip, end - n * tip, c - n * base],
            color,
            Stroke::NONE,
        ));
        painter.circle_filled(end, tip, color);
    }
    painter.circle_filled(c, r * 0.2, color);
}

/// OpenAI blossom for Codex: six interlaced links, in the text colour of the theme.
fn paint_openai_mark(painter: &egui::Painter, rect: Rect) {
    let color = if painter.ctx().style().visuals.dark_mode {
        Color32::from_rgb(236, 236, 236)
    } else {
        Color32::from_rgb(20, 20, 20)
    };
    let c = rect.center();
    let r = rect.width().min(rect.height()) * 0.5;
    let (length, radius, width, offset) = (r * 0.95, r * 0.24, (r * 0.11).max(1.1), r * 0.36);
    for k in 0..6 {
        let a = (k as f32 * 60.).to_radians();
        let u = Vec2::new(a.cos(), a.sin());
        let n = Vec2::new(-u.y, u.x);
        let center = c + n * offset;
        let half = length * 0.5 - radius;
        let mut points = Vec::with_capacity(40);
        for (end, from) in [(half, -90f32), (-half, 90.)] {
            for step in 0..=18 {
                let t = (from + step as f32 * 10.).to_radians();
                points.push(center + u * end + (u * t.cos() + n * t.sin()) * radius);
            }
        }
        painter.add(egui::Shape::closed_line(points, Stroke::new(width, color)));
    }
}

fn paint_antigravity_mark(painter: &egui::Painter, rect: Rect) {
    let ctx = painter.ctx();
    let id = egui::Id::new("antigravity-brand-mark");
    let texture = ctx.data(|data| data.get_temp::<egui::TextureHandle>(id)).unwrap_or_else(|| {
        let bytes = include_bytes!("../android/app/src/main/res/drawable-nodpi/antigravity.png");
        let image = image::load_from_memory(bytes).expect("bundled Antigravity PNG").into_rgba8();
        let pixels = egui::ColorImage::from_rgba_unmultiplied(
            [image.width() as usize, image.height() as usize], image.as_raw(),
        );
        let texture = ctx.load_texture("Antigravity", pixels, egui::TextureOptions::LINEAR);
        ctx.data_mut(|data| data.insert_temp(id, texture.clone()));
        texture
    });
    let square = Rect::from_center_size(rect.center(), Vec2::splat(rect.width().min(rect.height())));
    painter.image(texture.id(), square, Rect::from_min_max(Pos2::ZERO, Pos2::new(1., 1.)), Color32::WHITE);
}

fn paint_engine_icon(painter: &egui::Painter, rect: Rect, backend: &str) {
    match backend {
        "codex" => return paint_openai_mark(painter, rect),
        "agy" => return paint_antigravity_mark(painter, rect),
        "claude" | "pet" => return paint_claude_mark(painter, rect),
        _ => {}
    }
    use relay_desktop::agent_icons::{pixels, SIZE};
    let scale = painter.ctx().pixels_per_point();
    let extent = rect.width().min(rect.height());
    let cell = extent / SIZE as f32;
    let origin = rect.center() - Vec2::splat(extent * 0.5);
    let edge = |position: f32, index: usize| {
        let position = position + index as f32 * cell;
        if cell * scale >= 1.0 { (position * scale).round() / scale } else { position }
    };
    let mut mesh = egui::epaint::Mesh::default();
    for (y, row) in pixels(backend).iter().enumerate() {
        for (x, pixel) in row.iter().enumerate() {
            if let Some([r, g, b]) = pixel {
                let square = Rect::from_min_max(
                    Pos2::new(edge(origin.x, x), edge(origin.y, y)),
                    Pos2::new(edge(origin.x, x + 1), edge(origin.y, y + 1)),
                );
                mesh.add_colored_rect(square, Color32::from_rgb(*r, *g, *b));
            }
        }
    }
    painter.add(egui::Shape::mesh(mesh));
}

pub fn engine_icon(ui: &egui::Ui, rect: Rect, backend: &str) {
    paint_engine_icon(ui.painter(), rect, backend);
}

/// Specular 1px top-edge highlight on elevated glass cards.
pub fn specular_rim(painter: &egui::Painter, rect: Rect, radius: f32, light: bool) {
    let inset = radius.max(6.0);
    if rect.width() > inset * 2.2 {
        let rim = if light {
            Color32::from_white_alpha(170)
        } else {
            Color32::from_white_alpha(22)
        };
        painter.line_segment(
            [
                Pos2::new(rect.left() + inset, rect.top() + 0.75),
                Pos2::new(rect.right() - inset, rect.top() + 0.75),
            ],
            Stroke::new(1.0_f32, rim),
        );
    }
}

/// Counter-rotating dual orbital rings + breathing core for active AI agents.
pub fn orbital_core(ui: &egui::Ui, rect: Rect, color: Color32, spinning: bool) {
    let painter = ui.painter();
    let c = rect.center();
    let r = (rect.width().min(rect.height()) * 0.5 - 1.5).max(4.0);
    let animated = ui
        .ctx()
        .data(|d| d.get_temp::<bool>(egui::Id::new("relay-animations")))
        .unwrap_or(true);
    let t = if spinning && animated {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(24));
        ui.input(|i| i.time) as f32
    } else {
        0.65
    };
    painter.circle_filled(c, r, color.gamma_multiply(if spinning { 0.14 } else { 0.09 }));
    painter.circle_stroke(c, r, Stroke::new(1.0_f32, color.gamma_multiply(0.22)));
    let outer_arc: Vec<Pos2> = (0..=14)
        .map(|k| {
            let a = t * 2.6 + k as f32 / 14.0 * 3.5;
            c + Vec2::new(a.cos(), a.sin()) * r
        })
        .collect();
    painter.add(egui::Shape::line(outer_arc, Stroke::new(1.75_f32, color)));
    if r >= 7.0 {
        let inner_r = r * 0.60;
        let inner_arc: Vec<Pos2> = (0..=10)
            .map(|k| {
                let a = -t * 3.4 + 1.4 + k as f32 / 10.0 * 2.7;
                c + Vec2::new(a.cos(), a.sin()) * inner_r
            })
            .collect();
        painter.add(egui::Shape::line(
            inner_arc,
            Stroke::new(1.3_f32, color.gamma_multiply(0.75)),
        ));
    }
    let pulse = if spinning && animated {
        0.78 + ((t * 4.2).sin() * 0.5 + 0.5) * 0.34
    } else {
        0.85
    };
    painter.circle_filled(c, (r * 0.24 * pulse).max(1.8), color);
}

/// 5-bar kinetic harmonic waveform indicator for live streaming telemetry.
pub fn waveform(ui: &mut egui::Ui, color: Color32, height: f32) {
    let bars = 5usize;
    let width = bars as f32 * 4.2;
    let (r, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let animated = ui
        .ctx()
        .data(|d| d.get_temp::<bool>(egui::Id::new("relay-animations")))
        .unwrap_or(true);
    let time = if animated {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(28));
        ui.input(|i| i.time) as f32
    } else {
        1.1
    };
    for i in 0..bars {
        let wave = (time * 5.2 + i as f32 * 0.95).sin() * 0.5 + 0.5;
        let h = (height * (0.26 + 0.70 * wave)).max(3.0);
        let x = r.left() + i as f32 * 4.2 + 1.5;
        let bar = Rect::from_center_size(Pos2::new(x, r.center().y), Vec2::new(2.2, h));
        ui.painter()
            .rect_filled(bar, 1.1, color.gamma_multiply(0.50 + 0.50 * wave));
    }
}

/// Proportional multi-segment color bar visualizing Research / Edit / Terminal / Subagent steps.
pub fn phase_bar(
    painter: &egui::Painter,
    rect: Rect,
    tel: &crate::model::TurnTelemetry,
    light: bool,
    p: Palette,
) {
    painter.rect_filled(rect, rect.height() * 0.5, p.line.gamma_multiply(0.65));
    let other = tel
        .total_actions
        .saturating_sub(tel.reads + tel.edits + tel.commands + tel.delegations);
    let segments = [
        (tel.reads, phase_color(crate::model::ToolPhase::Research, light)),
        (tel.edits, phase_color(crate::model::ToolPhase::Edit, light)),
        (tel.commands, phase_color(crate::model::ToolPhase::Terminal, light)),
        (tel.delegations, phase_color(crate::model::ToolPhase::Subagent, light)),
        (other, phase_color(crate::model::ToolPhase::Other, light)),
    ];
    let sum: usize = segments.iter().map(|(n, _)| *n).sum();
    if sum == 0 || rect.width() < 6.0 {
        return;
    }
    let mut x = rect.left();
    let active_segments: Vec<_> = segments.into_iter().filter(|(n, _)| *n > 0).collect();
    let gaps = active_segments.len().saturating_sub(1) as f32 * 1.5;
    let usable = (rect.width() - gaps).max(2.0);
    for (idx, (count, col)) in active_segments.iter().enumerate() {
        let w = if idx + 1 == active_segments.len() {
            (rect.right() - x).max(2.0)
        } else {
            (usable * (*count as f32 / sum as f32)).max(3.0)
        };
        let seg = Rect::from_min_size(Pos2::new(x, rect.top()), Vec2::new(w, rect.height()));
        painter.rect_filled(seg, rect.height() * 0.5, *col);
        x += w + 1.5;
    }
}

/// Compact tinted icon + text pill for telemetry counters and engine tags.
pub fn mini_pill(
    ui: &mut egui::Ui,
    icon_opt: Option<Icon>,
    text: &str,
    color: Color32,
    p: Palette,
) -> Response {
    let galley = ui
        .painter()
        .layout_no_wrap(text.into(), FontId::proportional(10.5), color);
    let icon_space = if icon_opt.is_some() { 15.0 } else { 0.0 };
    let width = galley.size().x + icon_space + 14.0;
    let (r, resp) = ui.allocate_exact_size(Vec2::new(width, 20.0), Sense::hover());
    ui.painter()
        .rect_filled(r, 10.0, mix(p.surface, color, 0.14));
    ui.painter().rect_stroke(
        r,
        10.0,
        Stroke::new(1.0_f32, mix(p.line, color, 0.32)),
        egui::StrokeKind::Inside,
    );
    let mut tx = r.left() + 7.0;
    if let Some(ic) = icon_opt {
        icon(
            ui.painter(),
            Rect::from_center_size(Pos2::new(tx + 5.5, r.center().y), Vec2::splat(11.5)),
            ic,
            color,
        );
        tx += 14.5;
    }
    ui.painter().galley(
        Pos2::new(tx, r.center().y - galley.size().y * 0.5),
        galley,
        color,
    );
    resp
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
        ui.painter()
            .with_clip_rect(painted.shrink2(Vec2::new(6., 0.)))
            .galley(
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
    let height = if size.y > 0. { size.y } else { 32. };
    let icon_size = if text.is_empty() {
        (height * 0.62).clamp(15.0, 22.0)
    } else {
        17.
    };
    let gap = if which.is_some() && !text.is_empty() {
        6.5
    } else {
        0.
    };
    let inner = galley.size().x + gap + if which.is_some() { icon_size } else { 0. };
    let width = if size.x > 0. {
        size.x
    } else if text.is_empty() {
        height
    } else {
        inner + 24.
    };
    let (r, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
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
    let radius = (painted.height() * 0.28).clamp(6., 9.);
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
    if text.is_empty() {
        if let Some(which) = which {
            icon(
                ui.painter(),
                Rect::from_center_size(painted.center(), Vec2::splat(icon_size)),
                which,
                color,
            );
        }
    } else {
        let max_inner = (painted.width() - 12.).max(0.);
        let actual_inner = inner.min(max_inner);
        let mut x = painted.center().x - actual_inner * 0.5;
        if let Some(which) = which {
            icon(
                ui.painter(),
                Rect::from_center_size(
                    Pos2::new(x + icon_size * 0.5, painted.center().y),
                    Vec2::splat(icon_size),
                ),
                which,
                color,
            );
            x += icon_size + gap;
        }
        ui.painter()
            .with_clip_rect(painted.shrink2(Vec2::new(6., 0.)))
            .galley_with_override_text_color(
                Pos2::new(x, painted.center().y - galley.size().y * 0.5),
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
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 32.), Sense::click());
    let hover = ui.ctx().animate_bool_with_time(
        response.id.with("hover"),
        response.hovered(),
        duration(ui.ctx(), 0.16),
    );
    let fill = if active {
        mix(p.surface, accent(), 0.16)
    } else {
        mix(mix(p.base, p.surface, 0.45), p.line, 0.55 * hover)
    };
    ui.painter().rect_filled(r, 8., fill);
    ui.painter().rect_stroke(
        r,
        8.,
        Stroke::new(
            1.0_f32,
            if active {
                mix(p.line, accent(), 0.45)
            } else {
                mix(p.line.gamma_multiply(0.65), accent(), hover * 0.35)
            },
        ),
        egui::StrokeKind::Inside,
    );
    icon(
        ui.painter(),
        Rect::from_center_size(egui::pos2(r.left() + 15., r.center().y), Vec2::splat(15.)),
        which,
        if active {
            accent()
        } else {
            mix(p.muted, p.text, hover)
        },
    );
    ui.painter().text(
        egui::pos2(r.left() + 30., r.center().y),
        egui::Align2::LEFT_CENTER,
        text,
        FontId::proportional(12.5),
        p.text,
    );
    chevron(
        ui.painter(),
        Rect::from_center_size(r.right_center() - Vec2::new(13., 0.), Vec2::splat(13.)),
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
    ui.painter().rect_filled(r, 12., mix(p.side, p.surface, 0.55 + 0.45 * t));
    ui.painter().rect_stroke(
        r,
        12.,
        Stroke::new(1.0_f32, mix(p.line, accent(), 0.14 + t * 0.42)),
        egui::StrokeKind::Inside,
    );
    specular_rim(ui.painter(), r, 12., !ui.visuals().dark_mode);
    let tile = Rect::from_center_size(egui::pos2(r.left() + 31., r.center().y), Vec2::splat(36.));
    ui.painter()
        .rect_filled(tile, 10., mix(p.surface, accent(), 0.08 + 0.14 * t));
    icon(
        ui.painter(),
        tile.shrink(8.5),
        which,
        mix(p.muted, accent(), 0.35 + 0.65 * t),
    );
    let painter = ui.painter().with_clip_rect(r.shrink2(Vec2::new(12., 0.)));
    painter.text(
        egui::pos2(r.left() + 60., r.top() + 21.),
        egui::Align2::LEFT_CENTER,
        title,
        FontId::proportional(13.5),
        p.text,
    );
    painter.text(
        egui::pos2(r.left() + 60., r.top() + 43.),
        egui::Align2::LEFT_CENTER,
        detail,
        FontId::proportional(11.5),
        p.muted,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn bento_card(
    ui: &mut egui::Ui,
    width: f32,
    title: &str,
    detail: &str,
    which: Icon,
    tint: Color32,
    p: Palette,
) -> Response {
    let (r, response) = ui.allocate_exact_size(Vec2::new(width, 92.), Sense::click());
    let t = ui.ctx().animate_bool_with_time(
        response.id.with("hover"),
        response.hovered(),
        duration(ui.ctx(), 0.18),
    );
    ui.painter()
        .rect_filled(r, 13., mix(p.side, mix(p.surface, tint, 0.07), 0.6 + 0.4 * t));
    ui.painter().rect_stroke(
        r,
        13.,
        Stroke::new(1.0_f32, mix(p.line, tint, 0.18 + 0.45 * t)),
        egui::StrokeKind::Inside,
    );
    specular_rim(ui.painter(), r, 13., !ui.visuals().dark_mode);
    let tile = Rect::from_min_size(r.min + Vec2::new(14., 12.), Vec2::splat(30.));
    ui.painter()
        .rect_filled(tile, 8., mix(p.surface, tint, 0.14 + 0.12 * t));
    icon(ui.painter(), tile.shrink(6.5), which, tint);
    chevron(
        ui.painter(),
        Rect::from_center_size(Pos2::new(r.right() - 18., r.top() + 27.), Vec2::splat(13.)),
        0.,
        mix(p.muted, tint, t),
    );
    let painter = ui.painter().with_clip_rect(r.shrink2(Vec2::new(13., 6.)));
    painter.text(
        Pos2::new(r.left() + 14., r.top() + 55.),
        egui::Align2::LEFT_CENTER,
        title,
        FontId::proportional(13.5),
        p.text,
    );
    painter.text(
        Pos2::new(r.left() + 14., r.top() + 74.),
        egui::Align2::LEFT_CENTER,
        detail,
        FontId::proportional(11.0),
        p.muted,
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, title));
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
    let c = r.center();
    let a = accent();
    ui.painter().circle_filled(c, size * 0.50, a.gamma_multiply(0.08));
    ui.painter()
        .circle_stroke(c, size * 0.46, Stroke::new(1.1_f32, a.gamma_multiply(0.24)));
    ui.painter().circle_filled(c, size * 0.34, a.gamma_multiply(0.14));
    icon(ui.painter(), r.shrink(size * 0.22), Icon::Orbit, a);
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
        if *value { on_accent() } else { p.muted },
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
    let avail = visible.min(ui.available_width()).min(ui.clip_rect().width());
    let w = (avail - 40.).min(max_width).max(120.);
    let gap = ((avail - w) * 0.5).max(0.);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.;
        ui.add_space(gap);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.x = 8.;
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
