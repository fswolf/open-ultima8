//! The game's text is in the DOS character set (code page 437): its translations' letters, ä ö ü
//! ß é ñ…, are single bytes from 0x80 up (ä is 0x84). Text is read into Unicode here, and its
//! fonts (one glyph per byte) are found through [`byte`].

/// Code page 437's characters for 0x80–0xFF.
const HIGH: [char; 128] = [
    'Ç', 'ü', 'é', 'â', 'ä', 'à', 'å', 'ç', 'ê', 'ë', 'è', 'ï', 'î', 'ì', 'Ä', 'Å', //
    'É', 'æ', 'Æ', 'ô', 'ö', 'ò', 'û', 'ù', 'ÿ', 'Ö', 'Ü', '¢', '£', '¥', '₧', 'ƒ', //
    'á', 'í', 'ó', 'ú', 'ñ', 'Ñ', 'ª', 'º', '¿', '⌐', '¬', '½', '¼', '¡', '«', '»', //
    '░', '▒', '▓', '│', '┤', '╡', '╢', '╖', '╕', '╣', '║', '╗', '╝', '╜', '╛', '┐', //
    '└', '┴', '┬', '├', '─', '┼', '╞', '╟', '╚', '╔', '╩', '╦', '╠', '═', '╬', '╧', //
    '╨', '╤', '╥', '╙', '╘', '╒', '╓', '╫', '╪', '┘', '┌', '█', '▄', '▌', '▐', '▀', //
    'α', 'ß', 'Γ', 'π', 'Σ', 'σ', 'µ', 'τ', 'Φ', 'Θ', 'Ω', 'δ', '∞', 'φ', 'ε', '∩', //
    '≡', '±', '≥', '≤', '⌠', '⌡', '÷', '≈', '°', '∙', '·', '√', 'ⁿ', '²', '■', '\u{a0}',
];

/// Game bytes as text.
pub fn decode(d: &[u8]) -> String {
    d.iter().map(|&b| if b < 0x80 { b as char } else { HIGH[b as usize - 0x80] }).collect()
}

/// The game's byte for a character (`?` for one it hasn't got).
pub fn byte(c: char) -> u8 {
    match c {
        '\0'..='\u{7f}' => c as u8,
        _ => HIGH.iter().position(|&h| h == c).map_or(b'?', |i| 0x80 + i as u8),
    }
}

/// Text as game bytes.
pub fn encode(s: &str) -> Vec<u8> {
    s.chars().map(byte).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn german_letters_round_trip() {
        let game = b"Gr\x81\xe1e, M\x84dchen! \x8e\x99\x9a";
        assert_eq!(decode(game), "Grüße, Mädchen! ÄÖÜ");
        assert_eq!(encode(&decode(game)), game);
        assert_eq!(decode(b"Plain English."), "Plain English.");
        assert_eq!(byte('€'), b'?');
    }
}
