use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub fn truncate_to_width(value: &str, max_width: usize) -> String {
    if UnicodeWidthStr::width(value) <= max_width {
        return value.to_string();
    }
    if max_width == 0 {
        return String::new();
    }
    if max_width == 1 {
        return "…".to_string();
    }

    let target = max_width - 1;
    let mut width = 0;
    let mut out = String::new();
    for ch in value.chars() {
        let ch_width = ch.width().unwrap_or(0);
        if width + ch_width > target {
            break;
        }
        width += ch_width;
        out.push(ch);
    }
    out.push('…');
    out
}
