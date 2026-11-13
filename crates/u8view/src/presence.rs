//! Discord Rich Presence: where the Avatar is, what's going on, health and mana, and time played,
//! on the player's Discord profile. It talks to the Discord app on this computer over its local
//! socket, from its own thread (the game never waits on it); while Discord isn't running it does
//! nothing and tries again now and then.
use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The Discord application this shows as (discord.com/developers → Applications): its ID, and
/// the name of the art uploaded under Rich Presence → Art Assets. An empty ID: nothing is sent.
pub const APP_ID: &str = "1570753498172366848";
const LOGO: &str = "logo";
/// Discord takes at most five updates in 20 seconds: one every 15 is plenty. And how long to wait
/// before trying again when Discord isn't there.
const EVERY: Duration = Duration::from_secs(15);

/// Places on Pagan by map: the recall item's names (Central Tenebrae … Daemon's Crag), and the
/// maps that are part of them or are named for their music. Maps not listed are "Pagan".
const PLACES: &[(u16, &str)] = &[
    (3, "the Docks"),
    (5, "the East Road"),
    (6, "the Cemetery"),
    (8, "the Plateau"),
    (12, "Daemon's Crag"),
    (14, "Daemon's Crag"),
    (16, "Carthax Lake"),
    (26, "Carthax Lake"),
    (28, "Carthax Lake"),
    (31, "the Hall of the Mountain King"),
    (37, "Argentrock Isle"),
    (39, "Tenebrae"),
    (40, "Tenebrae"),
    (41, "Tenebrae"),
    (43, "the Pentagram"),
    (49, "the Catacombs"),
    (50, "the Catacombs"),
    (54, "the Shrine"),
];

/// The place name for map `map`.
pub fn place(map: u16) -> &'static str {
    PLACES.iter().find(|p| p.0 == map).map_or("Pagan", |p| p.1)
}

/// What's going on, for the status line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Doing {
    Exploring,
    Fighting,
    Talking,
    Reading,
    Menu,
    Watching,
    Dead,
}

impl Doing {
    fn text(self) -> &'static str {
        match self {
            Doing::Exploring => "Exploring",
            Doing::Fighting => "In combat",
            Doing::Talking => "Talking",
            Doing::Reading => "Reading",
            Doing::Menu => "In the menu",
            Doing::Watching => "Watching a film",
            Doing::Dead => "Dead",
        }
    }
}

/// What the profile shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub map: u16,
    pub doing: Doing,
    /// (now, most) of hit points and mana.
    pub hp: (i16, i16),
    pub mana: (i16, i16),
}

impl Status {
    /// The profile's two lines: where, then what's going on and how the Avatar is.
    pub fn lines(&self) -> (String, String) {
        let where_ = format!("In {}", place(self.map));
        let how = format!(
            "{} · HP {}/{} · Mana {}/{}",
            self.doing.text(),
            self.hp.0.max(0),
            self.hp.1,
            self.mana.0.max(0),
            self.mana.1
        );
        (where_, how)
    }
}

/// The game's side: hand it the status as often as you like; it passes on changes.
pub struct Presence {
    tx: Option<mpsc::Sender<Option<Status>>>,
    last: Option<Option<Status>>,
}

impl Presence {
    /// Start talking to Discord (nothing happens until [`APP_ID`] is set).
    pub fn start() -> Self {
        if APP_ID.is_empty() {
            return Self { tx: None, last: None };
        }
        let (tx, rx) = mpsc::channel();
        let started = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64);
        let spawned = std::thread::Builder::new().name("discord".into()).spawn(move || run(APP_ID, rx, started));
        Self { tx: spawned.ok().map(|_| tx), last: None }
    }

    /// Show `status`, or (None) nothing. Only changes are passed on.
    pub fn show(&mut self, status: Option<Status>) {
        if self.last.as_ref() == Some(&status) {
            return;
        }
        if let Some(tx) = &self.tx {
            _ = tx.send(status.clone());
        }
        self.last = Some(status);
    }
}

/// The Discord thread: keep the latest wanted status, send it at most every [`EVERY`], connecting
/// (again) when Discord is there; ends when the game drops its [`Presence`].
fn run(app_id: &str, rx: mpsc::Receiver<Option<Status>>, started: i64) {
    let mut client: Option<DiscordIpcClient> = None;
    let (mut wanted, mut shown): (Option<Status>, Option<Option<Status>>) = (None, None);
    let mut next = Instant::now();
    loop {
        match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(s) => wanted = s,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        while let Ok(s) = rx.try_recv() {
            wanted = s;
        }
        if shown.as_ref() == Some(&wanted) || Instant::now() < next {
            continue;
        }
        next = Instant::now() + EVERY;
        if client.is_none() {
            let mut c = DiscordIpcClient::new(app_id);
            if c.connect().is_err() {
                continue; // Discord isn't running: try again later
            }
            client = Some(c);
        }
        let c = client.as_mut().unwrap();
        let sent = match &wanted {
            Some(s) => {
                let (details, state) = s.lines();
                let a = activity::Activity::new()
                    .details(details)
                    .state(state)
                    .assets(activity::Assets::new().large_image(LOGO).large_text("Ultima VIII: Pagan"))
                    .timestamps(activity::Timestamps::new().start(started));
                c.set_activity(a)
            }
            None => c.clear_activity(),
        };
        // (Read Discord's answer, so they don't pile up unread on the socket.)
        let sent = sent.and_then(|()| c.recv().map(drop));
        match sent {
            Ok(()) => shown = Some(wanted.clone()),
            Err(_) => {
                _ = c.close(); // Discord went away: reconnect next time
                client = None;
            }
        }
    }
    if let Some(mut c) = client {
        _ = c.clear_activity();
        _ = c.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_status_reads_well() {
        let s = Status { map: 40, doing: Doing::Fighting, hp: (24, 30), mana: (-1, 20) };
        assert_eq!(s.lines(), ("In Tenebrae".into(), "In combat · HP 24/30 · Mana 0/20".into()));
        assert_eq!(place(37), "Argentrock Isle");
        assert_eq!(place(200), "Pagan", "maps without a name");
    }

    /// The whole way to a (pretend) Discord: handshake, then the activity with both lines, the
    /// logo and the start time; nothing more until something changes.
    #[cfg(unix)]
    #[test]
    fn it_tells_discord() {
        use serde_json::Value;
        use std::io::{Read, Write};
        use std::os::unix::net::UnixListener;
        let dir = std::env::temp_dir().join(format!("u8-discord-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let listener = UnixListener::bind(dir.join("discord-ipc-0")).unwrap();
        std::env::set_var("XDG_RUNTIME_DIR", &dir);
        let frame = |s: &mut std::os::unix::net::UnixStream| -> (u32, Value) {
            let mut h = [0u8; 8];
            s.read_exact(&mut h).unwrap();
            let (op, n) =
                (u32::from_le_bytes(h[..4].try_into().unwrap()), u32::from_le_bytes(h[4..].try_into().unwrap()));
            let mut d = vec![0; n as usize];
            s.read_exact(&mut d).unwrap();
            (op, serde_json::from_slice(&d).unwrap())
        };
        let reply = |s: &mut std::os::unix::net::UnixStream, v: &str| {
            s.write_all(&[1u32.to_le_bytes(), (v.len() as u32).to_le_bytes()].concat()).unwrap();
            s.write_all(v.as_bytes()).unwrap();
        };
        let (tx, rx) = mpsc::channel();
        let game = std::thread::spawn(move || run("1234", rx, 1_700_000_000_000));
        let status = Status { map: 37, doing: Doing::Exploring, hp: (30, 30), mana: (5, 20) };
        tx.send(Some(status)).unwrap();
        let (mut s, _) = listener.accept().unwrap();
        let (op, hello) = frame(&mut s);
        assert_eq!((op, hello["client_id"].as_str()), (0, Some("1234")), "handshake");
        reply(&mut s, r#"{"cmd":"DISPATCH","evt":"READY"}"#);
        let (op, set) = frame(&mut s);
        reply(&mut s, r#"{"cmd":"SET_ACTIVITY"}"#);
        let a = &set["args"]["activity"];
        assert_eq!((op, set["cmd"].as_str()), (1, Some("SET_ACTIVITY")));
        assert_eq!(a["details"], "In Argentrock Isle");
        assert_eq!(a["state"], "Exploring · HP 30/30 · Mana 5/20");
        assert_eq!(a["assets"]["large_image"], LOGO);
        assert_eq!(a["timestamps"]["start"], 1_700_000_000_000i64);
        drop(tx); // the game ends: the thread clears the status and goes
        let (_, cleared) = frame(&mut s);
        assert!(cleared["args"]["activity"].is_null());
        game.join().unwrap();
        std::fs::remove_dir_all(&dir).ok();
    }
}
