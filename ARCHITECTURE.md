# Architecture

Five crates in layers. Each depends only on the ones above it, so the game logic never touches a
window and the formats never know about game rules.

```
u8data      formats              bytes → structs. No rules, no drawing.
   │
u8engine    state + rules        World, Avatar, usecode VM, Game session. No pixels, no clock.
   │
u8render    pixels               sprites, upscalers, painter's sort, text → RGBA Canvas
   │
u8view / u8tool                  apps: window + GPU + audio + input / command line
u8cmp                            test harness: headless play, saves in and out, random walks
```

## u8data: formats

One module per file type: `flex` (archives), `palette`, `shape` (RLE frames), `typeflag` (shape
properties), `map` (FIXED/NONFIXED/ITEMCACH items, globs), `anim` (ANIM.DAT), `font`, `text` (the
game's text is DOS code page 437: decoded to Unicode where it's read, back to a byte per glyph
in the fonts), `lang` (which translation is played, and `lang::file` for files named by its
letter, falling back to English), `music`
(XMIDI, plus the SoundFont `Sequencer` behind the `synth` feature), `sound` (Sonarc-compressed
effects and speech), `save` (SAVEGAME bundles, read and written; `GameData::load_save` takes the
game state from one instead of GAMEDAT and keeps its files in `GameData::state`), `usecode`
(bytecode container, operand lengths, disassembler), `overlay` (WPNOVLAY.DAT), `weapons` (the
table compiled into U8.EXE, found by signature). `GameData::load` opens an install. Writers
(`flex::build`, `Item::record`, `map::store_actor`, `Save::to_bytes`) are the readers' inverses.

Parsers must never panic on bad input (`parsers_survive_garbage`, `real_data_mutations` tests).

## u8engine: the game

- **`world::World`**: the live map. Every item is an `Entry` (item + world box + `Traits` from
  TYPEFLAG), addressed by `ObjId` (NPC number, or 256+), including eggs, editor markers and
  invisible items, which scripts need; `Entry::shown()` says what a player would see. A
  world-space grid answers `area()` and collision (`try_move`, `try_move_air`, `floor_under`,
  `ceiling`, `clear_at`, `can_exist_at`). Every change marks the entry dirty; views pick changes up
  with `take_dirty()`. Actors block movement and are floors only when TYPEFLAG makes them land
  (rats, spiders: `Entry::underfoot`); eggs are never physical. Items can be inside
  others (`Entry::parent`; `contents`, `container`): container contents come from NONFIXED,
  what people carry from the item cache; such items are never on the map. The Avatar takes its
  belongings along through teleporters (`take_contents` / `give_contents`). `move_inside`,
  `move_to_map` and `fits` move things; `Game::move_item(id, Place)` is the player's version,
  with the rules (movable, within reach, not into itself). Saving writes container contents from
  the world (NONFIXED, nested by depth) and people's belongings into the item cache.
  Files: `world.rs` (entries, the grid, adding and removing), and in `world/`: `contents.rs` (containers and
  the void), `collision.rs`, `actors.rs` (combat state, targets, actions run for scripts,
  weapon overlays), `scenery.rs` (TYPEFLAG animation).
- **`avatar::Avatar`**: the player's Avatar, moved as the original moves him (WORLD.C, ANIM.C,
  ANIMPRIM.C): every move is an ANIM.DAT action played at its own pace ((repeat + 1) ticks a
  frame), each frame carrying him 4 × its forward amount and its rise, so walking, running and
  careful steps cover what their frames say. `avatar.rs` takes the `Input` as WORLD.C takes the
  mouse (pressed, held, let go, both buttons) into a queue of actions: turning about with
  `lookLeft`/`lookRight` (turnToFace), careful steps that teeter at an edge (`edgeBalance`),
  jumps (a climb that `World::anim_fits`, else straight up, else `jump`, aimed at the pointer's
  `Aim` if he's strong enough, as ANIMCLAS.C's StandingLeap), running leaps, advance and retreat,
  swings, kicks, blocks; `avatar/acts.rs` plays them: a move blocked stops the action, a run or
  leap into something is a bump (the game hurts him 1 in 3), a step over an edge drops him, the
  frames' sounds go out (`take_sounds`), and a falling Avatar with the move button held may
  catch a ledge (`cling`, GRAVITY.C) and hang (pull up or let go). He's moved at once, as the
  original moves him; `World::lag` only draws him going there over the frame's time. Falls end
  in `land`, or a hurt one in `knock_down` (fall back, up, dazed). Exertion comes back as an
  `Effort` the game turns into strength and dexterity. Whether someone is in
  combat lives in the `World` (scripts set and clear it too); the wielded weapon is a
  `world::Overlay` placed per pose from WPNOVLAY.DAT (`u8data::overlay`), drawn by the painter
  over the actor.
- **`eggs::Hatcher`**: fires eggs the Avatar walks into (once per visit): usecode eggs run their
  `hatch` event (class = quality + 0x47F), teleporters (frame ≠ 1) move the Avatar to the
  destination egg (frame 1) with the same id on the egg's target map. Monster eggs are left to scripts.
  An egg fires when the Avatar's footprint meets its box, egg ± range x 32 with both ends in, and
  z within 48 (`eggs::in_range`, as EGG.C); a range of 0 is the egg's own line.
- **`npc::Walk`**: an NPC walking where a script sent it (`pathfindToItem` queues a
  `usecode::Request::Walk`; the game steps it each tick and finishes the script's process on
  arrival or when it gets stuck). Routes come from **`path::find`**: A* over whole and part steps
  of the walk animation (advance in combat) in the 8 facings, each played out frame by frame with
  `World::step_from` (the same collision as walking), as ScummVM's Pathfinder does; a goal it
  can't reach gives no route (nobody walks part of the way). The walker
  takes them one `World::play_part` at a time (the animation moves it) and plans again when a
  step won't go; people are ignored when planning and planned round when they turn out to be in
  the way.
- **`camera::Camera`**: where the view looks: following an item (the Avatar, or whoever a
  cutscene centres on: `Camera::setCenterOn` 0xBE) or held and gliding to a point
  (`Camera::scrollTo` 0xB5, a process that ends on arrival). `Game::camera()` is the point hosts
  centre on; it resets to the Avatar on a new map or when no script is running. `Game::roof()`
  is the cutaway height, kept as the original's camera keeps it: the roof over the Avatar, looked
  up again when he moves while the camera is centred on him (a scene holding the camera, as
  Flash does, leaves it be). `camera::Shake` is a script quake's shake (`Game::shake`, stepped
  each tick as CAMERA.C slamQuake: a drift pulled back by half the offset, nudged at random,
  held to the strength; stopping puts the view back exactly).
- **`film::Film`**: an SKF film (`u8data::skf`) played on a 320×200 palette screen: pictures
  one per frame (`speed`/70 s each), events for music, effects, recorded lines and subtitles,
  waits (1/15 s) and fades; hands the host `Cue`s. The viewer's `films.rs` shows it (the intro
  before a new game's name prompt, the ending on the `playEndgame` intrinsic, both from the menu).
- **`touch`**: contact events (ScummVM's collideMove): when anyone in the fast area steps (a jump
  of more than 128 in a tick is a script's teleport: no events, as ScummVM), the player puts
  something down or picks it up, or something lands, what it newly touches (overlaps or stands on,
  `World::contacts`) gets `gotHit` and the mover `hit`; what it stops touching gets `release`.
  `game/items.rs` adds stealing (`Game::let_go`: `AvatarStoleSomething` to people within 640 of
  the Avatar, unless the thing is OWNED, which carrying makes it; told when the player lets go,
  before the drop is tried or the pile slider asks how many), `justMoved`, and things on top
  falling. `game/travel.rs`'s `arrivals` brings people scripts teleported to the map you enter.
  `extra` is the `.ou8` side file beside saves (also the Avatar's `Plight`: hanging, down, and
  `restored name minutes` for each piece of restored content put in).
  `restore` is the Restored content option (`Game::restored`, off by default): one table,
  `ENTRIES`, each a `Hook` (a map, maps while a flag is set, holding a thing, a death, minutes
  after another entry) and an `Act` (set or clear a story flag, place a thing, a quake that
  waits out a talk or scene); each runs once, on a map change or load and once a second.
  Turning it off stops new entries (a follow-up, `Hook::After`, still finishes its entry); what
  has happened stays. A thing isn't placed where one like it already lies, and
  the side file's `restored` lines are read even when its save was written over since.
  `kit` is a tester's kit (`Game::give_test_kit`: stats, a bag of every school's magic, the story
  flags it needs; `go_to` the recall item's places); `u8tool kit` saves it. `vm.cheats` answers
  the scripts' `avatarCanCheat` (the Avatar's look then opens the original's developers' menu).
- **`fast_area::FastArea`**: the live neighbourhood of the Avatar: items entering it get
  `enterFastArea` (NPC routines, ambient sounds, many eggs), leaving it `leaveFastArea`. It is
  the original's diamond of 512-unit regions, sized for a 320x200 view and widened by what a
  bigger window or zoom shows (`Game::set_view`), so people are live before they come into sight.
  Fights and loitering run only inside it, as in DOS. Someone a script teleports into it gets
  their `enterFastArea` at once, before the script goes on (`World::in_fast_area`). A script
  sending the Avatar somewhere on the same map takes the area there and then
  (`Game::avatar_jumped`, as ITEMCACH.C goTo), even when it sends him straight back: the Skull of
  Quakes relies on it. What glob eggs expand to (`Entry::glob`) is only found by scripts'
  searches inside the area, as the original expands them only there.
- **`usecode`**: the VM.
  - `vm.rs` the `Vm` and its scheduler; in `vm/`: `procs.rs` spawning and killing processes,
    `memory.rs` pointers (segment:offset) and global bits, `exec.rs` the opcode interpreter,
    `messages.rs` the `UiEvent`s and `Request`s scripts send out. As in ScummVM, a spawned process
    runs at once (until it first waits) before its parent goes on, and so does an event a script
    starts (the original's UnkProcess runs from its constructor); a `loop` passes over things
    that no longer fit its search (the original searched as it went). Stopping an actor's
    animation processes (resetRef with type 0xF0) stops what it's acting out
    (`World::stop_acting`; one begun this tick never shows); stopping a routine that waits on a
    walk stops the walk.
  - `stack.rs` (per-process 4 KB stack, checked push/pop), `heap.rs` (strings and lists by handle),
    `search.rs` (`loop` searches).
  - `intrinsics/`: **one table** (`TABLE`) of number → name → handler, used for dispatch,
    disassembly and the "unimplemented" report. Handlers are grouped by domain: `item`, `actor`,
    `egg`, `ui`, `audio`, `misc`.
  - `host.rs`: the `Host` trait is everything scripts can see of the game; `WorldHost` implements
    it over a `World`, with the other maps' worlds for people who are elsewhere (as the original,
    which keeps everyone in one world: a person's map is built when a script first changes them).
    A person teleported here from elsewhere is brought over (`Request::Fetch`, `Game::fetch`).
    Tests run the VM against a stub host.
  - Scripts talk to the player through `UiEvent`s (bark, ask, read, music, sfx); the UI answers
    with `Vm::finish` / `Vm::answer`.
- **`persist`**: a `Game` as a save: starts from the loaded files and writes back what the engine
  models (each visited map's movable items with what's inside them (`world::Origin` says which
  items those are), NPC records and what each person carries (ITEMCACH, reusing object slots;
  NPCDATA's equipment list from what's worn),
  global flags, the clock; our extras go in `OPENU8.DAT`).
- **`stats::Stats`**: everyone's STR/DEX/INT/HP/mana, sides (NPCDATA alignment byte: own
  nibble, enemies nibble) and state flags (bytes 0x1B/0x2F/0x30: dead, immortal (key characters
  start so), invincible, in combat, withstand death, playing dead; other bits kept as loaded),
  owned by the `Game`, read and changed by scripts through the `Host`, saved back into NPCDATA
  (in-combat from the worlds).
- **`combat`**: `impl Game`: a swing's reach (the attack frame's range × 32 units ahead),
  `receive_hit` (ScummVM's rules: defences, armour, a DEX roll, damage; death; the struck turn
  on the striker and go into combat at once, and stagger unless already acting something out,
  then stand (in stance if fighting) as they faced; things get `gotHit` or break); `call_for_help`: a
  blow from the Avatar brings in everyone with the struck one's alignment within 1024 (as DOS
  does; claude/attack-compare.md). Scripts' `Item::receiveHit` arrives as a `Request::Hit`
  (a setDead/clrDead after it in the same turn waits behind it). The Avatar's numbers come
  from its gear, everyone else's from `monsters`.
  `World::play` runs actor animations for scripts' `doAnim`, deaths and the like
  (`then_play` queues the next). `Game::gesture`: someone standing (or already talking) as
  they speak gestures, a rest of one to three seconds between (the original's BarkFidget).
- **`monsters`**: fighting numbers by shape (hit points, dexterity, damage, armour, damage and
  defence types, sides), ScummVM's u8monsters table.
- **`fight`**: `impl Game`: everyone in combat but the Avatar (ScummVM's CombatProcess): the
  quarry a blow or script named (`World::target`), else the nearest enemy; close in (`npc::Walk`),
  turn a step at a time, swing in reach, the blow landing on the frame that reaches; a pause by
  DEX. Joining a fight stops the person's scripts and walks and runs its going-into-combat
  script (event 17, `cast`: guards shout, townsfolk cry for Beren), which it waits for; those
  put back in a fight by a load or a return to the map don't run it again. Schedules skip
  fighters.
  Changelings (`Fighter::changeling`: shape 357 when it went in) follow the original's brain
  (COMBATBR.C): `Morph` steps through becoming the Avatar (shape 1, 5 to 25 s) and a tree (dead
  to the world while the Avatar is 2048 or more away, not on map 43); the side file keeps their
  `Morph` and timers, as the original keeps its brains in its process list.
- **The ethereal stack**: `World::{push_ethereal, ethereal_top}`, `Host::pop_ethereal(Pop)`:
  scripts make things (`Item::create`, `Actor::createActor`) or push them into the void, then pop
  the *top* of the stack back, to a point or into a container (ScummVM's reading). Left over on
  leaving a map: thrown away. `createActor` takes a slot from `vm.free_npcs` (the game refreshes
  it once a second and on map changes; the host re-checks the slot).
- **`fade`**: the palette's colour matrix (ScummVM's PaletteManager transforms: grey, storms,
  saturate…) and the fade under way; kept by the `Vm` (`vm.fade`), ticked with it; the viewer
  applies `rows()` in its presenting shader. `vm.quake` shakes the view.
- **`gravity`**: `impl Game`: things in the air (`hurl`, `fall`; ScummVM's GravityProcess):
  `World::sweep` moves them in 8-unit steps (checking the whole height passed, so flat floors
  catch them) and says what stopped them on which axes; walls halve and turn the speed, landing
  on non-floor (or hard) bounces at a random angle, edges slide off; people falling 80+ are hurt
  (the Avatar, over an edge (`Avatar::take_drop`) falls this way too, unsteered; hurt 6+ it's
  knocked flat, and while it's down its armour counts half and blows always land).
  Each tick in the air is one move for contact events (as ScummVM's collideMove a tick): what
  it meets that it wasn't touching where that tick began gets `hit`/`gotHit`, what it leaves
  `release`.
  `explode` (sprite 578, sfx 31/158, 6–11 blunt+fire within 160) and `ignite` (oil flasks).
  Struck things: `gotHit`, else explode, break, or get knocked flying.
- **`heal`**: the Avatar's recovery: 1 HP a real minute, mana at the half, hunger; `feedAvatar`.
- **`sprites`**: scripts' sprite effects (`createSprite`): a temporary item stepped through its
  frames, its process finished when done.
- **`spawn`**: `impl Game`: monster eggs (`hatch_monster`: shape in the egg's quality, id and
  activity in its map byte; a free NPC slot: shape 0 or put away, lowest first as the
  original's free list; HP/DEX rolled from `monsters`), putting hatched creatures away
  (void + ETHEREAL) when they leave the fast area or the map, loitering (activity 0), loot on
  death (`monsters::Loot`), the risen dead, and `use_item` (a dead body opens gump 12).
- **`equip`**: what's worn where (`Slot` from the TYPEFLAG equip nibble; on the paperdoll at
  the slot's spot in the worn frame, the one after the ground frame), weights in tenths (coins per
  hundred, reagents per ten; the paperdoll rounds up, as the original) and the 4 × STR limit; `Game::{equip, stow, move_item}` apply them.
- **`game::Game`**: a session (`Game::open(root, slot, map)`, `snapshot`, `save`): data + world + avatar + scripts. `update(dt, &Input)` advances
  movement and game ticks (30 Hz: scenery animation every TYPEFLAG-speed ticks → eggs → fast area → schedules →
  walks → loitering → the risen → fights → scripts) on fixed steps. It keeps the game clock (`minutes()`; continues from the save's kernel
  clock, KERNEL.DAT entry 20: a game hour is 15 real minutes) and runs every NPC's `schedule` event each game hour; `trigger(id, event)` runs look/use; `teleport(map, id)` takes the Avatar along;
  `load_map` swaps worlds and stops scripts. On entering a map, map items (not actors) get
  `cachedIn` (music objects pick the song); a new game hatches the FIRST egg (the opening). Maps you leave are kept as they were, so opened
  doors stay open when you come back. Files: `game.rs` (the session, `update`, the tick), in `game/`:
  `events.rs` (starting usecode events, running the scripts and carrying out their requests),
  `clock.rs` (the clock, schedules, walks), `items.rs` (the player handling things),
  `travel.rs` (maps and teleports).

The engine takes time as `dt` seconds, never reads the clock, so it is deterministic and testable.
Behaviour tests against a real install are in `u8engine/tests/`: `game.rs` (the map, moving,
scenery), `people.rs`, `items.rs`, `combat.rs`, `attacks.rs` (how people answer a blow),
`stealing.rs` (taking what isn't the Avatar's), `saves.rs`, with helpers in `tests/common/`.

## u8render: pixels

- `upscale`: nearest, xBRZ, banded parallel xBRZ.
- `sprite::SpriteCache`: frame → RGBA at a scale (seamless per-sprite upscaling, mirroring),
  with a memory budget (`begin_frame` evicts unused sprites when over).
- `painter::Painter`: screen-space index over a `World`, kept in sync incrementally
  (`sync` also reports whether anything visible changed), the per-frame depth sort (port of
  ScummVM's `SortItem::below`, except that two things flat in y on the same plane go nearer-last
  as in DOS), `draw` into a `Canvas` (the Avatar flagged unseen as a recolouring of what's
  behind him, `Canvas::recolour` with XFORMPAL's first table), and `pick` items under a point.
- `text`: outlined text and shaded panels.

## Apps

- **u8view**: `main.rs` owns the `Game`, a `Painter`, a `SpriteCache` and the script UI (`App`);
  each frame: input → `Game::update` → UI events → `Painter::sync` → draw if dirty → `gpu`
  presents. Two layers: the world (drawn at whole game pixels a little past the edges, `zoom`
  any 1–8, its frame placed by the GPU to a fraction of a pixel with sharp bilinear; xBRZ sprites
  made at the zoom, rounded; or the GPU's own looks in `shader.wgsl`: sharp bilinear, plain,
  xBR (after Hyllian's level 2: per screen pixel, each texel corner's edge found from a 4×4
  neighbourhood and cut at 45° or 2:1), CRT beams and shadow mask) and the overlay (windows,
  speech, menus, films; premultiplied alpha, its own `SpriteCache`, at the interface's whole
  `scale`). Mouse: `mouse_world` for the map, `mouse_game` for the overlay. The view follows the
  game's camera (`App::follow_camera`); the smooth camera (an option) eases only its height, to
  the Avatar's footing (`eye_height`). `App`'s work is split by concern: `window.rs` (winit
  events), `frame.rs` (advancing the game, script events, sounds, dying), `draw.rs` (drawing a
  frame: `redraw` → `draw_frame` → the world, then `draw_overlay` with `draw_windows` and
  `draw_speech`; `present`), `view.rs` (the camera, interface size, zoom and looks, picked for the
  screen until the player picks: `first_scale`, `first_zoom`), `gamedir.rs` (finding the game's
  files), `controls.rs` (the mouse), `commands.rs` (keys, save/load), `book.rs` (the game menu
  and the settings' values), `slider.rs` (the original's "how many?" for part of a pile).
  `input.rs` is the only place keys are bound (`Command`). `mouse.rs` is the original's pointer:
  aim and reach from the Avatar (→ `Input`), and the game's arrow cursors as OS cursors.
  `pad.rs` is the game controller (gilrs, feature `gamepad`; how it drives the game is in
  `pad/app.rs`): its buttons are bound in `BINDS`
  only, to a `Command`, the mouse's own buttons (`left_button`, `right_button`, `both_buttons`)
  or menu moves; its sticks walk (the same (facing, run, careful) as the pointer's arrow:
  `App::steering`, `pad::steer`) and move the one pointer the mouse shares (`App::moved_to`).
  One pad is followed at a time (`pad::Follow`; Steam can show the game two copies of one pad).
  `touch.rs` is the touch screen, the same way: a small machine (`Touch`) turns fingers into the
  mouse's moves and buttons (`App::mouse_button`, the one place a left or right press is
  routed), walking, jumps, zoom, and the screen buttons; `App::owner` says who moved the pointer
  last.
  `gumps.rs` is item windows (opened by scripts' `Item::openGump`: bags, chests, the paperdoll),
  `art.rs` the original's window art (U8GUMPS.FLX) and item areas (GUMPAGE.DAT), shared with the
  menus. `ui.rs` shows barks, conversation choices (numbered panel or the original's bulleted style)
  and notices, and `ui/readable.rs` books, scrolls, graves and plaques; book and scroll text is in `pagefont.rs`, and `cover.rs` adds a book's
  leather cover and turned-up page corners to the original's art. `menu.rs` is the original's book menus drawn with its own art (U8GUMPS.FLX
  35 book, 32 logo, 37 menu entries): main menu, the diary (Read loads, Write saves: Entry n is slot n - 1, Entry 1 the
  beginning, six to a spread, gumps 46 "Entry", 36 numbers, 34 page corners), the quit question
  (17 window, 18 question, 47/50 Yes/No); it returns `menu::Action`s for `App::menu_action`; the
  game is paused while it's open; its Options entry opens `panel.rs`, the settings panel (F1):
  tabs of `panel::Setting` rows whose labels and values come from `App::setting_row` and change
  through `App::change_setting`, plus a Keys list, drawn at screen resolution in the books'
  typeface; keys, mouse, pad and touch reach it before the book. `credits.rs` rolls the credits and quotes (decoded by
  `u8data::credits`) over a black screen, the game paused. `settings.rs` keeps the player's
  settings between runs (`~/.config/open-ultima8`, `%APPDATA%\open-ultima8` on Windows).
  `audio.rs` mixes music, effects and speech into one stream; effects tied to an item are
  re-placed (volume, pan) every frame from where the item is relative to the camera.
- **u8tool**: `main.rs` dispatches to one module per command group (`export`, `map`, which also
  makes `kit` saves, `uc`, `music`); `cli.rs` has the argument and image helpers.
- **u8cmp** (the harness for comparisons with DOS): `main.rs` dispatches to one module per
  command (`dump`, `play`, `fuzz` with `soak`, `talk`, `people`, `attack`, `steal`, `magic`) and
  keeps the helpers they share (`open`, `shot`, `arg`, `write_dir`, `Watch` for pictures and
  frame logs along a run); `drive.rs` plays the engine
  headless for all of them (`Drv`), `scene.rs` sets up scenes as in the original (where to stand,
  what's loose, where to point in the 320 by 200 view), `magic.rs` casts the kit's spells and
  compares their pictures with the world. `play` takes `restored=1` and `flag=N` (a global bit,
  decimal) steps, `put=ID,x,y,z` (a thing moved as it is) and `aim=ID` (where to click on it in
  DOS); `talk` takes `flag=HEX,..` to set story flags before talking.

## Adding things

| To add… | Go to |
|---|---|
| a file format | new module in `u8data`, loaded from `GameData` if it's always needed |
| an intrinsic | handler in `u8engine/src/usecode/intrinsics/<domain>.rs`, then its `TABLE` row |
| something scripts can query or change | a `Host` method (default impl if optional) + `WorldHost` |
| a new kind of thing in the world (containers, NPC AI) | `u8engine` module, driven from `Game::update` |
| something the player sees from scripts | a `UiEvent` variant, handled in `u8view/src/ui.rs` |
| a key | `Command` + `bind` in `u8view/src/input.rs`, action in `App::command` |
| a controller button | its row in `BINDS` in `u8view/src/pad.rs` (a `Command` if a key does it too) |
| a CLI command | a function in the matching `u8tool` module (and the usage text) |
| a harness command | a module in `u8cmp` with its function, its `main` arm and usage line |

## Checking a change

```sh
cargo test --workspace
U8_DATA=$PWD/U8 cargo test --workspace -- --include-ignored
cargo run --release -p u8tool -- U8 uc smoke 40     # every look/use/hatch on a map; prints stuck scripts and missing intrinsics
cargo run --release -p u8tool -- U8 uc live 40 600 walk=3        # play headless: eggs, fast area, scripts, teleports
cargo run --release -p u8tool -- U8 eggs 40         # every trigger on a map and where teleporters lead
cargo run --release -p u8tool -- U8 map 40 c c 800 600 2 before.png    # compare renders across a change
cargo run --release -p u8cmp -- play U8 2 at=4040,4800,0 walk=2,40 jump=2 wait=90 where   # scripted steps on save 2
cargo run --release -p u8cmp -- fuzz U8 1 seed=3 steps=3000     # random walking and jumping; notes voids, stuck spots, high climbs
```

`UC_TRACE=1` traces every instruction, `UC_CALLS=1` every intrinsic call.
