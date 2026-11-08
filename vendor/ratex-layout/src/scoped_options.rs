//! Per-call layout state. Kept separate from the public options so callers do
//! not retain caches or have to initialize implementation-specific fields.
use std::{cell::RefCell, collections::HashMap, ops::Deref, rc::Rc, sync::Arc};

use ratex_parser::ParseNode;
use ratex_types::{color::Color, math_style::MathStyle};

use crate::{FallbackGlyphMetrics, LayoutOptions};

#[derive(Debug, PartialEq, Eq, Hash)]
pub(crate) struct DelimiterKey {
    content: String,
    style: MathStyle,
    color: [u32; 4],
    align_relation_spacing: Option<u64>,
    inter_glyph_kern_em: u64,
    explicit_size_multiplier: u64,
}

#[derive(Clone, Default)]
pub(crate) struct ScopedOptions {
    options: LayoutOptions,
    pub(crate) fallback_glyph_metrics: Option<Arc<HashMap<char, FallbackGlyphMetrics>>>,
    #[cfg(test)]
    pub(crate) disable_cache: bool,
    delimiters: Rc<RefCell<HashMap<DelimiterKey, f64>>>,
}

impl Deref for ScopedOptions {
    type Target = LayoutOptions;
    fn deref(&self) -> &Self::Target {
        &self.options
    }
}

impl ScopedOptions {
    pub(crate) fn new(options: LayoutOptions) -> Self {
        Self {
            options,
            ..Self::default()
        }
    }

    pub(crate) fn with_fallback_metrics(
        self,
        metrics: Arc<HashMap<char, FallbackGlyphMetrics>>,
    ) -> Self {
        Self {
            fallback_glyph_metrics: Some(metrics),
            ..self
        }
    }

    pub(crate) fn with_style(&self, style: MathStyle) -> Self {
        Self {
            options: self.options.with_style(style),
            ..self.clone()
        }
    }

    pub(crate) fn with_color(&self, color: Color) -> Self {
        Self {
            options: self.options.with_color(color),
            ..self.clone()
        }
    }

    pub(crate) fn with_explicit_size_multiplier(&self, multiplier: f64) -> Self {
        Self {
            options: self.options.with_explicit_size_multiplier(multiplier),
            ..self.clone()
        }
    }

    pub(crate) fn with_middle_height(&self, height: Option<f64>) -> Self {
        let mut result = self.clone();
        result.options.leftright_delim_height = height;
        result
    }

    pub(crate) fn delimiter_key(
        &self,
        body: &[ParseNode],
        left: &str,
        right: &str,
    ) -> DelimiterKey {
        // Content keys also work for temporary ASTs synthesized by HTML/text
        // and proof-tree layout; pointer keys could alias after those are freed.
        // A scope containing its own middle resets the inherited height. The
        // fallback metrics Arc is immutable and identical throughout this
        // per-call cache, so neither serialize nor hash its potentially huge map.
        // Keep this exhaustive so new public options require a cache-key decision.
        let LayoutOptions {
            style,
            color,
            align_relation_spacing,
            leftright_delim_height: _,
            inter_glyph_kern_em,
            explicit_size_multiplier,
        } = &self.options;
        DelimiterKey {
            content: serde_json::to_string(&(body, left, right)).expect("serializable AST"),
            style: *style,
            color: [color.r, color.g, color.b, color.a].map(f32::to_bits),
            align_relation_spacing: align_relation_spacing.map(f64::to_bits),
            inter_glyph_kern_em: inter_glyph_kern_em.to_bits(),
            explicit_size_multiplier: explicit_size_multiplier.to_bits(),
        }
    }

    pub(crate) fn cached_delimiter_height(&self, key: &DelimiterKey) -> Option<f64> {
        #[cfg(test)]
        if self.disable_cache {
            return None;
        }
        self.delimiters.borrow().get(key).copied()
    }

    pub(crate) fn cache_delimiter_height(&self, key: DelimiterKey, height: f64) {
        #[cfg(test)]
        if self.disable_cache {
            return;
        }
        // Retain measurements, not copies of public owned subtrees. Reusing
        // the first-pass result prevents exponential nested middle measurement
        // without retaining a large expanded tree at every enclosing scope.
        self.delimiters.borrow_mut().insert(key, height);
    }
}
