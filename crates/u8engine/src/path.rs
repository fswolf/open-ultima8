//! Route finding as people actually walk (ScummVM's Pathfinder): A* over whole and part steps of
//! the walk animation (in combat, advance) in each of the 8 facings, each step played out frame
//! by frame against the world's own collision — climbing and dropping as walking does, the feet
//! taking turns — so a route is a list of steps the walker can take exactly as planned. Other
//! people are ignored unless `actors` (when re-planning around someone who's in the way).
use crate::geom::DIRS;
use crate::world::{anim_range, two_step, ObjId, World};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use u8data::GameData;

/// Expand at most this many positions (ScummVM: 200).
const MAX_EXPANDED: usize = 300;

type Pos = (i32, i32, i32);

/// One step of a route: the action facing `dir`, `frames` of it (0: the whole step).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step {
    pub dir: usize,
    pub frames: usize,
}

/// The steps, and where they end.
#[derive(Clone, Debug)]
pub struct Route {
    pub steps: Vec<Step>,
    pub end: (i32, i32, i32),
}

#[derive(Clone, Copy)]
struct Node {
    at: Pos,
    first: bool,
    dir: Option<usize>,
    cost: u32,
    parent: usize,
    frames: usize,
}

/// Distance using only the 8 directions (ScummVM's cost heuristic).
fn octile(a: (i32, i32), b: (i32, i32)) -> u32 {
    let (x, y) = ((a.0 - b.0).abs() as f32, (a.1 - b.1).abs() as f32);
    ((x - y).abs() + x.min(y) * std::f32::consts::SQRT_2) as u32
}

/// Positions already reached (within 8 units), bucketed by 8-unit cell.
#[derive(Default)]
struct Visited(HashMap<(i32, i32), Vec<Pos>>);

impl Visited {
    fn has(&self, p: Pos) -> bool {
        let (cx, cy) = (p.0 >> 3, p.1 >> 3);
        (-1..=1).any(|i| {
            (-1..=1).any(|j| {
                self.0.get(&(cx + i, cy + j)).is_some_and(|v| {
                    v.iter().any(|q| {
                        let d = (q.0 - p.0, q.1 - p.1, q.2 - p.2);
                        d.0 * d.0 + d.1 * d.1 + d.2 * d.2 < 64
                    })
                })
            })
        })
    }
    fn add(&mut self, p: Pos) {
        self.0.entry((p.0 >> 3, p.1 >> 3)).or_default().push(p);
    }
}

/// Steps of `action` taking `id` to within `reach` (either way) of `goal`; None if it can't be
/// reached (or not within the search), as ScummVM's Pathfinder: no walking part of the way. In
/// DOS Jenna, called in when Orlok is struck behind the bar, cries out and stays where she is.
pub fn find(
    world: &World,
    gd: &GameData,
    id: ObjId,
    action: usize,
    goal: (i32, i32),
    reach: i32,
    actors: bool,
) -> Option<Route> {
    let it = world.get(id)?;
    let anim = gd.anims.get(it.shape, action)?;
    let reached = |p: Pos| (p.0 - goal.0).abs().max((p.1 - goal.1).abs()) <= reach;
    let last = world.last_anim(id);
    let start = Node { at: (it.x, it.y, it.z), first: world.first_step(id), dir: None, cost: 0, parent: 0, frames: 0 };
    let mut nodes = vec![start];
    let mut open = BinaryHeap::new();
    open.push(Reverse((0u32, 0usize)));
    let mut visited = Visited::default();
    visited.add(start.at);
    let mut found = None;
    let mut expanded = 0;
    while let Some(Reverse((_, n))) = open.pop() {
        let node = nodes[n];
        if reached(node.at) {
            found = Some(n);
            break;
        }
        expanded += 1;
        if expanded > MAX_EXPANDED {
            break;
        }
        for (dir, &(ux, uy)) in DIRS.iter().enumerate() {
            let Some(frames) = anim.frames.get(dir) else { continue };
            // (After the first step the last action is always this one.)
            let ks = anim_range(anim.flags, frames.len(), node.first, node.dir.is_some() || last == action);
            // (A frame off the ground rises or sinks by its own amount.)
            let air = |k: usize| {
                if frames[k].flags & u8data::anim::AnimFrame::ON_GROUND == 0 {
                    frames[k].dz as i32
                } else {
                    0
                }
            };
            let fwd = |k: usize| 4 * frames[k].ddir as i32;
            let step = |p: Pos, k: usize| (p.0 + ux * fwd(k), p.1 + uy * fwd(k), p.2 + air(k));
            // Somewhere this step would end up (unobstructed) that's been reached already.
            if visited.has(ks.iter().fold(node.at, |p, &k| step(p, k))) {
                continue;
            }
            let (mut at, mut blocked) = (node.at, false);
            let mut closest = (octile((at.0, at.1), goal), 0, at);
            for (taken, &k) in ks.iter().enumerate() {
                match world.frame_step(id, at, &frames[k], dir, actors) {
                    Some(q) => at = q,
                    None => {
                        blocked = true;
                        break;
                    }
                }
                let d = octile((at.0, at.1), goal);
                if d < closest.0 {
                    closest = (d, taken + 1, at);
                }
            }
            let mut add = |to: Pos, first: bool, frames: usize| {
                let (dx, dy, dz) = ((to.0 - node.at.0) as f32, (to.1 - node.at.1) as f32, (to.2 - node.at.2) as f32);
                let turn =
                    node.dir
                        .map_or(0, |d| (d as i32 - dir as i32).rem_euclid(8))
                        .min(node.dir.map_or(0, |d| (dir as i32 - d as i32).rem_euclid(8))) as u32;
                let cost = node.cost + (dx * dx + dy * dy + dz * dz).sqrt() as u32 + 64 * turn;
                let left = octile((to.0, to.1), goal);
                let total = if reached(to) { 0 } else { 2 * cost + 3 * left };
                nodes.push(Node { at: to, first, dir: Some(dir), cost, parent: n, frames });
                open.push(Reverse((total, nodes.len() - 1)));
            };
            if !blocked && !visited.has(at) {
                add(at, node.first ^ two_step(anim.flags), 0);
            }
            visited.add(at);
            // Part of a step: where it comes closest, if that's short of the end (or blocked).
            if closest.1 != 0 && (closest.1 != ks.len() || blocked) && !visited.has(closest.2) {
                add(closest.2, node.first, closest.1);
                visited.add(closest.2);
            }
        }
    }
    let mut n = found?;
    let end = nodes[n].at;
    let mut steps = vec![];
    while n != 0 {
        steps.push(Step { dir: nodes[n].dir?, frames: nodes[n].frames });
        n = nodes[n].parent;
    }
    steps.reverse();
    (!steps.is_empty()).then_some(Route { steps, end })
}
