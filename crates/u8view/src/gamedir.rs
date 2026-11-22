//! Finding the game's files: the folder given, the one remembered, the game folder the program
//! sits in (or its U8 folder), or the player picks it.
use super::*;

/// `dir` if the game's files are in it, else its U8 folder (any case) if they're in that.
fn game_root(dir: &Path) -> Option<PathBuf> {
    let has_game = |p: &Path| u8data::find_file(p, "STATIC/U8SHAPES.FLX").is_ok();
    [Some(dir.to_path_buf()), u8data::find_file(dir, "U8").ok()].into_iter().flatten().find(|p| has_game(p))
}

/// Where the game's files are: the folder given on the command line; the one picked last time;
/// the game folder itself (the client copied into it) or a U8 folder in it — where it's started
/// from, the program's own folder and up to three above; or, failing those, the player picks the
/// folder (remembered for next time).
pub(crate) fn find_data(arg: Option<&String>) -> Result<PathBuf> {
    if let Some(a) = arg {
        return Ok(PathBuf::from(a));
    }
    let mut s = settings::Settings::load();
    if let Some(p) = Some(&s.data).filter(|d| !d.is_empty()).and_then(|d| game_root(Path::new(d))) {
        return Ok(p);
    }
    let mut dirs = vec![PathBuf::from(".")];
    if let Ok(exe) = std::env::current_exe() {
        dirs.extend(exe.ancestors().skip(1).take(4).map(PathBuf::from));
    }
    if let Some(p) = dirs.iter().find_map(|d| game_root(d)) {
        return Ok(p);
    }
    let found = pick_data().ok_or_else(|| {
        anyhow!("couldn't find Ultima 8's files (STATIC/U8SHAPES.FLX); pass the game folder, e.g. u8view ~/Games/U8")
    })?;
    s.data = found.display().to_string();
    s.save();
    Ok(found)
}

/// Ask for the game's folder until one has its files (None: the player gave up).
fn pick_data() -> Option<PathBuf> {
    use rfd::{FileDialog, MessageButtons, MessageDialog, MessageLevel};
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
    loop {
        let mut dialog =
            FileDialog::new().set_title("Where is Ultima 8? Pick the game's folder (the one with STATIC in it)");
        if let Some(h) = &home {
            dialog = dialog.set_directory(h);
        }
        let dir = dialog.pick_folder()?;
        if let Some(root) = game_root(&dir) {
            return Some(root);
        }
        MessageDialog::new()
            .set_level(MessageLevel::Warning)
            .set_title("Not Ultima 8")
            .set_description(format!(
                "{} doesn't have Ultima 8's files in it (STATIC, USECODE, SOUND…). Pick the folder the game is installed in.",
                dir.display()
            ))
            .set_buttons(MessageButtons::Ok)
            .show();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The game folder is found whether it's picked itself or the folder with a U8 (any case)
    /// in it; anything else isn't the game.
    #[test]
    fn the_game_folder_is_recognised() {
        let t = std::env::temp_dir().join(format!("u8-root-{}", std::process::id()));
        let game = t.join("Games/ultima8");
        std::fs::create_dir_all(game.join("static")).unwrap();
        std::fs::write(game.join("static/u8shapes.flx"), b"").unwrap();
        std::fs::create_dir_all(t.join("client/u8/STATIC")).unwrap();
        std::fs::write(t.join("client/u8/STATIC/U8SHAPES.FLX"), b"").unwrap();
        assert_eq!(game_root(&game), Some(game.clone()), "the game folder itself");
        assert_eq!(game_root(&t.join("client")), Some(t.join("client/u8")), "its U8 folder");
        assert_eq!(game_root(&t.join("Games")), None, "not the game");
        std::fs::remove_dir_all(&t).ok();
    }
}
