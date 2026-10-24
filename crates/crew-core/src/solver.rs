//! Perfect-information feasibility search.
//!
//! Depth-first over legal plays. A branch stops as soon as a task fails, and
//! succeeds as soon as every task is done. Positions between tricks that were
//! proven lost are cached by (remaining hands, leader, task signatures).

use crate::card::Card;
use crate::game::{Game, MAX_PLAYERS};
use crate::situation::{Owner, Situation};
use crate::task::{Kind, Status};
use std::collections::HashSet;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// A winning continuation: every card to play from the current position, in order.
    Feasible(Vec<Card>),
    Infeasible,
    /// A task's re-deal clause applies to this deal.
    Redeal(String),
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SolveError {
    Replay(String),
    /// Some task has owner `?`; use the draft search.
    UnknownOwner(String),
    /// The captain holds a task the captain may not take.
    CaptainNotAllowed(String),
}

impl fmt::Display for SolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SolveError::Replay(e) => write!(f, "{e}"),
            SolveError::UnknownOwner(id) => write!(f, "task '{id}' has no owner; run the draft search"),
            SolveError::CaptainNotAllowed(id) => write!(f, "the captain may not take '{id}'"),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Stats {
    pub nodes: u64,
    pub cache_hits: u64,
    pub cache_size: usize,
}

#[derive(Debug, Clone)]
pub struct Options<'a> {
    pub use_cache: bool,
    pub cancel: Option<&'a AtomicBool>,
}

impl Default for Options<'_> {
    fn default() -> Self {
        Options { use_cache: true, cancel: None }
    }
}

/// Tasks resolved to (kind, owner index); rule cards get owner 0 (ignored).
pub type Resolved = Vec<(&'static Kind, usize)>;

/// Check the situation and resolve owners.
pub fn prepare(s: &Situation) -> Result<(Game, Resolved), SolveError> {
    let g = s.game().map_err(|e| SolveError::Replay(e.to_string()))?;
    let mut tasks = Vec::new();
    for a in &s.tasks {
        let owner = match a.owner {
            Owner::Player(p) => p as usize,
            Owner::Crew => 0,
            Owner::Unknown => return Err(SolveError::UnknownOwner(a.card.id.clone())),
        };
        if a.owner == Owner::Player(g.captain) && !a.card.captain_allowed() {
            return Err(SolveError::CaptainNotAllowed(a.card.id.clone()));
        }
        tasks.push((&a.card.kind, owner));
    }
    Ok((g, tasks))
}

/// The first task whose re-deal clause applies, if any.
pub fn redeal(s: &Situation) -> Option<String> {
    s.tasks.iter().find(|a| a.card.redeal_triggered(&s.hands)).map(|a| a.card.id.clone())
}

pub fn solve(s: &Situation, opts: &Options) -> Result<(Verdict, Stats), SolveError> {
    let (g, tasks) = prepare(s)?;
    if let Some(id) = redeal(s) {
        return Ok((Verdict::Redeal(id), Stats::default()));
    }
    Ok(solve_game(g, &tasks, opts))
}

pub fn solve_game(mut g: Game, tasks: &Resolved, opts: &Options) -> (Verdict, Stats) {
    let mut search = Search { tasks, opts, failed: HashSet::new(), stats: Stats::default(), line: Vec::new() };
    let verdict = match search.dfs(&mut g) {
        None => Verdict::Cancelled,
        Some(true) => Verdict::Feasible(search.line),
        Some(false) => Verdict::Infeasible,
    };
    search.stats.cache_size = search.failed.len();
    (verdict, search.stats)
}

/// Combined status of all tasks (between tricks).
pub fn status(g: &Game, tasks: &Resolved) -> Status {
    let mut all_done = true;
    for &(k, o) in tasks {
        match k.eval(g, o) {
            Status::Failed => return Status::Failed,
            Status::Open => all_done = false,
            Status::Done => {}
        }
    }
    if all_done {
        Status::Done
    } else if g.is_over() {
        // Every task is exact at the end; reaching here means a task bug.
        debug_assert!(false, "open task at game end");
        Status::Failed
    } else {
        Status::Open
    }
}

#[derive(Hash, PartialEq, Eq)]
struct Key {
    hands: [u64; MAX_PLAYERS],
    leader: u8,
    sigs: Vec<u64>,
}

struct Search<'a, 'o> {
    tasks: &'a Resolved,
    opts: &'a Options<'o>,
    failed: HashSet<Key>,
    stats: Stats,
    line: Vec<Card>,
}

impl Search<'_, '_> {
    fn key(&self, g: &Game) -> Key {
        let mut hands = [0u64; MAX_PLAYERS];
        for (i, h) in g.hands[..g.n].iter().enumerate() {
            hands[i] = h.0;
        }
        Key { hands, leader: g.leader, sigs: self.tasks.iter().map(|&(k, o)| k.signature(g, o)).collect() }
    }

    /// Some(true) = winning line found (in `self.line`), Some(false) = lost, None = cancelled.
    fn dfs(&mut self, g: &mut Game) -> Option<bool> {
        self.stats.nodes += 1;
        if self.stats.nodes & 0xFFF == 0 {
            if let Some(c) = self.opts.cancel {
                if c.load(Ordering::Relaxed) {
                    return None;
                }
            }
        }
        let mut key = None;
        if g.current.is_empty() {
            match status(g, self.tasks) {
                Status::Failed => return Some(false),
                Status::Done => return Some(true),
                Status::Open => {}
            }
            if self.opts.use_cache {
                let k = self.key(g);
                if self.failed.contains(&k) {
                    self.stats.cache_hits += 1;
                    return Some(false);
                }
                key = Some(k);
            }
        }
        for card in g.legal_moves().iter() {
            g.play(card);
            self.line.push(card);
            let r = self.dfs(g);
            if r != Some(false) {
                if r.is_none() {
                    g.undo();
                }
                return r;
            }
            self.line.pop();
            g.undo();
        }
        if let Some(k) = key {
            self.failed.insert(k);
        }
        Some(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notation;

    fn situation(tasks: &str) -> Situation {
        let text = format!(
            "crew 1
players: 4
Player1: P2 P8 B4 B9 G1 G6 Y3 Y7 S2 S4
Player2: P1 P5 B1 B2 B6 G4 G9 Y2 Y6 S1
Player3: P3 P6 B3 B8 G2 G5 G8 Y1 Y8 S3
Player4: rest
tasks:
{tasks}"
        );
        notation::parse(&text).unwrap()
    }

    /// Replay a winning line and check every task ends done.
    fn check_line(s: &Situation, line: &[Card]) {
        let (mut g, tasks) = prepare(s).unwrap();
        for &c in line {
            assert!(g.legal_moves().contains(c), "illegal {c} in line");
            g.play(c);
        }
        assert!(g.current.is_empty());
        assert_eq!(status(&g, &tasks), Status::Done);
    }

    #[test]
    fn simple_feasible_task() {
        let s = situation("Player2: win B4");
        let (v, _) = solve(&s, &Options::default()).unwrap();
        match v {
            Verdict::Feasible(line) => check_line(&s, &line),
            v => panic!("expected feasible, got {v:?}"),
        }
    }

    #[test]
    fn impossible_task_is_proven_infeasible() {
        // Player2 holds only S1; S3 (Player3) can only lose to S4 (Player1).
        let s = situation("Player2: win S3");
        let (v, stats) = solve(&s, &Options::default()).unwrap();
        assert_eq!(v, Verdict::Infeasible, "{stats:?}");
    }

    #[test]
    fn several_tasks_and_a_rule_card() {
        let s = situation("Player2: win B4\nPlayer1: first-trick\nPlayer4: win P9 Y8\ncrew: rule M12 no-lead pink sub");
        let (v, stats) = solve(&s, &Options::default()).unwrap();
        match v {
            Verdict::Feasible(line) => check_line(&s, &line),
            v => panic!("expected feasible, got {v:?} {stats:?}"),
        }
    }

    #[test]
    fn captain_and_unknown_owner_are_rejected() {
        let s = situation("Player1: more than captain");
        assert!(matches!(solve(&s, &Options::default()), Err(SolveError::CaptainNotAllowed(_))));
        let s = situation("?: win B4");
        assert!(matches!(solve(&s, &Options::default()), Err(SolveError::UnknownOwner(_))));
    }

    #[test]
    fn redeal_clause() {
        // Player1 holds S2 and S4 -> "only S2" triggers a re-deal.
        let s = situation("Player3: only S2");
        assert_eq!(solve(&s, &Options::default()).unwrap().0, Verdict::Redeal("only S2".into()));
    }
}
