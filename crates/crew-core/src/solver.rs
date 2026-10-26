//! Perfect-information feasibility search.
//!
//! The search runs over plays. Between tricks every task is evaluated for
//! every player who may still own it (a *pair*). With fixed owners each task
//! has one pair. With unknown owners (`?`) one search covers all legal drafts:
//! a branch is lost once no complete draft survives, and won once some draft
//! has all its tasks done. Positions proven lost are cached by (remaining
//! hands, leader, surviving pairs and their task signatures).
//!
//! Two parts are pluggable through [`Options`]: the [`MoveDecider`] that
//! orders the moves, and the [`Strategy`] (depth-first or best-first).

use crate::card::{Card, CardSet, Suit};
use crate::decider::{MoveDecider, Position, DECK_ORDER, LOOKAHEAD, TASK_AWARE_RANDOM};
use crate::game::{Game, MAX_PLAYERS};
use crate::situation::{Owner, Situation};
use crate::task::{any_final_counts, merge, req_bounds, top_subs_count, Boundaries, CountRule, Kind, Req, Status, NO_BOUNDARIES};
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::fmt;
use std::hash::{BuildHasherDefault, Hasher};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// A winning continuation: every card to play from the current position, in order.
    Feasible(Vec<Card>),
    Infeasible,
    /// A task's re-deal clause applies to this deal.
    Redeal(String),
    /// Stopped through the cancel flag.
    Cancelled,
    /// Stopped at the node limit without an answer.
    Undecided,
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
    /// Branches cut between tricks, by the number of tricks played then.
    pub cut_at_trick: [u64; 14],
    /// Branches cut by: a task's own evaluation, the combined count/window
    /// check, the card destiny check, the cache of lost positions.
    pub cut_by: [u64; 4],
    /// How often each task (by index) was the one found failed.
    pub failed_task: Vec<u64>,
}

/// How the search space is explored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    /// Depth-first in the decider's move order. Little memory; needed to
    /// prove infeasibility efficiently.
    Dfs,
    /// Best-first over positions between tricks, most tasks done and deepest
    /// first. Keeps every open position in memory; falls back to depth-first
    /// once more than `max_positions` positions were generated.
    BestFirst { max_positions: usize },
    /// Depth-first restarts that cycle through the options' decider, the
    /// deck order and the task-aware order with random tie-breaks (a new
    /// salt each round), with node budgets doubling from `first_budget`. The
    /// cache of lost positions is kept across restarts (it does not depend on
    /// the move order). Search times vary a lot with the move order; this
    /// costs a small multiple of the best of them.
    Portfolio { first_budget: u64 },
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
    /// Orders the moves at every node.
    pub decider: &'a dyn MoveDecider,
    pub strategy: Strategy,
    /// At most this many lost positions are kept; the cache is emptied when full.
    pub cache_limit: usize,
    /// Stop with [`Verdict::Undecided`] after this many nodes.
    pub node_limit: Option<u64>,
    /// Hash the hands incrementally (Zobrist: card and holder) instead of by
    /// relative rank at each trick. Cheaper per position, but positions that
    /// differ only in which interchangeable cards are gone no longer share a key.
    pub zobrist: bool,
    /// Report searches as [`crate::trace::Event`]s.
    pub trace: Option<crate::trace::Trace<'a>>,
}

impl Default for Options<'_> {
    fn default() -> Self {
        Options {
            use_cache: true,
            merge_cards: true,
            count_check: true,
            cancel: None,
            progress: None,
            decider: &LOOKAHEAD,
            strategy: Strategy::Portfolio { first_budget: 100_000 },
            cache_limit: 4_000_000,
            node_limit: None,
            zobrist: false,
            trace: None,
        }
    }
}

impl<'a> Options<'a> {
    /// The same options; the next traced search is labelled `label`.
    pub fn labelled<'b>(&self, label: &'b str) -> Options<'b>
    where
        'a: 'b,
    {
        Options { trace: self.trace.map(|t| t.labelled(label)), ..self.clone() }
    }

    /// No cache, no merging, no joint count check, deck order, plain
    /// depth-first: the reference search.
    pub fn plain() -> Self {
        Options {
            use_cache: false,
            merge_cards: false,
            count_check: false,
            decider: &DECK_ORDER,
            strategy: Strategy::Dfs,
            ..Options::default()
        }
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

/// Feasibility with every owner fixed.
pub fn solve(s: &Situation, opts: &Options) -> Result<(Verdict, Stats), SolveError> {
    let (g, tasks) = prepare(s)?;
    if let Some(id) = redeal(s) {
        return Ok((Verdict::Redeal(id), Stats::default()));
    }
    let (v, _, stats) = traced(s, opts, |o| run(g, Model::fixed(&tasks), o));
    Ok((v, stats))
}

/// Short name of a verdict, as in trace events.
pub fn verdict_name(v: &Verdict) -> &'static str {
    match v {
        Verdict::Feasible(_) => "feasible",
        Verdict::Infeasible => "infeasible",
        Verdict::Redeal(_) => "redeal",
        Verdict::Undecided => "undecided",
        Verdict::Cancelled => "cancelled",
    }
}

/// Run a search on `s`, reported as one unit if the options trace.
fn traced<T>(s: &Situation, opts: &Options, f: impl FnOnce(&Options) -> (Verdict, T, Stats)) -> (Verdict, T, Stats) {
    let Some(t) = opts.trace else { return f(opts) };
    let id = t.start(t.label, s);
    let inner = Options { trace: Some(crate::trace::Trace { current: Some(id), ..t }), ..opts.clone() };
    let r = f(&inner);
    t.end(id, verdict_name(&r.0), Some(&r.2));
    r
}

pub fn solve_game(g: Game, tasks: &Resolved, opts: &Options) -> (Verdict, Stats) {
    let (v, _, stats) = run(g, Model::fixed(tasks), opts);
    (v, stats)
}

/// Feasibility over every legal draft of the `?` tasks, in one search. On
/// success also returns the owners of a working draft (one per task).
pub fn solve_drafts(s: &Situation, opts: &Options) -> Result<(Verdict, Option<Vec<Owner>>, Stats), SolveError> {
    let g = s.game().map_err(|e| SolveError::Replay(e.to_string()))?;
    for a in &s.tasks {
        if a.owner == Owner::Player(g.captain) && !a.card.captain_allowed() {
            return Err(SolveError::CaptainNotAllowed(a.card.id.clone()));
        }
    }
    if let Some(id) = redeal(s) {
        return Ok((Verdict::Redeal(id), None, Stats::default()));
    }
    let model = Model::drafts(s, &g);
    Ok(traced(s, opts, |o| run(g, model, o)))
}

/// Combined status of all tasks (between tricks), owners fixed.
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
        debug_assert!(false, "open task at game end");
        Status::Failed
    } else {
        Status::Open
    }
}

fn run(mut g: Game, model: Model, opts: &Options) -> (Verdict, Option<Vec<Owner>>, Stats) {
    let mut search = Search::new(&g, model, opts);
    let result = search.execute(&mut g);
    let verdict = match result {
        Some(true) => Verdict::Feasible(std::mem::take(&mut search.line)),
        Some(false) => Verdict::Infeasible,
        None => search.stopped.clone().unwrap_or(Verdict::Cancelled),
    };
    let owners = matches!(verdict, Verdict::Feasible(_)).then(|| search.model.owners_of(search.won));
    search.stats.cache_size = search.failed.len();
    (verdict, owners, search.stats)
}

/// What deciding one draft found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Feasible, with a winning line.
    Works(Vec<Card>),
    /// Infeasible.
    Fails,
    /// Not decided (cancelled before).
    Open,
}

/// The result of deciding a list of drafts, one [`Decision`] per draft.
pub type DraftDecisions = Vec<Decision>;

/// Decide every draft in `drafts` (one owner per task, as in
/// [`Situation::tasks`]); exact. Rounds with a node budget that doubles each
/// round: every open draft is first checked against the winning lines found so
/// far (a line is legal whatever the owners), then searched alone; after each
/// round one search over all open drafts together may prove them all
/// infeasible at once (or settle those its line completes). `progress` gets
/// (feasible so far, still open); `decided` gets (draft index, decision) as
/// each draft is settled. If cancelled, or once the budget would exceed
/// `opts.node_limit`, the drafts decided so far keep their answer and the
/// rest are [`Decision::Open`].
pub fn decide_drafts(
    s: &Situation,
    drafts: &[Vec<Owner>],
    opts: &Options,
    progress: Option<&dyn Fn(usize, usize)>,
    decided: Option<&dyn Fn(usize, &Decision)>,
) -> Result<DraftDecisions, SolveError> {
    let Some(t) = opts.trace else { return decide_drafts_in(s, drafts, opts, progress, decided) };
    // One unit, with each search inside it.
    let unit = t.start(t.label, s);
    let inner = Options { trace: Some(t.child(unit, "search")), ..opts.clone() };
    let r = decide_drafts_in(s, drafts, &inner, progress, decided);
    let summary = match &r {
        Ok(d) => {
            let works = d.iter().filter(|x| matches!(x, Decision::Works(_))).count();
            let open = d.iter().filter(|x| **x == Decision::Open).count();
            format!("{works} of {} work, {open} open", d.len())
        }
        Err(e) => format!("error: {e}"),
    };
    t.end(unit, &summary, None);
    r
}

fn decide_drafts_in(
    s: &Situation,
    drafts: &[Vec<Owner>],
    opts: &Options,
    progress: Option<&dyn Fn(usize, usize)>,
    decided: Option<&dyn Fn(usize, &Decision)>,
) -> Result<DraftDecisions, SolveError> {
    let g = s.game().map_err(|e| SolveError::Replay(e.to_string()))?;
    if redeal(s).is_some() {
        return Ok(vec![Decision::Fails; drafts.len()]);
    }
    let with_owners = |d: &[Owner]| {
        let mut x = s.clone();
        for (a, o) in x.tasks.iter_mut().zip(d) {
            a.owner = *o;
        }
        x
    };
    // Does this line, played from the situation, complete the draft's tasks?
    let completes = |d: &[Owner], line: &[Card]| -> bool {
        let Ok((mut h, tasks)) = prepare(&with_owners(d)) else { return false };
        for &c in line {
            if !h.legal_moves().contains(c) {
                return false;
            }
            h.play(c);
        }
        h.current.is_empty() && status(&h, &tasks) == Status::Done
    };
    let as_index = |d: &Vec<Owner>| -> Vec<u8> { d.iter().map(|o| if let Owner::Player(p) = o { *p } else { 0 }).collect() };
    let cancelled = || opts.cancel.is_some_and(|c| c.load(Ordering::Relaxed));
    let mut out = vec![Decision::Open; drafts.len()];
    let settle = |out: &mut DraftDecisions, i: usize, d: Decision| {
        if let Some(f) = decided {
            f(i, &d);
        }
        out[i] = d;
    };
    let mut lines: Vec<Vec<Card>> = Vec::new();
    let mut open: Vec<usize> = (0..drafts.len()).collect();
    let mut found = 0;
    let cap = opts.node_limit.unwrap_or(u64::MAX);
    let mut budget: u64 = 100_000.min(cap);
    while !open.is_empty() {
        // Each open draft alone.
        let mut still = Vec::new();
        for (k, &i) in open.iter().enumerate() {
            if let Some(f) = progress {
                f(found, open.len() - k + still.len());
            }
            if let Some(line) = lines.iter().find(|l| completes(&drafts[i], l)) {
                settle(&mut out, i, Decision::Works(line.clone()));
                found += 1;
                continue;
            }
            let label = format!("draft {}/{} · budget {}", i + 1, drafts.len(), budget);
            let o = Options { node_limit: Some(budget), trace: opts.trace.map(|t| t.labelled(&label)), ..opts.clone() };
            match solve(&with_owners(&drafts[i]), &o)?.0 {
                Verdict::Feasible(line) => {
                    settle(&mut out, i, Decision::Works(line.clone()));
                    found += 1;
                    lines.push(line);
                }
                Verdict::Infeasible | Verdict::Redeal(_) => settle(&mut out, i, Decision::Fails),
                Verdict::Undecided => still.push(i),
                Verdict::Cancelled => return Ok(out),
            }
            if cancelled() {
                return Ok(out);
            }
        }
        open = still;
        if open.is_empty() {
            break;
        }
        // All open drafts together.
        let mut model = Model::drafts(s, &g);
        model.allowed = Some(open.iter().map(|&i| as_index(&drafts[i])).collect());
        let label = format!("all {} open drafts together · budget {}", open.len(), budget);
        let o = Options { node_limit: Some(budget), trace: opts.trace.map(|t| t.labelled(&label)), ..opts.clone() };
        match traced(s, &o, |o| run(g.clone(), model, o)).0 {
            Verdict::Infeasible => {
                for &i in &open {
                    settle(&mut out, i, Decision::Fails);
                }
                open.clear();
            }
            Verdict::Feasible(line) => {
                open.retain(|&i| {
                    let done = completes(&drafts[i], &line);
                    if done {
                        settle(&mut out, i, Decision::Works(line.clone()));
                        found += 1;
                    }
                    !done
                });
                lines.push(line);
            }
            Verdict::Cancelled => return Ok(out),
            _ => {}
        }
        if budget >= cap {
            break;
        }
        budget = budget.saturating_mul(2).min(cap);
    }
    Ok(out)
}

// ---------------------------------------------------------------- model: tasks and possible owners

const fn bit(t: usize, p: usize) -> u128 {
    1u128 << (5 * t + p)
}

fn bits_of(mask: u128, t: usize) -> u8 {
    (mask >> (5 * t)) as u8 & 0x1F
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    /// Rule card: applies to the crew (evaluated as player 0).
    Global,
    /// Owner fixed (one pair).
    Fixed,
    /// Owner open: any surviving pair, subject to the draft rule.
    Open,
}

struct Model {
    kinds: Vec<&'static Kind>,
    slots: Vec<Slot>,
    /// Initially possible pairs.
    alive0: u128,
    /// Seats left per player for the open tasks; `None`: no legal draft.
    caps: Option<[u8; MAX_PLAYERS]>,
    open: Vec<usize>,
    /// If set, only these drafts count (owner index per task; 0 for rule cards).
    allowed: Option<Vec<Vec<u8>>>,
}

impl Model {
    fn fixed(tasks: &Resolved) -> Model {
        assert!(tasks.len() <= 25, "at most 25 tasks");
        let mut m = Model { kinds: Vec::new(), slots: Vec::new(), alive0: 0, caps: Some([0; MAX_PLAYERS]), open: Vec::new(), allowed: None };
        for (t, &(k, o)) in tasks.iter().enumerate() {
            m.kinds.push(k);
            m.slots.push(if k.is_rule() { Slot::Global } else { Slot::Fixed });
            m.alive0 |= bit(t, if k.is_rule() { 0 } else { o });
        }
        m
    }

    fn drafts(s: &Situation, g: &Game) -> Model {
        assert!(s.tasks.len() <= 25, "at most 25 tasks");
        let mut m = Model { kinds: Vec::new(), slots: Vec::new(), alive0: 0, caps: None, open: Vec::new(), allowed: None };
        for (t, a) in s.tasks.iter().enumerate() {
            m.kinds.push(&a.card.kind);
            match a.owner {
                Owner::Crew => {
                    m.slots.push(Slot::Global);
                    m.alive0 |= bit(t, 0);
                }
                Owner::Player(p) => {
                    m.slots.push(Slot::Fixed);
                    m.alive0 |= bit(t, p as usize);
                }
                Owner::Unknown => {
                    m.slots.push(Slot::Open);
                    m.open.push(t);
                    for p in 0..g.n {
                        if p != g.captain as usize || a.card.captain_allowed() {
                            m.alive0 |= bit(t, p);
                        }
                    }
                }
            }
        }
        if let Some(c) = crate::draft::capacities(&s.tasks, g.n, g.captain as usize) {
            let mut caps = [0u8; MAX_PLAYERS];
            for (p, &k) in c.iter().enumerate() {
                caps[p] = k as u8;
            }
            m.caps = Some(caps);
        }
        m
    }

    /// Give each open task, from the k-th on, a player of `mask` with a free seat.
    fn assign(&self, k: usize, mask: u128, caps: &mut [u8; MAX_PLAYERS], out: &mut [u8]) -> bool {
        let Some(&t) = self.open.get(k) else { return true };
        let options = bits_of(mask, t);
        for p in 0..MAX_PLAYERS {
            if options & (1 << p) != 0 && caps[p] > 0 {
                caps[p] -= 1;
                out[t] = p as u8;
                let ok = self.assign(k + 1, mask, caps, out);
                caps[p] += 1;
                if ok {
                    return true;
                }
            }
        }
        false
    }

    /// Is there a complete legal draft using only pairs in `mask`?
    fn draft_in(&self, mask: u128, memo: &mut HashMap<u128, bool, Fast>) -> bool {
        let Some(caps) = self.caps else { return false };
        for (t, s) in self.slots.iter().enumerate() {
            if *s != Slot::Open && bits_of(mask, t) == 0 {
                return false;
            }
        }
        if let Some(list) = &self.allowed {
            if let Some(&ok) = memo.get(&mask) {
                return ok;
            }
            let ok = list.iter().any(|d| d.iter().enumerate().all(|(t, &p)| bits_of(mask, t) & (1 << p) != 0 || self.slots[t] == Slot::Global && bits_of(mask, t) != 0));
            memo.insert(mask, ok);
            return ok;
        }
        if self.open.is_empty() {
            return true;
        }
        if let Some(&ok) = memo.get(&mask) {
            return ok;
        }
        let mut out = vec![0u8; self.slots.len()];
        let mut free = caps;
        let ok = self.assign(0, mask, &mut free, &mut out);
        memo.insert(mask, ok);
        ok
    }

    /// Owners of a complete draft within `mask` (which must have one).
    fn owners_of(&self, mask: u128) -> Vec<Owner> {
        if let Some(list) = &self.allowed {
            if let Some(d) = list.iter().find(|d| d.iter().enumerate().all(|(t, &p)| bits_of(mask, t) & (1 << p) != 0 || self.slots[t] == Slot::Global)) {
                return self
                    .slots
                    .iter()
                    .zip(d)
                    .map(|(s, &p)| if *s == Slot::Global { Owner::Crew } else { Owner::Player(p) })
                    .collect();
            }
        }
        let mut out = vec![0u8; self.slots.len()];
        let mut caps = self.caps.unwrap_or([0; MAX_PLAYERS]);
        self.assign(0, mask, &mut caps, &mut out);
        self.slots
            .iter()
            .enumerate()
            .map(|(t, s)| match s {
                Slot::Global => Owner::Crew,
                Slot::Fixed => Owner::Player(bits_of(mask, t).trailing_zeros() as u8),
                Slot::Open => Owner::Player(out[t]),
            })
            .collect()
    }

    /// Player a pair stands for when evaluating its task.
    fn eval_owner(&self, t: usize, p: usize) -> usize {
        if self.slots[t] == Slot::Global {
            0
        } else {
            p
        }
    }
}

// ---------------------------------------------------------------- search

/// Who may still win each card, from the requirements of tasks with a settled
/// owner: false if two players must both win a card, a player must win a card
/// it must not win, every player must avoid a card that has to be played, or
/// a player must win more cards of a set than the others leave for it.
fn destiny_ok(g: &Game, settled: &[(usize, Req)]) -> bool {
    if settled.is_empty() {
        return true;
    }
    let mut claim = [0u8; 40];
    let mut forbid = [0u8; 40];
    for &(p, ref r) in settled {
        for c in r.need.iter() {
            let i = c.index() as usize;
            if claim[i] & !(1 << p) != 0 {
                return false;
            }
            claim[i] |= 1 << p;
        }
        for c in r.avoid.iter() {
            forbid[c.index() as usize] |= 1 << p;
        }
    }
    let everyone = (1u8 << g.n) - 1;
    let r = g.tricks_left() as u32;
    for p in 0..g.n {
        // Cards in a hand with more cards than tricks left may stay unplayed.
        let must_play = g.hands[p].len() <= r;
        for c in g.hands[p].iter() {
            let i = c.index() as usize;
            if claim[i] & forbid[i] != 0 || (must_play && forbid[i] == everyone) {
                return false;
            }
        }
    }
    for &(p, ref r) in settled {
        // "All cards of one of these sets": some set must be free of other
        // players' claims and of cards this player must not win.
        if r.n_alternatives > 0
            && !r.alternatives().iter().any(|set| {
                set.iter().all(|c| {
                    let i = c.index() as usize;
                    claim[i] & !(1 << p) == 0 && forbid[i] & (1 << p) == 0
                })
            })
        {
            return false;
        }
        for &(set, k) in r.demands() {
            let left = set.iter().filter(|c| {
                let i = c.index() as usize;
                claim[i] & !(1 << p) == 0 && forbid[i] & (1 << p) == 0
            });
            if (left.count() as u8) < k {
                return false;
            }
        }
    }
    true
}

/// Zobrist hash of all hands.
fn hands_zobrist(g: &Game) -> u128 {
    let mut z = 0;
    for p in 0..g.n {
        for c in g.hands[p].iter() {
            z ^= zobrist(c, p);
        }
    }
    z
}

/// Random 128-bit code for "card `c` is in the hand of player `p`".
fn zobrist(c: Card, p: usize) -> u128 {
    static TABLE: std::sync::OnceLock<[[u128; MAX_PLAYERS]; 40]> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        let mut rng = crate::rng::Rng::new(0x2B7E_1516_28AE_D2A6);
        let mut t = [[0u128; MAX_PLAYERS]; 40];
        for row in t.iter_mut() {
            for v in row.iter_mut() {
                *v = (rng.next_u64() as u128) << 64 | rng.next_u64() as u128;
            }
        }
        t
    })[c.index() as usize][p]
}

/// A cheap multiply-rotate hasher for the search tables: their keys are
/// already hashes or small bit masks, so the default SipHash is wasted work.
#[derive(Default)]
struct FastHasher(u64);

impl Hasher for FastHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u64(b as u64);
        }
    }
    fn write_u64(&mut self, x: u64) {
        self.0 = (self.0.rotate_left(29) ^ x).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }
    fn write_u128(&mut self, x: u128) {
        self.write_u64(x as u64);
        self.write_u64((x >> 64) as u64);
    }
}

type Fast = BuildHasherDefault<FastHasher>;

/// Two independent 64-bit multiply-rotate hashes; a collision of both is
/// negligible (about 2^-128 per pair).
struct KeyHasher(u64, u64);

impl KeyHasher {
    fn new() -> Self {
        KeyHasher(0x243F_6A88_85A3_08D3, 0x1319_8A2E_0370_7344)
    }
    fn push(&mut self, w: u64) {
        self.0 = (self.0.rotate_left(5) ^ w).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        self.1 = (self.1.rotate_left(7) ^ w).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    }
    fn finish(&self) -> u128 {
        (self.0 as u128) << 64 | self.1 as u128
    }
}

struct Search<'o, 'a> {
    model: Model,
    opts: &'o Options<'a>,
    /// Where tasks tell cards of a suit apart.
    boundaries: Boundaries,
    has_count_rules: bool,
    count_memo: HashMap<u128, bool, Fast>,
    draft_memo: HashMap<u128, bool, Fast>,
    failed: HashSet<u128, Fast>,
    stats: Stats,
    line: Vec<Card>,
    /// Done pairs of the winning position.
    won: u128,
    /// Why the search stopped early (cancel or node limit).
    stopped: Option<Verdict>,
    /// Zobrist hash of (card, holder) over all cards in hand.
    zob: u128,
    /// Move order in use (the options' decider, or another during restarts).
    decider: &'o dyn MoveDecider,
    /// Node count at which the current restart stops.
    round_limit: Option<u64>,
    /// Restart number and its node budget (portfolio), for trace events.
    restart: u32,
    budget: u64,
    /// Reused buffers of the combined check.
    pairs_buf: Vec<(u128, Option<CountRule>, (u8, u8))>,
    settled_buf: Vec<(usize, Req)>,
}

impl<'o, 'a> Search<'o, 'a> {
    fn new(g: &Game, model: Model, opts: &'o Options<'a>) -> Self {
        let boundaries = if opts.merge_cards {
            model.kinds.iter().fold(NO_BOUNDARIES, |acc, k| merge(acc, k.boundaries()))
        } else {
            [0x1FF; 5]
        };
        Search {
            model,
            opts,
            boundaries,
            // The combined check covers trick-count rules and card requirements alike.
            has_count_rules: opts.count_check,
            count_memo: HashMap::default(),
            draft_memo: HashMap::default(),
            failed: HashSet::default(),
            stats: Stats::default(),
            line: Vec::new(),
            won: 0,
            stopped: None,
            zob: hands_zobrist(g),
            decider: opts.decider,
            round_limit: None,
            restart: 0,
            budget: 0,
            pairs_buf: Vec::new(),
            settled_buf: Vec::new(),
        }
    }

    /// Run the configured strategy from `g` (restored afterwards).
    fn execute(&mut self, g: &mut Game) -> Option<bool> {
        let alive = self.model.alive0;
        match self.opts.strategy {
            Strategy::Dfs => self.dfs(g, alive, 0),
            Strategy::Portfolio { first_budget } => self.portfolio(g, alive, first_budget),
            Strategy::BestFirst { max_positions } => match self.best_first(g, alive, max_positions) {
                Some(r) => r,
                None => {
                    self.line.clear();
                    self.dfs(g, alive, 0)
                }
            },
        }
    }
}

impl Search<'_, '_> {
    /// Evaluate every surviving pair between tricks. Returns the updated
    /// (alive, done) pairs and the combined status.
    fn evaluate(&mut self, g: &Game, mut alive: u128, mut done: u128) -> (u128, u128, Status) {
        for t in 0..self.model.kinds.len() {
            let open = bits_of(alive & !done, t);
            if open != 0 {
                let k = self.model.kinds[t];
                for p in 0..g.n {
                    if open & (1 << p) == 0 {
                        continue;
                    }
                    match k.eval(g, self.model.eval_owner(t, p)) {
                        Status::Failed => alive &= !bit(t, p),
                        Status::Done => done |= bit(t, p),
                        Status::Open => {}
                    }
                }
            }
            if bits_of(alive, t) == 0 {
                if self.stats.failed_task.len() <= t {
                    self.stats.failed_task.resize(self.model.kinds.len(), 0);
                }
                self.stats.failed_task[t] += 1;
                return (alive, done, Status::Failed);
            }
        }
        if !self.model.draft_in(alive, &mut self.draft_memo) {
            return (alive, done, Status::Failed);
        }
        if self.model.draft_in(done, &mut self.draft_memo) {
            return (alive, done, Status::Done);
        }
        if g.is_over() {
            debug_assert!(false, "open task at game end");
            return (alive, done, Status::Failed);
        }
        (alive, done, Status::Open)
    }

    /// Combined check of all open tasks (needs `count_check`):
    /// - each surviving pair's trick-count rule and the trick window its own
    ///   card requirements imply ([`req_bounds`]); a pair whose window is
    ///   empty cannot be in any working draft;
    /// - for tasks whose owner is settled (fixed, or the only one left), the
    ///   requirements of each player together, and who may still win each
    ///   card ([`Search::destiny_ok`]);
    /// - then: is there a final split of the tricks under which some draft of
    ///   surviving pairs keeps every rule and window?
    /// Memoised by everything the answer depends on.
    fn counts_possible(&mut self, g: &Game, alive: u128, done: u128) -> bool {
        let mut h = KeyHasher::new();
        let mut key: u64 = g.tricks.first().map_or(7, |t| t.winner as u64);
        for p in 0..g.n {
            key = key << 8 | (g.trick_counts[p] as u64) << 3 | top_subs_count(g, p);
        }
        h.push(key << 4 | g.tricks_left() as u64);
        let mut usable = alive;
        let mut pairs = std::mem::take(&mut self.pairs_buf);
        let mut settled = std::mem::take(&mut self.settled_buf);
        pairs.clear();
        settled.clear();
        let ok = self.counts_possible_with(g, alive, done, &mut usable, &mut pairs, &mut settled, h);
        self.pairs_buf = pairs;
        self.settled_buf = settled;
        ok
    }

    #[allow(clippy::too_many_arguments)]
    fn counts_possible_with(
        &mut self,
        g: &Game,
        alive: u128,
        done: u128,
        usable: &mut u128,
        pairs: &mut Vec<(u128, Option<CountRule>, (u8, u8))>,
        settled: &mut Vec<(usize, Req)>,
        mut h: KeyHasher,
    ) -> bool {
        for t in 0..self.model.kinds.len() {
            let open = bits_of(alive & !done, t);
            if open == 0 {
                continue;
            }
            let k = self.model.kinds[t];
            let is_settled = bits_of(done, t) == 0 && open.count_ones() == 1;
            for p in 0..g.n {
                if open & (1 << p) == 0 {
                    continue;
                }
                let owner = self.model.eval_owner(t, p);
                let rule = k.count_rule(g, owner);
                let global = self.model.slots[t] == Slot::Global;
                let req = if global { Req::default() } else { k.requirements(g, p) };
                // A settled task's requirements go into its player's combined
                // window, which is at least as strict as its own.
                let window = if global || is_settled || req == Req::default() {
                    Some((0, u8::MAX))
                } else {
                    req_bounds(g, p, std::iter::once(&req))
                };
                let Some(window) = window else {
                    *usable &= !bit(t, p);
                    continue;
                };
                if rule.is_some() || window != (0, u8::MAX) {
                    pairs.push((bit(t, p), rule, window));
                }
                if is_settled && !global && req != Req::default() {
                    settled.push((p, req));
                }
            }
        }
        let mut windows = [(0u8, u8::MAX); MAX_PLAYERS];
        for (p, w) in windows.iter_mut().enumerate().take(g.n) {
            if !settled.iter().any(|(q, _)| *q == p) {
                continue;
            }
            match req_bounds(g, p, settled.iter().filter(|(q, _)| *q == p).map(|(_, r)| r)) {
                Some(b) => *w = b,
                None => return false,
            }
        }
        if !destiny_ok(g, settled) {
            self.stats.cut_by[2] += 1;
            return false;
        }
        let usable = *usable;
        h.push(usable as u64);
        h.push((usable >> 64) as u64);
        for w in &windows[..g.n] {
            h.push((w.0 as u64) << 8 | w.1 as u64);
        }
        for &(b, _, w) in pairs.iter() {
            h.push(b.trailing_zeros() as u64 | (w.0 as u64) << 8 | (w.1 as u64) << 16);
        }
        let memo_key = h.finish();
        if let Some(&ok) = self.count_memo.get(&memo_key) {
            return ok;
        }
        let (model, memo) = (&self.model, &mut self.draft_memo);
        let ok = model.draft_in(usable, memo)
            && any_final_counts(g, |f| {
                if (0..f.len()).any(|p| f[p] < windows[p].0 || f[p] > windows[p].1) {
                    return false;
                }
                let mut m = usable;
                for &(b, rule, w) in pairs.iter() {
                    let pl = (b.trailing_zeros() % 5) as usize;
                    let fits = rule.is_none_or(|r| r.holds(f)) && f[pl] >= w.0 && f[pl] <= w.1;
                    if !fits {
                        m &= !b;
                    }
                }
                model.draft_in(m, memo)
            });
        self.count_memo.insert(memo_key, ok);
        ok
    }

    fn boundary_above(&self, suit: Suit, v: u8) -> bool {
        v < suit.max_value() && self.boundaries[suit as usize] & (1 << (v - 1)) != 0
    }

    /// Position key between tricks. Cards are encoded by relative rank: per
    /// suit, the owners of the remaining cards from high to low, each with
    /// its class (how many task boundaries lie above it). Positions that
    /// differ only in which interchangeable cards are gone play out
    /// identically, so they share a key.
    fn key(&self, g: &Game, alive: u128, done: u128) -> u128 {
        let mut h = KeyHasher::new();
        if self.opts.zobrist {
            h.push(self.zob as u64);
            h.push((self.zob >> 64) as u64);
        } else {
            self.push_hands(g, &mut h);
        }
        self.push_rest(g, alive, done, &mut h);
        h.finish()
    }

    /// The hands by relative rank: per suit, the holders of the remaining
    /// cards from high to low with their class.
    fn push_hands(&self, g: &Game, h: &mut KeyHasher) {
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
            h.push(w);
        }
    }

    fn push_rest(&self, g: &Game, alive: u128, done: u128, h: &mut KeyHasher) {
        h.push(g.leader as u64);
        h.push(alive as u64);
        h.push((alive >> 64) as u64);
        h.push(done as u64);
        h.push((done >> 64) as u64);
        let open = alive & !done;
        for (t, k) in self.model.kinds.iter().enumerate() {
            let ps = bits_of(open, t);
            for p in 0..g.n {
                if ps & (1 << p) != 0 {
                    h.push(k.signature(g, self.model.eval_owner(t, p), Status::Open));
                }
            }
        }
    }

    fn play(&mut self, g: &mut Game, card: Card) -> bool {
        self.zob ^= zobrist(card, g.to_play());
        g.play(card)
    }

    /// Undo `card`, the last card played; afterwards its player is to play again.
    fn undo(&mut self, g: &mut Game, card: Card) {
        g.undo();
        self.zob ^= zobrist(card, g.to_play());
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

    /// Moves in the decider's order, in `buf`; returns how many.
    fn ordered_moves(&self, g: &Game, alive: u128, done: u128, buf: &mut [Card; 14]) -> usize {
        let mut k = 0;
        for c in self.moves(g).iter() {
            buf[k] = c;
            k += 1;
        }
        let pos = Position { game: g, kinds: &self.model.kinds, alive, done };
        self.decider.order(&pos, &mut buf[..k]);
        k
    }

    fn trace_progress(&self) {
        if let Some(t) = self.opts.trace {
            if let Some(id) = t.current {
                (t.sink)(&crate::trace::Event::Progress {
                    id,
                    nodes: self.stats.nodes,
                    restart: self.restart,
                    budget: self.budget,
                    cache: self.failed.len(),
                });
            }
        }
    }

    /// Count a node; false if the search must stop (cancel or node limit).
    fn tick(&mut self) -> bool {
        self.stats.nodes += 1;
        if self.stats.nodes & 0xFFF == 0 {
            if self.opts.cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
                self.stopped = Some(Verdict::Cancelled);
                return false;
            }
            if self.opts.node_limit.is_some_and(|l| self.stats.nodes >= l) {
                self.stopped = Some(Verdict::Undecided);
                return false;
            }
            if self.round_limit.is_some_and(|l| self.stats.nodes >= l) {
                return false;
            }
            if self.stats.nodes & 0xF_FFFF == 0 {
                if let Some(p) = self.opts.progress {
                    p(self.stats.nodes);
                }
                self.trace_progress();
            }
        }
        true
    }

    /// Between tricks: evaluate, count check and cache lookup. `Err(result)`
    /// ends this branch; `Ok` gives the updated pairs and the cache key.
    fn boundary(&mut self, g: &Game, alive: u128, done: u128) -> Result<(u128, u128, Option<u128>), bool> {
        let (alive, done, st) = self.evaluate(g, alive, done);
        let played = g.tricks.len().min(13);
        match st {
            Status::Failed => {
                self.stats.cut_by[0] += 1;
                self.stats.cut_at_trick[played] += 1;
                return Err(false);
            }
            Status::Done => {
                self.won = done;
                return Err(true);
            }
            Status::Open => {}
        }
        if self.has_count_rules {
            let destiny_before = self.stats.cut_by[2];
            if !self.counts_possible(g, alive, done) {
                if self.stats.cut_by[2] == destiny_before {
                    self.stats.cut_by[1] += 1;
                }
                self.stats.cut_at_trick[played] += 1;
                return Err(false);
            }
        }
        let mut key = None;
        if self.opts.use_cache {
            let k = self.key(g, alive, done);
            if self.failed.contains(&k) {
                self.stats.cache_hits += 1;
                self.stats.cut_by[3] += 1;
                self.stats.cut_at_trick[played] += 1;
                return Err(false);
            }
            key = Some(k);
        }
        Ok((alive, done, key))
    }

    fn remember_lost(&mut self, key: u128) {
        if self.failed.len() >= self.opts.cache_limit {
            self.failed.clear();
        }
        self.failed.insert(key);
    }

    /// Some(true) = winning line found (in `self.line`), Some(false) = lost, None = stopped.
    fn dfs(&mut self, g: &mut Game, mut alive: u128, mut done: u128) -> Option<bool> {
        if !self.tick() {
            return None;
        }
        let mut key = None;
        if g.current.is_empty() {
            match self.boundary(g, alive, done) {
                Err(r) => return Some(r),
                Ok((a, d, k)) => (alive, done, key) = (a, d, k),
            }
        }
        let mut buf = [Card::from_index(0); 14];
        let k = self.ordered_moves(g, alive, done, &mut buf);
        for &card in &buf[..k] {
            self.play(g, card);
            self.line.push(card);
            let r = self.dfs(g, alive, done);
            if r != Some(false) {
                if r.is_none() {
                    self.undo(g, card);
                }
                return r;
            }
            self.line.pop();
            self.undo(g, card);
        }
        if let Some(k) = key {
            self.remember_lost(k);
        }
        Some(false)
    }

    /// Restarts alternating move orders with doubling budgets; see
    /// [`Strategy::Portfolio`]. Same result convention as [`Search::dfs`].
    fn portfolio(&mut self, g: &mut Game, alive: u128, first_budget: u64) -> Option<bool> {
        let mut budget = first_budget.max(1);
        let mut round = 0;
        loop {
            let deciders: [&dyn MoveDecider; 3] =
                [self.opts.decider, &DECK_ORDER, &TASK_AWARE_RANDOM[round % TASK_AWARE_RANDOM.len()]];
            round += 1;
            for &d in &deciders {
                self.decider = d;
                self.round_limit = Some(self.stats.nodes.saturating_add(budget));
                self.restart += 1;
                self.budget = budget;
                self.trace_progress();
                self.line.clear();
                let r = self.dfs(g, alive, 0);
                if r.is_some() || self.stopped.is_some() {
                    return r;
                }
            }
            budget = budget.saturating_mul(2);
        }
    }

    /// Best-first over positions between tricks. `None` if more than
    /// `max_positions` were generated (the caller falls back to depth-first).
    fn best_first(&mut self, root: &Game, alive: u128, max_positions: usize) -> Option<Option<bool>> {
        struct Node {
            parent: usize,
            /// Cards played from the parent to reach this position.
            cards: Vec<Card>,
        }
        let mut nodes = vec![Node { parent: usize::MAX, cards: Vec::new() }];
        let mut states: Vec<Option<(Game, u128, u128)>> = vec![None];
        let mut heap = BinaryHeap::new();
        let mut seen: HashSet<u128, Fast> = HashSet::default();
        let line_to = |nodes: &Vec<Node>, mut i: usize, last: &[Card]| {
            let mut parts = vec![last.to_vec()];
            while i != usize::MAX {
                parts.push(nodes[i].cards.clone());
                i = nodes[i].parent;
            }
            parts.into_iter().rev().flatten().collect::<Vec<Card>>()
        };
        // The root may be in the middle of a trick; it is expanded like any node.
        let (mut a0, mut d0) = (alive, 0);
        if root.current.is_empty() {
            match self.boundary(root, a0, d0) {
                Err(r) => return Some(Some(r)),
                Ok((a, d, _)) => (a0, d0) = (a, d),
            }
        }
        states[0] = Some((root.clone(), a0, d0));
        heap.push((0u32, 0usize));
        while let Some((_, i)) = heap.pop() {
            let (mut g, a, d) = states[i].take().unwrap();
            self.zob = hands_zobrist(&g);
            // Every way to finish the trick in progress, depth-first.
            let mut children: Vec<(Vec<Card>, Game, u128, u128)> = Vec::new();
            let mut path = Vec::new();
            let mut found: Option<Vec<Card>> = None;
            let ok = self.expand(&mut g, a, d, &mut path, &mut children, &mut found);
            if !ok {
                return Some(None);
            }
            if let Some(last) = found {
                self.line = line_to(&nodes, i, &last);
                return Some(Some(true));
            }
            for (cards, child, ca, cd) in children {
                if self.opts.zobrist {
                    self.zob = hands_zobrist(&child);
                }
                let key = self.key(&child, ca, cd);
                if !seen.insert(key) || self.failed.contains(&key) {
                    continue;
                }
                let tasks_done = (0..self.model.kinds.len()).filter(|&t| bits_of(cd, t) != 0).count() as u32;
                let prio = tasks_done * 64 + child.tricks.len() as u32;
                nodes.push(Node { parent: i, cards });
                states.push(Some((child, ca, cd)));
                heap.push((prio, nodes.len() - 1));
                if nodes.len() > max_positions {
                    return None;
                }
            }
        }
        Some(Some(false))
    }

    /// Play out the trick in progress in every way (merged moves); collect the
    /// open positions after it, or stop at a won one (`found`). False if stopped.
    fn expand(
        &mut self,
        g: &mut Game,
        alive: u128,
        done: u128,
        path: &mut Vec<Card>,
        out: &mut Vec<(Vec<Card>, Game, u128, u128)>,
        found: &mut Option<Vec<Card>>,
    ) -> bool {
        let mut buf = [Card::from_index(0); 14];
        let k = self.ordered_moves(g, alive, done, &mut buf);
        for &card in &buf[..k] {
            if !self.tick() {
                return false;
            }
            let trick_done = self.play(g, card);
            path.push(card);
            if trick_done {
                match self.boundary(g, alive, done) {
                    Err(true) => {
                        *found = Some(path.clone());
                    }
                    Err(false) => {}
                    Ok((a, d, _)) => out.push((path.clone(), g.clone(), a, d)),
                }
            } else if !self.expand(g, alive, done, path, out, found) {
                path.pop();
                self.undo(g, card);
                return false;
            }
            path.pop();
            self.undo(g, card);
            if found.is_some() {
                return true;
            }
        }
        true
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

    /// A random late-game situation (`left` tricks to go) with `k` random
    /// tasks of unknown owner.
    fn random_open_situation(rng: &mut crate::rng::Rng, n: usize, left: usize, k: usize) -> Situation {
        use crate::catalogue::catalogue;
        use crate::situation::{Assignment, PlayedTrick};
        let hands = Game::random_hands(n, rng);
        let mut g = Game::new(&hands).unwrap();
        while g.tricks_left() > left || !g.current.is_empty() {
            let moves: Vec<Card> = g.legal_moves().iter().collect();
            g.play(moves[rng.below(moves.len())]);
        }
        let pool: Vec<_> = catalogue().iter().filter(|t| !t.is_rule()).collect();
        Situation {
            hands,
            tasks: (0..k).map(|_| Assignment { card: pool[rng.below(pool.len())], owner: Owner::Unknown }).collect(),
            played: g.tricks.iter().map(|t| PlayedTrick { leader: t.leader, cards: t.cards.clone() }).collect(),
            ..Situation::default()
        }
    }

    /// One search over all drafts must agree with solving every draft on its
    /// own, and its answer must be a legal draft whose line does every task.
    #[test]
    fn draft_search_in_one_tree_agrees_with_each_draft() {
        use crate::rng::Rng;
        let mut rng = Rng::new(7);
        let (mut feasible, mut infeasible) = (0, 0);
        for case in 0..150 {
            let n = 3 + case % 3;
            let mut s = random_open_situation(&mut rng, n, if n == 5 { 3 } else { 4 }, 2 + case % 3);
            let g = s.game().unwrap();
            if case % 3 == 1 {
                // Mixed: one owner fixed, the others open.
                let mut p = rng.below(n);
                if p == g.captain as usize && !s.tasks[0].card.captain_allowed() {
                    p = (p + 1) % n;
                }
                s.tasks[0].owner = Owner::Player(p as u8);
            }
            if redeal(&s).is_some() {
                continue;
            }
            let mut any = false;
            let mut drafts = 0;
            crate::draft::for_each_draft(&s.tasks, n, g.captain as usize, |owners| {
                drafts += 1;
                let mut d = s.clone();
                for (a, o) in d.tasks.iter_mut().zip(owners) {
                    a.owner = *o;
                }
                any = matches!(solve(&d, &Options::plain()).unwrap().0, Verdict::Feasible(_));
                !any
            });
            let (v, owners, _) = solve_drafts(&s, &Options::default()).unwrap();
            match v {
                Verdict::Feasible(line) => {
                    assert!(any, "case {case}: one tree feasible, every draft infeasible");
                    let owners = owners.unwrap();
                    let mut d = s.clone();
                    for (a, o) in d.tasks.iter_mut().zip(&owners) {
                        a.owner = *o;
                    }
                    let mut legal = false;
                    crate::draft::for_each_draft(&s.tasks, n, g.captain as usize, |o| {
                        legal |= o == owners.as_slice();
                        !legal
                    });
                    assert!(legal, "case {case}: {owners:?} is not a legal draft");
                    check_line(&d, &line);
                    feasible += 1;
                }
                Verdict::Infeasible => {
                    assert!(!any, "case {case}: one tree infeasible, but a draft works ({drafts} drafts)");
                    infeasible += 1;
                }
                v => panic!("case {case}: {v:?}"),
            }
        }
        assert!(feasible > 20 && infeasible > 20, "feasible {feasible}, infeasible {infeasible}");
    }

    #[test]
    fn best_first_agrees_with_depth_first() {
        use crate::rng::Rng;
        let mut rng = Rng::new(11);
        let mut fallbacks_and_full = (0, 0);
        for case in 0..120 {
            let n = 3 + case % 3;
            let s = random_open_situation(&mut rng, n, if n == 5 { 3 } else { 5 }, 2 + case % 2);
            if redeal(&s).is_some() {
                continue;
            }
            // Small limits force the fallback to depth-first; large ones finish best-first.
            let max_positions = if case % 2 == 0 { 50 } else { 1_000_000 };
            let bf = Options { strategy: Strategy::BestFirst { max_positions }, ..Options::default() };
            let (a, ao, _) = solve_drafts(&s, &Options::default()).unwrap();
            let (b, bo, _) = solve_drafts(&s, &bf).unwrap();
            match (&a, &b) {
                (Verdict::Feasible(_), Verdict::Feasible(line)) => {
                    let mut d = s.clone();
                    for (t, o) in d.tasks.iter_mut().zip(bo.unwrap()) {
                        t.owner = o;
                    }
                    check_line(&d, line);
                    assert!(ao.is_some());
                }
                (Verdict::Infeasible, Verdict::Infeasible) => {}
                _ => panic!("case {case}: dfs {a:?} vs best-first {b:?}"),
            }
            if case % 2 == 0 { fallbacks_and_full.0 += 1 } else { fallbacks_and_full.1 += 1 }
        }
        assert!(fallbacks_and_full.0 > 30 && fallbacks_and_full.1 > 30);
    }

    #[test]
    fn node_limit_gives_undecided() {
        // Proving this impossible takes millions of nodes (no pink leads: P9 can only be discarded).
        let s = situation("Player4: win P9 Y8\ncrew: rule M12 no-lead pink sub");
        let opts = Options { node_limit: Some(10_000), ..Options::default() };
        let (v, stats) = solve(&s, &opts).unwrap();
        assert_eq!(v, Verdict::Undecided, "{stats:?}");
        assert!(stats.nodes < 20_000);
    }

    #[test]
    fn zobrist_keys_agree_with_rank_keys() {
        use crate::rng::Rng;
        let mut rng = Rng::new(23);
        let mut seen = (0, 0);
        for case in 0..150 {
            let n = 3 + case % 3;
            let s = random_open_situation(&mut rng, n, if n == 5 { 3 } else { 5 }, 2 + case % 2);
            if redeal(&s).is_some() {
                continue;
            }
            let reference = solve_drafts(&s, &Options::default()).unwrap().0;
            for strategy in [Strategy::Dfs, Strategy::BestFirst { max_positions: 200 }] {
                let opts = Options { zobrist: true, strategy, ..Options::default() };
                let (v, owners, _) = solve_drafts(&s, &opts).unwrap();
                match (&reference, &v) {
                    (Verdict::Feasible(_), Verdict::Feasible(line)) => {
                        let mut d = s.clone();
                        for (t, o) in d.tasks.iter_mut().zip(owners.unwrap()) {
                            t.owner = o;
                        }
                        check_line(&d, line);
                        seen.0 += 1;
                    }
                    (Verdict::Infeasible, Verdict::Infeasible) => seen.1 += 1,
                    _ => panic!("case {case} {strategy:?}: rank keys {reference:?}, zobrist {v:?}"),
                }
            }
        }
        assert!(seen.0 > 50 && seen.1 > 50, "{seen:?}");
    }

    #[test]
    fn portfolio_agrees_with_depth_first() {
        use crate::rng::Rng;
        let mut rng = Rng::new(31);
        let (mut feasible, mut infeasible) = (0, 0);
        for case in 0..150 {
            let n = 3 + case % 3;
            let s = random_open_situation(&mut rng, n, if n == 5 { 3 } else { 5 }, 2 + case % 2);
            if redeal(&s).is_some() {
                continue;
            }
            let reference = solve_drafts(&s, &Options { strategy: Strategy::Dfs, ..Options::default() }).unwrap().0;
            // A tiny first budget forces several restarts.
            let opts = Options { strategy: Strategy::Portfolio { first_budget: 1 + case as u64 % 50 }, ..Options::default() };
            let (v, owners, _) = solve_drafts(&s, &opts).unwrap();
            match (&reference, &v) {
                (Verdict::Feasible(_), Verdict::Feasible(line)) => {
                    let mut d = s.clone();
                    for (t, o) in d.tasks.iter_mut().zip(owners.unwrap()) {
                        t.owner = o;
                    }
                    check_line(&d, line);
                    feasible += 1;
                }
                (Verdict::Infeasible, Verdict::Infeasible) => infeasible += 1,
                _ => panic!("case {case}: dfs {reference:?}, portfolio {v:?}"),
            }
        }
        assert!(feasible > 30 && infeasible > 30, "{feasible} {infeasible}");
    }

    /// The combined checks (trick windows from card requirements, card
    /// destiny) must never prune a position that has a winning line.
    #[test]
    fn combined_checks_are_sound() {
        use crate::catalogue::catalogue;
        use crate::rng::Rng;
        let mut rng = Rng::new(97);
        let pool: Vec<_> = catalogue().iter().filter(|t| !t.is_rule()).collect();
        let (mut feasible, mut infeasible, mut pruned) = (0, 0, 0);
        for case in 0..300 {
            let n = 3 + case % 3;
            let mut g = Game::new(&Game::random_hands(n, &mut rng)).unwrap();
            let left = if n == 5 { 5 } else { 6 };
            while g.tricks_left() > left || !g.current.is_empty() {
                let moves: Vec<Card> = g.legal_moves().iter().collect();
                g.play(moves[rng.below(moves.len())]);
            }
            let k = 2 + rng.below(3);
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
            let with = solve_game(g.clone(), &tasks, &Options { strategy: Strategy::Dfs, ..Options::default() });
            let without = solve_game(g.clone(), &tasks, &Options { count_check: false, strategy: Strategy::Dfs, ..Options::default() });
            match (&with.0, &without.0) {
                (Verdict::Feasible(_), Verdict::Feasible(_)) => feasible += 1,
                (Verdict::Infeasible, Verdict::Infeasible) => {
                    infeasible += 1;
                    if with.1.nodes < without.1.nodes {
                        pruned += 1;
                    }
                }
                _ => panic!("case {case}: with checks {:?}, without {:?}; tasks {tasks:?}", with.0, without.0),
            }
        }
        assert!(feasible > 30 && infeasible > 30, "{feasible} {infeasible}");
        assert!(pruned > 10, "the checks should cut some proofs short: {pruned}");
    }

    /// Impossibilities visible from the hands are found at once, not by search.
    #[test]
    fn obvious_impossibilities_are_immediate() {
        // Player1 holds P2: it cannot also play the submarine that wins it.
        // Player2 does not hold S3: taking it needs a higher submarine it would win too.
        // "more yellow than blue" with "no yellow" for the same player.
        // All of a colour needs more than one trick (each holder plays one card per trick).
        for tasks in [
            "Player1: win P2 with sub",
            "Player2: only S3",
            "Player2: more yellow than blue\nPlayer2: no yellow",
            "Player3: all of a colour\nPlayer3: tricks =1",
        ] {
            let (v, stats) = solve(&situation(tasks), &Options::default()).unwrap();
            assert_eq!(v, Verdict::Infeasible, "{tasks}");
            assert!(stats.nodes <= 2, "{tasks}: {} nodes", stats.nodes);
        }
    }

    /// Deciding all drafts by repeated searches agrees with solving each one,
    /// and every line returned completes its draft.
    #[test]
    fn decide_drafts_agrees_with_each_draft() {
        use crate::rng::Rng;
        let mut rng = Rng::new(51);
        let (mut feasible, mut infeasible) = (0, 0);
        for case in 0..80 {
            let n = 3 + case % 3;
            let s = random_open_situation(&mut rng, n, if n == 5 { 3 } else { 4 }, 2 + case % 3);
            let g = s.game().unwrap();
            if redeal(&s).is_some() {
                continue;
            }
            let mut drafts = Vec::new();
            crate::draft::for_each_draft(&s.tasks, n, g.captain as usize, |o| {
                drafts.push(o.to_vec());
                true
            });
            let decided = decide_drafts(&s, &drafts, &Options::default(), None, None).unwrap();
            for (d, r) in drafts.iter().zip(&decided) {
                let mut fixed = s.clone();
                for (a, o) in fixed.tasks.iter_mut().zip(d) {
                    a.owner = *o;
                }
                let alone = solve(&fixed, &Options::plain()).unwrap().0;
                match (r, &alone) {
                    (Decision::Works(line), Verdict::Feasible(_)) => {
                        check_line(&fixed, line);
                        feasible += 1;
                    }
                    (Decision::Fails, Verdict::Infeasible) => infeasible += 1,
                    _ => panic!("case {case} draft {d:?}: together {r:?}, alone {alone:?}"),
                }
            }
        }
        assert!(feasible > 50 && infeasible > 50, "{feasible} {infeasible}");
    }

    /// With a node limit per draft, drafts not settled within it stay open
    /// and the others keep their answer.
    #[test]
    fn decide_drafts_honours_the_node_limit() {
        use crate::rng::Rng;
        let mut rng = Rng::new(53);
        let mut open = 0;
        for _ in 0..30 {
            let s = random_open_situation(&mut rng, 4, 4, 3);
            if redeal(&s).is_some() {
                continue;
            }
            let g = s.game().unwrap();
            let mut drafts = Vec::new();
            crate::draft::for_each_draft(&s.tasks, 4, g.captain as usize, |o| {
                drafts.push(o.to_vec());
                true
            });
            let capped = decide_drafts(&s, &drafts, &Options { node_limit: Some(50), ..Options::default() }, None, None).unwrap();
            let all = decide_drafts(&s, &drafts, &Options::default(), None, None).unwrap();
            for (c, a) in capped.iter().zip(&all) {
                match c {
                    Decision::Open => open += 1,
                    Decision::Works(_) => assert!(matches!(a, Decision::Works(_))),
                    Decision::Fails => assert_eq!(a, &Decision::Fails),
                }
            }
        }
        assert!(open > 0);
    }

    /// A cancelled decision keeps the drafts decided before the cancel.
    #[test]
    fn cancelled_decide_drafts_keeps_decided_drafts() {
        use crate::rng::Rng;
        let mut rng = Rng::new(52);
        let mut kept = 0;
        for _ in 0..40 {
            let s = random_open_situation(&mut rng, 4, 4, 3);
            if redeal(&s).is_some() {
                continue;
            }
            let g = s.game().unwrap();
            let mut drafts = Vec::new();
            crate::draft::for_each_draft(&s.tasks, 4, g.captain as usize, |o| {
                drafts.push(o.to_vec());
                true
            });
            let cancel = AtomicBool::new(false);
            // Cancel as soon as half the drafts are decided.
            let half = drafts.len() / 2;
            let stop = |_: usize, open: usize| {
                if open <= drafts.len() - half {
                    cancel.store(true, Ordering::Relaxed);
                }
            };
            let opts = Options { cancel: Some(&cancel), ..Options::default() };
            let seen = std::cell::RefCell::new(Vec::new());
            let note = |i: usize, d: &Decision| seen.borrow_mut().push((i, d.clone()));
            let decided = decide_drafts(&s, &drafts, &opts, Some(&stop), Some(&note)).unwrap();
            // Every settled draft was reported once, with its final answer.
            let mut seen = seen.into_inner();
            seen.sort_by_key(|x| x.0);
            let settled: Vec<_> = decided.iter().cloned().enumerate().filter(|(_, d)| *d != Decision::Open).collect();
            assert_eq!(seen, settled);
            let all = decide_drafts(&s, &drafts, &Options::default(), None, None).unwrap();
            assert!(!all.contains(&Decision::Open));
            for (d, a) in decided.iter().zip(&all) {
                match d {
                    Decision::Open => {}
                    Decision::Works(_) => assert!(matches!(a, Decision::Works(_))),
                    Decision::Fails => assert_eq!(a, &Decision::Fails),
                }
            }
            let done = decided.iter().filter(|d| **d != Decision::Open).count();
            if done > 0 && done < drafts.len() {
                kept += 1;
            }
        }
        assert!(kept > 5, "{kept}");
    }

    /// Player1 holds S2 and S3; the only higher submarine elsewhere is S4, which
    /// can beat just one of them: Player1 certainly wins a trick with a submarine.
    #[test]
    fn submarine_pairing_gives_certain_wins() {
        for task in ["no subs", "tricks =0"] {
            let text = format!(
                "crew 1
players: 4
Player1: S2 S3 P1 P2 P3 P4 P5 P6 P7 P8
Player2: S4 P9 B1 B2 B3 B4 B5 B6 B7 B8
Player3: S1 B9 G1 G2 G3 G4 G5 G6 G7 G8
Player4: rest
tasks:
Player1: {task}"
            );
            let s = crate::notation::parse(&text).unwrap();
            let (v, stats) = solve(&s, &Options::default()).unwrap();
            assert_eq!(v, Verdict::Infeasible, "{task}");
            assert!(stats.nodes <= 2, "{task}: {} nodes", stats.nodes);
        }
    }

    #[test]
    fn lookahead_order_agrees_with_depth_first() {
        use crate::rng::Rng;
        let mut rng = Rng::new(88);
        let (mut feasible, mut infeasible) = (0, 0);
        for case in 0..150 {
            let n = 3 + case % 3;
            let s = random_open_situation(&mut rng, n, if n == 5 { 3 } else { 5 }, 2 + case % 2);
            if redeal(&s).is_some() {
                continue;
            }
            let reference =
                solve_drafts(&s, &Options { strategy: Strategy::Dfs, decider: &crate::decider::DECK_ORDER, ..Options::default() }).unwrap().0;
            let opts = Options { decider: &crate::decider::LOOKAHEAD, ..Options::default() };
            let (v, owners, _) = solve_drafts(&s, &opts).unwrap();
            match (&reference, &v) {
                (Verdict::Feasible(_), Verdict::Feasible(line)) => {
                    let mut d = s.clone();
                    for (t, o) in d.tasks.iter_mut().zip(owners.unwrap()) {
                        t.owner = o;
                    }
                    check_line(&d, line);
                    feasible += 1;
                }
                (Verdict::Infeasible, Verdict::Infeasible) => infeasible += 1,
                _ => panic!("case {case}: dfs {reference:?}, lookahead {v:?}"),
            }
        }
        assert!(feasible > 30 && infeasible > 30, "{feasible} {infeasible}");
    }
}
