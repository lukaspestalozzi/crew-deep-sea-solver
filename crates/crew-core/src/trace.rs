//! What the solver is doing, as a stream of events, for inspection: the
//! web app's work lists, `crew --trace`, later a bot's log (which adds its
//! own decision events to the same stream). Units of work nest: a deal holds
//! a draft count, which holds searches. Times are added by the receiver
//! (the core has no clock in WebAssembly).

use crate::card::Card;
use crate::situation::Situation;
use crate::solver::Stats;
use std::sync::atomic::{AtomicU64, Ordering};

pub enum Event<'a> {
    /// A unit starts: one search, or a group of searches (a deal, a draft
    /// count). `situation` is what it works on (owners `?` where open).
    Start { id: u64, parent: Option<u64>, label: &'a str, situation: &'a Situation },
    /// A search, about every million positions and at each restart.
    Progress { id: u64, nodes: u64, restart: u32, budget: u64, cache: usize },
    /// The unit ends: "feasible", "infeasible", "undecided", "cancelled",
    /// "redeal", or a group's summary. Searches give their statistics.
    End { id: u64, result: &'a str, stats: Option<&'a Stats> },
    /// A move chosen by a move order (a bot): the position before it, the
    /// legal moves best first, the move played, and what the solver says
    /// about the position after it ("feasible", "infeasible", "undecided",
    /// or "" if not checked).
    Decision { id: u64, parent: Option<u64>, label: &'a str, situation: &'a Situation, ranked: &'a [Card], chosen: Card, after: &'a str },
}

pub type Sink<'a> = dyn Fn(&Event) + 'a;

/// Where events go, and the unit that new units belong to.
#[derive(Clone, Copy)]
pub struct Trace<'a> {
    pub sink: &'a Sink<'a>,
    pub parent: Option<u64>,
    /// Label for the next search started with this trace.
    pub label: &'a str,
    /// The search running (set by the solver, for its progress events).
    pub current: Option<u64>,
}

static NEXT: AtomicU64 = AtomicU64::new(1);

impl<'a> Trace<'a> {
    pub fn new(sink: &'a Sink<'a>) -> Self {
        Trace { sink, parent: None, label: "search", current: None }
    }

    /// Start a unit; returns its id.
    pub fn start(&self, label: &str, situation: &Situation) -> u64 {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        (self.sink)(&Event::Start { id, parent: self.parent, label, situation });
        id
    }

    pub fn end(&self, id: u64, result: &str, stats: Option<&Stats>) {
        (self.sink)(&Event::End { id, result, stats });
    }

    pub fn decision(&self, label: &str, situation: &Situation, ranked: &[Card], chosen: Card, after: &str) {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        (self.sink)(&Event::Decision { id, parent: self.parent, label, situation, ranked, chosen, after });
    }

    /// The same sink for units inside `id`, labelled `label`.
    pub fn child(&self, id: u64, label: &'a str) -> Trace<'a> {
        Trace { sink: self.sink, parent: Some(id), label, current: None }
    }

    /// The same parent, another label for the next search.
    pub fn labelled(&self, label: &'a str) -> Trace<'a> {
        Trace { label, ..*self }
    }
}

impl Event<'_> {
    /// One line of JSON (the format of `crew --trace` and the web app).
    pub fn to_json(&self) -> String {
        match self {
            Event::Start { id, parent, label, situation } => format!(
                r#"{{"ev":"start","id":{id},"parent":{},"label":{},"text":{}}}"#,
                parent.map_or("null".to_string(), |p| p.to_string()),
                json_str(label),
                json_str(&crate::notation::write(situation)),
            ),
            Event::Progress { id, nodes, restart, budget, cache } => {
                format!(r#"{{"ev":"progress","id":{id},"nodes":{nodes},"restart":{restart},"budget":{budget},"cache":{cache}}}"#)
            }
            Event::End { id, result, stats } => {
                let stats = stats.map_or("null".to_string(), |s| {
                    format!(
                        r#"{{"nodes":{},"cache_hits":{},"cache_size":{},"cut_by":{:?},"cut_at_trick":{:?},"failed_task":{:?}}}"#,
                        s.nodes, s.cache_hits, s.cache_size, s.cut_by, s.cut_at_trick, s.failed_task
                    )
                });
                format!(r#"{{"ev":"end","id":{id},"result":{},"stats":{stats}}}"#, json_str(result))
            }
            Event::Decision { id, parent, label, situation, ranked, chosen, after } => format!(
                r#"{{"ev":"decision","id":{id},"parent":{},"label":{},"text":{},"ranked":[{}],"chosen":"{chosen}","after":{}}}"#,
                parent.map_or("null".to_string(), |p| p.to_string()),
                json_str(label),
                json_str(&crate::notation::write(situation)),
                ranked.iter().map(|c| format!("\"{c}\"")).collect::<Vec<_>>().join(","),
                json_str(after),
            ),
        }
    }
}

fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_escapes_text() {
        assert_eq!(json_str("a\"b\\c\nd"), r#""a\"b\\c\nd""#);
        let e = Event::Progress { id: 3, nodes: 10, restart: 1, budget: 100, cache: 5 };
        assert_eq!(e.to_json(), r#"{"ev":"progress","id":3,"nodes":10,"restart":1,"budget":100,"cache":5}"#);
    }
}
