use ab_glyph::{Font as _, OutlinedGlyph, ScaleFont};
use glyph_brush_layout::{BuiltInLineBreaker, GlyphPositioner, Layout, LineBreak, LineBreaker, SectionGeometry, SectionText};
use crate::fonts::Font;

pub(super) struct PositionedText {
    pub glyphs: Vec<OutlinedGlyph>,
    pub height: f32,
}

#[derive(Clone, Copy, Debug)]
struct WordWrap<'a> {
    font: &'a Font,
    scale: u32,
    width: u32,
}

impl std::hash::Hash for WordWrap<'_> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (std::sync::Arc::as_ptr(self.font), self.scale, self.width).hash(state);
    }
}

impl LineBreaker for WordWrap<'_> {
    fn line_breaks<'a>(&self, text: &'a str) -> Box<dyn Iterator<Item = LineBreak> + 'a> {
        let scaled = self.font.as_scaled(f32::from_bits(self.scale));
        let mut breaks = Vec::new();
        let mut start = 0;
        for end in BuiltInLineBreaker::UnicodeLineBreaker.line_breaks(text) {
            let word = &text[start..end.offset()];
            let (word_width, _) = word.trim_end_matches(char::is_whitespace).chars().fold((0.0, None), |(width, previous), character| {
                let id = scaled.glyph_id(character);
                (width + scaled.h_advance(id) + previous.map(|previous| scaled.kern(previous, id)).unwrap_or(0.0), Some(id))
            });
            if word_width > f32::from_bits(self.width) {
                breaks.extend(word.char_indices().filter(|(index, character)| *index > 0 && !character.is_control()).map(|(index, _)| LineBreak::Soft(start + index)));
            }
            breaks.push(end);
            start = end.offset();
        }
        Box::new(breaks.into_iter())
    }
}

fn pixel_scale(font: &Font, size: f32) -> f32 {
    size * font.height_unscaled() / font.units_per_em().unwrap()
}

pub(super) fn advance_width(font: &Font, character: char, size: f32) -> f32 {
    font.as_scaled(pixel_scale(font, size)).h_advance(font.glyph_id(character))
}

pub(super) fn position_text(font: &Font, text: &str, size: f32, width: f32, line_height: Option<f32>) -> PositionedText {
    let scale = pixel_scale(font, size);
    let scaled = font.as_scaled(scale);
    let natural_height = scaled.height() + scaled.line_gap();
    let pitch = line_height.unwrap_or(natural_height.ceil());
    let positioned = Layout::default().line_breaker(WordWrap { font, scale: scale.to_bits(), width: width.to_bits() }).calculate_glyphs(
        &[font.as_ref()],
        &SectionGeometry { bounds: (width, f32::INFINITY), ..Default::default() },
        &[SectionText { text, scale: scale.into(), ..Default::default() }],
    );
    let mut height = 0.0_f32;
    let glyphs = positioned.into_iter().filter_map(|section| {
        let mut glyph = section.glyph;
        let line = ((glyph.position.y - scaled.ascent()) / natural_height).round();
        let top = line * pitch;
        height = height.max(top + natural_height.ceil());
        glyph.position.y = top + scaled.ascent().ceil();
        let outline = font.outline_glyph(glyph)?;
        height = height.max(outline.px_bounds().max.y);
        Some(outline)
    }).collect();
    PositionedText { glyphs, height }
}
