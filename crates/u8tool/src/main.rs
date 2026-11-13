//! CLI: inspect, export and test U8 data. One module per command group; each `run` takes the
//! parsed [`Args`](cli::Args) and loaded data.
#[macro_use]
mod cli;
mod export;
mod map;
mod music;
mod uc;

use anyhow::{bail, Context, Result};
use u8data::GameData;

const USAGE: &str = "usage: u8tool <data-dir> <command>
  info                                       archive stats
  shape <n> [scale] [out-dir]                export every frame of shape n as PNG
  sheet <first> <count> [scale] [out.png]    contact sheet of shapes (frame 0)
  anim <shape> <action> [scale] [out.png] [weapon kind]
                                             sheet of an actor animation: 8 directions × frames
                                             (with a weapon's overlay shape held the WPNOVLAY way `kind`)
  fonts [text] [out.png]                     every font rendering a line of text
  cursors [out.png]                          every mouse cursor (numbered)
  gumps [out.png] [n,n…]                     every gump (windows, books, menus), or all frames of some
  film <intro|endgame> [seconds,…] [out-prefix]   stills of a film at those times (default 5,20,40,60,80)
  map <map> <x|c> <y|c> <w|all> <h|all> [scale] [out.png] [editor] [cut=Z]
                                             render a map area (screen coords; c = centre, all = whole map)
  map <map> all [scale] [out.png] [editor] [cut=Z]   render the whole map
  items <map> <x> <y>                        list items under a screen point, topmost first
  eggs <map>                                 triggers on a map: usecode eggs, monster eggs, teleporters
  avatar                                     where the Avatar is in the saved game
  weapons                                    U8.EXE's weapon table (damage, bonuses, how held)
  saves                                      saved games (use one with u8view --save N, or uc live … save=N)
  kit <slot> [from=N] [at=place]             a tester's save (slots 1–11 show in the diary): STR/DEX/INT 25, every school's
                                             magic in a bag, the recall item (and its places); from saved game N
                                             (default: new); at tenebrae, plateau, catacombs, mountainking,
                                             argentrock, carthax, carthax-drained or crag
  uc list | uc dis <class> [event|@hexoffset|all]   list / disassemble usecode classes
  uc run <map> <x> <y> <look|use|hatch|N> [cut=Z] [png=out.png] [answers…]
                                             run an event on the item under a screen point
                                             (`s <shape>` / `id <id>` instead of x y picks by shape or id)
  uc smoke <map>                             look at and use every scripted item, hatch every egg; report problems
  uc live <map> [ticks] [walk=DIR] [at=x,y,z] [save=N] [write=N]
                                             play headless: eggs, fast area and scripts run; prints what happens
                                             (write=N saves the result to slot N)
  music [name|list] [out.wav] [seconds]      render a song to WAV with a GM SoundFont ($U8_SOUNDFONT)
  sfx list | sfx <n|name> [out.wav]          sound effects (SOUND.FLX)
  speech <shape> list | speech <shape> <n> [out.wav]   spoken lines (SOUND/E<shape>.FLX)
scale: 1 = original, N = xBRZ N× (2–6 native), nN = nearest N× (e.g. n3)
env: UC_TRACE=1 traces every instruction, UC_CALLS=1 every intrinsic call";

fn main() -> Result<()> {
    let args = cli::Args::parse(std::env::args().skip(1).collect());
    if args.len() < 2 || args.has("-h") || args.has("--help") {
        bail!("{USAGE}");
    }
    let gd = GameData::load(args.data()).with_context(|| format!("loading data from {}", args.data().display()))?;
    match args.get(1, "").as_str() {
        "info" | "map" | "items" | "eggs" | "saves" | "kit" | "avatar" | "weapons" => map::run(&args, &gd),
        "shape" | "sheet" | "anim" | "fonts" | "cursors" | "gumps" | "film" => export::run(&args, &gd),
        "uc" => uc::run(&args, &gd),
        "music" | "sfx" | "speech" => music::run(&args),
        c => bail!("unknown command {c:?}\n{USAGE}"),
    }
}
