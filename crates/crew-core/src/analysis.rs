//! Analyses built on the solver: draft search.

use crate::card::Card;
use crate::draft;
use crate::rng::Rng;
use crate::situation::{Owner, Situation};
use crate::solver::{self, Options, SolveError, Verdict};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};

/// Result of searching over drafts.
#[derive(Debug, Clone)]
pub struct DraftReport {
    /// Number of legal drafts.
    pub total: u64,
    /// Drafts actually solved.
    pub checked: u64,
    /// Feasible drafts among those checked.
    pub working: u64,
    /// True if `checked < total` because drafts were sampled.
    pub sampled: bool,
    /// Exact answer to "does any legal draft work?"; `None` if cancelled.
    pub any_works: Option<bool>,
    /// A working draft and its winning line.
    pub example: Option<(Vec<Owner>, Vec<Card>)>,
    /// The working draft closest to `reference`, how many tasks change owner,
    /// and its winning line.
    pub closest: Option<(Vec<Owner>, usize, Vec<Card>)>,
    /// (task index, player) pairs ruled out because that task alone is impossible for that player.
    pub impossible_pairs: Vec<(usize, usize)>,
}

#[derive(Debug, Clone)]
pub struct DraftOptions<'a> {
    /// Solve every draft if there are at most this many, else sample this many.
    pub limit: u64,
    pub seed: u64,
    /// Owners to compare against for the smallest fix (e.g. the draft entered).
    pub reference: Option<Vec<Owner>>,
    pub cancel: Option<&'a AtomicBool>,
}

impl Default for DraftOptions<'_> {
    fn default() -> Self {
        DraftOptions { limit: 2000, seed: 1, reference: None, cancel: None }
    }
}

fn distance(a: &[Owner], b: &[Owner]) -> usize {
    a.iter().zip(b).filter(|(x, y)| x != y).count()
}

/// Search the drafts for the tasks with owner `?` (fixed owners stay).
pub fn draft_search(s: &Situation, opts: &DraftOptions) -> Result<DraftReport, SolveError> {
    let g = s.game().map_err(|e| SolveError::Replay(e.to_string()))?;
    let (n, captain) = (g.n, g.captain as usize);
    let cancelled = || opts.cancel.is_some_and(|c| c.load(Ordering::Relaxed));
    let solve_opts = Options { cancel: opts.cancel, ..Options::default() };

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
        sampled: false,
        any_works: None,
        example: None,
        closest: None,
        impossible_pairs,
    };

    // Solve one draft; returns None if cancelled.
    let solve_draft = |owners: &[Owner], report: &mut DraftReport| -> Option<bool> {
        let mut d = s.clone();
        for (a, o) in d.tasks.iter_mut().zip(owners) {
            a.owner = *o;
        }
        report.checked += 1;
        match solver::solve(&d, &solve_opts) {
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
                Some(true)
            }
            Ok((Verdict::Cancelled, _)) => None,
            _ => Some(false),
        }
    };

    if total <= opts.limit {
        // Exhaustive: drafts with an impossible pair are failures without solving.
        let mut cancelled_flag = false;
        let mut visited = 0u64;
        draft::for_each_draft(&s.tasks, n, captain, |owners| {
            visited += 1;
            let has_bad = owners.iter().enumerate().any(|(i, o)| matches!(o, Owner::Player(p) if s.tasks[i].owner == Owner::Unknown && !allowed(i, *p as usize)));
            if has_bad {
                report.checked += 1;
                return true;
            }
            match solve_draft(owners, &mut report) {
                None => {
                    cancelled_flag = true;
                    false
                }
                Some(_) => true,
            }
        });
        if !cancelled_flag {
            report.any_works = Some(report.working > 0);
        }
        return Ok(report);
    }

    // Sampled: estimate the share of working drafts.
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
        let has_bad = owners.iter().enumerate().any(|(i, o)| matches!(o, Owner::Player(p) if s.tasks[i].owner == Owner::Unknown && !allowed(i, *p as usize)));
        if has_bad {
            report.checked += 1;
            continue;
        }
        if solve_draft(&owners, &mut report).is_none() {
            return Ok(report);
        }
    }
    if report.working > 0 {
        report.any_works = Some(true);
        return Ok(report);
    }
    // No sampled draft works: settle the yes/no question exactly, skipping
    // drafts with impossible pairs.
    let mut found = Some(false);
    draft::for_each_draft_filtered(&s.tasks, n, captain, &allowed, |owners| {
        if cancelled() {
            found = None;
            return false;
        }
        let mut probe = DraftReport { ..report.clone() };
        match solve_draft(owners, &mut probe) {
            Some(true) => {
                report.example = probe.example;
                report.closest = probe.closest;
                found = Some(true);
                false
            }
            Some(false) => true,
            None => {
                found = None;
                false
            }
        }
    });
    report.any_works = found;
    Ok(report)
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
    fn sampled_search_estimates() {
        let s = situation("?: win B4\n?: win P9 Y8\n?: first-trick\n?: tricks =2");
        let r = draft_search(&s, &DraftOptions { limit: 10, ..Default::default() }).unwrap();
        assert!(r.sampled);
        assert_eq!(r.checked, 10);
        assert_eq!(r.any_works, Some(true));
    }
}
