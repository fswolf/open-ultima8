//! The driver: plays the engine headless a tick at a time, answering its questions and showing
//! its speech as a player would, and noting what was said.
use super::*;

#[derive(Default)]
pub(super) struct Drv {
    pub(super) answer: Option<usize>,
    pub(super) queue: std::collections::VecDeque<usize>,
    pub(super) targets: std::collections::VecDeque<u32>,
    pub(super) silent: bool,
    pub(super) bark_ticks: Option<u32>,
    pub(super) log: bool,
    pub(super) t: u32,
    pub(super) barks: Vec<(u32, u32)>,
    pub(super) asks: u32,
    pub(super) said: u32,
    pub(super) open_ask: Option<(u32, u32)>,
    pub(super) cycle: Option<usize>,
    pub(super) cap: u32,
    pub(super) script: Vec<String>,
    pub(super) keep: bool,
    pub(super) stamp: bool,
}

impl Drv {
    pub(super) fn run(&mut self, g: &mut Game, n: u32, input: &mut Input) {
        for _ in 0..n {
            let st = g.update(1.0 / 30.0, input);
            (input.jump, input.combat, input.attack, input.kick) = (false, false, false, false);
            self.t += 1;
            let t = self.t;
            if st.map_changed && self.log {
                println!("[t{t}] now on map {}", g.map());
            }
            for e in g.take_events() {
                match e {
                    UiEvent::Bark { pid, item, text } => {
                        self.said += 1;
                        if self.keep {
                            let at = if self.stamp { format!("[{:.1}s] ", t as f32 / 30.0) } else { String::new() };
                            self.script.push(format!("  {at}{item}: {}", text.trim()));
                        }
                        if self.log {
                            println!("[t{t}] {item} says: {}", text.trim());
                        }
                        let ticks = self.bark_ticks.unwrap_or(60 + 2 * text.len() as u32);
                        self.barks.push((pid as u32, ticks));
                    }
                    UiEvent::Ask { pid, options, item } => {
                        self.asks += 1;
                        if self.silent {
                            if self.log {
                                println!("[t{t}] {item} asks {options:?} -> (no answer, waits 924 ticks)");
                            }
                            self.open_ask = Some((pid as u32, 924));
                            if self.keep {
                                let at = if self.stamp { format!("[{:.1}s] ", t as f32 / 30.0) } else { String::new() };
                                self.script.push(format!("  {at}? {options:?} -> (silence)"));
                            }
                            continue;
                        }
                        let last = options.len().saturating_sub(1);
                        let mut pick = self.queue.pop_front().or(self.answer).unwrap_or(last).min(last);
                        if let Some(c) = self.cycle.as_mut() {
                            if self.asks <= self.cap {
                                pick = *c % options.len().max(1);
                                *c += 1;
                            } else {
                                pick = last;
                            }
                        }
                        if self.keep {
                            self.script
                                .push(format!("  ? {options:?} -> {}", options.get(pick).cloned().unwrap_or_default()));
                        }
                        if self.log {
                            println!("[t{t}] {item} asks {options:?} -> {pick}");
                        }
                        g.vm().unwrap().answer(pid, &options, pick);
                    }
                    UiEvent::Read { pid, text, .. } => {
                        if self.log {
                            println!("[t{t}] read: {}", text.trim().replace('\n', " / "));
                        }
                        g.vm().unwrap().finish(pid, 0);
                    }
                    UiEvent::Target { pid } => {
                        let pick = self.targets.pop_front().unwrap_or(0);
                        if self.log {
                            println!("[t{t}] target asked -> {pick}");
                        }
                        g.vm().unwrap().finish(pid, pick)
                    }
                    UiEvent::Slider { pid, min, .. } => g.vm().unwrap().finish(pid, min as u16 as u32),
                    UiEvent::Music(n) if self.log => println!("[t{t}] music {n}"),
                    UiEvent::OpenGump { item, gump } if self.log => println!("[t{t}] open gump {gump} for {item}"),
                    UiEvent::CloseGump { item } if self.log => println!("[t{t}] close gump for {item}"),
                    UiEvent::Endgame if self.log => println!("[t{t}] endgame"),
                    _ => {}
                }
            }
            if let Some((pid, left)) = self.open_ask {
                if left <= 1 {
                    g.vm().unwrap().answer_nothing(pid as _);
                    self.open_ask = None;
                    if self.log {
                        println!("[t{t}] question timed out");
                    }
                } else {
                    self.open_ask = Some((pid, left - 1));
                }
            }
            let mut done = vec![];
            for b in self.barks.iter_mut() {
                b.1 = b.1.saturating_sub(1);
                if b.1 == 0 {
                    done.push(b.0);
                }
            }
            self.barks.retain(|b| b.1 > 0);
            for pid in done {
                g.vm().unwrap().finish(pid as _, 0);
            }
        }
    }
}
