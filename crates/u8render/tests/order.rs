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
