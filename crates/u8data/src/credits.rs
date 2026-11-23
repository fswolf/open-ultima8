//! STATIC/ECREDITS.DAT (credits; the first letter is the language) and QUOTES.DAT: text lightly
//! scrambled — each byte XORed with a value from its position — with NULs between lines. Lines
//! carry the original's layout marks (`+` title, `&` yellow centred, `}` red centred, `~` yellow
//! indented, `@` the end, `*` a line break, an `&…` run a rule); the viewer lays them out.

/// Unscramble a credits or quotes file into its lines.
pub fn decode(d: &[u8]) -> Vec<String> {
    let plain: Vec<u8> = d
        .iter()
        .enumerate()
        .map(|(i, &c)| {
            let x = match i {
                0 | 1 => 0,
                2 => 0xE1,
                _ => 0x20 * (i + 1) + (i >> 1) + (i % 0x40) * ((i & 0xC0) >> 6) * 0x40,
            };
            c ^ x as u8
        })
        .collect();
    // (In the DOS character set: the French and German credits' accented letters.)
    crate::text::decode(&plain).split('\0').map(|l| l.replace("%%", "%")).collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn unscrambles() {
        // "+AB" and a NUL, scrambled as the game does.
        let plain = b"+AB\0x";
        let d: Vec<u8> = plain
            .iter()
            .enumerate()
            .map(|(i, &c)| {
                c ^ match i {
                    0 | 1 => 0,
                    2 => 0xE1,
                    _ => (0x20 * (i + 1) + (i >> 1) + (i % 0x40) * ((i & 0xC0) >> 6) * 0x40) as u8,
                }
            })
            .collect();
        assert_eq!(super::decode(&d), ["+AB", "x"]);
    }
}
