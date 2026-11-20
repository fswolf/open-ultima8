//! Draw order checked against the DOS game.
use u8render::{project, DrawOpts, Painter};

/// Map 41: two tapestries (shape 329) side by side on one wall, overlapping; DOS draws the nearer
/// one (larger x: the unicorn, frame 4) over the other. Elsewhere ScummVM's screen-space rule
/// still decides (a wall and its edge, things flat in x).
#[test]
#[ignore] // needs U8 data
fn pictures_side_by_side_on_a_wall_draw_the_nearer_on_top() {
    let Ok(dir) = std::env::var("U8_DATA") else { return };
    let gd = u8data::GameData::load_fixed(std::path::Path::new(&dir)).unwrap();
    let mut world = u8engine::world::World::build(&gd, 41);
    let find = |w: &u8engine::world::World, x| {
        w.objects().find(|&id| w.get(id).is_some_and(|it| (it.shape, it.x, it.y) == (329, x, 13855))).unwrap()
    };
    let (far, near) = (find(&world, 18751), find(&world, 18815));
    let (sx, sy) = project(18800, 13855, 70);
    let order = Painter::new().order(&gd, &mut world, [sx - 80, sy - 60, 160, 120], DrawOpts::default());
    let at = |id| order.iter().position(|&o| o == id).unwrap();
    assert!(at(far) < at(near), "the unicorn (x 18815) over the other");
}

/// The Avatar flagged unseen (0x10) is still drawn, as the original does it: every pixel his
/// frame covers shows what's behind him through XFORMPAL's first table; elsewhere nothing
/// changes. Anyone else so flagged isn't drawn at all.
#[test]
#[ignore] // needs U8 data
fn the_unseen_avatar_recolours_what_is_behind_him() {
    use u8engine::world::{item_flag::INVISIBLE, AVATAR};
    let Ok(dir) = std::env::var("U8_DATA") else { return };
    let g = u8engine::Game::open(std::path::Path::new(&dir), Some(2), None).unwrap();
    let opts = DrawOpts { max_z: g.roof(), ..Default::default() }; // (under the castle's roof)
    let (gd, mut world) = (g.data, g.world);
    let a = world.get(AVATAR).unwrap();
    let (sx, sy) = project(a.x, a.y, a.z);
    let (w, h) = (120u32, 140u32);
    let draw = |world: &mut u8engine::world::World| {
        let mut px = vec![0u8; (w * h * 4) as usize];
        let mut canvas = u8render::Canvas::new(&mut px, w, h);
        let mut cache = u8render::SpriteCache::unscaled();
        Painter::new().draw(&gd, world, &mut cache, (sx - 60, sy - 110), &mut canvas, opts);
        px
    };
    let seen = draw(&mut world);
    world.set(&gd, AVATAR, u8data::map::Item { flags: a.flags | INVISIBLE, ..a });
    let ghost = draw(&mut world);
    world.set_void(AVATAR, true);
    let behind = draw(&mut world);
    let (mut covered, mut changed) = (0, 0);
    for (k, ((g, b), s)) in ghost.chunks(4).zip(behind.chunks(4)).zip(seen.chunks(4)).enumerate() {
        let want = gd.xform.unseen(&gd.palette, [b[0], b[1], b[2]]);
        if s == b {
            // (Not his; or his, in the colour of what's behind.)
            assert!(g == b || g[..3] == want, "pixel {k}: not his, unchanged");
            continue;
        }
        covered += 1;
        assert_eq!(&g[..3], &want, "pixel {k}: what's behind, recoloured");
        changed += (g != b) as u32;
    }
    assert!(covered > 300 && changed > covered / 2, "his shape shows: {changed} of {covered} pixels");
    // Someone else unseen: not drawn.
    let other = (2..256).find(|&n| world.get(n).is_some_and(|it| it.flags & INVISIBLE == 0)).unwrap();
    let o = world.get(other).unwrap();
    world.set(&gd, other, u8data::map::Item { flags: o.flags | INVISIBLE, ..o });
    let (ox, oy) = project(o.x, o.y, o.z);
    let order = Painter::new().order(&gd, &mut world, [ox - 40, oy - 80, 80, 100], opts);
    assert!(!order.contains(&other), "others flagged unseen aren't drawn");
}
