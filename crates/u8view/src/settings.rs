//! The player's settings, kept between runs in `~/.config/open-ultima8/settings.cfg` as
//! `key = value` lines (unknown keys are ignored, missing ones take the defaults).
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// Upscaler by name (see `Mode::name`).
    pub upscaler: String,
    /// The interface's size (whole steps) and the world's zoom; 0: none chosen yet, they're
    /// picked for the screen (its size and how many pixels it has to a point).
    pub scale: u32,
    pub zoom: f32,
    pub classic_text: bool,
    pub music: bool,
    pub effects: bool,
    /// Footsteps heard.
    pub footsteps: bool,
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
    /// Restored content: pieces the original has but never links up (not the original game).
    pub restored: bool,
    /// Show where the Avatar is and what's going on on the player's Discord profile.
    pub discord: bool,
    /// Where the game's files are, once the player has picked the folder (empty: look for them).
    pub data: String,
    /// The game's been played to the end (the main menu then offers the quotes and the ending).
    pub finished: bool,
    /// A game controller: in use; its pointer's speed (0.1–1); its sticks' dead zone; the right
    /// stick walks and the left moves the pointer.
    pub pad: bool,
    pub pad_speed: f32,
    pub pad_deadzone: f32,
    pub swap_sticks: bool,
    /// Buttons on the screen's edge once it's been touched.
    pub touch_buttons: bool,
    /// The language to play in, by its files' letter (empty: the install's own; see
    /// `u8data::lang`). Takes effect at the next start.
    pub language: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            upscaler: "nearest".into(),
            scale: 0,
            zoom: 0.0,
            classic_text: false,
            music: true,
            effects: true,
            footsteps: true,
            volume: 0.5,
            ambience: 0.5,
            cutaway: true,
            smooth_camera: true,
            fullscreen: false,
            left_handed: false,
            gamma: 1.0,
            colour_fix: false,
            cheats: false,
            restored: false,
            discord: true,
            data: String::new(),
            finished: false,
            pad: true,
            pad_speed: 0.5,
            pad_deadzone: 0.2,
            swap_sticks: false,
            touch_buttons: true,
            language: String::new(),
        }
    }
}

fn path() -> Option<PathBuf> {
    Some(dir()?.join("settings.cfg"))
}

/// Our own folder for settings (a SoundFont can go there too).
pub fn dir() -> Option<PathBuf> {
    Some(config_dir(|k| std::env::var_os(k), cfg!(windows))?.join("open-ultima8"))
}

/// Where settings go: `XDG_CONFIG_HOME`, else on Windows `%APPDATA%` (Windows has no `HOME` as
/// a rule), else `~/.config`.
fn config_dir(var: impl Fn(&str) -> Option<std::ffi::OsString>, windows: bool) -> Option<PathBuf> {
    let home = || var("HOME").map(|h| PathBuf::from(h).join(".config"));
    let appdata = || var("APPDATA").filter(|_| windows).map(PathBuf::from);
    var("XDG_CONFIG_HOME").map(PathBuf::from).or_else(appdata).or_else(home)
}

impl Settings {
    pub fn load() -> Self {
        let text = path().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
        Self::parse(&text)
    }

    pub fn parse(text: &str) -> Self {
        let mut s = Self::default();
        let (mut zoom, mut v2) = (None, false);
        for (k, v) in text.lines().filter_map(|l| l.split_once('=')).map(|(k, v)| (k.trim(), v.trim())) {
            let on = v == "true";
            match k {
                "upscaler" => s.upscaler = v.to_string(),
                "scale" => s.scale = v.parse().unwrap_or(s.scale),
                "zoom" => zoom = v.parse::<f32>().ok().filter(|z| z.is_finite()),
                "version" => v2 = v.parse::<u32>().is_ok_and(|n| n >= 2),
                "classic_text" => s.classic_text = on,
                "music" => s.music = on,
                "effects" => s.effects = on,
                "footsteps" => s.footsteps = on,
                "volume" => s.volume = v.parse::<f32>().map_or(s.volume, |x| x.clamp(0.0, 1.0)),
                "ambience" => s.ambience = v.parse::<f32>().map_or(s.ambience, |x| x.clamp(0.0, 1.0)),
                "cutaway" => s.cutaway = on,
                "smooth_camera" => s.smooth_camera = on,
                "fullscreen" => s.fullscreen = on,
                "left_handed" => s.left_handed = on,
                "gamma" => s.gamma = v.parse::<f32>().map_or(s.gamma, |x| x.clamp(0.3, 3.0)),
                "colour_fix" => s.colour_fix = on,
                "cheats" => s.cheats = on,
                "restored" => s.restored = on,
                "discord" => s.discord = on,
                "data" => s.data = v.to_string(),
                "language" => s.language = v.chars().take(1).collect::<String>().to_ascii_uppercase(),
                "finished" => s.finished = on,
                "pad" => s.pad = on,
                "pad_speed" => s.pad_speed = v.parse::<f32>().map_or(s.pad_speed, |x| x.clamp(0.1, 1.0)),
                "pad_deadzone" => s.pad_deadzone = v.parse::<f32>().map_or(s.pad_deadzone, |x| x.clamp(0.05, 0.5)),
                "swap_sticks" => s.swap_sticks = on,
                "touch_buttons" => s.touch_buttons = on,
                _ => {}
            }
        }
        // (Before the zoom, the scale was both; with neither chosen, both are picked. Files from
        // before the zoom was picked for the screen (no `version` line) wrote its old default, 2,
        // even when nobody chose it: with no interface size chosen either, that's not chosen.)
        s.zoom = zoom.unwrap_or(if s.scale == 0 { 0.0 } else { s.scale.max(2) as f32 }).max(0.0);
        if !v2 && s.scale == 0 && s.zoom == 2.0 {
            s.zoom = 0.0;
        }
        s
    }

    pub fn to_text(&self) -> String {
        format!(
            "# Open Ultima8 settings (written by the game; edit while it isn't running)\n\
             version = 2\nupscaler = {}\nscale = {}\nzoom = {:.2}\nclassic_text = {}\nmusic = {}\neffects = {}\nfootsteps = {}\nvolume = {:.1}\nambience = {:.1}\n\
             cutaway = {}\nsmooth_camera = {}\nfullscreen = {}\nleft_handed = {}\ngamma = {:.1}\ncolour_fix = {}\ncheats = {}\nrestored = {}\n\
             discord = {}\ndata = {}\nfinished = {}\npad = {}\npad_speed = {:.1}\npad_deadzone = {:.2}\nswap_sticks = {}\ntouch_buttons = {}\nlanguage = {}\n",
            self.upscaler,
            self.scale,
            self.zoom,
            self.classic_text,
            self.music,
            self.effects,
            self.footsteps,
            self.volume,
            self.ambience,
            self.cutaway,
            self.smooth_camera,
            self.fullscreen,
            self.left_handed,
            self.gamma,
            self.colour_fix,
            self.cheats,
            self.restored,
            self.discord,
            self.data,
            self.finished,
            self.pad,
            self.pad_speed,
            self.pad_deadzone,
            self.swap_sticks,
            self.touch_buttons,
            self.language
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

    /// Windows keeps them in %APPDATA% (it has no HOME); elsewhere ~/.config, or XDG_CONFIG_HOME.
    #[test]
    fn settings_have_a_home_on_every_system() {
        let env = |vars: &'static [(&'static str, &'static str)]| {
            move |k: &str| vars.iter().find(|v| v.0 == k).map(|v| v.1.into())
        };
        let win = env(&[("APPDATA", r"C:\Users\r\AppData\Roaming")]);
        assert_eq!(config_dir(win, true), Some(PathBuf::from(r"C:\Users\r\AppData\Roaming")));
        assert_eq!(config_dir(win, false), None, "(APPDATA only counts on Windows)");
        let unix = env(&[("HOME", "/home/r"), ("APPDATA", "x")]);
        assert_eq!(config_dir(unix, false), Some(PathBuf::from("/home/r/.config")));
        let xdg = env(&[("HOME", "/home/r"), ("XDG_CONFIG_HOME", "/cfg"), ("APPDATA", "x")]);
        assert_eq!(config_dir(xdg, true), Some(PathBuf::from("/cfg")));
        let git_bash = env(&[("HOME", "/c/Users/r"), ("APPDATA", "C:/A")]);
        assert_eq!(config_dir(git_bash, true), Some(PathBuf::from("C:/A")), "APPDATA before a HOME");
    }

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
            pad: false,
            pad_speed: 0.8,
            pad_deadzone: 0.3,
            swap_sticks: true,
            touch_buttons: false,
            language: "G".into(),
            restored: true,
            footsteps: false,
            ..Default::default()
        };
        assert_eq!(Settings::parse(&s.to_text()), s);
        let odd = Settings::parse("scale = lots\nvolume = 9\nnonsense\nwho = me\n");
        assert_eq!((odd.scale, odd.volume, odd.zoom), (0, 1.0, 0.0), "no sizes yet: picked for the screen");
        assert_eq!(Settings::parse("scale = 4\n").zoom, 4.0, "an older file's scale was the zoom too");
        assert_eq!(Settings::parse("scale = 0\nzoom = 2.00\n").zoom, 0.0, "0.5.3's untouched default: picked");
        assert_eq!(Settings::parse("scale = 0\nzoom = 3.00\n").zoom, 3.0, "a zoom chosen: kept");
        let two = Settings { zoom: 2.0, ..Default::default() };
        assert_eq!(Settings::parse(&two.to_text()).zoom, 2.0, "now 2 chosen stays chosen");
        assert_eq!(Settings::parse("volume = 0.6\n").ambience, 0.5, "background sounds default to half");
        let pad = Settings::parse("pad_speed = 7\npad_deadzone = -1\n");
        assert_eq!(
            (pad.pad, pad.pad_speed, pad.pad_deadzone),
            (true, 1.0, 0.05),
            "the pad's on, odd values kept in range"
        );
    }
}
