//! Analyses built on the solver: draft search.

use crate::card::Card;
use crate::draft;
use crate::rng::Rng;
use crate::situation::{Owner, Situation};
use crate::solver::{self, Decision, Options, SolveError, Verdict};
use std::collections::HashSet;
use std::sync::atomic::AtomicBool;

/// Result of searching over drafts.
#[derive(Debug, Clone)]
pub struct DraftReport {
    /// Number of legal drafts.
    pub total: u64,
    /// Drafts decided (this part's share; all drafts if not sampled or split).
    pub checked: u64,
    /// Feasible drafts among those checked.
    pub working: u64,
    /// Drafts left without an answer (only when cancelled).
    pub undecided: u64,
    /// True if `checked < total` because drafts were sampled.
    pub sampled: bool,
    /// Exact answer to "does any legal draft work?" (one search over all
    /// drafts); `None` if cancelled.
    pub any_works: Option<bool>,
    /// A working draft and its winning line.
    pub example: Option<(Vec<Owner>, Vec<Card>)>,
    /// The working draft closest to `reference`, how many tasks change owner,
    /// and its winning line.
    pub closest: Option<(Vec<Owner>, usize, Vec<Card>)>,
    /// (task index, player) pairs ruled out because that task alone is impossible for that player.
    pub impossible_pairs: Vec<(usize, usize)>,
    /// Every draft of this part (sampled or all) with its decision.
    pub drafts: Vec<(Vec<Owner>, Decision)>,
}

#[derive(Clone)]
pub struct DraftOptions<'a> {
    /// Solve every draft if there are at most this many, else sample this many.
    pub limit: u64,
    pub seed: u64,
    /// Owners to compare against for the smallest fix (e.g. the draft entered).
    pub reference: Option<Vec<Owner>>,
    pub cancel: Option<&'a AtomicBool>,
    /// Called with (drafts decided, drafts planned, working so far, drafts
    /// still open) before each search.
    pub progress: Option<&'a dyn Fn(u64, u64, u64, u64)>,
    /// (k, n): decide only every n-th draft starting at k, for running the
    /// count in n parallel workers. Part 0 also answers "does any draft work?".
    pub part: (usize, usize),
    /// Lost positions kept per search (see [`Options::cache_limit`]).
    pub cache_limit: usize,
    /// Called with each draft of this part (owners, decision) as it is
    /// settled, so a caller that is stopped keeps what was found.
    pub decided: Option<&'a OnDecided<'a>>,
}

/// Callback with a settled draft: (owners, decision).
pub type OnDecided<'a> = dyn Fn(&[Owner], &Decision) + 'a;

impl Default for DraftOptions<'_> {
    fn default() -> Self {
        DraftOptions {
            limit: 2000,
            seed: 1,
            reference: None,
            cancel: None,
            progress: None,
            part: (0, 1),
            cache_limit: Options::default().cache_limit,
            decided: None,
        }
    }
}

fn distance(a: &[Owner], b: &[Owner]) -> usize {
    a.iter().zip(b).filter(|(x, y)| x != y).count()
}

/// Search the drafts for the tasks with owner `?` (fixed owners stay): the
/// exact yes/no over all drafts in one search, then every draft (or a sample
/// above `limit`) decided by [`solver::decide_drafts`].
pub fn draft_search(s: &Situation, opts: &DraftOptions) -> Result<DraftReport, SolveError> {
    let g = s.game().map_err(|e| SolveError::Replay(e.to_string()))?;
    let (n, captain) = (g.n, g.captain as usize);
    let solve_opts = Options { cancel: opts.cancel, cache_limit: opts.cache_limit, ..Options::default() };

    let (part, parts) = (opts.part.0, opts.part.1.max(1));
    let (any_works, one_tree_example) = if part != 0 {
        (None, None)
    } else {
        match solver::solve_drafts(s, &solve_opts)? {
            (Verdict::Feasible(line), Some(owners), _) => (Some(true), Some((owners, line))),
            (Verdict::Infeasible | Verdict::Redeal(_), _, _) => (Some(false), None),
            _ => (None, None),
        }
    };

    // Rule out (task, player) pairs that are impossible even alone.
    let mut impossible = vec![vec![false; n]; s.tasks.len()];
    let mut impossible_pairs = Vec::new();
    for (i, a) in s.tasks.iter().enumerate() {
        if a.owner != Owner::Unknown {
            continue;
        }
        for p in 0..n {
            if p == captain && !a.card.captain_allowed() {
                continue;
            }
            let mut single = s.clone();
            single.tasks = vec![crate::situation::Assignment { card: a.card, owner: Owner::Player(p as u8) }];
            if let Ok((Verdict::Infeasible, _)) = solver::solve(&single, &solve_opts) {
                impossible[i][p] = true;
                impossible_pairs.push((i, p));
            }
        }
    }
    let allowed = |i: usize, p: usize| !impossible[i][p];

    let total = draft::count_drafts(&s.tasks, n, captain);
    let mut report = DraftReport {
        total,
        checked: 0,
        working: 0,
        undecided: 0,
        sampled: false,
        any_works,
        example: None,
        closest: None,
        impossible_pairs,
        drafts: Vec::new(),
    };

    // The drafts to decide: all of them, or a sample.
    let mut drafts: Vec<Vec<Owner>> = Vec::new();
    if total <= opts.limit {
        draft::for_each_draft(&s.tasks, n, captain, |owners| {
            drafts.push(owners.to_vec());
            true
        });
    } else {
        report.sampled = true;
        let mut rng = Rng::new(opts.seed);
        let mut seen: HashSet<Vec<Owner>> = HashSet::new();
        let mut tries = 0;
        while (seen.len() as u64) < opts.limit && tries < opts.limit * 20 {
            tries += 1;
            let Some(owners) = draft::random_draft(&s.tasks, n, captain, &mut rng) else { break };
            if seen.insert(owners.clone()) {
                drafts.push(owners);
            }
        }
    }
    // This worker's share.
    let drafts: Vec<Vec<Owner>> = drafts.into_iter().enumerate().filter(|(i, _)| i % parts == part).map(|(_, d)| d).collect();
    let planned = drafts.len() as u64;
    // Drafts with a pair impossible on its own fail without a search.
    let bad = |owners: &[Owner]| {
        owners.iter().enumerate().any(|(i, o)| matches!(o, Owner::Player(p) if s.tasks[i].owner == Owner::Unknown && !allowed(i, *p as usize)))
    };
    let rest: Vec<Vec<Owner>> = drafts.iter().filter(|d| !bad(d)).cloned().collect();
    let tell = |working: usize, open: usize| {
        if let Some(p) = opts.progress {
            p(planned - open as u64, planned, working as u64, open as u64);
        }
    };
    if let Some(f) = opts.decided {
        for d in drafts.iter().filter(|d| bad(d)) {
            f(d, &Decision::Fails);
        }
    }
    let note = |i: usize, d: &Decision| {
        if let Some(f) = opts.decided {
            f(&rest[i], d);
        }
    };
    let mut decided = solver::decide_drafts(s, &rest, &solve_opts, Some(&tell), Some(&note))?.into_iter();
    for owners in drafts {
        let d = if bad(&owners) { Decision::Fails } else { decided.next().expect("one decision per draft") };
        match &d {
            Decision::Open => report.undecided += 1,
            Decision::Fails => report.checked += 1,
            Decision::Works(line) => {
                report.checked += 1;
                report.working += 1;
                if let Some(r) = &opts.reference {
                    let dist = distance(&owners, r);
                    if report.closest.as_ref().is_none_or(|c| dist < c.1) {
                        report.closest = Some((owners.clone(), dist, line.clone()));
                    }
                }
                if report.example.is_none() {
                    report.example = Some((owners.clone(), line.clone()));
                }
            }
        }
        report.drafts.push((owners, d));
    }
    if report.example.is_none() {
        report.example = one_tree_example;
    }
    Ok(report)
}

/// Who takes which task, and which tasks share a player, over the decided
/// drafts of a [`DraftReport`]. Working drafts are compared with all decided
/// drafts, so what the draft rules force is not reported. (The web page
/// computes the same live, with a filter.)
#[derive(Debug, Default, Clone, PartialEq)]
pub struct DraftRelations {
    /// (task, owner): every working draft has it, though the rules allow otherwise.
    pub must: Vec<(usize, Owner)>,
    /// (task, owner): no working draft has it, though the rules allow it.
    pub never: Vec<(usize, Owner)>,
    /// Task pairs with the same owner in every working draft / in none.
    pub always_together: Vec<(usize, usize)>,
    pub never_together: Vec<(usize, usize)>,
    /// (task, task, share with the same owner among working drafts, among
    /// all drafts): the largest gaps, at least [`TENDENCY_GAP`], at most 5.
    pub tendencies: Vec<(usize, usize, f64, f64)>,
}

/// Minimum gap between the two shares for a tendency.
pub const TENDENCY_GAP: f64 = 0.25;

pub fn draft_relations(drafts: &[(Vec<Owner>, Decision)]) -> DraftRelations {
    let rows: Vec<(&Vec<Owner>, bool)> =
        drafts.iter().filter(|(_, d)| *d != Decision::Open).map(|(o, d)| (o, matches!(d, Decision::Works(_)))).collect();
    let mut r = DraftRelations::default();
    let works = rows.iter().filter(|x| x.1).count();
    let Some(n) = rows.first().map(|x| x.0.len()) else { return r };
    if works == 0 {
        return r;
    }
    let tasks: Vec<usize> = (0..n).filter(|&t| rows.iter().any(|x| x.0[t] != rows[0].0[t])).collect();
    // (working drafts, all drafts) with the property.
    let count = |p: &dyn Fn(&[Owner]) -> bool| {
        rows.iter().fold((0, 0), |(w, a), x| if p(x.0) { (w + x.1 as usize, a + 1) } else { (w, a) })
    };
    for &t in &tasks {
        let mut owners: Vec<Owner> = rows.iter().map(|x| x.0[t]).collect();
        owners.sort_by_key(|o| format!("{o:?}"));
        owners.dedup();
        for o in owners {
            match count(&|d| d[t] == o) {
                (w, a) if w == works && a < rows.len() => r.must.push((t, o)),
                (0, a) if a > 0 => r.never.push((t, o)),
                _ => {}
            }
        }
    }
    let mut rest = Vec::new();
    for (k, &t1) in tasks.iter().enumerate() {
        for &t2 in &tasks[k + 1..] {
            match count(&|d| d[t1] == d[t2]) {
                (w, a) if w == works && a < rows.len() => r.always_together.push((t1, t2)),
                (0, a) if a > 0 => r.never_together.push((t1, t2)),
                (_, 0) => {}
                (w, a) => rest.push((t1, t2, w as f64 / works as f64, a as f64 / rows.len() as f64)),
            }
        }
    }
    let gap = |x: &(usize, usize, f64, f64)| (x.2 - x.3).abs();
    rest.retain(|x| gap(x) >= TENDENCY_GAP);
    rest.sort_by(|x, y| gap(y).total_cmp(&gap(x)));
    rest.truncate(5);
    r.tendencies = rest;
    r
}

/// A pair of tasks that one player cannot fulfil together, even without the
/// other tasks, though each alone is possible for that player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairConflict {
    pub tasks: (usize, usize),
    /// Players for whom the pair fails.
    pub players: Vec<usize>,
    /// Players for whom both tasks are possible alone (the pair was tried).
    pub tried: Vec<usize>,
}

/// Every pair of tasks with owner `?`, given to one player with the rest of
/// the mission removed (played tricks kept). `part` (k, n): only every n-th
/// pair from k, for parallel workers. Progress: (pairs done, pairs of this
/// part). `None` if cancelled.
pub fn pair_conflicts(
    s: &Situation,
    part: (usize, usize),
    opts: &Options,
    progress: Option<&dyn Fn(usize, usize)>,
) -> Result<Option<Vec<PairConflict>>, SolveError> {
    let g = s.game().map_err(|e| SolveError::Replay(e.to_string()))?;
    let (n, captain) = (g.n, g.captain as usize);
    let open: Vec<usize> = (0..s.tasks.len()).filter(|&i| s.tasks[i].owner == Owner::Unknown).collect();
    let with = |tasks: &[usize], p: usize| {
        let mut x = s.clone();
        x.tasks = tasks.iter().map(|&i| crate::situation::Assignment { card: s.tasks[i].card, owner: Owner::Player(p as u8) }).collect();
        x
    };
    // None: cancelled.
    let feasible = |tasks: &[usize], p: usize| -> Result<Option<bool>, SolveError> {
        Ok(match solver::solve(&with(tasks, p), opts)?.0 {
            Verdict::Feasible(_) => Some(true),
            Verdict::Infeasible | Verdict::Redeal(_) => Some(false),
            _ => None,
        })
    };
    let may_take = |i: usize, p: usize| p != captain || s.tasks[i].card.captain_allowed();
    let pairs: Vec<(usize, usize)> = open
        .iter()
        .enumerate()
        .flat_map(|(k, &i)| open[k + 1..].iter().map(move |&j| (i, j)))
        .enumerate()
        .filter(|(k, _)| k % part.1.max(1) == part.0)
        .map(|(_, x)| x)
        .collect();
    // Possible alone, for the tasks of this part's pairs.
    let mut alone = vec![vec![false; n]; s.tasks.len()];
    for &i in open.iter().filter(|&&i| pairs.iter().any(|&(a, b)| a == i || b == i)) {
        for p in (0..n).filter(|&p| may_take(i, p)) {
            let Some(ok) = feasible(&[i], p)? else { return Ok(None) };
            alone[i][p] = ok;
        }
    }
    let mut out = Vec::new();
    for (k, &(i, j)) in pairs.iter().enumerate() {
        if let Some(f) = progress {
            f(k, pairs.len());
        }
        let tried: Vec<usize> = (0..n).filter(|&p| alone[i][p] && alone[j][p]).collect();
        let mut players = Vec::new();
        for &p in &tried {
            let Some(ok) = feasible(&[i, j], p)? else { return Ok(None) };
            if !ok {
                players.push(p);
            }
        }
        if !players.is_empty() {
            out.push(PairConflict { tasks: (i, j), players, tried });
        }
    }
    Ok(Some(out))
}

// ---------------------------------------------------------------- conflicts

/// Smallest set of tasks (indices into `s.tasks`) that cannot all be
/// fulfilled together from the deal, by deletion: each task in the result is
/// needed for the conflict. Played tricks are ignored, so this explains why
/// the mission was impossible from the start (use [`point_of_no_return`] for
/// games that became impossible during play). Requires every owner fixed.
/// `None` if feasible from the deal, or cancelled. One solve per task.
pub fn smallest_conflict(s: &Situation, cancel: Option<&AtomicBool>) -> Result<Option<Vec<usize>>, SolveError> {
    let opts = Options { cancel, ..Options::default() };
    let infeasible = |keep: &[usize]| -> Result<Option<bool>, SolveError> {
        let mut sub = s.clone();
        sub.played.clear();
        sub.tasks = keep.iter().map(|&i| s.tasks[i].clone()).collect();
        Ok(match solver::solve(&sub, &opts)?.0 {
            Verdict::Infeasible => Some(true),
            Verdict::Cancelled => None,
            _ => Some(false),
        })
    };
    let mut keep: Vec<usize> = (0..s.tasks.len()).collect();
    match infeasible(&keep)? {
        Some(true) => {}
        _ => return Ok(None),
    }
    let mut i = 0;
    while i < keep.len() {
        let mut without = keep.clone();
        without.remove(i);
        match infeasible(&without)? {
            Some(true) => keep = without,
            Some(false) => i += 1,
            None => return Ok(None),
        }
    }
    Ok(Some(keep))
}

// ---------------------------------------------------------------- point of no return

/// Where a played game became unwinnable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoReturn {
    /// The mission was never winnable from the deal.
    FromTheStart,
    /// Still winnable after the last card entered.
    StillWinnable,
    /// Winnable before this card, lost after it.
    At {
        /// 0-based trick index.
        trick: usize,
        player: usize,
        card: Card,
        /// Legal cards at that moment that would have kept a winning line.
        alternatives: Vec<Card>,
    },
}

/// Replays the entered tricks and finds the first card after which no winning
/// line exists. Uses binary search: along the played line, winnability can
/// only switch from yes to no once. Requires every owner fixed.
pub fn point_of_no_return(s: &Situation, cancel: Option<&AtomicBool>) -> Result<Option<NoReturn>, SolveError> {
    let (_, tasks) = solver::prepare(s)?;
    let cards: Vec<Card> = s.played.iter().flat_map(|t| t.cards.iter().copied()).collect();
    let opts = Options { cancel, ..Options::default() };
    let start = crate::game::Game::new(&s.hands).map_err(|e| SolveError::Replay(e.to_string()))?;
    let at = |k: usize| {
        let mut g = start.clone();
        for &c in &cards[..k] {
            g.play(c);
        }
        g
    };
    let winnable = |k: usize| -> Option<bool> {
        match solver::solve_game(at(k), &tasks, &opts).0 {
            Verdict::Feasible(_) => Some(true),
            Verdict::Infeasible => Some(false),
            _ => None,
        }
    };
    let Some(first) = winnable(0) else { return Ok(None) };
    if !first {
        return Ok(Some(NoReturn::FromTheStart));
    }
    let Some(last) = winnable(cards.len()) else { return Ok(None) };
    if last {
        return Ok(Some(NoReturn::StillWinnable));
    }
    // Invariant: winnable(lo) && !winnable(hi).
    let (mut lo, mut hi) = (0, cards.len());
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        match winnable(mid) {
            Some(true) => lo = mid,
            Some(false) => hi = mid,
            None => return Ok(None),
        }
    }
    // The card at index `lo` lost the game.
    let g = at(lo);
    let player = g.to_play();
    let trick = g.tricks.len();
    let mut alternatives = Vec::new();
    for c in g.legal_moves().iter() {
        if c == cards[lo] {
            continue;
        }
        let mut h = g.clone();
        h.play(c);
        match solver::solve_game(h, &tasks, &opts).0 {
            Verdict::Feasible(_) => alternatives.push(c),
            Verdict::Cancelled => return Ok(None),
            _ => {}
        }
    }
    Ok(Some(NoReturn::At { trick, player, card: cards[lo], alternatives }))
}

// ---------------------------------------------------------------- difficulty

/// How forgiving a winning line is.
#[derive(Debug, Clone, PartialEq)]
pub struct Tightness {
    /// Moments along the winning line where some legal card would lose the mission.
    pub critical: usize,
    /// Moments with more than one legal card.
    pub choices: usize,
    /// Average share of losing cards over the moments with a choice (0..1).
    pub tightness: f64,
}

/// Walks the given winning line from the situation's position; at every card
/// with a choice, tests each legal alternative. Requires every owner fixed.
pub fn tightness(s: &Situation, line: &[Card], cancel: Option<&AtomicBool>) -> Result<Option<Tightness>, SolveError> {
    let (mut g, tasks) = solver::prepare(s)?;
    let opts = Options { cancel, ..Options::default() };
    let (mut critical, mut choices, mut share_sum) = (0, 0, 0.0);
    for &card in line {
        let legal: Vec<Card> = g.legal_moves().iter().collect();
        if legal.len() > 1 {
            choices += 1;
            let mut losing = 0;
            for &c in &legal {
                if c == card {
                    continue;
                }
                let mut h = g.clone();
                h.play(c);
                match solver::solve_game(h, &tasks, &opts).0 {
                    Verdict::Infeasible => losing += 1,
                    Verdict::Cancelled => return Ok(None),
                    _ => {}
                }
            }
            if losing > 0 {
                critical += 1;
            }
            share_sum += losing as f64 / legal.len() as f64;
        }
        g.play(card);
    }
    let tightness = if choices > 0 { share_sum / choices as f64 } else { 0.0 };
    Ok(Some(Tightness { critical, choices, tightness }))
}

/// Share of random deals on which these tasks can be fulfilled with some
/// legal draft (one search over all drafts per deal). A first pass gives each
/// deal up to `opts.node_limit` nodes; with `settle`, the deals left open are
/// then searched to the end. `part` (k, n): only every n-th deal from k, for
/// parallel workers. Progress: (deals done, deals of this part). Returns
/// (solvable, still open, deals of this part); `None` if cancelled in the
/// first pass.
#[allow(clippy::too_many_arguments)]
pub fn deal_robustness(
    cards: &[&'static crate::catalogue::TaskCard],
    players: usize,
    deals: usize,
    settle: bool,
    seed: u64,
    part: (usize, usize),
    opts: &Options,
    progress: Option<&dyn Fn(usize, usize)>,
) -> Option<(usize, usize, usize)> {
    use crate::game::Game;
    use crate::situation::Assignment;
    let mut rng = Rng::new(seed);
    let (part, parts) = (part.0, part.1.max(1));
    let mine = (0..deals).filter(|d| d % parts == part).count();
    let mut ok = 0;
    let mut open = Vec::new();
    let mut done = 0;
    for d in 0..deals {
        // Every part draws all deals (same sequence) and solves its share.
        let hands = Game::random_hands(players, &mut rng);
        if d % parts != part {
            continue;
        }
        if let Some(f) = progress {
            f(done, mine);
        }
        done += 1;
        let s = Situation {
            hands,
            tasks: cards.iter().map(|&card| Assignment { card, owner: Owner::Unknown }).collect(),
            ..Situation::default()
        };
        match solver::solve_drafts(&s, opts) {
            Ok((Verdict::Feasible(_), _, _)) => ok += 1,
            Ok((Verdict::Cancelled, _, _)) => return None,
            Ok((Verdict::Undecided, _, _)) => open.push(s),
            _ => {}
        }
    }
    if settle {
        while let Some(s) = open.pop() {
            if let Some(f) = progress {
                f(mine - open.len() - 1, mine);
            }
            match solver::solve_drafts(&s, &Options { node_limit: None, ..opts.clone() }) {
                Ok((Verdict::Feasible(_), _, _)) => ok += 1,
                Ok((Verdict::Cancelled, _, _)) => {
                    open.push(s);
                    break;
                }
                _ => {}
            }
        }
    }
    Some((ok, open.len(), mine))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notation;

    fn situation(tasks: &str) -> Situation {
        notation::parse(&format!(
            "crew 1
players: 4
Player1: P2 P8 B4 B9 G1 G6 Y3 Y7 S2 S4
Player2: P1 P5 B1 B2 B6 G4 G9 Y2 Y6 S1
Player3: P3 P6 B3 B8 G2 G5 G8 Y1 Y8 S3
Player4: rest
tasks:
{tasks}"
        ))
        .unwrap()
    }

    #[test]
    fn exhaustive_draft_search_counts_and_finds_closest_fix() {
        let s = situation("?: win B4\n?: win P9 Y8\n?: first-trick\n?: tricks =2");
        let reference: Vec<Owner> = [1, 3, 0, 2].iter().map(|&p| Owner::Player(p)).collect();
        let r = draft_search(&s, &DraftOptions { reference: Some(reference.clone()), ..Default::default() }).unwrap();
        assert_eq!(r.total, 24);
        assert_eq!(r.checked, 24);
        assert!(!r.sampled);
        assert!(r.working > 0 && r.working <= 24);
        assert_eq!(r.any_works, Some(true));
        let (owners, line) = r.example.unwrap();
        let mut d = s.clone();
        for (a, o) in d.tasks.iter_mut().zip(&owners) {
            a.owner = *o;
        }
        // The example's line must really win.
        let (mut g, tasks) = solver::prepare(&d).unwrap();
        for c in line {
            g.play(c);
        }
        assert_eq!(solver::status(&g, &tasks), crate::task::Status::Done);
        // The closest draft's own line must win for that draft.
        let (owners, dist, line) = r.closest.unwrap();
        assert!(dist <= 4);
        let mut d = s.clone();
        for (a, o) in d.tasks.iter_mut().zip(&owners) {
            a.owner = *o;
        }
        let (mut g, tasks) = solver::prepare(&d).unwrap();
        for c in line {
            g.play(c);
        }
        assert_eq!(solver::status(&g, &tasks), crate::task::Status::Done);
    }

    #[test]
    fn impossible_pairs_are_reported() {
        // Only Player1 (S4) or Player3 (S3 itself) can win S3.
        let s = situation("?: win S3");
        let r = draft_search(&s, &DraftOptions::default()).unwrap();
        let mut players: Vec<usize> = r.impossible_pairs.iter().map(|&(_, p)| p).collect();
        players.sort();
        assert_eq!(players, vec![1, 3]);
        assert_eq!(r.any_works, Some(true));
    }

    #[test]
    fn draft_relations_compare_working_with_all_drafts() {
        let p = |i: u8| Owner::Player(i);
        let line = vec![Card::parse("B1").unwrap()];
        let mut rows = Vec::new();
        for a in 0..2 {
            for b in 0..2 {
                for c in 0..2 {
                    let works = a == b && c == 1;
                    rows.push((vec![p(a), p(b), p(c), Owner::Crew], if works { Decision::Works(line.clone()) } else { Decision::Fails }));
                }
            }
        }
        let r = draft_relations(&rows);
        assert_eq!(r.must, vec![(2, p(1))]);
        assert_eq!(r.never, vec![(2, p(0))]);
        assert_eq!(r.always_together, vec![(0, 1)]);
        assert!(r.never_together.is_empty() && r.tendencies.is_empty());
        // Open drafts are left out; rules that never allow a pair are not reported.
        let ruled: Vec<_> = rows.iter().filter(|(o, _)| o[0] != o[1]).map(|(o, _)| (o.clone(), Decision::Works(line.clone()))).collect();
        let mut with_open = ruled.clone();
        with_open.push((vec![p(0), p(0), p(0), Owner::Crew], Decision::Open));
        assert_eq!(draft_relations(&with_open), draft_relations(&ruled));
        assert!(draft_relations(&ruled).never_together.is_empty());
    }

    #[test]
    fn pair_conflicts_find_tasks_that_cannot_share_a_player() {
        // Winning B4 or Y1 needs a trick; "win no tricks" allows none.
        let s = situation("?: win B4\n?: tricks =0\n?: win Y1");
        let all = pair_conflicts(&s, (0, 1), &Options::default(), None).unwrap().unwrap();
        let pairs: Vec<(usize, usize)> = all.iter().map(|c| c.tasks).collect();
        assert!(pairs.contains(&(0, 1)) && pairs.contains(&(1, 2)), "{all:?}");
        for c in all.iter().filter(|c| c.tasks.1 == 1 || c.tasks.0 == 1) {
            assert!(!c.tried.is_empty());
            assert_eq!(c.players, c.tried, "fails for every player who could take both");
        }
        // Parts add up to the whole.
        let mut parts: Vec<PairConflict> =
            (0..2).flat_map(|k| pair_conflicts(&s, (k, 2), &Options::default(), None).unwrap().unwrap()).collect();
        parts.sort_by_key(|c| c.tasks);
        assert_eq!(parts, all);
    }

    #[test]
    fn smallest_conflict_finds_the_two_clashing_tasks() {
        // Player2 must win trick 1 and Player3 must win the first 2 tricks: clash.
        let s = situation("Player2: win B4\nPlayer2: first-trick\nPlayer3: first 2\nPlayer4: no pink");
        let c = smallest_conflict(&s, None).unwrap().unwrap();
        let ids: Vec<&str> = c.iter().map(|&i| s.tasks[i].card.id.as_str()).collect();
        assert_eq!(ids, vec!["first-trick", "first 2"]);
        let ok = situation("Player2: win B4");
        assert_eq!(smallest_conflict(&ok, None).unwrap(), None);
    }

    #[test]
    fn point_of_no_return_on_a_played_line() {
        // Player2 must win B4 (held by Player1). Trick 1: Player1 leads B4 and
        // Player2 plays low, Player3 B3, Player4 B5 wins B4: lost at Player2's B1.
        let mut s = situation("Player2: win B4");
        s.played = vec![crate::situation::PlayedTrick {
            leader: 0,
            cards: ["B4", "B1", "B3", "B5"].iter().map(|c| Card::parse(c).unwrap()).collect(),
        }];
        match point_of_no_return(&s, None).unwrap().unwrap() {
            NoReturn::At { trick, player, card, alternatives } => {
                assert_eq!((trick, player, card.to_string()), (0, 1, "B1".to_string()));
                // B6 would have beaten B5; B2 would not.
                let alts: Vec<String> = alternatives.iter().map(|c| c.to_string()).collect();
                assert_eq!(alts, vec!["B6"]);
            }
            other => panic!("unexpected {other:?}"),
        }
        let fresh = situation("Player2: win B4");
        assert_eq!(point_of_no_return(&fresh, None).unwrap(), Some(NoReturn::StillWinnable));
        let never = situation("Player2: win S3");
        assert_eq!(point_of_no_return(&never, None).unwrap(), Some(NoReturn::FromTheStart));
    }

    #[test]
    fn tightness_counts_critical_moments() {
        let s = situation("Player2: win B4");
        let Verdict::Feasible(line) = solver::solve(&s, &Options::default()).unwrap().0 else { panic!() };
        let t = tightness(&s, &line, None).unwrap().unwrap();
        assert!(t.choices > 0);
        assert!(t.critical >= 1 && t.critical <= t.choices);
        assert!(t.tightness > 0.0 && t.tightness < 1.0);
    }

    #[test]
    fn deal_robustness_of_an_easy_and_an_impossible_set() {
        let easy = [crate::catalogue::find("first-trick").unwrap()];
        assert_eq!(deal_robustness(&easy, 4, 10, true, 1, (0, 1), &Options::default(), None), Some((10, 0, 10)));
        let clash = [crate::catalogue::find("first-trick").unwrap(), crate::catalogue::find("first 2").unwrap()];
        // Same player may take both only with 2 tasks per seat; with 4 players each takes one.
        assert_eq!(deal_robustness(&clash, 4, 5, true, 1, (0, 1), &Options::default(), None), Some((0, 0, 5)));
    }

    #[test]
    fn sampled_search_estimates() {
        let s = situation("?: win B4\n?: win P9 Y8\n?: first-trick\n?: tricks =2");
        let r = draft_search(&s, &DraftOptions { limit: 10, ..Default::default() }).unwrap();
        assert!(r.sampled);
        assert_eq!(r.checked, 10);
        assert_eq!(r.any_works, Some(true));
    }

    #[test]
    fn draft_count_matches_each_draft_solved_alone() {
        let s = situation("?: win B4\n?: win P9 Y8\n?: first-trick\n?: tricks =2");
        let r = draft_search(&s, &DraftOptions::default()).unwrap();
        let g = s.game().unwrap();
        let mut working = 0;
        draft::for_each_draft(&s.tasks, g.n, g.captain as usize, |owners| {
            let mut d = s.clone();
            for (a, o) in d.tasks.iter_mut().zip(owners) {
                a.owner = *o;
            }
            working += matches!(solver::solve(&d, &Options::plain()).unwrap().0, Verdict::Feasible(_)) as u64;
            true
        });
        assert_eq!((r.working, r.undecided, r.checked), (working, 0, r.total));
    }

    #[test]
    fn parts_add_up_to_the_whole() {
        let s = situation("?: win B4\n?: win P9 Y8\n?: first-trick\n?: tricks =2");
        let whole = draft_search(&s, &DraftOptions::default()).unwrap();
        let parts: Vec<DraftReport> =
            (0..3).map(|k| draft_search(&s, &DraftOptions { part: (k, 3), ..Default::default() }).unwrap()).collect();
        assert_eq!(parts.iter().map(|r| r.checked).sum::<u64>(), whole.checked);
        assert_eq!(parts.iter().map(|r| r.working).sum::<u64>(), whole.working);
        assert_eq!(parts[0].any_works, whole.any_works);
        assert!(parts[1].any_works.is_none());

        let cards = [crate::catalogue::find("win B4").unwrap(), crate::catalogue::find("first 2").unwrap()];
        let all = deal_robustness(&cards, 4, 12, true, 5, (0, 1), &Options::default(), None).unwrap();
        let split: Vec<_> = (0..3).map(|k| deal_robustness(&cards, 4, 12, true, 5, (k, 3), &Options::default(), None).unwrap()).collect();
        assert_eq!(split.iter().map(|r| r.0).sum::<usize>(), all.0);
        assert_eq!(split.iter().map(|r| r.2).sum::<usize>(), all.2);
    }
}
