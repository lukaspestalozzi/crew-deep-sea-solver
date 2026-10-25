//! Analyses built on the solver: draft search.

use crate::card::Card;
use crate::draft;
use crate::rng::Rng;
use crate::situation::{Owner, Situation};
use crate::solver::{self, Options, SolveError, Verdict};
use std::collections::HashSet;
use std::sync::atomic::AtomicBool;

/// Result of searching over drafts.
#[derive(Debug, Clone)]
pub struct DraftReport {
    /// Number of legal drafts.
    pub total: u64,
    /// Drafts actually solved.
    pub checked: u64,
    /// Feasible drafts among those checked.
    pub working: u64,
    /// Drafts checked without an answer (only when not settled, or cancelled).
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
}

#[derive(Clone)]
pub struct DraftOptions<'a> {
    /// Solve every draft if there are at most this many, else sample this many.
    pub limit: u64,
    pub seed: u64,
    /// Owners to compare against for the smallest fix (e.g. the draft entered).
    pub reference: Option<Vec<Owner>>,
    pub cancel: Option<&'a AtomicBool>,
    /// Called with (drafts checked, drafts planned, working so far, drafts
    /// still open): after each draft of the first pass, then while the open
    /// ones are settled.
    pub progress: Option<&'a dyn Fn(u64, u64, u64, u64)>,
    /// Nodes per draft in the first pass; drafts that reach it stay open.
    pub node_limit: Option<u64>,
    /// After the first pass, search the open drafts to the end (exact count).
    pub settle: bool,
}

impl Default for DraftOptions<'_> {
    fn default() -> Self {
        DraftOptions {
            limit: 2000,
            seed: 1,
            reference: None,
            cancel: None,
            progress: None,
            node_limit: Some(1_000_000),
            settle: true,
        }
    }
}

fn distance(a: &[Owner], b: &[Owner]) -> usize {
    a.iter().zip(b).filter(|(x, y)| x != y).count()
}

/// Search the drafts for the tasks with owner `?` (fixed owners stay): the
/// exact yes/no over all drafts in one search, then each draft counted on its
/// own (up to `node_limit` nodes each).
pub fn draft_search(s: &Situation, opts: &DraftOptions) -> Result<DraftReport, SolveError> {
    let g = s.game().map_err(|e| SolveError::Replay(e.to_string()))?;
    let (n, captain) = (g.n, g.captain as usize);
    let solve_opts = Options { cancel: opts.cancel, node_limit: opts.node_limit, ..Options::default() };

    let (any_works, one_tree_example) = match solver::solve_drafts(s, &Options { cancel: opts.cancel, ..Options::default() })? {
        (Verdict::Feasible(line), Some(owners), _) => (Some(true), Some((owners, line))),
        (Verdict::Infeasible | Verdict::Redeal(_), _, _) => (Some(false), None),
        _ => (None, None),
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
    };

    // Solve one draft (up to `limit` nodes); None if cancelled. A draft that
    // reaches the limit is kept in `open` for the second pass.
    let planned = total.min(opts.limit);
    let solve_draft = |owners: &[Owner], limit: Option<u64>, report: &mut DraftReport, open: &mut Vec<Vec<Owner>>| -> Option<()> {
        let mut d = s.clone();
        for (a, o) in d.tasks.iter_mut().zip(owners) {
            a.owner = *o;
        }
        match solver::solve(&d, &Options { node_limit: limit, ..solve_opts.clone() }) {
            Ok((Verdict::Feasible(line), _)) => {
                report.working += 1;
                if let Some(r) = &opts.reference {
                    let dist = distance(owners, r);
                    if report.closest.as_ref().is_none_or(|c| dist < c.1) {
                        report.closest = Some((owners.to_vec(), dist, line.clone()));
                    }
                }
                if report.example.is_none() {
                    report.example = Some((owners.to_vec(), line));
                }
            }
            Ok((Verdict::Cancelled, _)) => return None,
            Ok((Verdict::Undecided, _)) => open.push(owners.to_vec()),
            _ => {}
        }
        Some(())
    };
    let tell = |r: &DraftReport, open: usize| {
        if let Some(p) = opts.progress {
            p(r.checked, planned, r.working, open as u64);
        }
    };
    let bad = |owners: &[Owner]| {
        owners.iter().enumerate().any(|(i, o)| matches!(o, Owner::Player(p) if s.tasks[i].owner == Owner::Unknown && !allowed(i, *p as usize)))
    };

    // First pass, with the node limit: every draft, or a sample.
    let mut open: Vec<Vec<Owner>> = Vec::new();
    let mut cancelled = false;
    if total <= opts.limit {
        // Exhaustive: drafts with an impossible pair are failures without solving.
        draft::for_each_draft(&s.tasks, n, captain, |owners| {
            report.checked += 1;
            if !bad(owners) && solve_draft(owners, opts.node_limit, &mut report, &mut open).is_none() {
                cancelled = true;
                return false;
            }
            tell(&report, open.len());
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
            if !seen.insert(owners.clone()) {
                continue;
            }
            report.checked += 1;
            if !bad(&owners) && solve_draft(&owners, opts.node_limit, &mut report, &mut open).is_none() {
                cancelled = true;
                break;
            }
            tell(&report, open.len());
        }
    }

    // Second pass: the open drafts, to the end.
    if opts.settle && !cancelled {
        let mut rest: Vec<Vec<Owner>> = Vec::new();
        while let Some(owners) = open.pop() {
            tell(&report, open.len() + 1);
            if solve_draft(&owners, None, &mut report, &mut rest).is_none() {
                open.push(owners);
                break;
            }
        }
        open.extend(rest);
    }
    report.undecided = open.len() as u64;
    if report.example.is_none() {
        report.example = one_tree_example;
    }
    Ok(report)
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
/// deal up to `node_limit` nodes; with `settle`, the deals left open are then
/// searched to the end. Progress: (deals done, deals). Returns (solvable,
/// still open, deals); `None` if cancelled in the first pass.
#[allow(clippy::too_many_arguments)]
pub fn deal_robustness(
    cards: &[&'static crate::catalogue::TaskCard],
    players: usize,
    deals: usize,
    node_limit: Option<u64>,
    settle: bool,
    seed: u64,
    cancel: Option<&AtomicBool>,
    progress: Option<&dyn Fn(usize, usize)>,
) -> Option<(usize, usize, usize)> {
    use crate::game::Game;
    use crate::situation::Assignment;
    let mut rng = Rng::new(seed);
    let mut ok = 0;
    let mut open = Vec::new();
    for d in 0..deals {
        if let Some(f) = progress {
            f(d.saturating_sub(open.len()), deals);
        }
        let s = Situation {
            hands: Game::random_hands(players, &mut rng),
            tasks: cards.iter().map(|&card| Assignment { card, owner: Owner::Unknown }).collect(),
            ..Situation::default()
        };
        match solver::solve_drafts(&s, &Options { cancel, node_limit, ..Options::default() }) {
            Ok((Verdict::Feasible(_), _, _)) => ok += 1,
            Ok((Verdict::Cancelled, _, _)) => return None,
            Ok((Verdict::Undecided, _, _)) => open.push(s),
            _ => {}
        }
    }
    if settle {
        while let Some(s) = open.pop() {
            if let Some(f) = progress {
                f(deals - open.len() - 1, deals);
            }
            match solver::solve_drafts(&s, &Options { cancel, ..Options::default() }) {
                Ok((Verdict::Feasible(_), _, _)) => ok += 1,
                Ok((Verdict::Cancelled, _, _)) => {
                    open.push(s);
                    break;
                }
                _ => {}
            }
        }
    }
    Some((ok, open.len(), deals))
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
        assert_eq!(deal_robustness(&easy, 4, 10, None, true, 1, None, None), Some((10, 0, 10)));
        let clash = [crate::catalogue::find("first-trick").unwrap(), crate::catalogue::find("first 2").unwrap()];
        // Same player may take both only with 2 tasks per seat; with 4 players each takes one.
        assert_eq!(deal_robustness(&clash, 4, 5, None, true, 1, None, None), Some((0, 0, 5)));
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
    fn settling_open_drafts_gives_the_exact_count() {
        let s = situation("?: win B4\n?: win P9 Y8\n?: first-trick\n?: tricks =2");
        let exact = draft_search(&s, &DraftOptions { node_limit: None, ..Default::default() }).unwrap();
        // A tiny first-pass limit leaves most drafts open; settling must close all of them.
        let settled = draft_search(&s, &DraftOptions { node_limit: Some(50), ..Default::default() }).unwrap();
        assert_eq!((settled.working, settled.undecided, settled.checked), (exact.working, 0, exact.checked));
        let open = draft_search(&s, &DraftOptions { node_limit: Some(50), settle: false, ..Default::default() }).unwrap();
        assert!(open.undecided > 0 && open.working + open.undecided >= exact.working);
    }
}
