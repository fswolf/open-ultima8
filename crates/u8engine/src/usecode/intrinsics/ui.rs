//! Intrinsics that show something to the player. Each queues a [`UiEvent`] and returns a process
//! the UI finishes when the player is done, so the script can wait on it.
use super::Ctx;
use crate::usecode::vm::{ReadKind, UiEvent};

fn show(c: &mut Ctx, ev: impl FnOnce(u16) -> UiEvent) -> u32 {
    let pid = c.vm.spawn_native(c.this());
    c.vm.events.push_back(ev(pid));
    pid as u32
}

/// bark(text): speech over the item.
pub fn bark(c: &mut Ctx) -> u32 {
    let (item, text) = (c.this(), c.string(c.arg2(4)));
    show(c, |pid| UiEvent::Bark { pid, item, text })
}
/// ask(options): a conversation menu; the answer is the chosen string.
pub fn ask(c: &mut Ctx) -> u32 {
    let (item, options) = (c.this(), c.vm.heap.slist(c.arg2(4)));
    show(c, |pid| UiEvent::Ask { pid, item, options })
}
/// getSliderInput(min, max, step): the player picks a number; the script stops here until then
/// and gets it as this call's result (ScummVM's SliderGump: setReturnValue, then wake).
pub fn slider(c: &mut Ctx) -> u32 {
    let (min, max, step) = (c.arg2(4) as i16, c.arg2(6) as i16, c.arg2(8) as i16);
    let pid = c.vm.spawn_native(0);
    c.vm.events.push_back(UiEvent::Slider { pid, min, max, step });
    c.vm.wait_on(c.pid, pid);
    c.vm.wake_with_value(c.pid);
    min as u16 as u32
}

fn read(c: &mut Ctx, kind: ReadKind, gump: u16, text: String) -> u32 {
    let item = c.this();
    show(c, |pid| UiEvent::Read { pid, kind, item, gump, text })
}
/// Book::read(text). The Grimoire's Spell of Resurrection (quality 0x66) shows another book's
/// text in some versions — its script checks 0x66 twice — so, as ScummVM does (bug #12503), it
/// gets its own: the text the unreachable script would show.
pub fn read_book(c: &mut Ctx) -> u32 {
    let mut text = c.string(c.arg2(4));
    if c.item().is_some_and(|i| i.shape == 0x120 && i.quality == 0x66) {
        text = usecode_text(c, b"The Spell of Resurrection~").unwrap_or(text);
    }
    read(c, ReadKind::Book, 0, text)
}
pub fn read_scroll(c: &mut Ctx) -> u32 {
    let text = c.string(c.arg2(4));
    read(c, ReadKind::Scroll, 0, text)
}
/// Grave::read(gump, text) and Plaque::read(gump, text).
pub fn read_grave(c: &mut Ctx) -> u32 {
    let (gump, text) = (c.arg2(4), c.string(c.arg2(6)));
    read(c, ReadKind::Grave, gump, text)
}
pub fn read_plaque(c: &mut Ctx) -> u32 {
    let (gump, text) = (c.arg2(4), c.string(c.arg2(6)));
    read(c, ReadKind::Plaque, gump, text)
}

/// The string constant in the usecode that starts with `start`.
fn usecode_text(c: &Ctx, start: &[u8]) -> Option<String> {
    (0..c.usecode.class_count() as u32).find_map(|k| {
        let d = c.usecode.class(k)?;
        let at = d.windows(start.len()).position(|w| w == start)?;
        let len = d[at..].iter().position(|&b| b == 0)?;
        Some(String::from_utf8_lossy(&d[at..at + len]).into_owned())
    })
}
