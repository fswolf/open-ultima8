//! The player's settings, kept between runs in `~/.config/open-ultima8/settings.cfg` as
//! `key = value` lines (unknown keys are ignored, missing ones take the defaults).
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// Upscaler by name (see `Mode::name`).
    pub upscaler: String,
    /// The interface's size (whole steps) and the world's zoom.
    pub scale: u32,
    pub zoom: f32,
    pub classic_text: bool,
    pub music: bool,
    pub effects: bool,
    /// 0–1.
    pub volume: f32,
    /// Background sounds (fires, drips, fountains), 0–1.
    pub ambience: f32,
    pub cutaway: bool,
    /// The view eases up and down with the Avatar's footing instead of every hop (ours).
    pub smooth_camera: bool,
    pub fullscreen: bool,
    pub left_handed: bool,
    pub gamma: f32,
    pub colour_fix: bool,
    /// The original's cheats: clicking the Avatar opens its developers' menu.
    pub cheats: bool,
    /// Show where the Avatar is and what's going on on the player's Discord profile.
    pub discord: bool,
    /// Where the game's files are, once the player has picked the folder (empty: look for them).
    pub data: String,
    /// The game's been played to the end (the main menu then offers the quotes and the ending).
    pub finished: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            upscaler: "nearest".into(),
            scale: 2,
            zoom: 2.0,
            classic_text: false,
            music: true,
            effects: true,
            volume: 0.5,
            ambience: 0.5,
            cutaway: true,
            smooth_camera: true,
            fullscreen: false,
            left_handed: false,
            gamma: 1.0,
            colour_fix: false,
            cheats: false,
            discord: true,
            data: String::new(),
            finished: false,
        }
    }
}

fn path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("open-ultima8").join("settings.cfg"))
}

impl Settings {
    pub fn load() -> Self {
        let text = path().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
        Self::parse(&text)
    }

    pub fn parse(text: &str) -> Self {
        let mut s = Self::default();
        let mut zoom = None;
        for (k, v) in text.lines().filter_map(|l| l.split_once('=')).map(|(k, v)| (k.trim(), v.trim())) {
            let on = v == "true";
            match k {
                "upscaler" => s.upscaler = v.to_string(),
                "scale" => s.scale = v.parse().unwrap_or(s.scale),
                "zoom" => zoom = v.parse::<f32>().ok().filter(|z| z.is_finite()),
                "classic_text" => s.classic_text = on,
                "music" => s.music = on,
                "effects" => s.effects = on,
                "volume" => s.volume = v.parse::<f32>().map_or(s.volume, |x| x.clamp(0.0, 1.0)),
                "ambience" => s.ambience = v.parse::<f32>().map_or(s.ambience, |x| x.clamp(0.0, 1.0)),
                "cutaway" => s.cutaway = on,
                "smooth_camera" => s.smooth_camera = on,
                "fullscreen" => s.fullscreen = on,
                "left_handed" => s.left_handed = on,
                "gamma" => s.gamma = v.parse::<f32>().map_or(s.gamma, |x| x.clamp(0.3, 3.0)),
                "colour_fix" => s.colour_fix = on,
                "cheats" => s.cheats = on,
                "discord" => s.discord = on,
                "data" => s.data = v.to_string(),
                "finished" => s.finished = on,
                _ => {}
            }
        }
        // (Before the zoom, the scale was both.)
        s.zoom = zoom.unwrap_or(s.scale as f32);
        s
    }

    pub fn to_text(&self) -> String {
        format!(
            "# Open Ultima8 settings (written by the game; edit while it isn't running)\n\
             upscaler = {}\nscale = {}\nzoom = {:.2}\nclassic_text = {}\nmusic = {}\neffects = {}\nvolume = {:.1}\nambience = {:.1}\n\
             cutaway = {}\nsmooth_camera = {}\nfullscreen = {}\nleft_handed = {}\ngamma = {:.1}\ncolour_fix = {}\ncheats = {}\n\
             discord = {}\ndata = {}\nfinished = {}\n",
            self.upscaler,
            self.scale,
            self.zoom,
            self.classic_text,
            self.music,
            self.effects,
            self.volume,
            self.ambience,
            self.cutaway,
            self.smooth_camera,
            self.fullscreen,
            self.left_handed,
            self.gamma,
            self.colour_fix,
            self.cheats,
            self.discord,
            self.data,
            self.finished
        )
    }

    pub fn save(&self) {
        let Some(p) = path() else { return };
        let write = || -> std::io::Result<()> {
            std::fs::create_dir_all(p.parent().unwrap())?;
            std::fs::write(&p, self.to_text())
        };
        if let Err(e) = write() {
            eprintln!("couldn't save settings to {}: {e}", p.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_and_tolerate_junk() {
        let s = Settings {
            upscaler: "xBRZ sprites".into(),
            scale: 3,
            zoom: 3.75,
            music: false,
            volume: 0.7,
            ambience: 0.3,
            discord: false,
            smooth_camera: false,
            data: "/home/me/Games/Ultima 8".into(),
            finished: true,
            ..Default::default()
        };
        assert_eq!(Settings::parse(&s.to_text()), s);
        let odd = Settings::parse("scale = lots\nvolume = 9\nnonsense\nwho = me\n");
        assert_eq!((odd.scale, odd.volume, odd.zoom), (2, 1.0, 2.0));
        assert_eq!(Settings::parse("scale = 4\n").zoom, 4.0, "an older file's scale was the zoom too");
        assert_eq!(Settings::parse("volume = 0.6\n").ambience, 0.5, "background sounds default to half");
    }
}
