//! Things to read: books and scrolls (laid out in the books' typeface, paged, with our cover
//! and turned-up corners) and graves and plaques (in the game's own fonts).
use super::*;

/// How the original shows something to read (ScummVM's BookGump, ScrollGump, ReadableGump):
/// U8GUMPS shape, page size and where pages go in it (a book has two side by side). The text
/// is in [`PageFont`] where the original had its font 9. A book also gets our cover and its
/// pages' bottom outer corners (in the gump) turn up when there are more ([`crate::cover`]).
pub(super) struct Layout {
    pub(super) gump: u32,
    pub(super) page: (u32, u32),
    pub(super) boxes: &'static [(i32, i32)],
    pub(super) corners: &'static [(i32, i32)],
}
// (ScummVM's book pages are 129 high: ours stop 3 short, so the turned-up corners clear the text.)
pub(super) const BOOK: Layout =
    Layout { gump: 6, page: (123, 126), boxes: &[(9, 5), (150, 5)], corners: &[(6, 140), (278, 140)] };
pub(super) const SCROLL: Layout = Layout { gump: 19, page: (204, 115), boxes: &[(22, 29)], corners: &[] };
pub(super) const GRAVE_FONT: usize = 11;
pub(super) const PLAQUE_FONT: usize = 10;

/// Something open to read. Books and scrolls page on a click (or Space) and close on a double
/// click, Esc, or a click past the last page; graves and plaques close on any click or key.
pub(super) struct Readable {
    pub(super) pid: Pid,
    pub(super) gump: u32,
    /// Top-left of each page shown at once, in the gump, and a page's size.
    pub(super) boxes: Vec<(i32, i32)>,
    pub(super) size: (i32, i32),
    /// A book's: its pages' bottom outer corners, in the gump (none: no cover either).
    pub(super) corners: &'static [(i32, i32)],
    /// Where the gump was last drawn (for clicks on the corners).
    pub(super) at: Option<(i32, i32)>,
    pub(super) pages: Pages,
    /// The page in the first box.
    pub(super) first: usize,
    pub(super) sign: bool,
    /// Put away by a key only (the Avatar's tombstone: clicks don't, in the DOS game).
    pub(super) keys_only: bool,
    pub(super) last_click: Option<Instant>,
}

/// What's on the pages: graves' and plaques' words in the game's own fonts (game pixels), or
/// book and scroll pages laid out for [`PageFont`], drawn at the screen's resolution (kept for
/// the scale they were drawn at).
pub(super) enum Pages {
    Game(Vec<TextImage>),
    Type { lines: Vec<Vec<String>>, drawn: Option<(u32, Vec<TextImage>)> },
}

impl Pages {
    pub(super) fn len(&self) -> usize {
        match self {
            Pages::Game(p) => p.len(),
            Pages::Type { lines, .. } => lines.len(),
        }
    }
}

impl Readable {
    pub(super) fn new(
        (fonts, type_font): (&Fonts, &PageFont),
        pal: &Palette,
        pid: Pid,
        (kind, gump): (ReadKind, u16),
        text: &str,
        art: Option<&GumpArt>,
    ) -> Self {
        let lay = match kind {
            ReadKind::Book => BOOK,
            ReadKind::Scroll => SCROLL,
            ReadKind::Grave | ReadKind::Plaque => {
                // The whole text, centred in the gump less 8 each side (no paging: * breaks lines).
                let font = if kind == ReadKind::Grave { GRAVE_FONT } else { PLAQUE_FONT };
                let (gw, gh) = art.map_or((200, 120), |a| a.size(gump as u32));
                let w = (gw - 16).max(16) as u32;
                let (lines, _) = fonts.typeset(font, text, w, 0, false, true);
                let img = fonts.render_set(font, &lines, pal, w, true);
                let at = ((gw - img.w as i32) / 2, (gh - img.h as i32) / 2);
                let size = (img.w as i32, img.h as i32);
                let pages = Pages::Game(vec![img]);
                let boxes = vec![at];
                return Self {
                    pid,
                    gump: gump as u32,
                    boxes,
                    size,
                    pages,
                    corners: &[],
                    at: None,
                    first: 0,
                    sign: true,
                    keys_only: false,
                    last_click: None,
                };
            }
        };
        let pages = Pages::Type { lines: type_font.pages(text, lay.page), drawn: None };
        let size = (lay.page.0 as i32, lay.page.1 as i32);
        let boxes = lay.boxes.to_vec();
        let corners = lay.corners;
        Self {
            pid,
            gump: lay.gump,
            boxes,
            size,
            pages,
            corners,
            at: None,
            first: 0,
            sign: false,
            keys_only: false,
            last_click: None,
        }
    }

    /// Turn back a page; false if this is the first.
    pub(super) fn back(&mut self) -> bool {
        let n = self.boxes.len();
        if self.sign || self.first == 0 {
            return false;
        }
        self.first = self.first.saturating_sub(n);
        true
    }

    /// Is there a page before the first shown (left), or after the last (right)?
    pub(super) fn more(&self, left: bool) -> bool {
        match left {
            true => self.first > 0,
            false => self.first + self.boxes.len() < self.pages.len(),
        }
    }

    /// Is (x, y) on the turned-up corner of the left page (where a click turns back)?
    pub(super) fn on_back(&self, (x, y): (i32, i32)) -> bool {
        let (Some(at), Some(&c)) = (self.at, self.corners.first()) else { return false };
        self.more(true) && crate::cover::on_curl((x - at.0, y - at.1), c, true)
    }

    /// Turn the page; false if there was no more.
    pub(super) fn next(&mut self) -> bool {
        let n = self.boxes.len();
        if self.sign || self.first + n >= self.pages.len() {
            return false;
        }
        self.first += n;
        true
    }

    pub(super) fn draw(
        &mut self,
        canvas: &mut Canvas,
        art: Option<&mut GumpArt>,
        type_font: &PageFont,
        ss: u32,
        (vw, vh): (i32, i32),
    ) {
        let (gw, gh) = art.as_ref().map_or((0, 0), |a| a.size(self.gump));
        let (x0, y0) = match art {
            Some(art) if gw > 0 => {
                let at = ((vw - gw) / 2, (vh - gh) / 2);
                if !self.corners.is_empty() {
                    crate::cover::cover(canvas, ss, (at.0, at.1, gw, gh));
                }
                art.draw(canvas, self.gump, 0, at.0, at.1);
                self.at = Some(at);
                at
            }
            _ => {
                // No window art: the text on a dark panel.
                let w = self.boxes.iter().map(|b| b.0).max().unwrap_or(0) + self.size.0;
                let h = self.boxes.iter().map(|b| b.1).max().unwrap_or(0) + self.size.1;
                let at = ((vw - w) / 2, (vh - h) / 2);
                text::shade(canvas, ss, [at.0 - 8, at.1 - 6, w + 16, h + 12]);
                at
            }
        };
        let first = self.first;
        match &mut self.pages {
            Pages::Game(pages) => {
                for (b, p) in self.boxes.iter().zip(pages.iter().skip(first)) {
                    text::draw_plain(canvas, ss, p, x0 + b.0, y0 + b.1);
                }
            }
            Pages::Type { lines, drawn } => {
                if drawn.as_ref().is_none_or(|d| d.0 != ss) {
                    *drawn = Some((ss, lines.iter().map(|l| type_font.render(l, ss)).collect()));
                }
                let s = ss as i32;
                for (b, p) in self.boxes.iter().zip(drawn.as_ref().unwrap().1.iter().skip(first)) {
                    text::draw_blended(canvas, p, (x0 + b.0) * s, (y0 + b.1) * s);
                }
            }
        }
        // (over the text: a corner turned up hides what's under it)
        for (k, &c) in self.corners.iter().enumerate().filter(|_| self.at.is_some()) {
            let left = k == 0;
            if self.more(left) {
                crate::cover::curl(canvas, ss, (x0 + c.0, y0 + c.1), left);
            }
        }
    }
}

/// What a key does to pages open to read.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PageKey {
    On,
    Back,
    Other,
}
