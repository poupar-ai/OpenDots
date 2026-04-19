//! Estimativa heurística de tokens para orçamento de injeção de contexto.
//!
//! Porta literal de `estimateTokens(str)` em `lib.cjs`:
//! ```js
//! function estimateTokens(str) {
//!   if (!str) return 0;
//!   return Math.ceil(str.length / 3.5);
//! }
//! ```
//!
//! ## Armadilha de paridade — `str.length` do JS ≠ `String::len()` do Rust
//!
//! Em JavaScript, `str.length` conta **UTF-16 code units** (legacy do
//! design da V8). Um `é` é 1 unit, mas um emoji suplementar `😀` é 2
//! (surrogate pair), e um emoji composto com ZWJ como `👨‍👩‍👧` pode
//! chegar a 8.
//!
//! Em Rust, `String::len()` retorna **bytes UTF-8**. Para `café`:
//! - JS: `.length` = 4
//! - Rust: `.len()` = 5 (porque `é` ocupa 2 bytes)
//!
//! Para paridade byte-a-byte do valor retornado, usamos
//! `encode_utf16().count()` — exatamente a mesma contagem que o JS.
//!
//! ## Caller real
//!
//! `plugin/scripts/session-init.cjs:140` passa blocos de texto
//! (recap, rules, claude.md). Tipicamente PT/EN com acentos — a
//! divergência UTF-16 ≠ UTF-8 morde imediatamente se não tratarmos.

/// Estima tokens em um bloco de texto usando a heurística
/// `ceil(utf16_units / 3.5)`.
///
/// Retorna `0` para string vazia (equivalente a `if (!str)` do JS
/// aplicado a `&str`, que só pode ser `""` em Rust).
pub fn estimate_tokens(s: &str) -> usize {
    if s.is_empty() {
        return 0;
    }
    let utf16_len = s.encode_utf16().count();
    // `Math.ceil(n/3.5)` em JS: casting para f64 e `ceil` batem para
    // qualquer inteiro até ~2^53 (f64 precisa representar exato), o
    // que cobre todos os tamanhos de texto realistas.
    (utf16_len as f64 / 3.5_f64).ceil() as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_zero() {
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn ascii_matches_js_length_ceil() {
        // "hello world" length=11 → ceil(11/3.5)=4
        assert_eq!(estimate_tokens("hello world"), 4);
    }

    #[test]
    fn single_bmp_char_counts_as_one_unit() {
        // ☕ é BMP, length JS = 1, ceil(1/3.5) = 1
        assert_eq!(estimate_tokens("☕"), 1);
    }

    #[test]
    fn supplementary_emoji_counts_as_two_units() {
        // 😀 é U+1F600, fora do BMP → surrogate pair → JS length = 2
        assert_eq!(estimate_tokens("😀"), 1); // ceil(2/3.5)=1
    }

    #[test]
    fn zwj_sequence_counts_all_units() {
        // 👨‍👩‍👧 = 3 emojis suplementares (2 units cada) + 2 ZWJs (1 unit
        // cada) = 8 UTF-16 units. ceil(8/3.5) = 3.
        assert_eq!(estimate_tokens("👨‍👩‍👧"), 3);
    }

    #[test]
    fn portuguese_text_counts_utf16_not_bytes() {
        // "café" tem 4 chars / 4 UTF-16 units / 5 bytes UTF-8.
        // Precisa retornar ceil(4/3.5) = 2, não ceil(5/3.5) = 2
        // (coincide neste caso). Usamos exemplo maior para diferenciar:
        // "cafééééééé" → 10 UTF-16 units, 16 bytes UTF-8.
        //   ceil(10/3.5) = 3 (correto, JS)
        //   ceil(16/3.5) = 5 (errado, bytes)
        assert_eq!(estimate_tokens("caféééééé"), 3);
        assert_eq!("caféééééé".len(), 15); // confirma bytes ≠ units
        assert_eq!("caféééééé".encode_utf16().count(), 9);
    }

    #[test]
    fn short_string_zero_is_not_empty() {
        // "0" não é vazia em Rust (&str), tokens = ceil(1/3.5) = 1.
        // Paridade com `!"0"` em JS que é false.
        assert_eq!(estimate_tokens("0"), 1);
    }
}
