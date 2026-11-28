//! What's inside what, moving things between the map and containers, and the void (the
//! ethereal stack scripts park things on).
use super::*;

/// Where in a container's window things put into it at random land.
pub const POP_AREA: (u32, u32) = (60, 40);

impl World {
    /// Put `it` inside `parent` (it keeps its window position in x, y). Returns its id.
    pub fn add_inside(&mut self, gd: &GameData, mut it: Item, parent: ObjId) -> ObjId {
        it.flags |= item_flag::CONTAINED; // (scripts ask: "the focus must be held")
        let id = self.new_id();
        let idx = self.entries.len();
        self.entries.push(Entry { void: true, parent: Some(parent), ..Entry::new(gd, it) });
        self.ids.push(id);
        self.marked.push(false);
        self.by_id.insert(id, idx);
        self.touch(idx);
        id
    }

    /// Add item-cache contents belonging to `owners` (and everything inside those, recursively).
    pub fn add_carried(&mut self, gd: &GameData, all: &[map::Contained], owners: &[ObjId]) {
        let mut ids: HashMap<u16, ObjId> = owners.iter().map(|&o| (o, o)).collect();
        let mut queue: Vec<u16> = owners.to_vec();
        while let Some(p) = queue.pop() {
            for c in all.iter().filter(|c| c.parent == p) {
                if ids.contains_key(&c.id) {
                    continue; // a loop in the data
                }
                let id = self.add_inside(gd, c.item, ids[&p]);
                ids.insert(c.id, id);
                queue.push(c.id);
            }
        }
    }

    /// What's directly inside `id` (a container, or a person's belongings), in order.
    pub fn contents(&self, id: ObjId) -> Vec<ObjId> {
        (0..self.entries.len())
            .filter(|&i| !self.entries[i].gone && self.entries[i].parent == Some(id))
            .map(|i| self.ids[i])
            .collect()
    }

    /// The container `id` is in, if any.
    pub fn container(&self, id: ObjId) -> Option<ObjId> {
        self.index(id).and_then(|i| self.entries[i].parent)
    }

    /// Take `id`'s belongings off this map (for it to carry them to another): everything inside
    /// it, recursively, as (old id, old container id, item). They're gone from here afterwards.
    pub fn take_contents(&mut self, id: ObjId) -> Vec<(ObjId, ObjId, Item)> {
        let mut out = vec![];
        let mut queue = vec![id];
        while let Some(p) = queue.pop() {
            for c in self.contents(p) {
                if let Some(it) = self.get(c) {
                    out.push((c, p, it));
                }
                queue.push(c);
                self.remove(c);
            }
        }
        out
    }

    /// Put belongings taken with [`take_contents`](Self::take_contents) back under `id` here.
    pub fn give_contents(&mut self, gd: &GameData, id: ObjId, items: &[(ObjId, ObjId, Item)]) {
        let mut ids: HashMap<ObjId, ObjId> = HashMap::from([(id, id)]);
        for &(old, parent, it) in items {
            if let Some(&p) = ids.get(&parent) {
                let new = self.add_inside(gd, it, p);
                ids.insert(old, new);
            }
        }
    }

    /// This map's NONFIXED.DAT entry: every movable item on the map, containers with their contents.
    /// Destroyed items and items in the void are left out.
    pub fn movable_records(&self) -> Vec<u8> {
        let inside = self.inside_map();
        let mut out = Vec::new();
        for (i, e) in self.entries.iter().enumerate() {
            if e.origin == Origin::Movable && e.present() && !self.disposable(self.ids[i]) {
                out.extend_from_slice(&e.it.record());
                self.nest(&inside, i, 1, &mut out);
            }
        }
        out
    }

    /// What's inside `id` as NONFIXED-style records nested by depth (empty if it's no container).
    pub fn contents_records(&self, id: ObjId) -> Vec<u8> {
        let mut out = Vec::new();
        if let Some(i) = self.index(id) {
            self.nest(&self.inside_map(), i, 1, &mut out);
        }
        out
    }

    fn inside_map(&self) -> HashMap<ObjId, Vec<usize>> {
        let mut inside: HashMap<ObjId, Vec<usize>> = HashMap::new();
        for (i, e) in self.entries.iter().enumerate() {
            if let (Some(p), false) = (e.parent, e.gone) {
                inside.entry(p).or_default().push(i);
            }
        }
        inside
    }

    fn nest(&self, inside: &HashMap<ObjId, Vec<usize>>, i: usize, depth: usize, out: &mut Vec<u8>) {
        if self.entries[i].traits.family != family::CONTAINER {
            return;
        }
        for &c in inside.get(&self.ids[i]).map_or(&[][..], Vec::as_slice) {
            out.extend_from_slice(&self.entries[c].it.contained_record(depth));
            self.nest(inside, c, depth + 1, out);
        }
    }

    /// The outermost container `id` is in (itself if it's on the map).
    pub fn root(&self, id: ObjId) -> ObjId {
        let mut r = id;
        for _ in 0..64 {
            match self.container(r) {
                Some(p) => r = p,
                None => break,
            }
        }
        r
    }

    /// Is `id` inside `outer` (at any depth)?
    pub fn is_inside(&self, id: ObjId, outer: ObjId) -> bool {
        let mut r = id;
        for _ in 0..64 {
            match self.container(r) {
                Some(p) if p == outer => return true,
                Some(p) => r = p,
                None => return false,
            }
        }
        false
    }

    /// Put `id` inside `parent` at window position `at` (off the map).
    pub fn move_inside(&mut self, gd: &GameData, id: ObjId, parent: ObjId, at: (i32, i32)) {
        let Some(i) = self.index(id) else { return };
        let mut it = self.entries[i].it;
        (it.x, it.y, it.z) = (at.0, at.1, 0);
        it.flags = (it.flags | item_flag::CONTAINED) & !item_flag::EQUIPPED;
        self.entries[i].parent = Some(parent);
        self.entries[i].void = true;
        self.update(gd, i, it);
        self.ethereal.retain(|&e| e != id);
    }

    /// Put `id` on the map at (x, y, z), out of whatever it was in.
    pub fn move_to_map(&mut self, gd: &GameData, id: ObjId, (x, y, z): (i32, i32, i32)) {
        let Some(i) = self.index(id) else { return };
        let mut it = self.entries[i].it;
        (it.x, it.y, it.z) = (x, y, z);
        it.flags &= !(item_flag::CONTAINED | item_flag::EQUIPPED);
        if self.entries[i].parent.is_some() || self.entries[i].void {
            self.requeue(i, &it); // (back onto the map)
        }
        self.entries[i].parent = None;
        self.entries[i].void = false;
        self.update(gd, i, it);
        self.ethereal.retain(|&e| e != id);
    }

    /// Into `parent` at a random spot in its window (ScummVM's moveToContainer); the Avatar's
    /// now, if that's where it ends up.
    pub fn put_into(&mut self, gd: &GameData, id: ObjId, parent: ObjId) {
        let spot = (self.roll(POP_AREA.0) as i32, self.roll(POP_AREA.1) as i32);
        self.move_inside(gd, id, parent, spot);
        if self.root(id) == AVATAR {
            self.own(gd, id);
        }
    }

    /// `id` and everything in it are the Avatar's (OWNED: taking them isn't stealing).
    pub fn own(&mut self, gd: &GameData, id: ObjId) {
        if let Some(mut it) = self.get(id) {
            it.flags |= item_flag::OWNED;
            self.set(gd, id, it);
        }
        for c in self.contents(id) {
            self.own(gd, c);
        }
    }

    /// Gone for good. What was inside spills into the container's own holder (a bag, a person)
    /// or onto the map where it stood (ScummVM's Container::destroy → removeContents).
    pub fn destroy(&mut self, gd: &GameData, id: ObjId) {
        let (parent, at) = (self.container(id), self.get(id).map(|it| (it.x, it.y, it.z)));
        for c in self.contents(id) {
            match (parent, at) {
                (Some(p), _) => self.put_into(gd, c, p),
                (None, Some(at)) => self.move_to_map(gd, c, at),
                _ => {}
            }
        }
        self.remove(id);
    }

    /// In the void (off the map, still known)?
    pub fn is_void(&self, id: ObjId) -> bool {
        self.index(id).is_some_and(|i| self.entries[i].void)
    }

    /// Take an item off the map without forgetting it (usecode's ethereal void), or put it back.
    pub fn set_void(&mut self, id: ObjId, void: bool) {
        if let Some(i) = self.index(id) {
            if self.entries[i].void && !void {
                let it = self.entries[i].it;
                self.requeue(i, &it);
            }
            self.entries[i].void = void;
            self.touch(i);
        }
        if !void {
            self.ethereal.retain(|&e| e != id);
        }
    }

    /// Push `id` into the void, on top of the ethereal stack (scripts pop it out somewhere).
    pub fn push_ethereal(&mut self, id: ObjId) {
        if self.index(id).is_some() {
            self.set_void(id, true);
            self.ethereal.retain(|&e| e != id);
            self.ethereal.push(id);
        }
    }

    /// The thing on top of the ethereal stack (things gone meanwhile are dropped).
    pub fn ethereal_top(&mut self) -> Option<ObjId> {
        while let Some(&id) = self.ethereal.last() {
            if self.index(id).is_some() {
                return Some(id);
            }
            self.ethereal.pop();
        }
        None
    }

    /// Leaving the map: whatever is still in the ethereal stack is thrown away.
    pub fn clear_ethereal(&mut self) {
        for id in std::mem::take(&mut self.ethereal) {
            self.remove(id);
        }
    }
}
