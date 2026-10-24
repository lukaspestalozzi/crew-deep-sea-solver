//! Perfect-information feasibility search.
//!
//! Depth-first over legal plays. A branch stops as soon as a task fails, and
//! succeeds as soon as every task is done. Positions between tricks that were
//! proven lost are cached by (remaining hands, leader, task signatures).

use crate::card::{Card, CardSet, Suit};
use crate::game::Game;
use crate::situation::{Owner, Situation};
use crate::task::{counts_possible, merge, top_subs_count, Boundaries, CountRule, Kind, Status, NO_BOUNDARIES};
use std::collections::{HashMap, HashSet};
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

#[derive(Clone)]
pub struct Options<'a> {
    /// Cache positions proven lost.
    pub use_cache: bool,
    /// Treat interchangeable cards as one move.
    pub merge_cards: bool,
    /// Check all trick-count rules jointly at each trick boundary.
    pub count_check: bool,
    pub cancel: Option<&'a AtomicBool>,
    /// Called with the node count about every million nodes.
    pub progress: Option<&'a dyn Fn(u64)>,
}

impl Default for Options<'_> {
    fn default() -> Self {
        Options { use_cache: true, merge_cards: true, count_check: true, cancel: None, progress: None }
    }
}

impl Options<'_> {
    /// No cache, no merging, no joint count check: the reference search.
    pub fn plain() -> Self {
        Options { use_cache: false, merge_cards: false, count_check: false, cancel: None, progress: None }
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
    let boundaries = if opts.merge_cards {
        tasks.iter().fold(NO_BOUNDARIES, |acc, (k, _)| merge(acc, k.boundaries()))
    } else {
        [0x1FF; 5]
    };
    let mut search = Search {
        tasks,
        opts,
        boundaries,
        statuses: Vec::with_capacity(tasks.len()),
        // Rule M23 only yields its rule after trick 1, so probe a finished game shape.
        has_count_rules: opts.count_check
            && tasks.iter().any(|&(k, o)| k.count_rule(&g, o).is_some() || matches!(k, Kind::FirstWinnerAlwaysLeads)),
        count_memo: HashMap::new(),
        failed: HashSet::new(),
        stats: Stats::default(),
        line: Vec::new(),
    };
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
    status_into(g, tasks, &mut Vec::new())
}

/// As [`status`], also recording each task's status (stops at the first failure).
fn status_into(g: &Game, tasks: &Resolved, out: &mut Vec<Status>) -> Status {
    out.clear();
    let mut all_done = true;
    for &(k, o) in tasks {
        let st = k.eval(g, o);
        out.push(st);
        match st {
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

/// Two independent 64-bit multiply-rotate hashes; a collision of both is
/// negligible (about 2^-128 per pair).
fn key_hash(words: impl Iterator<Item = u64>) -> u128 {
    let (mut a, mut b) = (0x243F_6A88_85A3_08D3u64, 0x1319_8A2E_0370_7344u64);
    for w in words {
        a = (a.rotate_left(5) ^ w).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        b = (b.rotate_left(7) ^ w).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    }
    (a as u128) << 64 | b as u128
}

struct Search<'a, 'o> {
    tasks: &'a Resolved,
    opts: &'a Options<'o>,
    /// Where tasks tell cards of a suit apart.
    boundaries: Boundaries,
    statuses: Vec<Status>,
    has_count_rules: bool,
    count_memo: HashMap<u64, bool>,
    failed: HashSet<u128>,
    stats: Stats,
    line: Vec<Card>,
}

impl Search<'_, '_> {
    /// Joint check of all trick-count rules, memoised by current counts,
    /// guaranteed submarine wins and the trick-1 winner.
    fn counts_possible(&mut self, g: &Game) -> bool {
        let mut key: u64 = g.tricks.first().map_or(7, |t| t.winner as u64);
        for p in 0..g.n {
            key = key << 8 | (g.trick_counts[p] as u64) << 3 | top_subs_count(g, p);
        }
        key = key << 4 | g.tricks_left() as u64;
        if let Some(&ok) = self.count_memo.get(&key) {
            return ok;
        }
        let rules: Vec<CountRule> = self.tasks.iter().filter_map(|&(k, o)| k.count_rule(g, o)).collect();
        let ok = counts_possible(g, &rules);
        self.count_memo.insert(key, ok);
        ok
    }

    fn boundary_above(&self, suit: Suit, v: u8) -> bool {
        v < suit.max_value() && self.boundaries[suit as usize] & (1 << (v - 1)) != 0
    }

    /// Position key between tricks. Cards are encoded by relative rank: per
    /// suit, the owners of the remaining cards from high to low, each with
    /// its class (how many task boundaries lie above it). Positions that
    /// differ only in which interchangeable cards are gone play out
    /// identically, so they share a key. Needs `self.statuses` to be current.
    fn key(&self, g: &Game) -> u128 {
        let mut words = [0u64; 32];
        let mut len = 0;
        let mut push = |w: u64| {
            words[len] = w;
            len += 1;
        };
        let mut owner = [0u8; 40];
        for p in 0..g.n {
            for c in g.hands[p].iter() {
                owner[c.index() as usize] = p as u8 + 1;
            }
        }
        for suit in Suit::ALL {
            // At most 9 cards x 7 bits (3 owner + 4 class) after a leading 1.
            let mut w: u64 = 1;
            let mut class = 0u64;
            for v in (1..=suit.max_value()).rev() {
                if self.boundary_above(suit, v) {
                    class += 1;
                }
                let o = owner[Card::new(suit, v).index() as usize] as u64;
                if o != 0 {
                    w = w << 7 | o << 4 | class;
                }
            }
            push(w);
        }
        push(g.leader as u64);
        for (i, &(k, o)) in self.tasks.iter().enumerate() {
            push(k.signature(g, o, self.statuses[i]));
        }
        key_hash(words[..len].iter().copied())
    }

    /// Legal moves with interchangeable cards collapsed to one representative.
    /// Two cards of one suit in the same hand are interchangeable when no task
    /// boundary lies between them and no card between them is held by another
    /// player or lies in the current trick (cards already played are transparent).
    fn moves(&self, g: &Game) -> CardSet {
        let legal = g.legal_moves();
        let hand = g.hands[g.to_play()];
        let in_trick: CardSet = g.current.iter().copied().collect();
        let separators = g.unplayed().minus(hand).union(in_trick);
        let mut out = CardSet::EMPTY;
        for suit in Suit::ALL {
            if legal.intersect(suit.cards()).is_empty() {
                continue;
            }
            let mut in_run = false;
            for v in (1..=suit.max_value()).rev() {
                if self.boundary_above(suit, v) {
                    in_run = false;
                }
                let c = Card::new(suit, v);
                if legal.contains(c) {
                    if !in_run {
                        out.insert(c);
                        in_run = true;
                    }
                } else if separators.contains(c) {
                    in_run = false;
                }
            }
        }
        out
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
            if self.stats.nodes & 0xF_FFFF == 0 {
                if let Some(p) = self.opts.progress {
                    p(self.stats.nodes);
                }
            }
        }
        let mut key = None;
        if g.current.is_empty() {
            let mut statuses = std::mem::take(&mut self.statuses);
            let st = status_into(g, self.tasks, &mut statuses);
            self.statuses = statuses;
            match st {
                Status::Failed => return Some(false),
                Status::Done => return Some(true),
                Status::Open => {}
            }
            if self.has_count_rules && !self.counts_possible(g) {
                return Some(false);
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
        for card in self.moves(g).iter() {
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
        let s = situation("Player2: win B4\nPlayer1: first-trick\nPlayer4: win P9 Y8\ncrew: rule M23 first winner leads");
        let (v, stats) = solve(&s, &Options::default()).unwrap();
        match v {
            Verdict::Feasible(line) => check_line(&s, &line),
            v => panic!("expected feasible, got {v:?} {stats:?}"),
        }
    }

    #[test]
    fn rule_card_blocking_pink_leads_makes_p9_unwinnable() {
        // Nobody may lead pink, so P9 is only ever discarded and never wins.
        let s = situation("Player4: win P9 Y8\ncrew: rule M12 no-lead pink sub");
        let (v, stats) = solve(&s, &Options::default()).unwrap();
        assert_eq!(v, Verdict::Infeasible, "{stats:?}");
    }

    #[test]
    fn captain_and_unknown_owner_are_rejected() {
        let s = situation("Player1: more than captain");
        assert!(matches!(solve(&s, &Options::default()), Err(SolveError::CaptainNotAllowed(_))));
        let s = situation("?: win B4");
        assert!(matches!(solve(&s, &Options::default()), Err(SolveError::UnknownOwner(_))));
    }

    /// The optimised search (cache + merged cards) must agree with a plain
    /// search on random late-game positions with random tasks.
    #[test]
    fn optimised_search_agrees_with_plain_search() {
        use crate::catalogue::catalogue;
        use crate::rng::Rng;
        let mut rng = Rng::new(42);
        let cat = catalogue();
        let (mut feasible, mut infeasible) = (0, 0);
        for case in 0..400 {
            let n = 3 + case % 3;
            let mut g = Game::new(&Game::random_hands(n, &mut rng)).unwrap();
            let left = if n == 5 { 3 } else { 4 };
            while g.tricks_left() > left || !g.current.is_empty() {
                let moves: Vec<Card> = g.legal_moves().iter().collect();
                g.play(moves[rng.below(moves.len())]);
            }
            // Every third case draws only trick-count tasks, to exercise the joint count check.
            let pool: Vec<&crate::catalogue::TaskCard> = if case % 3 == 0 {
                cat.iter().filter(|t| t.kind.count_rule(&g, 0).is_some()).collect()
            } else {
                cat.iter().collect()
            };
            let k = 1 + rng.below(3);
            let tasks: Resolved = (0..k)
                .map(|_| {
                    let t = pool[rng.below(pool.len())];
                    let mut o = rng.below(n);
                    if !t.captain_allowed() && o == g.captain as usize {
                        o = (o + 1) % n;
                    }
                    (&t.kind, o)
                })
                .collect();
            let fast = solve_game(g.clone(), &tasks, &Options::default()).0;
            let plain = solve_game(g.clone(), &tasks, &Options::plain()).0;
            let ids: Vec<String> = tasks.iter().map(|(k, o)| format!("{k:?}@{o}")).collect();
            match (&fast, &plain) {
                (Verdict::Feasible(line), Verdict::Feasible(_)) => {
                    feasible += 1;
                    let mut h = g.clone();
                    for &c in line {
                        assert!(h.legal_moves().contains(c));
                        h.play(c);
                    }
                    assert_eq!(status(&h, &tasks), Status::Done, "case {case}: {ids:?}");
                }
                (Verdict::Infeasible, Verdict::Infeasible) => infeasible += 1,
                _ => panic!("case {case} ({n} players): fast {fast:?} vs plain {plain:?}; tasks {ids:?}"),
            }
        }
        // Make sure both outcomes were exercised.
        assert!(feasible > 50 && infeasible > 50, "feasible {feasible}, infeasible {infeasible}");
    }

    #[test]
    fn redeal_clause() {
        // Player1 holds S2 and S4 -> "only S2" triggers a re-deal.
        let s = situation("Player3: only S2");
        assert_eq!(solve(&s, &Options::default()).unwrap().0, Verdict::Redeal("only S2".into()));
    }
}
