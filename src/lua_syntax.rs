//! Lua 5.1 lexical highlighting. Never evaluates source.
use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Plain,
    Keyword,
    String,
    Comment,
    Number,
    Call,
}

fn long_end(bytes: &[u8], start: usize) -> Option<usize> {
    if bytes.get(start) != Some(&b'[') {
        return None;
    }
    let mut body = start + 1;
    while bytes.get(body) == Some(&b'=') {
        body += 1;
    }
    if bytes.get(body) != Some(&b'[') {
        return None;
    }
    let equals = body - start - 1;
    let mut i = body + 1;
    while i < bytes.len() {
        if bytes[i] == b']' {
            let mut close = i + 1;
            while bytes.get(close) == Some(&b'=') {
                close += 1;
            }
            if close - i - 1 == equals && bytes.get(close) == Some(&b']') {
                return Some(close + 1);
            }
        }
        i += 1;
    }
    Some(bytes.len())
}

pub fn tokens(source: &str) -> Vec<(Range<usize>, Kind)> {
    let bytes = source.as_bytes();
    let mut spans: Vec<(Range<usize>, Kind)> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        let kind;
        if bytes[i..].starts_with(b"--") {
            i = long_end(bytes, i + 2)
                .unwrap_or_else(|| source[i..].find('\n').map_or(bytes.len(), |n| i + n));
            kind = Kind::Comment;
        } else if matches!(bytes[i], b'"' | b'\'') {
            let quote = bytes[i];
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    i = (i + 2).min(bytes.len());
                } else if bytes[i] == quote {
                    i += 1;
                    break;
                } else {
                    i += 1;
                }
            }
            // Escaped Unicode bytes must not produce a non-character boundary.
            while !source.is_char_boundary(i) {
                i += 1;
            }
            kind = Kind::String;
        } else if let Some(end) = long_end(bytes, i) {
            i = end;
            kind = Kind::String;
        } else if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            kind = match &source[start..i] {
                "and" | "break" | "do" | "else" | "elseif" | "end" | "false" | "for"
                | "function" | "if" | "in" | "local" | "nil" | "not" | "or" | "repeat"
                | "return" | "then" | "true" | "until" | "while" => Kind::Keyword,
                _ if source[i..].trim_start().starts_with('(') => Kind::Call,
                _ => Kind::Plain,
            };
        } else if bytes[i].is_ascii_digit()
            || (bytes[i] == b'.' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit))
        {
            if bytes[i..].starts_with(b"0x") || bytes[i..].starts_with(b"0X") {
                i += 2;
                while i < bytes.len() && bytes[i].is_ascii_hexdigit() {
                    i += 1;
                }
            } else {
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                if bytes.get(i) == Some(&b'.') && bytes.get(i + 1) != Some(&b'.') {
                    i += 1;
                    while i < bytes.len() && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                if matches!(bytes.get(i), Some(b'e' | b'E')) {
                    i += 1;
                    if matches!(bytes.get(i), Some(b'+' | b'-')) {
                        i += 1;
                    }
                    while i < bytes.len() && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                }
            }
            kind = Kind::Number;
        } else {
            i += source[i..].chars().next().unwrap().len_utf8();
            kind = Kind::Plain;
        }
        if let Some((range, _)) = spans.last_mut().filter(|(_, previous)| *previous == kind) {
            range.end = i;
        } else {
            spans.push((start..i, kind));
        }
    }
    spans
}

#[derive(Default)]
pub struct Highlighter {
    job: egui::text::LayoutJob,
    style: Option<(bool, egui::FontId)>,
}

pub fn argument_ranges(source: &str) -> Vec<Range<usize>> {
    if source.trim().is_empty() {
        return Vec::new();
    }
    let mut mask = vec![true; source.len()];
    for (range, kind) in tokens(source) {
        if matches!(kind, Kind::String | Kind::Comment) {
            mask[range].fill(false);
        }
    }
    let mut depth = 0i32;
    let mut start = 0;
    let mut result = Vec::new();
    for (i, byte) in source.bytes().enumerate() {
        if !mask[i] {
            continue;
        }
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b',' if depth == 0 => {
                result.push(start..i);
                start = i + 1;
            }
            _ => {}
        }
    }
    result.push(start..source.len());
    result
}
impl Highlighter {
    pub fn layout(
        &mut self,
        ui: &egui::Ui,
        source: &str,
        width: f32,
    ) -> std::sync::Arc<egui::Galley> {
        let font = egui::TextStyle::Monospace.resolve(ui.style());
        let dark = ui.visuals().dark_mode;
        if self.job.text != source || self.style.as_ref() != Some(&(dark, font.clone())) {
            self.job = Default::default();
            for (range, kind) in tokens(source) {
                let rgb = match (kind, dark) {
                    (Kind::Keyword, true) => [198, 148, 235],
                    (Kind::Keyword, false) => [113, 40, 153],
                    (Kind::String, true) => [168, 207, 150],
                    (Kind::String, false) => [43, 112, 47],
                    (Kind::Comment, true) => [142, 153, 165],
                    (Kind::Comment, false) => [91, 107, 114],
                    (Kind::Number, true) => [233, 185, 127],
                    (Kind::Number, false) => [151, 83, 15],
                    (Kind::Call, true) => [116, 198, 231],
                    (Kind::Call, false) => [0, 94, 139],
                    (Kind::Plain, _) => {
                        let c = ui.visuals().text_color();
                        [c.r(), c.g(), c.b()]
                    }
                };
                self.job.append(
                    &source[range],
                    0.,
                    egui::TextFormat {
                        font_id: font.clone(),
                        color: egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]),
                        ..Default::default()
                    },
                );
            }
            self.style = Some((dark, font));
        }
        self.job.wrap.max_width = width;
        ui.fonts_mut(|fonts| fonts.layout_job(self.job.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn argument_inputs_do_not_split_strings_tables_or_nested_calls() {
        let source = "GetPlayer(), \"hello, Mickey\", {1, 2}, nested(3, 4), [=[a,b]=]";
        let args: Vec<_> = argument_ranges(source)
            .into_iter()
            .map(|r| source[r].trim())
            .collect();
        assert_eq!(
            args,
            [
                "GetPlayer()",
                "\"hello, Mickey\"",
                "{1, 2}",
                "nested(3, 4)",
                "[=[a,b]=]"
            ]
        );
    }
    #[test]
    fn cached_layout_preserves_source_in_light_and_dark_themes() {
        let ctx = egui::Context::default();
        let mut highlighter = Highlighter::default();
        for dark in [true, false] {
            ctx.set_visuals(if dark {
                egui::Visuals::dark()
            } else {
                egui::Visuals::light()
            });
            let mut output = ctx.run_ui(Default::default(), |ui| {
                for source in ["local name = \"Mickey\"\nWait(1)", "--[[unfinished", ""] {
                    let galley = highlighter.layout(ui, source, 300.);
                    assert_eq!(galley.job.text, source);
                    let cached = highlighter.layout(ui, source, 300.);
                    assert_eq!(cached.job.text, source);
                }
            });
            output.textures_delta.clear();
        }
    }
    #[test]
    fn lexical_context_and_numbers_are_classified_without_changing_text() {
        let source = "local x = 0x2A + .5e-2 --[=[ if Wait(99) ]=]\nBark(\"é -- if\", [[end]])";
        let spans = tokens(source);
        assert_eq!(
            spans
                .iter()
                .map(|(r, _)| &source[r.clone()])
                .collect::<String>(),
            source
        );
        assert!(spans
            .iter()
            .any(|(r, k)| *k == Kind::Number && &source[r.clone()] == ".5e-2"));
        assert!(spans
            .iter()
            .any(|(r, k)| *k == Kind::Comment && source[r.clone()].contains("Wait")));
        assert_eq!(spans.iter().filter(|(_, k)| *k == Kind::Call).count(), 1);
    }
    #[test]
    fn retail_sources_and_incomplete_input_have_valid_token_boundaries() {
        for source in crate::lua_cook::repository::ENTRIES
            .iter()
            .map(|e| e.source)
            .chain([
                "'unterminated",
                "--[==[comment",
                "[=[text",
                "1..2",
                "\"\\é",
                "",
            ])
        {
            let spans = tokens(source);
            assert_eq!(
                spans
                    .iter()
                    .map(|(r, _)| &source[r.clone()])
                    .collect::<String>(),
                source
            );
        }
    }
}
