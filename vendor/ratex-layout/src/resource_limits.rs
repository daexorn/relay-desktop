//! Validation of output work in completed layouts, without creating drawing items.
use crate::layout_box::{BoxContent, LayoutBox, VBoxChildKind};

/// Maximum estimated drawing items accepted by [`validate_layout`].
pub const MAX_LAYOUT_OUTPUT_ITEMS: usize = 65_536;

/// The count is independent of translation and uniform scaling. DisplayList
/// conversion uses this same count, avoiding an unbounded floating-point loop.
/// Layout validation checks dimensions before using it; conversion itself
/// does not enforce an output budget.
pub(crate) fn vertical_dash_count(height: f64, thickness: f64) -> usize {
    (height / (8.0 * thickness)).ceil().max(0.0) as usize
}

fn own_cost(box_: &LayoutBox) -> Result<usize, String> {
    Ok(match &box_.content {
        BoxContent::Glyph { .. } | BoxContent::Rule { .. } | BoxContent::SvgPath { .. } => 1,
        BoxContent::GlyphRun { glyphs } => glyphs.len(),
        BoxContent::Fraction { bar_thickness, .. } => usize::from(*bar_thickness > 0.0),
        BoxContent::Radical { .. } => 2, // Surd glyph and overbar.
        BoxContent::Angl { .. } | BoxContent::Overline { .. } | BoxContent::Underline { .. } => 1,
        BoxContent::Framed {
            has_border,
            bg_color,
            ..
        } => 4 * usize::from(*has_border) + usize::from(bg_color.is_some()),
        BoxContent::ProofTree { rules, .. } => rules.len(),
        BoxContent::Array {
            col_separators,
            hlines_before_row,
            rule_thickness,
            ..
        } => {
            let mut cost = hlines_before_row
                .iter()
                .fold(0usize, |sum, row| sum.saturating_add(row.len()));
            let dashed = col_separators
                .iter()
                .filter(|sep| **sep == Some(true))
                .count();
            let solid = col_separators
                .iter()
                .filter(|sep| **sep == Some(false))
                .count();
            cost = cost.saturating_add(solid);
            if dashed > 0 {
                let height = box_.height + box_.depth;
                let period = 8.0 * rule_thickness;
                if !height.is_finite() || height < 0.0 || !period.is_finite() || period <= 0.0 {
                    return Err("Invalid dashed separator geometry".into());
                }
                cost = cost.saturating_add(
                    vertical_dash_count(height, *rule_thickness).saturating_mul(dashed),
                );
            }
            cost
        }
        BoxContent::HBox(_)
        | BoxContent::VBox(_)
        | BoxContent::SupSub { .. }
        | BoxContent::OpLimits { .. }
        | BoxContent::Accent { .. }
        | BoxContent::LeftRight { .. }
        | BoxContent::RaiseBox { .. }
        | BoxContent::Scaled { .. }
        | BoxContent::Kern
        | BoxContent::Empty => 0,
    })
}

// Exhaustive traversal: new box variants must explicitly define their children
// and own cost. The input tree stays alive and immutable throughout the walk.
fn children<'a>(box_: &'a LayoutBox, mut visit: impl FnMut(&'a LayoutBox)) {
    match &box_.content {
        BoxContent::HBox(boxes) => boxes.iter().for_each(visit),
        BoxContent::VBox(boxes) => {
            for child in boxes {
                if let VBoxChildKind::Box(body) = &child.kind {
                    visit(body);
                }
            }
        }
        BoxContent::Fraction { numer, denom, .. } => {
            visit(numer);
            visit(denom);
        }
        BoxContent::SupSub { base, sup, sub, .. } | BoxContent::OpLimits { base, sup, sub, .. } => {
            visit(base);
            if let Some(sup) = sup {
                visit(sup);
            }
            if let Some(sub) = sub {
                visit(sub);
            }
        }
        BoxContent::Radical { body, index, .. } => {
            visit(body);
            if let Some(index) = index {
                visit(index);
            }
        }
        BoxContent::Accent { base, accent, .. } => {
            visit(base);
            visit(accent);
        }
        BoxContent::LeftRight { left, right, inner } => {
            visit(left);
            visit(right);
            visit(inner);
        }
        BoxContent::Array {
            cells,
            row_tags,
            tag_col_width,
            ..
        } => {
            for row in cells {
                for cell in row {
                    visit(cell);
                }
            }
            if *tag_col_width > 0.0 {
                for tag in row_tags.iter().flatten() {
                    visit(tag);
                }
            }
        }
        BoxContent::Framed { body, .. }
        | BoxContent::RaiseBox { body, .. }
        | BoxContent::Scaled { body, .. }
        | BoxContent::Angl { body, .. }
        | BoxContent::Overline { body, .. }
        | BoxContent::Underline { body, .. } => visit(body),
        BoxContent::ProofTree { children, .. } => {
            for child in children {
                visit(&child.box_);
            }
        }
        BoxContent::Glyph { .. }
        | BoxContent::GlyphRun { .. }
        | BoxContent::Rule { .. }
        | BoxContent::SvgPath { .. }
        | BoxContent::Kern
        | BoxContent::Empty => {}
    }
}

/// Validate the output-work budget of a completed layout without creating drawing items.
///
/// Call after [`crate::layout`] and before [`crate::to_display_list`] when handling
/// untrusted input. This does not modify the layout. The check covers drawing-item
/// costs and dashed separator dimensions, not arbitrary hand-built tree validity.
/// Layouts derived from parser-produced ASTs are the supported input contract.
pub fn validate_layout(root: &LayoutBox) -> Result<(), String> {
    output_cost(root).map(|_| ())
}

fn output_cost(root: &LayoutBox) -> Result<usize, String> {
    let mut pending = vec![root];
    let mut cost = 0usize;
    while let Some(box_) = pending.pop() {
        cost = cost.saturating_add(own_cost(box_)?);
        if cost > MAX_LAYOUT_OUTPUT_ITEMS {
            return Err("Layout resource limit exceeded".into());
        }
        children(box_, |child| pending.push(child));
    }
    Ok(cost)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{layout, to_display_list, LayoutOptions};
    use ratex_types::math_style::MathStyle;

    #[test]
    fn layout_cost_matches_output_across_primitives_and_scales() {
        let formulas = [
            r"x^2_i+\frac{a}{b}+\binom{n}{k}",
            r"\sqrt[3]{x}+\sum_{i=0}^n i+\hat{x}+\overline{x}+\underline{y}",
            r"\text{abc}+\colorbox{red}{x}+\fbox{x}+\angl{x}",
            r"\left(\frac{x}{y}\middle|z\right)",
            r"\pmb{\pmb{\begin{array}{:c|r:}\hline x&y\\z\end{array}}}",
            r"\begin{align}x&=y\tag{one}\\z&=0\tag{two}\end{align}",
            r"\begin{prooftree}\AxiomC{P}\UnaryInfC{Q}\end{prooftree}",
            r"\tiny\begin{array}{:c:}\rule{1em}{10em}\end{array}",
            r"\htmlStyle{font-size:2em;}{\begin{array}{:c:}x\end{array}}",
            r"\raisebox{10000000000000000em}{\begin{array}{:c:}x\end{array}}",
            r"\phantom{x}+\vphantom{y}+\smash{z}+\mathllap{w}",
        ];
        for style in [
            MathStyle::Display,
            MathStyle::Text,
            MathStyle::Script,
            MathStyle::ScriptScript,
        ] {
            for input in formulas {
                let ast =
                    ratex_parser::parse(input).unwrap_or_else(|err| panic!("{input}: {err:?}"));
                let boxes = layout(&ast, &LayoutOptions::default().with_style(style));
                assert_eq!(
                    output_cost(&boxes).unwrap(),
                    to_display_list(&boxes).items.len(),
                    "{input} ({style:?})"
                );
            }
        }
    }

    #[test]
    fn invalid_dashed_dimensions_are_rejected_without_conversion() {
        let ast = ratex_parser::parse(r"\begin{array}{:c}x\end{array}").unwrap();
        let BoxContent::HBox(mut boxes) = layout(&ast, &LayoutOptions::default()).content else {
            panic!("expected expression");
        };
        let mut array = boxes.remove(0);
        assert!(matches!(array.content, BoxContent::Array { .. }));
        for height in [f64::NAN, f64::INFINITY, -1.0] {
            array.height = height;
            array.depth = 0.0;
            assert_eq!(
                output_cost(&array).unwrap_err(),
                "Invalid dashed separator geometry"
            );
        }
        array.height = 1.0;
        for thickness in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::MAX] {
            if let BoxContent::Array { rule_thickness, .. } = &mut array.content {
                *rule_thickness = thickness;
            }
            assert_eq!(
                output_cost(&array).unwrap_err(),
                "Invalid dashed separator geometry"
            );
        }
    }

    #[test]
    fn dash_count_handles_full_period_and_partial_tail() {
        assert_eq!(vertical_dash_count(0.0, 0.0625), 0);
        assert_eq!(vertical_dash_count(2.0, 0.0625), 4);
        assert_eq!(vertical_dash_count(2.01, 0.0625), 5);
    }
}
