//! `js_trim` — helper para emular `String.prototype.trim` do JS, que
//! strip também BOM (`U+FEFF`).
//!
//! ## Divergência corrigida
//!
//! Rust `str::trim` usa `char::is_whitespace` (Unicode property
//! `WS=Y`), que NÃO inclui ZWNBSP (U+FEFF). JS trim inclui. Para
//! paridade byte-a-byte em `loadMiniContext`, adicionamos BOM ao set
//! de whitespace aceito.
//!
//! LS (U+2028) e PS (U+2029) já são WS=Y em Unicode, cobertos por
//! `char::is_whitespace` do Rust. BOM é a única divergência relevante.
//!
//! Documentado em `docs/migration_findings.md` como F3-01.

/// Trim whitespace no estilo JS, inclusive BOM (`U+FEFF`).
pub fn js_trim(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace() || c == '\u{FEFF}')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_standard_whitespace() {
        assert_eq!(js_trim("  hello  "), "hello");
        assert_eq!(js_trim("\n\thello\r\n"), "hello");
    }

    #[test]
    fn trims_bom_prefix() {
        assert_eq!(js_trim("\u{FEFF}hello"), "hello");
    }

    #[test]
    fn trims_bom_suffix() {
        assert_eq!(js_trim("hello\u{FEFF}"), "hello");
    }

    #[test]
    fn trims_bom_both_sides() {
        assert_eq!(js_trim("\u{FEFF}hello\u{FEFF}"), "hello");
    }

    #[test]
    fn trims_mixed_bom_and_whitespace() {
        assert_eq!(js_trim("\u{FEFF}\n hello \t\u{FEFF}"), "hello");
    }

    #[test]
    fn preserves_internal_bom() {
        assert_eq!(js_trim("hel\u{FEFF}lo"), "hel\u{FEFF}lo");
    }

    #[test]
    fn empty_string_returns_empty() {
        assert_eq!(js_trim(""), "");
    }

    #[test]
    fn only_whitespace_returns_empty() {
        assert_eq!(js_trim("  \n\t\u{FEFF}  "), "");
    }

    #[test]
    fn unicode_whitespace_trimmed() {
        // U+00A0 NBSP e U+2028 LS são WS=Y.
        assert_eq!(js_trim("\u{00A0}hi\u{2028}"), "hi");
    }
}
