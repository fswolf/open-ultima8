//! Cast everything in the tester's kit and look at how each spell's pictures sit in the world:
//! which are drawn over things nearer than they are, under things farther or below, or change
//! places with the Avatar from frame to frame.
use super::*;
use std::collections::{BTreeMap, HashMap, HashSet};

const SPELLS: [u32; 4] = [746, 396, 397, 827];

fn name(shape: u32, frame: u32, q: u16) -> String {
    let n: &[&str] = match shape {
        746 => &[
            "Death Speak",
            "Mask of Death",
            "Rock Flesh",
            "Summon Dead",
            "Open Ground",
            "Create Golem",
            "Withstand Death",
            "Grant Peace",
            "Call Quake",
            "Meteor Shower",
        ],
        396 => &[
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            "Divination",
            "Healing Touch",
            "Aerial Servant",
            "Reveal",
            "Restoration",
            "Fade from Sight",
            "Air Walk",
            "Hear Truth",
            "Intervention",
            "Theurgy 17",
        ],
        _ => &[],
    };
    if shape == 397 {
        let s = [
            "",
            "Ignite",
            "Extinguish",
            "Flash",
            "Flame Bolt",
            "Endure Heat",
            "Fire Shield",
            "Armor of Flames",
            "Create Fire",
            "Explosion",
            "Summon Daemon",
            "Banish Daemon",
            "Conflagration",
        ];
        return format!("Sorcery {} {}", q >> 8, s.get((q >> 8) as usize).copied().unwrap_or(""));
    }
    if shape == 827 {
        return format!("Scroll q{q}");
    }
    format!("{} ({shape}/{frame})", n.get(frame as usize).copied().unwrap_or("?"))
}

/// Cast the kit's spells; spell pictures against the world.
/// magic <root> <slot> out=dir [at=x,y,z,map] [secs=10] [every=3] [only=shape[,frame[,q]]]
///       [target=self|near|floor:dx,dy|ID] [trace=1] [dump=T shapes=..] [state=dir]
pub(super) fn magic(root: &Path, slot: Option<u32>, args: &[String]) {
    let num = |k: &str, d: i32| arg(args, k).and_then(|v| v.parse().ok()).unwrap_or(d);
    let list = |k: &str| -> Vec<i32> {
        arg(args, k).map_or(vec![], |v| v.split(',').filter_map(|x| x.parse().ok()).collect())
    };
    let out = PathBuf::from(arg(args, "out").expect("out=dir"));
    let (secs, every) = (num("secs", 10) as u32, num("every", 3).max(1) as u32);
    let (at, only) = (list("at"), list("only"));
    // what's in the kit
    let kit: Vec<(u32, u32, u16)> = {
        let mut g = open(root, slot);
        let bag = g.give_test_kit().expect("bag");
        g.world
            .contents(bag)
            .into_iter()
            .filter_map(|o| g.world.get(o))
            .map(|i| (i.shape, i.frame, i.quality))
            .collect()
    };
    for (shape, frame, q) in kit {
        if !SPELLS.contains(&shape) || (shape == 827 && q < 50) {
            continue;
        }
        if !only.is_empty()
            && (only[0] as u32 != shape
                || only.get(1).is_some_and(|&f| f as u32 != frame)
                || only.get(2).is_some_and(|&x| x as u16 != q))
        {
            continue;
        }
        let mut g = open(root, slot);
        if at.len() == 4 {
            g.put_avatar(at[3] as u16, (at[0], at[1], at[2]));
        }
        let mut d = Drv { keep: true, stamp: true, answer: Some(0), ..Default::default() };
        d.run(&mut g, 20, &mut Input::default());
        let bag = g.give_test_kit().expect("bag");
        let Some(id) = g
            .world
            .contents(bag)
            .into_iter()
            .find(|&o| g.world.get(o).is_some_and(|i| (i.shape, i.frame, i.quality) == (shape, frame, q)))
        else {
            continue;
        };
        if let Some(dirp) = arg(args, "state") {
            // a start state for the original: the spell's thing alone at the top left of the pack
            if let Some(pack) = g.backpack() {
                g.world.move_inside(&g.data, id, pack, (num("px", 30), num("py", 30)));
            }
            write_dir(&g.snapshot(), Path::new(dirp));
            println!("state for {} written to {dirp}; pack {:?} thing {id}", name(shape, frame, q), g.backpack());
            for o in g.backpack().map(|p| g.world.contents(p)).unwrap_or_default() {
                let i = g.world.get(o).unwrap();
                println!("  in pack: {o} shape {} f{} at {},{}", i.shape, i.frame, i.x, i.y);
            }
        }
        let a = g.world.get(AVATAR).unwrap();
        let target = match arg(args, "target") {
            Some("near") => (2..256u16)
                .filter(|&i| !g.world.is_void(i))
                .filter_map(|i| {
                    g.world.get(i).map(|n| (i, (n.x - a.x).abs().max((n.y - a.y).abs()), (n.z - a.z).abs()))
                })
                .filter(|n| n.1 < 400 && n.2 < 40)
                .min_by_key(|n| n.1)
                .map_or(AVATAR, |n| n.0),
            Some(v) if v.starts_with("floor:") => {
                // the floor piece under a spot dx,dy from the Avatar (top at his feet)
                let o: Vec<i32> = v[6..].split(',').filter_map(|x| x.parse().ok()).collect();
                let (px, py) = (a.x + o[0], a.y + o[1]);
                (0..g.world.len())
                    .map(|i| g.world.id(i))
                    .find(|&c| {
                        g.world
                            .abs_box(c)
                            .is_some_and(|b| b.z1 == a.z && b.x0 < px && px <= b.x1 && b.y0 < py && py <= b.y1)
                    })
                    .unwrap_or(AVATAR)
            }
            Some(v) => v.parse().unwrap_or(AVATAR),
            None => AVATAR,
        };
        d.targets = std::iter::repeat_n(target as u32, 20).collect();
        let before: HashSet<u16> = (0..g.world.len()).map(|i| g.world.id(i)).collect();
        let tag = format!("{shape}_{frame}_{q}");
        let dir = out.join(&tag);
        std::fs::create_dir_all(&dir).unwrap();
        let (mut painter, mut cache) = (Painter::new(), SpriteCache::unscaled());
        g.trigger(id, u8engine::usecode::event::USE);
        d.t = 0;
        let mut seen: BTreeMap<u32, (u32, u32, u32, u32)> = BTreeMap::new(); // shape -> (first tick, last tick, min frame, max frame)
        let mut issues: BTreeMap<String, (u32, u32, u32)> = BTreeMap::new(); // what -> (frames, first tick, most pixels)
        let mut rel: HashMap<u16, (bool, u32)> = HashMap::new(); // thing -> (over the Avatar, changes)
        let mut shots = 0;
        for t in 0..secs * 30 {
            d.run(&mut g, 1, &mut Input::default());
            let Some((cx, cy, cz)) = g.camera() else { break };
            let (sx, sy) = project(cx, cy, cz);
            let view = [sx - 160, sy - 100, 320, 200];
            let max_z = g.roof();
            let opts = DrawOpts { max_z, ..Default::default() };
            if arg(args, "trace").is_some() {
                let av = g.world.get(AVATAR);
                println!(
                    "  t{t} avatar {:?} flags {:?} camera {cx},{cy},{cz} roof cut {max_z:?} shape {:?} rect {:?}",
                    av.map(|i| (i.x, i.y, i.z)),
                    av.map(|i| format!("{:04x}", i.flags)),
                    av.map(|i| (i.shape, i.frame)),
                    painter.rect(&g.world, AVATAR)
                );
            }
            let order = painter.order(&g.data, &mut g.world, view, opts);
            let rank: HashMap<u16, usize> = order.iter().enumerate().map(|(i, &o)| (o, i)).collect();
            let new: Vec<u16> = order.iter().copied().filter(|o| !before.contains(o)).collect();
            let mut bad = false;
            if num("dump", -1) == t as i32 {
                for &s in &new {
                    let (Some(it), Some(b)) = (g.world.get(s), g.world.abs_box(s)) else { continue };
                    println!(
                        "  t{t} thing {s} shape {} f{} fl{:04x} box x {}..{} y {}..{} z {}..{} rank {} rect {:?}",
                        it.shape,
                        it.frame,
                        it.flags,
                        b.x0,
                        b.x1,
                        b.y0,
                        b.y1,
                        b.z0,
                        b.z1,
                        rank[&s],
                        painter.rect(&g.world, s)
                    );
                }
                for sh in list("shapes") {
                    for &c in &order {
                        let (Some(it), Some(b)) = (g.world.get(c), g.world.abs_box(c)) else { continue };
                        if it.shape == sh as u32 {
                            let tf = g.data.typeflags.get(it.shape);
                            println!("  t{t} other {c} shape {} f{} fl{:04x} box x {}..{} y {}..{} z {}..{} rank {} rect {:?} typeflags {:?}", it.shape, it.frame, it.flags, b.x0, b.x1, b.y0, b.y1, b.z0, b.z1, rank[&c], painter.rect(&g.world, c), tf);
                        }
                    }
                }
                let a = g.world.abs_box(AVATAR).unwrap();
                println!(
                    "  t{t} avatar box x {}..{} y {}..{} z {}..{} rank {}",
                    a.x0,
                    a.x1,
                    a.y0,
                    a.y1,
                    a.z0,
                    a.z1,
                    rank.get(&AVATAR).copied().unwrap_or(0)
                );
            }
            let mut frame_bad: BTreeMap<String, u32> = BTreeMap::new();
            for &s in &new {
                let (Some(it), Some(bs), Some(rs)) = (g.world.get(s), g.world.abs_box(s), painter.rect(&g.world, s))
                else {
                    continue;
                };
                let e = seen.entry(it.shape).or_insert((t, t, it.frame, it.frame));
                (e.1, e.2, e.3) = (t, e.2.min(it.frame), e.3.max(it.frame));
                let step = if rs[2] * rs[3] > 1500 { 3 } else { 2 };
                let mut py = rs[1];
                while py < rs[1] + rs[3] {
                    let mut px = rs[0];
                    while px < rs[0] + rs[2] {
                        px += step;
                        let (qx, qy) = (px - step, py);
                        if qx < view[0] || qy < view[1] || qx >= view[0] + 320 || qy >= view[1] + 200 {
                            continue;
                        }
                        let stack = painter.pick_all(&g.data, &mut g.world, qx, qy, true, opts);
                        let Some(at) = stack.iter().position(|&o| o == s) else { continue };
                        for (k, &c) in stack.iter().enumerate() {
                            if c == s || new.contains(&c) {
                                continue;
                            }
                            let (Some(ci), Some(bc)) = (g.world.get(c), g.world.abs_box(c)) else { continue };
                            let s_behind = bs.x1 <= bc.x0 || bs.y1 <= bc.y0 || bs.z1 <= bc.z0;
                            let c_behind = bc.x1 <= bs.x0 || bc.y1 <= bs.y0 || bc.z1 <= bs.z0;
                            let who = if c == AVATAR {
                                " (the Avatar)"
                            } else if c < 256 {
                                " (a person)"
                            } else {
                                ""
                            };
                            let what = if k < at && c_behind && !s_behind {
                                Some("HIDDEN BY something farther or below it")
                            } else if k > at && s_behind && !c_behind {
                                Some("DRAWN OVER something nearer or above it")
                            } else if k < at
                                && c >= 256
                                && bs.x0 < bc.x1
                                && bc.x0 < bs.x1
                                && bs.y0 < bc.y1
                                && bc.y0 < bs.y1
                                && bs.z0 < bc.z1
                                && bc.z0 < bs.z1
                                && bs.z0 < bc.z0
                            {
                                Some("SUNK INTO and covered by")
                            } else {
                                None
                            };
                            if let Some(w) = what {
                                *frame_bad
                                    .entry(format!("spell shape {} {w}: shape {}{who}", it.shape, ci.shape))
                                    .or_default() += (step * step) as u32;
                            }
                            if c == AVATAR && !s_behind && !c_behind && k.abs_diff(at) == 1 {
                                let over = at < k;
                                let r = rel.entry(s).or_insert((over, 0));
                                if r.0 != over {
                                    *r = (over, r.1 + 1);
                                }
                            }
                        }
                    }
                    py += step;
                }
            }
            for (k, px) in frame_bad {
                if px >= 12 {
                    bad = true;
                    let e = issues.entry(k).or_insert((0, t, 0));
                    (e.0, e.2) = (e.0 + 1, e.2.max(px));
                }
            }
            if (!new.is_empty() && t % every == 0 && shots < 400) || (bad && t % 2 == 0 && shots < 400) {
                shot(&mut g, &mut painter, &mut cache, dir.join(format!("t{t:04}.png")).to_str().unwrap());
                shots += 1;
            }
        }
        let e = g.world.get(AVATAR).unwrap();
        let vm = g.vm().unwrap();
        println!(
            "=== {tag} {} target {} | script errors {} | frames kept {shots} | avatar ends at {},{},{}",
            name(shape, frame, q),
            if target == AVATAR { "self".into() } else { format!("{target}") },
            vm.errors,
            e.x,
            e.y,
            e.z
        );
        for l in d.script.iter().take(6) {
            println!("{l}");
        }
        for (s, (t0, t1, f0, f1)) in &seen {
            println!(
                "  picture shape {s} frames {f0}..{f1} from {:.1}s to {:.1}s",
                *t0 as f32 / 30.0,
                *t1 as f32 / 30.0
            );
        }
        for (k, (n, t0, px)) in &issues {
            println!("  ORDER {k}: {n} frames, up to {px} px, first at t{t0:04}");
        }
        let flips: u32 = rel.values().map(|r| r.1).sum();
        if flips > 0 {
            println!("  FLICKER a spell picture changed places with the Avatar {flips} times");
        }
    }
}
