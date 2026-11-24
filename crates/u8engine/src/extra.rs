//! What the original's saves have no room for, kept beside them in SAVEGAME/U8SAVE.nnn.ou8:
//! use built up towards the Avatar's next STR/DEX/INT point, the healing clock and hunger, whom
//! fighters on the current map are after, where changelings are in their changes (the original
//! keeps their brains in its process list), the Avatar hanging from a ledge or knocked flat, and
//! which pieces of restored content have been put in. It carries a fingerprint of the save it goes
//! with, so a save written since by the original game (DOSBox) isn't paired with stale numbers.
use crate::avatar::Plight;
use crate::game::Game;
use crate::world::Target;
use std::path::{Path, PathBuf};

const HEADER: &str = "open-ultima8 1";

/// The side file of a save file.
pub fn path_for(save: &Path) -> PathBuf {
    PathBuf::from(format!("{}.ou8", save.display()))
}

/// FNV-1a over the save's bytes.
pub fn fingerprint(d: &[u8]) -> u64 {
    d.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3))
}

impl Game {
    /// The side file's text for a save whose bytes have `print`.
    pub fn extra(&self, print: u64) -> String {
        let g = self.growth;
        let (ticks, hunger) = self.recovery.parts();
        let mut s =
            format!("{HEADER}\nsave {print:016x}\ngrowth {} {} {}\nrecovery {ticks} {hunger}\n", g[0], g[1], g[2]);
        if let Some(a) = &self.avatar {
            match a.plight() {
                Some(Plight::Hanging) => s += &format!("avatar {} hanging\n", a.dir),
                Some(Plight::Down) => s += &format!("avatar {} down\n", a.dir),
                None => {}
            }
        }
        for id in self.world.fighters() {
            let t = self.world.target(id);
            s += &format!("target {id} {} {}\n", t.now, t.fixed);
        }
        let mut morphs: Vec<_> = self.fights.iter().filter(|(_, f)| f.changeling).collect();
        morphs.sort_by_key(|(&id, _)| id);
        for (id, f) in morphs {
            let m = crate::fight::Morph::ALL.iter().position(|&m| m == f.morph).unwrap_or(0);
            s += &format!("changeling {id} {m} {} {}\n", f.clone_wait, f.tree_wait);
        }
        for (name, at) in &self.restored.done {
            s += &format!("restored {name} {at}\n");
        }
        s
    }

    /// Take the side file's numbers, if it goes with a save of `print`. False if it doesn't.
    /// Which restored pieces were put in is taken either way: a save written over since by the
    /// original is the same game played on, and they'd be put in a second time.
    pub fn apply_extra(&mut self, text: &str, print: u64) -> bool {
        let mut lines = text.lines();
        if lines.next() != Some(HEADER) {
            return false;
        }
        let ours = lines.next() == Some(&format!("save {print:016x}")[..]);
        for line in lines {
            let mut w = line.split_whitespace();
            let key = w.next().unwrap_or_default();
            if key == "restored" {
                let (name, at) = (w.next(), w.next().and_then(|m| m.parse().ok()).unwrap_or(0));
                self.restored.done.extend(name.map(|n| (n.to_string(), at)));
                continue;
            }
            if !ours {
                continue;
            }
            if key == "avatar" {
                let (dir, p) = (w.next().and_then(|d| d.parse().ok()), w.next());
                let p = match p {
                    Some("hanging") => Some(Plight::Hanging),
                    Some("down") => Some(Plight::Down),
                    _ => None,
                };
                if let (Some(dir), Some(p), Some(a)) = (dir, p, &mut self.avatar) {
                    a.restore(&self.data, &mut self.world, dir, p);
                }
                continue;
            }
            let n: Vec<u32> = w.filter_map(|v| v.parse().ok()).collect();
            match (key, n.as_slice()) {
                ("growth", &[s, d, i]) => self.growth = [s, d, i],
                ("recovery", &[t, h]) => self.recovery.set_parts(t, h as u16),
                ("target", &[id, now, fixed]) if self.world.get(id as u16).is_some() => {
                    let t = Target { now: now as u16, fixed: fixed as u16 };
                    self.world.restore_target(id as u16, t);
                }
                ("changeling", &[id, m, clone, tree]) if self.world.get(id as u16).is_some() => {
                    let (id, morph) =
                        (id as u16, crate::fight::Morph::ALL.get(m as usize).copied().unwrap_or_default());
                    if self.stats.get(id).is_some_and(|s| s.dead) && !morph.tree() {
                        continue; // (killed since)
                    }
                    if !self.world.in_combat(id) {
                        self.world.set_in_combat(id, true);
                    }
                    let f = self.fights.entry(id).or_default();
                    f.changeling = true;
                    f.morph = morph;
                    (f.clone_wait, f.tree_wait) = (clone, tree);
                }
                _ => {}
            }
        }
        ours
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::AVATAR;
    use u8data::map::Item;

    #[test]
    #[ignore]
    fn the_side_file_keeps_what_the_save_cannot() {
        let Ok(dir) = std::env::var("U8_DATA") else { return };
        let open = || Game::open(Path::new(&dir), u8data::fixed_slot(Path::new(&dir)), Some(40)).unwrap();
        let mut g = open();
        g.growth = [5, 7, 1];
        g.recovery.set_parts(100, 33);
        g.world.restore_target(50, Target { now: 1, fixed: 1 });
        let text = g.extra(42);
        let mut h = open();
        assert!(!h.apply_extra(&text, 43), "not for another save");
        assert_eq!(h.growth, [0; 3]);
        assert!(h.apply_extra(&text, 42));
        assert_eq!((h.growth, h.recovery.parts()), ([5, 7, 1], (100, 33)));
        assert_eq!(h.world.target(50), Target { now: 1, fixed: 1 });
        assert!(h.world.in_combat(50));
    }

    /// Hanging from a ledge, or knocked flat, comes back as it was (facing the same way).
    #[test]
    #[ignore]
    fn the_side_file_keeps_the_avatar_hanging_or_down() {
        use crate::avatar::Input;
        use u8data::anim::action;
        let Ok(dir) = std::env::var("U8_DATA") else { return };
        // (Up off the ground, as if at a ledge: on the ground, a hanging Avatar just stands.)
        let open = || {
            let mut g = Game::open(Path::new(&dir), u8data::fixed_slot(Path::new(&dir)), Some(40)).unwrap();
            let at = g.world.get(AVATAR).unwrap();
            let up = (0..16).map(|k| Item { z: at.z + 24, x: at.x + 32 * k, ..at }).find(|&it| {
                g.world.set(&g.data, AVATAR, it);
                g.world.floor_under(AVATAR).is_some_and(|f| f < it.z - 16)
            });
            g.world.set(&g.data, AVATAR, up.expect("somewhere in the air"));
            g
        };
        for (p, word) in [(Plight::Hanging, "hanging"), (Plight::Down, "down")] {
            let mut g = open();
            let a = g.avatar.as_mut().unwrap();
            a.restore(&g.data, &mut g.world, 5, p);
            let text = g.extra(7);
            assert!(text.contains(&format!("avatar 5 {word}")), "{text}");
            let mut h = open();
            assert!(h.apply_extra(&text, 7));
            let a = h.avatar.as_ref().unwrap();
            assert_eq!((a.plight(), a.dir), (Some(p), 5));
            h.update(1.0 / 30.0, &Input::default());
            let want = if p == Plight::Hanging { action::HANG } else { action::STAND_UP };
            assert_eq!(h.avatar.as_ref().unwrap().action(), want);
        }
        assert!(!open().extra(7).contains("avatar"), "nothing to keep when it's just standing");
    }
}
