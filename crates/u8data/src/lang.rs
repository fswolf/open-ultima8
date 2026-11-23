//! The game's languages. A translated install keeps its own files beside the English ones, named
//! by the language's letter: USECODE/GUSECODE.FLX (the scripts, so all the words), STATIC/GINTRO.SKF
//! (the intro's subtitles), STATIC/GCREDITS.DAT, SOUND/G<n>.FLX (speech). The game plays in the
//! translation it finds (or the one the player picks), and any file a translation lacks is
//! taken from English.
use crate::{find_file, Result};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};

/// The original's languages, by the letter their files start with.
pub const LANGUAGES: [(char, &str); 5] =
    [('E', "English"), ('G', "German"), ('F', "French"), ('S', "Spanish"), ('J', "Japanese")];

/// The language the player picked (0: none, it's found).
static WANTED: AtomicU8 = AtomicU8::new(0);

/// Play in `lang` where its files are there (None: the one found).
pub fn prefer(lang: Option<char>) {
    WANTED.store(lang.map_or(0, |c| c.to_ascii_uppercase() as u8), Ordering::Relaxed);
}

/// The languages whose scripts are in the install.
pub fn available(root: &Path) -> Vec<char> {
    LANGUAGES.iter().map(|l| l.0).filter(|c| find_file(root, &format!("USECODE/{c}USECODE.FLX")).is_ok()).collect()
}

/// The language to play in: the one picked, if it's there; else a translation, if there is one
/// (an install of one keeps the English files too); else English.
pub fn of(root: &Path) -> char {
    let have = available(root);
    let wanted = WANTED.load(Ordering::Relaxed) as char;
    if have.contains(&wanted) {
        return wanted;
    }
    have.into_iter().find(|&c| c != 'E').unwrap_or('E')
}

pub fn name(lang: char) -> &'static str {
    LANGUAGES.iter().find(|l| l.0 == lang).map_or("English", |l| l.1)
}

/// A file named by language (`{}` stands for its letter), in the language played, else English.
pub fn file(root: &Path, pattern: &str) -> Result<PathBuf> {
    let at = |c: char| find_file(root, &pattern.replace("{}", &c.to_string()));
    at(of(root)).or_else(|_| at('E'))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A German install (English and German scripts) plays in German, English if picked; files a
    /// translation hasn't got come from English.
    #[test]
    fn a_translation_is_found_and_can_be_put_aside() {
        let root = std::env::temp_dir().join(format!("u8-lang-{}", std::process::id()));
        std::fs::create_dir_all(root.join("usecode")).unwrap();
        std::fs::create_dir_all(root.join("STATIC")).unwrap();
        for f in ["usecode/eusecode.flx", "usecode/GUSECODE.FLX", "STATIC/EINTRO.SKF"] {
            std::fs::write(root.join(f), b"").unwrap();
        }
        assert_eq!(available(&root), ['E', 'G']);
        assert_eq!(of(&root), 'G');
        assert!(file(&root, "USECODE/{}USECODE.FLX").unwrap().ends_with("GUSECODE.FLX"));
        assert!(file(&root, "STATIC/{}INTRO.SKF").unwrap().ends_with("EINTRO.SKF"), "English when it hasn't one");
        prefer(Some('e'));
        assert_eq!(of(&root), 'E');
        prefer(Some('F'));
        assert_eq!(of(&root), 'G', "not there: the one found");
        prefer(None);
        std::fs::remove_dir_all(&root).ok();
    }
}
