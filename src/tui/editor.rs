use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Cursor positions are UTF-8 boundaries, movement/deletion use whole graphemes.
#[derive(Default, Clone)]
pub struct Editor {
    pub text: String,
    pub cursor: usize,
}
impl Drop for Editor {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.text.zeroize();
    }
}
impl Editor {
    pub fn new(text: String) -> Self {
        let cursor = text.len();
        Self { text, cursor }
    }
    pub fn take(&mut self) -> String {
        self.cursor = 0;
        std::mem::take(&mut self.text)
    }
    pub fn insert(&mut self, text: &str) {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        let text: String = text
            .chars()
            .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
            .collect();
        if self.text.len() + text.len() > 1024 * 1024 {
            return;
        }
        self.text.insert_str(self.cursor, &text);
        self.cursor += text.len();
    }
    pub fn left(&mut self) {
        self.cursor = self.text[..self.cursor]
            .grapheme_indices(true)
            .next_back()
            .map_or(0, |(i, _)| i);
    }
    pub fn right(&mut self) {
        if let Some(g) = self.text[self.cursor..].graphemes(true).next() {
            self.cursor += g.len();
        }
    }
    pub fn backspace(&mut self) {
        let end = self.cursor;
        self.left();
        self.text.replace_range(self.cursor..end, "");
    }
    pub fn delete(&mut self) {
        let start = self.cursor;
        self.right();
        self.text.replace_range(start..self.cursor, "");
        self.cursor = start;
    }
    pub fn home(&mut self) {
        self.cursor = self.text[..self.cursor].rfind('\n').map_or(0, |p| p + 1);
    }
    pub fn end(&mut self) {
        self.cursor += self.text[self.cursor..]
            .find('\n')
            .unwrap_or(self.text.len() - self.cursor);
    }
    pub fn kill_line(&mut self) {
        let end = self.cursor;
        self.home();
        self.text.replace_range(self.cursor..end, "");
    }
    pub fn word_back(&mut self) {
        let end = self.cursor;
        while self.cursor > 0
            && self.text[..self.cursor]
                .chars()
                .next_back()
                .is_some_and(char::is_whitespace)
        {
            self.left();
        }
        while self.cursor > 0
            && self.text[..self.cursor]
                .chars()
                .next_back()
                .is_some_and(|c| !c.is_whitespace())
        {
            self.left();
        }
        self.text.replace_range(self.cursor..end, "");
    }
    pub fn vertical(&mut self, direction: i32) {
        let start = self.text[..self.cursor].rfind('\n').map_or(0, |p| p + 1);
        let column = UnicodeWidthStr::width(&self.text[start..self.cursor]);
        let target = if direction < 0 {
            if start == 0 {
                return;
            }
            self.text[..start - 1].rfind('\n').map_or(0, |p| p + 1)
        } else {
            let Some(next) = self.text[self.cursor..].find('\n') else {
                return;
            };
            self.cursor + next + 1
        };
        self.cursor = target;
        let mut width = 0;
        for g in self.text[target..]
            .split('\n')
            .next()
            .unwrap_or("")
            .graphemes(true)
        {
            if width + g.width() > column {
                break;
            }
            width += g.width();
            self.cursor += g.len();
        }
    }
    /// The exact same hard wrap is used for display and cursor placement.
    pub fn visual(&self, columns: usize, secret: bool) -> (Vec<String>, (usize, usize)) {
        let columns = columns.max(1);
        let mut rows = vec![String::new()];
        let mut x = 0;
        let mut cursor = (0, 0);
        for (offset, g) in self
            .text
            .grapheme_indices(true)
            .chain(std::iter::once((self.text.len(), "")))
        {
            let shown = if secret && !g.is_empty() {
                "•"
            } else if g == "\t" {
                "    "
            } else {
                g
            };
            let width = shown.width();
            if !g.is_empty() && g != "\n" && x + width > columns {
                rows.push(String::new());
                x = 0;
            }
            if offset == self.cursor {
                cursor = (x, rows.len() - 1);
            }
            if g == "\n" {
                rows.push(String::new());
                x = 0;
            } else {
                rows.last_mut().unwrap().push_str(shown);
                x += width;
            }
        }
        if cursor.0 >= columns {
            rows.push(String::new());
            cursor = (0, rows.len() - 1);
        }
        (rows, cursor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn editing_keeps_cyrillic_and_combined_emoji_intact() {
        let mut e = Editor::new("Привет 👩‍💻е\u{301}".into());
        e.backspace();
        assert_eq!(e.text, "Привет 👩‍💻");
        e.left();
        e.delete();
        assert_eq!(e.text, "Привет ");
        e.insert("мир");
        e.home();
        e.right();
        e.delete();
        assert_eq!(e.text, "Пивет мир");
    }
    #[test]
    fn vertical_movement_preserves_display_column() {
        let mut e = Editor::new("界аб\nдлинный\nя".into());
        e.cursor = "界а".len();
        e.vertical(1);
        assert_eq!(&e.text[..e.cursor], "界аб\nдли");
        e.vertical(1);
        assert_eq!(e.cursor, e.text.len());
        e.vertical(-1);
        assert_eq!(&e.text[..e.cursor], "界аб\nд");
    }
    #[test]
    fn paste_is_literal_and_cannot_inject_terminal_controls() {
        let mut e = Editor::default();
        e.insert("/stop\r\n\u{1b}[31m\u{0}привет");
        assert_eq!(e.text, "/stop\n[31mпривет");
        assert_eq!(e.cursor, e.text.len());
        let (rows, position) = e.visual(7, false);
        assert_eq!(rows[0], "/stop");
        assert_eq!(position.1, 2);
        let (_, position) = Editor::new("1234".into()).visual(4, false);
        assert_eq!(position, (0, 1));
    }
}
