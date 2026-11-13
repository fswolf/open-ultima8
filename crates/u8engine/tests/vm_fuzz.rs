//! The VM must survive any bytecode (random programs against a stub world don't panic), and
//! hand-written programs check opcodes against ScummVM's UCMachine.
use std::collections::HashMap;
use u8data::{flex::Flex, map::Item, usecode::Usecode};
use u8engine::usecode::{Host, UiEvent, Vm};
use u8engine::ObjId;

#[derive(Default)]
struct Stub {
    items: HashMap<ObjId, Item>,
    next: ObjId,
}

impl Host for Stub {
    fn item(&self, id: ObjId) -> Option<Item> {
        self.items.get(&id).copied()
    }
    fn set_item(&mut self, id: ObjId, it: Item) {
        self.items.insert(id, it);
    }
    fn destroy(&mut self, id: ObjId) {
        self.items.remove(&id);
    }
    fn create(&mut self, it: Item, _on_map: bool) -> ObjId {
        self.next += 1;
        self.items.insert(self.next, it);
        self.next
    }
    fn typeflag(&self, _shape: u32) -> [u8; 8] {
        [0x0B, 0x10, 0x20, 0x22, 0, 0, 3, 0]
    }
    fn frame_count(&self, _shape: u32) -> u32 {
        4
    }
    fn area(&self, _x: i32, _y: i32, _range: i32) -> Vec<ObjId> {
        self.items.keys().copied().collect()
    }
    fn map_num(&self) -> u16 {
        1
    }
    fn set_ethereal(&mut self, _id: ObjId, _on: bool) {}
}

/// A Flex holding the given entries.
fn flex(entries: &[Vec<u8>]) -> Vec<u8> {
    let mut d = vec![0x1a; 0x80];
    d[0x54..0x58].copy_from_slice(&(entries.len() as u32).to_le_bytes());
    let mut off = 0x80 + entries.len() * 8;
    for e in entries {
        d.extend((off as u32).to_le_bytes());
        d.extend((e.len() as u32).to_le_bytes());
        off += e.len();
    }
    entries.iter().for_each(|e| d.extend(e));
    d
}

/// Usecode with one class (0) whose `look` handler is `code`.
fn usecode(code: &[u8]) -> Usecode {
    let mut class = vec![0u8; 12 + 32 * 4];
    class[12..16].copy_from_slice(&(32u32 * 4).to_le_bytes()); // event 0 → right after the table
    class.extend(code);
    let names = vec![0u8; 4 + 13];
    Usecode::new(Flex::parse(flex(&[vec![], names, class])).unwrap())
}

fn run(code: &[u8], ticks: usize) -> Vm {
    let uc = usecode(code);
    let mut host = Stub { next: 300, ..Default::default() };
    host.items.insert(256, Item { x: 100, y: 100, ..Default::default() });
    let mut vm = Vm::new(&uc, None);
    vm.call_event(&uc, 0, 0, 256, &[]);
    for _ in 0..ticks {
        vm.tick(&uc, &mut host);
        while let Some(e) = vm.events.pop_front() {
            match e {
                UiEvent::Bark { pid, .. } | UiEvent::Read { pid, .. } => vm.finish(pid, 0),
                UiEvent::Ask { pid, options, .. } => vm.answer(pid, &options, 0),
                _ => {}
            }
        }
    }
    vm
}

#[test]
fn random_bytecode_never_panics() {
    let mut seed = 0x1234_5679_u32;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };
    let rounds = std::env::var("FUZZ_ROUNDS").ok().and_then(|r| r.parse().ok()).unwrap_or(300);
    let mut errs = 0;
    for _ in 0..rounds {
        let len = 1 + rnd() as usize % 300;
        // Bias towards real opcodes (0x00–0x7A) so programs get past the first instruction.
        let code: Vec<u8> = (0..len).map(|_| if rnd() % 4 == 0 { rnd() as u8 } else { (rnd() % 0x7B) as u8 }).collect();
        errs += run(&code, 8).errors;
    }
    assert!(errs > rounds / 2, "most random programs should fail cleanly, not silently do nothing");
}

#[test]
fn runaway_spawning_is_capped() {
    // loop: spawn inline (0x58) a copy of ourselves, then jump back.
    let code = [0x58, 0, 0, 0, 0, 0, 0, 0, 0, 0x52, 0xF4, 0xFF];
    let vm = run(&code, 3);
    assert!(vm.errors > 0, "the process cap should have refused spawns");
    assert!(vm.running() <= 1024);
}

#[test]
fn stack_overflow_is_an_error_not_a_panic() {
    // push.w 1 forever.
    let code = [0x0B, 1, 0, 0x52, 0xFA, 0xFF];
    let vm = run(&code, 2);
    assert_eq!(vm.running(), 0);
    assert!(vm.errors > 0);
}

/// Code starts at event 0's offset; an inline spawn names code at `off + delta` (+ BASE).
const START: usize = 32 * 4;

#[test]
fn inline_spawn_pushes_the_new_pid_and_implies_leaves_the_target() {
    // push pid (me); spawn inline → child pid; implies (me waits for the child, the child's pid
    // stays); pop it into globals 0–15; ret.   child: suspend; ret.
    let k = 1 + 9 + 3 + 4 + 1;
    let [lo, hi] = (START as u16).to_le_bytes();
    let code = [
        vec![0x59],
        vec![0x58, 0, 0, lo, hi, k as u8, 0, 0, 0],
        vec![0x54, 1, 1],
        vec![0x4F, 0, 0, 16],
        vec![0x50],
        vec![0x53, 0x50],
    ]
    .concat();
    let vm = run(&code, 3);
    assert_eq!(vm.errors, 0);
    assert_eq!(u16::from_le_bytes([vm.globals[0], vm.globals[1]]), 2, "the child's pid (the parent is 1)");
    assert_eq!(vm.running(), 0, "the parent woke when the child ended");
}

#[test]
fn byte_locals_push_sign_extended() {
    // init 2; push.b -2; pop.b bp-1; push.b bp-1 (→ 0xFFFE); pop into globals 0–15; ret.
    let code = [0x5A, 2, 0x0A, 0xFE, 0x00, 0xFF, 0x3E, 0xFF, 0x4F, 0, 0, 16, 0x50];
    let vm = run(&code, 1);
    assert_eq!(vm.errors, 0);
    assert_eq!(u16::from_le_bytes([vm.globals[0], vm.globals[1]]), 0xFFFE);
}

#[test]
fn slider_input_waits_and_returns_the_choice() {
    // push step 1, max 10, min 2, this; calli getSliderInput; push ret.w; pop into globals 0–15; ret.
    let code = [
        vec![0x0B, 1, 0, 0x0B, 10, 0, 0x0B, 2, 0],
        vec![0x0C, 0, 1, 0, 0x80],
        vec![0x0F, 10, 0x4B, 0x00],
        vec![0x5E, 0x4F, 0, 0, 16, 0x50],
    ]
    .concat();
    let uc = usecode(&code);
    let mut host = Stub { next: 300, ..Default::default() };
    host.items.insert(256, Item::default());
    let mut vm = Vm::new(&uc, None);
    vm.call_event(&uc, 0, 0, 256, &[]);
    vm.tick(&uc, &mut host);
    let Some(UiEvent::Slider { pid, min: 2, max: 10, step: 1 }) = vm.events.pop_front() else { panic!("asks") };
    for _ in 0..5 {
        vm.tick(&uc, &mut host);
    }
    assert_eq!(vm.running(), 2, "waiting for the answer");
    vm.finish(pid, 7);
    vm.tick(&uc, &mut host);
    assert_eq!((vm.errors, vm.running()), (0, 0));
    assert_eq!(u16::from_le_bytes([vm.globals[0], vm.globals[1]]), 7, "the choice, as the call's result");
}
