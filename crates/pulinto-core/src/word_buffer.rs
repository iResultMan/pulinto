//! Word buffer state machine and undo stack management.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversionRecord {
    pub original: String,
    pub converted: String,
    pub from_layout_id: String,
    pub to_layout_id: String,
}

#[derive(Debug, Clone, Default)]
pub struct WordBuffer {
    buffer: String,
    undo_stack: Vec<ConversionRecord>,
}

impl WordBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_char(&mut self, ch: char) {
        self.buffer.push(ch);
    }

    pub fn pop_char(&mut self) {
        self.buffer.pop();
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
    }

    pub fn current_word(&self) -> &str {
        &self.buffer
    }

    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    pub fn len(&self) -> usize {
        self.buffer.chars().count()
    }

    pub fn is_delimiter(ch: char) -> bool {
        // DO NOT split on '.', ',', ';', '\'', '[', ']'
        // because in the Russian layout, these keys map to actual letters:
        // ',' -> 'б', '.' -> 'ю', ';' -> 'ж', '\'' -> 'э', '[' -> 'х', ']' -> 'ъ'
        matches!(ch, ' ' | '\t' | '\n' | '\r' | '?' | '!' | ':')
    }

    pub fn record_conversion(
        &mut self,
        original: String,
        converted: String,
        from_layout_id: String,
        to_layout_id: String,
    ) {
        self.undo_stack.push(ConversionRecord {
            original,
            converted,
            from_layout_id,
            to_layout_id,
        });
    }

    pub fn pop_undo(&mut self) -> Option<ConversionRecord> {
        self.undo_stack.pop()
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_word_buffer_flow() {
        let mut wb = WordBuffer::new();
        wb.push_char('g');
        wb.push_char('h');
        wb.push_char('b');
        assert_eq!(wb.current_word(), "ghb");

        wb.pop_char();
        assert_eq!(wb.current_word(), "gh");

        wb.clear();
        assert!(wb.is_empty());
    }

    #[test]
    fn test_undo_stack() {
        let mut wb = WordBuffer::new();
        wb.record_conversion(
            "ghbdtn".to_string(),
            "привет".to_string(),
            "us".to_string(),
            "ru".to_string(),
        );

        assert!(wb.can_undo());
        let rec = wb.pop_undo().unwrap();
        assert_eq!(rec.original, "ghbdtn");
        assert_eq!(rec.converted, "привет");
        assert!(!wb.can_undo());
    }
}
