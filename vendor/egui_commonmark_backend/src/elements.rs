use egui::{self, epaint, NumExt, RichText, Sense, TextStyle, Ui, Vec2};

#[inline]
pub fn rule(ui: &mut Ui, end_line: bool) {
    ui.add(egui::Separator::default().horizontal());
    // This does not add a new line, but instead ends the separator
    if end_line {
        newline(ui);
    }
}

#[inline]
pub fn soft_break(ui: &mut Ui) {
    ui.label(" ");
}

#[inline]
pub fn newline(ui: &mut Ui) {
    // End the existing text row, rather than drawing a two-row newline label.
    // Block boundaries may request this twice (paragraph end + next start).
    // Repeating a break on an empty row must not add an empty paragraph; in a
    // list item's child UI it would also push text below its number/bullet.
    if ui.cursor().left() > ui.max_rect().left() + 0.5 {
        ui.end_row();
        // egui carries the previous row's height forward. A multi-line item
        // must not leave its height attached to the next one-line paragraph.
        ui.set_row_height(ui.text_style_height(&TextStyle::Body));
    }
}

pub fn bullet_point(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(width_body_space(ui) * 4.0, height_body(ui)),
        Sense::hover(),
    );
    ui.painter().circle_filled(
        rect.center(),
        rect.height() / 6.0,
        ui.visuals().strong_text_color(),
    );
}

pub fn bullet_point_hollow(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(width_body_space(ui) * 4.0, height_body(ui)),
        Sense::hover(),
    );
    ui.painter().circle(
        rect.center(),
        rect.height() / 6.0,
        egui::Color32::TRANSPARENT,
        egui::Stroke::new(0.6_f32, ui.visuals().strong_text_color()),
    );
}

pub fn number_point(ui: &mut Ui, number: &str) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(width_body_space(ui) * 4.0, height_body(ui)),
        Sense::hover(),
    );
    ui.painter().text(
        rect.right_center(),
        egui::Align2::RIGHT_CENTER,
        format!("{number}."),
        TextStyle::Body.resolve(ui.style()),
        ui.visuals().strong_text_color(),
    );
}

#[inline]
pub fn footnote_start(ui: &mut Ui, note: &str) {
    ui.label(RichText::new(note).raised().strong().small());
}

pub fn footnote(ui: &mut Ui, text: &str) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(width_body_space(ui) * 4.0, height_body(ui)),
        Sense::hover(),
    );
    ui.painter().text(
        rect.right_top(),
        egui::Align2::RIGHT_TOP,
        format!("{text}."),
        TextStyle::Small.resolve(ui.style()),
        ui.visuals().strong_text_color(),
    );
}

fn height_body(ui: &Ui) -> f32 {
    ui.text_style_height(&TextStyle::Body)
}

fn width_body_space(ui: &Ui) -> f32 {
    let id = TextStyle::Body.resolve(ui.style());
    ui.fonts(|f| f.glyph_width(&id, ' '))
}

/// Enhanced/specialized version of egui's code blocks. This one features copy button and borders
pub fn code_block<'t>(
    ui: &mut Ui,
    max_width: f32,
    text: &str,
    layouter: &'t mut dyn FnMut(&Ui, &str, f32) -> std::sync::Arc<egui::Galley>,
) {
    let text = text.strip_suffix('\n').unwrap_or(text);

    // Laid out by hand rather than through TextEdit: the block is exactly the column wide,
    // the code sits inside even padding, and the right side is kept free for the copy button.
    let (pad_x, pad_y, button_room) = (10.0_f32, 7.0_f32, 30.0_f32);
    let width = max_width.max(80.0);
    let galley = layouter(ui, text, (width - pad_x - button_room).max(40.0));
    let height = galley.size().y.max(ui.fonts(|f| f.row_height(&egui::TextStyle::Monospace.resolve(ui.style())))) + pad_y * 2.0;
    let (frame_rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    ui.painter().rect(
        frame_rect,
        egui::CornerRadius::same(8),
        ui.visuals().extreme_bg_color,
        ui.visuals().widgets.noninteractive.bg_stroke,
        egui::StrokeKind::Inside,
    );
    let text_rect = egui::Rect::from_min_size(frame_rect.min + egui::vec2(pad_x, pad_y), galley.size());
    let label = ui.put(text_rect, egui::Label::new(egui::WidgetText::Galley(galley)).selectable(true).wrap_mode(egui::TextWrapMode::Extend));
    let output_id = label.id;

    // Copy icon: painted, not an emoji glyph, so it sits inside the block's top-right corner
    // and lines up the same in every theme and for one-line blocks.
    // Centred on a one-line block, level with the first line on a longer one.
    let side = 20.0_f32;
    let line = ui.fonts(|f| f.row_height(&egui::TextStyle::Monospace.resolve(ui.style())));
    let first_line_centre = frame_rect.top() + pad_y + line * 0.5;
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(frame_rect.right() - 7.0 - side * 0.5, first_line_centre.min(frame_rect.center().y)),
        egui::vec2(side, side),
    );

    // Check if we should show a tick instead of the copy icon if the text was copied and the mouse is hovered
    let persistent_id = ui.make_persistent_id(output_id);
    let copied_icon = ui.memory_mut(|m| *m.data.get_temp_mut_or_default::<bool>(persistent_id));

    let copy_button = ui
        .interact(icon_rect, persistent_id.with("copy"), egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text("Копировать");
    {
        let painter = ui.painter();
        let visuals = ui.visuals();
        let ink = if copy_button.hovered() { visuals.strong_text_color() } else { visuals.weak_text_color() };
        // A pad under the icon keeps long lines from showing through it.
        painter.rect_filled(icon_rect.expand(2.0), 5.0, if copy_button.hovered() { visuals.widgets.hovered.weak_bg_fill } else { visuals.extreme_bg_color });
        let stroke = egui::Stroke::new(1.3_f32, ink);
        let c = icon_rect.center();
        let z = side / 20.0;
        if copied_icon {
            painter.add(egui::Shape::line(
                vec![c + egui::vec2(-4.5 * z, 0.0), c + egui::vec2(-1.2 * z, 3.4 * z), c + egui::vec2(4.8 * z, -3.6 * z)],
                egui::Stroke::new(1.6_f32, ink),
            ));
        } else {
            let back = egui::Rect::from_min_size(c + egui::vec2(-5.5 * z, -5.5 * z), egui::vec2(8.0 * z, 8.0 * z));
            let front = back.translate(egui::vec2(3.0 * z, 3.0 * z));
            painter.rect_stroke(back, 2.0, stroke, egui::StrokeKind::Middle);
            painter.rect_filled(front, 2.0, if copy_button.hovered() { visuals.widgets.hovered.weak_bg_fill } else { visuals.extreme_bg_color });
            painter.rect_stroke(front, 2.0, stroke, egui::StrokeKind::Middle);
        }
    }

    // Update icon state in persistent memory
    if copied_icon && !copy_button.hovered() {
        ui.memory_mut(|m| *m.data.get_temp_mut_or_default(persistent_id) = false);
    }
    if !copied_icon && copy_button.clicked() {
        ui.memory_mut(|m| *m.data.get_temp_mut_or_default(persistent_id) = true);
    }

    if copy_button.clicked() {
        ui.ctx().copy_text(text.to_owned());
    }
    // Blocks one after another do not touch.
    ui.add_space(4.0);
}

// Stripped down version of egui's Checkbox. The only difference is that this
// creates a noninteractive checkbox. ui.add_enabled could have been used instead,
// but it makes the checkbox too grey.
pub struct ImmutableCheckbox<'a> {
    checked: &'a mut bool,
}

impl<'a> ImmutableCheckbox<'a> {
    pub fn without_text(checked: &'a mut bool) -> Self {
        ImmutableCheckbox { checked }
    }
}

impl<'a> egui::Widget for ImmutableCheckbox<'a> {
    fn ui(self, ui: &mut Ui) -> egui::Response {
        let ImmutableCheckbox { checked } = self;

        let spacing = &ui.spacing();
        let icon_width = spacing.icon_width;

        let mut desired_size = egui::vec2(icon_width, 0.0);
        desired_size = desired_size.at_least(Vec2::splat(spacing.interact_size.y));
        desired_size.y = desired_size.y.max(icon_width);
        let (rect, response) = ui.allocate_exact_size(desired_size, Sense::click());

        if ui.is_rect_visible(rect) {
            let visuals = ui.style().visuals.noninteractive();
            let (small_icon_rect, big_icon_rect) = ui.spacing().icon_rectangles(rect);
            ui.painter().add(epaint::RectShape::new(
                big_icon_rect.expand(visuals.expansion),
                visuals.corner_radius,
                visuals.bg_fill,
                visuals.bg_stroke,
                egui::StrokeKind::Inside,
            ));

            if *checked {
                // Check mark:
                ui.painter().add(egui::Shape::line(
                    vec![
                        egui::pos2(small_icon_rect.left(), small_icon_rect.center().y),
                        egui::pos2(small_icon_rect.center().x, small_icon_rect.bottom()),
                        egui::pos2(small_icon_rect.right(), small_icon_rect.top()),
                    ],
                    visuals.fg_stroke,
                ));
            }
        }

        response
    }
}

pub fn blockquote(ui: &mut Ui, accent: egui::Color32, add_contents: impl FnOnce(&mut Ui)) {
    let start = ui.painter().add(egui::Shape::Noop);
    let response = egui::Frame::new()
        // offset the frame so that we can use the space for the horizontal line and other stuff
        // By not using a separator we have better control
        .outer_margin(egui::Margin {
            left: 10,
            ..Default::default()
        })
        .show(ui, add_contents)
        .response;

    // FIXME: Add some rounding

    ui.painter().set(
        start,
        egui::epaint::Shape::line_segment(
            [
                egui::pos2(response.rect.left_top().x, response.rect.left_top().y + 5.0),
                egui::pos2(
                    response.rect.left_bottom().x,
                    response.rect.left_bottom().y - 5.0,
                ),
            ],
            egui::Stroke::new(3.0_f32, accent),
        ),
    );
}
