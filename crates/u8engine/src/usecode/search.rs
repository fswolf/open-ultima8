//! `loop` searches: candidate items by search type, filtered by a small RPN script.
use super::host::Host;
use crate::world::ObjId;
use u8data::map::Item;

/// Bytes the original reserves on the stack for each search type's state (scripts clean up
/// after the loop themselves, so the size must match).
pub(crate) fn frame_size(stype: u8) -> usize {
    match stype {
        4 => 40,
        5 => 42,
        6 => 61,
        _ => 52,
    }
}

/// Items a search of type `stype` considers, in the order it meets them (ScummVM's UCMachine
/// 0x70): 2/3 an area around item `a` (where its outermost container is) of range `b`, 3 with
/// what's inside; 4/5 container `b`'s contents (5 all the way down); 6 what's on top of `a`
/// (unless 0xFFFF) and under `b` (unless 0xFFFF; then `b` is the item).
pub(crate) fn candidates(host: &dyn Host, stype: u8, a: u16, b: u16) -> Vec<ObjId> {
    match stype {
        2 | 3 => {
            let mut root = a;
            for _ in 0..32 {
                match host.container(root) {
                    Some(p) => root = p,
                    None => break,
                }
            }
            host.item(root).map_or(vec![], |it| host.search_area((it.x, it.y), b as i32, stype == 3))
        }
        4 => host.contents(b),
        5 => {
            fn all(host: &dyn Host, c: ObjId, out: &mut Vec<ObjId>) {
                for k in host.contents(c) {
                    out.push(k);
                    all(host, k, out);
                }
            }
            let mut v = vec![];
            all(host, b, &mut v);
            v
        }
        6 => {
            let (above, below) = (a != 0xFFFF, b != 0xFFFF);
            host.search_surface(if below { b } else { a }, above, below)
        }
        _ => vec![],
    }
}

/// Evaluate a search script against an item (ScummVM's Item::checkLoopScript). Tokens: bytes 0
/// and 1 false and true, `%` an i16 literal, item fields (`#` NPC number, `?` status flags, `*`
/// quality, `:` family, `@` shape, `` ` `` frame), `A`–`Z` "shape is one of the next 1–26 i16s",
/// `a`–`z` the same for the frame, `!` not, `& + = > < ] [` binary ops, `$` end.
pub(crate) fn matches(script: &[u8], it: &Item, host: &dyn Host) -> bool {
    let tf = host.typeflag(it.shape);
    let lit =
        |k: usize| i16::from_le_bytes([script.get(k).copied().unwrap_or(0), script.get(k + 1).copied().unwrap_or(0)]);
    let mut st: Vec<i32> = vec![];
    let mut i = 0;
    while i < script.len() {
        let t = script[i];
        i += 1;
        let v = match t {
            b'$' => break,
            0 | 1 => t as i32,
            b'%' => {
                i += 2;
                lit(i - 2) as i32
            }
            b'#' => it.npc as i32,
            b'?' => it.flags as i32,
            b'*' => it.quality as i32,
            b':' => (tf[1] >> 4) as i32,
            b'@' => it.shape as i32,
            b'`' => it.frame as i32,
            b'A'..=b'Z' | b'a'..=b'z' => {
                // One of a list: shapes (capitals) or frames.
                let (n, field) = if t.is_ascii_uppercase() { (t - b'@', it.shape) } else { (t - b'`', it.frame) };
                let hit = (0..n as usize).any(|k| lit(i + 2 * k) as u16 as u32 == field);
                i += 2 * n as usize;
                hit as i32
            }
            b'!' => (st.pop().unwrap_or(0) == 0) as i32,
            b'&' | b'+' | b'=' | b'>' | b'<' | b']' | b'[' => {
                let (b, a) = (st.pop().unwrap_or(0), st.pop().unwrap_or(0));
                (match t {
                    b'&' => a != 0 && b != 0,
                    b'+' => a != 0 || b != 0,
                    b'=' => a == b,
                    b'>' => a > b,
                    b'<' => a < b,
                    b']' => a >= b,
                    _ => a <= b,
                }) as i32
            }
            _ => continue, // (unknown: ScummVM warns and skips it)
        };
        st.push(v);
    }
    st.last().is_none_or(|&v| v != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoHost;
    impl Host for NoHost {
        fn item(&self, _: ObjId) -> Option<Item> {
            None
        }
        fn set_item(&mut self, _: ObjId, _: Item) {}
        fn destroy(&mut self, _: ObjId) {}
        fn create(&mut self, _: Item, _: bool) -> ObjId {
            0
        }
        fn typeflag(&self, _: u32) -> [u8; 8] {
            [0; 8]
        }
        fn frame_count(&self, _: u32) -> u32 {
            1
        }
        fn area(&self, _: i32, _: i32, _: i32) -> Vec<ObjId> {
            vec![]
        }
        fn map_num(&self) -> u16 {
            0
        }
        fn set_ethereal(&mut self, _: ObjId, _: bool) {}
    }

    #[test]
    fn search_scripts_read_as_scummvm_does() {
        let it = Item { shape: 124, frame: 2, quality: 7, npc: 3, flags: 0x401, ..Default::default() };
        let yes = |s: &[u8]| matches(s, &it, &NoHost);
        // The cheat menu's "Run Pyros": shape 0x7C and frame 2.
        assert!(yes(b"%\x7c\x00@=%\x02\x00`=&$"));
        assert!(!yes(b"%\x7c\x00@=%\x03\x00`=&$"), "another frame");
        assert!(yes(b"%\x03\x00#=$"), "# is the NPC number");
        assert!(yes(b"%\x01\x04?=$"), "? the status flags");
        assert!(yes(b"%\x07\x00*=$"));
        assert!(yes(b"C\x01\x00\x7c\x00\x09\x00$"), "C: one of three shapes");
        assert!(!yes(b"B\x01\x00\x09\x00$"));
        assert!(yes(b"b\x05\x00\x02\x00$"), "b: one of two frames");
        assert!(yes(&[1, b'$']) && !yes(&[0, b'$']));
    }
}
