//! Shared code for egui_commonmark and egui_commonmark_macro. Breaking changes will happen and
//! should ideally not be relied upon. Only items that can been seen in this documentation
//! can be safely used directly.

#[doc(hidden)]
pub mod alerts;
#[doc(hidden)]
pub mod elements;
#[doc(hidden)]
pub mod misc;
#[doc(hidden)]
pub mod pulldown;

#[cfg(feature = "embedded_image")]
mod data_url_loader;

// For ease of use in proc macros
#[doc(hidden)]
pub use {
    alerts::{alert_ui, Alert, AlertBundle},
    // Pretty much every single element in this module is used by the proc macros
    elements::*,
    misc::{prepare_show, CodeBlock, CommonMarkOptions, Image, Link},
};

// The only struct that is allowed to use directly. (If one does not need egui_commonmark)
pub use misc::CommonMarkCache;

/// Takes [`egui::Ui`], the math text to be rendered and whether it is inline
pub type RenderMathFn = dyn Fn(&mut egui::Ui, &str, bool);

/// Relay: size of a typeset formula around its baseline, in points.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MathExtent {
    pub width: f32,
    pub above: f32,
    pub below: f32,
    /// Whether the formula stays in the text row, or moves to a row of its own.
    pub inline: bool,
}

/// Relay: takes [`egui::Ui`] and the math text. Lets the text around a formula make room.
pub type RenderMathExtentFn = dyn Fn(&egui::Ui, &str) -> MathExtent;

/// Relay: line height of the text being laid out, when formulas made it larger than usual.
pub fn math_line_height(ui: &egui::Ui) -> Option<f32> {
    ui.ctx().data(|data| data.get_temp(egui::Id::new("relay-math-line")))
}

#[doc(hidden)]
pub fn set_math_line_height(ui: &egui::Ui, line: Option<f32>) {
    let id = egui::Id::new("relay-math-line");
    ui.ctx().data_mut(|data| match line {
        Some(line) => data.insert_temp(id, line),
        None => data.remove::<f32>(id),
    });
}
/// Takes [`egui::Ui`] and the html text to be rendered/used
pub type RenderHtmlFn = dyn Fn(&mut egui::Ui, &str);
