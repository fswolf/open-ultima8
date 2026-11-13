//! Strings and lists that scripts hold by 16-bit handle. Lists store fixed-size elements;
//! "string lists" hold string handles and own those strings.
use super::stack::le16;
use std::collections::HashMap;

#[derive(Clone)]
pub(crate) struct List {
    pub elem: usize,
    pub data: Vec<u8>,
}

impl List {
    pub fn len(&self) -> usize {
        self.data.len() / self.elem.max(1)
    }
    pub fn get(&self, i: usize) -> &[u8] {
        &self.data[i * self.elem..(i + 1) * self.elem]
    }
    pub fn get_u16(&self, i: usize) -> u16 {
        le16(self.get(i))
    }
    pub fn handles(&self) -> Vec<u16> {
        (0..self.len()).map(|i| self.get_u16(i)).collect()
    }
}

pub(crate) struct Heap {
    strings: HashMap<u16, String>,
    lists: HashMap<u16, List>,
    next_str: u16,
    next_list: u16,
}

/// Insert under the next free non-zero handle (a full table overwrites rather than spinning forever).
fn alloc<T>(map: &mut HashMap<u16, T>, next: &mut u16, v: T) -> u16 {
    let mut id = *next;
    for _ in 0..u16::MAX {
        if !map.contains_key(&id) {
            break;
        }
        id = id.wrapping_add(1).max(1);
    }
    *next = id.wrapping_add(1).max(1);
    map.insert(id, v);
    id
}

impl Heap {
    pub fn new() -> Self {
        Self { strings: HashMap::new(), lists: HashMap::new(), next_str: 1, next_list: 1 }
    }
    pub fn clear(&mut self) {
        self.strings.clear();
        self.lists.clear();
    }
    pub fn counts(&self) -> (usize, usize) {
        (self.strings.len(), self.lists.len())
    }

    // --- strings ---
    pub fn new_string(&mut self, s: String) -> u16 {
        alloc(&mut self.strings, &mut self.next_str, s)
    }
    pub fn string(&self, id: u16) -> String {
        self.strings.get(&id).cloned().unwrap_or_default()
    }
    pub fn dup_string(&mut self, id: u16) -> u16 {
        let s = self.string(id);
        self.new_string(s)
    }
    pub fn free_string(&mut self, id: u16) {
        self.strings.remove(&id);
    }
    /// `y += x`, freeing x.
    pub fn concat(&mut self, y: u16, x: u16) {
        let s = self.string(y) + &self.string(x);
        self.strings.remove(&x);
        self.strings.insert(y, s);
    }
    /// Compare two strings, freeing both.
    pub fn str_eq(&mut self, x: u16, y: u16) -> bool {
        let eq = self.string(x) == self.string(y);
        self.strings.remove(&x);
        self.strings.remove(&y);
        eq
    }

    // --- lists ---
    pub fn new_list(&mut self, elem: usize, data: Vec<u8>) -> u16 {
        alloc(&mut self.lists, &mut self.next_list, List { elem, data })
    }
    pub fn list(&self, id: u16) -> Option<&List> {
        self.lists.get(&id)
    }
    /// Element `i` of a list (None if out of range).
    pub fn elem(&self, id: u16, i: usize) -> Option<Vec<u8>> {
        self.lists.get(&id).filter(|l| i < l.len()).map(|l| l.get(i).to_vec())
    }
    pub fn set_elem(&mut self, id: u16, i: usize, v: &[u8]) {
        if let Some(l) = self.lists.get_mut(&id) {
            if i < l.len() {
                let n = v.len().min(l.elem);
                l.data[i * l.elem..i * l.elem + n].copy_from_slice(&v[..n]);
            }
        }
    }
    /// Copy a list (string lists get their own copies of the strings).
    pub fn dup_list(&mut self, id: u16, strings: bool) -> u16 {
        let mut l = self.lists.get(&id).cloned().unwrap_or(List { elem: 2, data: vec![] });
        if strings {
            for i in 0..l.len() {
                let s = self.dup_string(l.get_u16(i));
                l.data[i * 2..i * 2 + 2].copy_from_slice(&s.to_le_bytes());
            }
        }
        alloc(&mut self.lists, &mut self.next_list, l)
    }
    pub fn free_list(&mut self, id: u16, strings: bool) {
        if let Some(l) = self.lists.remove(&id) {
            if strings {
                l.handles().iter().for_each(|h| self.free_string(*h));
            }
        }
    }
    /// Strings of a string list.
    pub fn slist(&self, id: u16) -> Vec<String> {
        self.lists.get(&id).map_or(vec![], |l| l.handles().into_iter().map(|h| self.string(h)).collect())
    }

    /// `y += x`, consuming x. String lists skip strings y already has (and free them).
    pub fn append(&mut self, y: u16, x: u16, strings: bool) {
        let Some(l) = self.lists.remove(&x) else { return };
        if !strings {
            if let Some(ly) = self.lists.get_mut(&y) {
                ly.data.extend_from_slice(&l.data);
            }
            return;
        }
        for k in 0..l.len() {
            let s = self.string(l.get_u16(k));
            let present = self.lists.get(&y).is_some_and(|ly| ly.handles().iter().any(|&h| self.string(h) == s));
            if present {
                self.free_string(l.get_u16(k));
            } else if let Some(ly) = self.lists.get_mut(&y) {
                ly.data.extend_from_slice(l.get(k));
            }
        }
    }

    /// Remove from y every element that's in x, consuming x. String lists compare by text and
    /// free the strings involved.
    pub fn remove_all(&mut self, y: u16, x: u16, strings: bool) {
        let Some(l) = self.lists.remove(&x) else { return };
        let gone: Vec<Vec<u8>> = (0..l.len()).map(|k| l.get(k).to_vec()).collect();
        let gone_s: Vec<String> = if strings { l.handles().iter().map(|&h| self.string(h)).collect() } else { vec![] };
        if strings {
            l.handles().iter().for_each(|h| self.free_string(*h));
        }
        let Some(ly) = self.lists.get(&y) else { return };
        let elems: Vec<Vec<u8>> = (0..ly.len()).map(|k| ly.get(k).to_vec()).collect();
        let (mut keep, mut dropped) = (vec![], vec![]);
        for el in elems {
            let hit = if strings { gone_s.contains(&self.string(le16(&el))) } else { gone.contains(&el) };
            if hit {
                dropped.push(el);
            } else {
                keep.extend(el);
            }
        }
        if let Some(ly) = self.lists.get_mut(&y) {
            ly.data = keep;
        }
        if strings {
            dropped.iter().for_each(|el| self.free_string(le16(el)));
        }
    }

    /// Is `elem` in list `id`? Consumes the list (and, for strings, the probe string).
    pub fn contains(&mut self, id: u16, elem: &[u8], strings: bool) -> bool {
        let found = match self.lists.get(&id) {
            Some(l) if strings => {
                let s = self.string(le16(elem));
                l.handles().iter().any(|&h| self.string(h) == s)
            }
            Some(l) => (0..l.len()).any(|k| l.get(k) == elem),
            None => false,
        };
        if strings {
            self.free_string(le16(elem));
        }
        self.free_list(id, strings);
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_lists_dedupe_and_remove_by_text() {
        let mut h = Heap::new();
        let s = |h: &mut Heap, t: &str| h.new_string(t.into()).to_le_bytes();
        let (a, b) = (s(&mut h, "a"), s(&mut h, "b"));
        let y = h.new_list(2, [a, b].concat());
        let (a2, c) = (s(&mut h, "a"), s(&mut h, "c"));
        let x = h.new_list(2, [a2, c].concat());
        h.append(y, x, true);
        assert_eq!(h.slist(y), ["a", "b", "c"]);
        let b2 = s(&mut h, "b");
        let x = h.new_list(2, b2.to_vec());
        h.remove_all(y, x, true);
        assert_eq!(h.slist(y), ["a", "c"]);
        h.free_list(y, true);
        assert_eq!(h.counts(), (0, 0));
    }
}
