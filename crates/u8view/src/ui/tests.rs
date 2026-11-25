use super::*;
use std::path::PathBuf;
use u8data::flex::Flex;

struct Data {
    fonts: Fonts,
    pal: Palette,
    art: GumpArt,
    uc: u8data::usecode::Usecode,
}
struct Shots {
    pal: Palette,
    art: GumpArt,
}

fn data() -> Option<Data> {
    let root = PathBuf::from(std::env::var("U8_DATA").ok()?);
    let read = |p: &str| std::fs::read(u8data::find_file(&root, p).unwrap()).unwrap();
    let pal = Palette::parse(&read("STATIC/U8PAL.PAL")).unwrap();
    let fonts = Fonts::parse(&Flex::parse(read("STATIC/U8FONTS.FLX")).unwrap());
    let art = GumpArt::load(&root, &pal).unwrap();
    Some(Data { fonts, pal, art, uc: u8data::load_usecode(&root).unwrap().0 })
}

/// The usecode's string constant starting with `start`.
fn game_text(uc: &u8data::usecode::Usecode, start: &str) -> String {
    (0..uc.class_count() as u32)
        .find_map(|k| {
            let d = uc.class(k)?;
            let at = d.windows(start.len()).position(|w| w == start.as_bytes())?;
            let n = d[at..].iter().position(|&b| b == 0)?;
            Some(String::from_utf8_lossy(&d[at..at + n]).into_owned())
        })
        .unwrap()
}

/// Speech colour goes with who's speaking (ScummVM's dialogFontForActor): the Avatar and
/// the Guardian red, NPCs white/orange/green by number (Corinth, 15, white; a castle guard,
/// 50, green — as Ryan's DOS shots show them), anything else yellow.
#[test]
fn speech_colour_goes_with_the_speaker() {
    assert_eq!((speech_font(1), speech_font(GUARDIAN)), (6, 6));
    assert_eq!((speech_font(15), speech_font(2), speech_font(50)), (0, 7, 7));
    assert_eq!((speech_font(256), speech_font(257), speech_font(4000)), (5, 8, 8));
}

/// Draw over a 320×200 view at 3 screen pixels a game pixel; with `U8_SHOTS` set, written
/// there as `name`.ppm.
fn shot(ui: &mut ScriptUi, art: &mut GumpArt, name: &str) {
    let mut px = vec![0u8; 960 * 600 * 4];
    art.set_style(3, false);
    ui.draw_reading(&mut Canvas::new(&mut px, 960, 600), Some(art), 3, (320, 200));
    if let Ok(out) = std::env::var("U8_SHOTS") {
        let mut ppm = b"P6 960 600 255\n".to_vec();
        px.chunks(4).for_each(|p| ppm.extend(&p[..3]));
        std::fs::write(format!("{out}/{name}.ppm"), ppm).unwrap();
    }
}

fn reading(ui: &ScriptUi) -> &Readable {
    ui.panels.iter().find_map(|p| if let Panel::Read(r) = p { Some(r) } else { None }).unwrap()
}

/// A book opens on the book gump, two pages at a time, `~` breaking lines; a click turns
/// the pages, and a click past the last (or a double click) puts it away.
/// Speech in the original's box (194 wide, five lines a page) and time: a short line stays
/// about 3 s (DOS: "Hello there." 3.2 s); a long one pages, and a click turns the page.
#[test]
#[ignore]
fn speech_pages_and_stays_as_long_as_the_originals() {
    let Some(Data { fonts, pal, uc, .. }) = data() else { return };
    let (mut vm, mut ui) = (Vm::new(&uc, None), ScriptUi::new(fonts));
    ui.style = Style::Classic;
    let short = ui.bark_pages(8, "Hello there.", &pal, None);
    assert_eq!(short.len(), 1);
    assert!((3.0..3.3).contains(&short[0].1.as_secs_f32()), "{:?}", short[0].1);
    assert_eq!(short[0].0.h, 11, "one line");
    let long = "So many words that they won't fit in the speech box at once, it has to turn its page. ".repeat(4);
    let pages = ui.bark_pages(8, long.trim(), &pal, None);
    assert!(pages.len() >= 2 && pages.iter().all(|(img, _)| img.h <= BARK_HEIGHT), "{}", pages.len());
    assert_eq!(pages[0].0.h, 4 * 10 + 11, "five lines, 10 apart");
    ui.push(UiEvent::Bark { pid: 1, item: 2, text: long.clone() }, &pal, None, None);
    let first = match &ui.panels[0] {
        Panel::Bark { more, .. } => more.len(),
        _ => panic!(),
    };
    assert!(ui.skip_barks(&mut vm) && !ui.is_empty(), "a click turns the page");
    assert!(matches!(&ui.panels[0], Panel::Bark { more, .. } if more.len() == first - 1));
    // In the modern style a long page stays up long enough to read.
    ui.style = Style::Modern;
    assert!(ui.bark_pages(8, long.trim(), &pal, None)[0].1 > pages[0].1);
}

/// The original's replies: "@ answer" left to right 4 apart, a new row past 160 wide, rows
/// 10 apart, flush left, over the Avatar (below it when there's no room above).
#[test]
#[ignore]
fn classic_replies_flow_in_rows_over_the_avatar() {
    let Some(Data { fonts, pal, .. }) = data() else { return };
    let mut ui = ScriptUi::new(fonts);
    ui.style = Style::Classic;
    let options = ["Who are you?", "What is your duty?", "Where am I?", "Bye"].map(String::from).to_vec();
    ui.push(UiEvent::Ask { pid: 1, item: 50, options }, &pal, None, None);
    let mut px = vec![0u8; 320 * 200 * 4];
    let rects = |ui: &mut ScriptUi, px: &mut Vec<u8>, at: [i32; 4]| {
        ui.draw(&mut Canvas::new(px, 320, 200), 1, (320, 200), |id| (id == 1).then_some(at));
        let Panel::Ask { rects, .. } = &ui.panels[0] else { panic!() };
        rects.clone()
    };
    let r = rects(&mut ui, &mut px, [160, 120, 0, 40]);
    let rows: Vec<i32> = r.iter().map(|r| r[1]).collect();
    // As DOS has the guard's: three rows, "Bye" beside "Where am I?".
    assert_eq!((rows[1] - rows[0], rows[2] - rows[1], rows[3] - rows[2]), (10, 10, 0), "{rows:?}");
    assert_eq!((r[1][0], r[2][0]), (r[0][0], r[0][0]), "flush left");
    assert_eq!(r[2][0] + r[2][2] + 4, r[3][0], "4 apart");
    let bottom = r.iter().map(|r| r[1] + r[3]).max().unwrap();
    assert_eq!(bottom, 120, "on top of the anchor");
    let r = rects(&mut ui, &mut px, [160, 15, 0, 40]);
    assert!(r[0][1] >= 15 + 40, "below the feet when there's no room above: {r:?}");
}

#[test]
#[ignore]
fn a_question_left_unanswered_ends_after_the_originals_wait() {
    let Some(Data { fonts, pal, uc, .. }) = data() else { return };
    let (mut vm, mut ui) = (Vm::new(&uc, None), ScriptUi::new(fonts));
    ui.push(UiEvent::Ask { pid: 1, item: 0, options: vec!["Bye.".into()] }, &pal, None, None);
    let mut run = |ui: &mut ScriptUi, secs: f32| {
        for _ in 0..(secs * 30.0) as usize {
            ui.expire(&mut vm, 1.0 / 30.0);
        }
    };
    run(&mut ui, 30.7);
    assert!(ui.blocking(), "still asking");
    run(&mut ui, 0.2);
    assert!(!ui.blocking(), "answered with nothing");
}

#[test]
#[ignore]
fn books_read_like_books() {
    let Some(Data { fonts, pal, art, uc }) = data() else { return };
    let (mut d, mut vm, mut ui) = (Shots { pal, art }, Vm::new(&uc, None), ScriptUi::new(fonts));
    let text = game_text(&uc, "GUARD LOG: ~~Bloodwatch: Guard changed.");
    let read = |kind, gump, text: &str| UiEvent::Read { pid: 1, kind, item: 0, gump, text: text.into() };
    ui.push(read(ReadKind::Book, 0, &text), &d.pal, None, Some(&d.art));
    let r = reading(&ui);
    assert_eq!((r.gump, r.boxes.len()), (6, 2));
    let Pages::Type { lines, .. } = &r.pages else { panic!("in the page font") };
    let lines = &lines[0];
    assert!(lines[..2] == ["GUARD LOG:", ""] && lines[2].starts_with("Bloodwatch: "), "{lines:?}");
    assert!(lines.iter().all(|l| !l.contains('~')));
    // Short enough for two pages: the first click is past the end.
    assert!(r.pages.len() <= 2, "one spread");
    shot(&mut ui, &mut d.art, "book");
    assert!(ui.blocking());
    ui.click(0, 0, &mut vm);
    assert!(!ui.blocking(), "closed");

    // A long one pages; a double click closes it.
    let long = game_text(&uc, "The Spell of Resurrection~~");
    ui.push(read(ReadKind::Book, 0, &long), &d.pal, None, Some(&d.art));
    assert!(reading(&ui).pages.len() > 4, "{} pages", reading(&ui).pages.len());
    shot(&mut ui, &mut d.art, "book_long");
    assert!(ui.read_key(PageKey::On, &mut vm), "Space turns the page");
    assert_eq!(reading(&ui).first, 2);
    shot(&mut ui, &mut d.art, "book_long2");
    assert!(!ui.read_key(PageKey::Other, &mut vm), "other keys don't");
    ui.click(0, 0, &mut vm);
    assert_eq!(reading(&ui).first, 4);
    ui.click(0, 0, &mut vm);
    assert!(!ui.blocking(), "double click: closed");

    // A scroll: one page in the scroll gump.
    ui.push(read(ReadKind::Scroll, 0, &long), &d.pal, None, Some(&d.art));
    assert_eq!((reading(&ui).gump, reading(&ui).boxes.len()), (19, 1));
    shot(&mut ui, &mut d.art, "scroll");
    ui.close(&mut vm);
}

/// Ours on the original's book: a leather cover round it, and the bottom outer corner of a
/// page turned up where there's a page to turn to; a click on the left one turns back, as
/// Left does (Space on); at the first pages, back does nothing.
#[test]
#[ignore]
fn books_have_a_cover_and_turned_up_corners() {
    let Some(Data { fonts, pal, mut art, uc }) = data() else { return };
    let (mut vm, mut ui) = (Vm::new(&uc, None), ScriptUi::new(fonts));
    let text = game_text(&uc, "The Spell of Resurrection~~");
    let ev = UiEvent::Read { pid: 1, kind: ReadKind::Book, item: 0, gump: 0, text };
    ui.push(ev, &pal, None, Some(&art));
    let mut px = vec![0u8; 960 * 600 * 4];
    let mut draw = |ui: &mut ScriptUi, px: &mut Vec<u8>| {
        px.fill(0);
        ui.draw_reading(&mut Canvas::new(px, 960, 600), Some(&mut art), 3, (320, 200));
    };
    draw(&mut ui, &mut px);
    let at = (18, 25); // (the 284×150 gump centred in 320×200)
    let pixel = |px: &[u8], (x, y): (i32, i32)| {
        let d = (((y * 3 + 1) * 960 + x * 3 + 1) * 4) as usize;
        [px[d], px[d + 1], px[d + 2], px[d + 3]]
    };
    let leather = pixel(&px, (at.0 - 3, at.1 + 75));
    assert!(leather[3] == 255 && leather[0] > leather[2] + 20, "the cover beside the pages: {leather:?}");
    assert!(!reading(&ui).more(true) && reading(&ui).more(false), "only the right corner up");
    assert!(ui.read_key(PageKey::Back, &mut vm) && reading(&ui).first == 0, "nothing before the first");
    assert!(ui.read_key(PageKey::On, &mut vm) && reading(&ui).first == 2);
    draw(&mut ui, &mut px);
    let (l, r) = (BOOK.corners[0], BOOK.corners[1]);
    assert!(reading(&ui).more(true) && reading(&ui).more(false), "both corners up");
    // (the flap's light back, a little in from each corner)
    let flap = |c: (i32, i32), dx: i32| pixel(&px, (at.0 + c.0 + dx, at.1 + c.1 - 7));
    assert!(flap(l, 7)[0] > 230 && flap(r, -7)[0] > 230, "{:?} {:?}", flap(l, 7), flap(r, -7));
    ui.click(at.0 + l.0 + 3, at.1 + l.1 - 3, &mut vm);
    assert_eq!(reading(&ui).first, 0, "the left corner turns back");
    assert!(ui.read_key(PageKey::On, &mut vm) && ui.read_key(PageKey::Back, &mut vm));
    assert_eq!(reading(&ui).first, 0, "and Left");
    ui.close(&mut vm);
}

/// Graves and plaques: the script's gump, `*` breaking centred lines; any key puts them away.
#[test]
#[ignore]
fn graves_and_plaques_centre_their_words() {
    let Some(Data { fonts, pal, art, uc }) = data() else { return };
    let (mut d, mut vm, mut ui) = (Shots { pal, art }, Vm::new(&uc, None), ScriptUi::new(fonts));
    let ev = UiEvent::Read {
        pid: 1,
        kind: ReadKind::Plaque,
        item: 0,
        gump: 16,
        text: "THE MONASTERY*OF THE*THEURGISTS ".into(),
    };
    ui.push(ev, &d.pal, None, Some(&d.art));
    let r = reading(&ui);
    assert!(r.sign && r.gump == 16 && r.pages.len() == 1);
    let (gw, gh) = d.art.size(16);
    let Pages::Game(pages) = &r.pages else { panic!("in the plaque's own font") };
    let (at, img) = (r.boxes[0], &pages[0]);
    assert_eq!((at.0, img.w as i32), (8, gw - 16), "centred, 8 in from each side");
    assert!(at.1 >= 0 && at.1 + img.h as i32 <= gh);
    assert_eq!(img.h, 2 * 18 + 14, "three lines, 18 apart");
    shot(&mut ui, &mut d.art, "plaque");
    assert!(ui.read_key(PageKey::Other, &mut vm));
    assert!(!ui.blocking());
    let ev = UiEvent::Read {
        pid: 1,
        kind: ReadKind::Grave,
        item: 0,
        gump: 0x1b,
        text: "SIVLE*THE MINSTREL*WANDERS NO MORE".into(),
    };
    ui.push(ev, &d.pal, None, Some(&d.art));
    shot(&mut ui, &mut d.art, "grave");
    ui.click(5, 5, &mut vm);
    assert!(!ui.blocking(), "a click puts it away");
    // The Avatar's own tombstone: as the DOS game, clicks don't put it away; a key does.
    let ev = UiEvent::Read {
        pid: Pid::MAX,
        kind: ReadKind::Grave,
        item: 0,
        gump: 27,
        text: "HERE LIES*THE AVATAR*REST IN PEACE".into(),
    };
    ui.push(ev, &d.pal, None, Some(&d.art));
    ui.keys_only();
    ui.click(5, 5, &mut vm);
    ui.click(5, 5, &mut vm);
    assert!(ui.reading(), "clicks, even double, leave it");
    assert!(ui.read_key(PageKey::Other, &mut vm) && !ui.reading(), "a key puts it away");
}

/// Speech is placed as the original's (BARK.C): the spot in the view nearest over the speaker,
/// clear of it and of other speech; off the view, the nearest spot in it. Then it keeps that
/// place beside the speaker as they or the view move, until its next page.
#[test]
fn speech_finds_a_clear_place_and_keeps_by_the_speaker() {
    let meets =
        |a: [i32; 4], b: [i32; 4]| a[0] < b[0] + b[2] && b[0] < a[0] + a[2] && a[1] < b[1] + b[3] && b[1] < a[1] + a[3];
    let (view, size) = ((320, 200), (60, 11));
    let guard = [150, 100, 20, 50];
    let (x, y) = place_bark((160, 100), guard, size, view, &[]);
    let first = [x, y, size.0, size.1];
    assert!(
        !meets(first, guard) && y + size.1 <= 100 && y + size.1 > 84 && (x + 30 - 160).abs() <= 4,
        "over him: {first:?}"
    );
    let other = [170, 104, 20, 50];
    let (x, y) = place_bark((180, 104), other, size, view, &[first]);
    assert!(!meets([x, y, size.0, size.1], first) && !meets([x, y, size.0, size.1], other), "clear of the first");
    assert_eq!(place_bark((-200, -100), [-210, -100, 20, 50], size, view, &[]), (0, 0), "nearest spot in view");
}

#[test]
#[ignore]
fn speech_moves_with_its_speaker() {
    let Some(Data { fonts, pal, .. }) = data() else { return };
    let mut ui = ScriptUi::new(fonts);
    ui.push(UiEvent::Bark { pid: 1, item: 2, text: "Please sir, I have no more money.".into() }, &pal, None, None);
    let mut px = vec![0u8; 320 * 200 * 4];
    let mut at = |r: [i32; 4]| {
        ui.draw(&mut Canvas::new(&mut px, 320, 200), 1, (320, 200), |_| Some(r));
        bark_rect(&ui.panels[0], &|_| Some(r)).unwrap()
    };
    let a = at([150, 100, 20, 50]);
    let b = at([180, 110, 20, 50]);
    assert_eq!((b[0] - a[0], b[1] - a[1]), (30, 10), "moved with him");
    let c = at([-400, -300, 20, 50]);
    assert_eq!((c[0] - a[0], c[1] - a[1]), (-550, -400), "off the view with him, not stuck at its edge");
}
