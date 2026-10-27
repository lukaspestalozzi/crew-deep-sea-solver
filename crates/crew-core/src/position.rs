//! One position in play (a situation with owners fixed and some cards
//! played), for the Position screen and for bots: who is to play, what each
//! move order would play, which moves still win, and how a move order fares
//! when it plays on by itself.

use crate::card::Card;
use crate::decider::{MoveDecider, Position, DECK_ORDER, LOOKAHEAD, TASK_AWARE};
use crate::game::Game;
use crate::situation::{PlayedTrick, Situation};
use crate::solver::{self, Options, Resolved, SolveError, Verdict};
use crate::task::Status;
use std::sync::atomic::Ordering;

/// The move orders compared (name, decider), in this order everywhere.
pub const ORDERS: [(&str, &dyn MoveDecider); 4] = [
    ("deck order", &DECK_ORDER),
    ("task-aware", &TASK_AWARE),
    ("lookahead", &LOOKAHEAD),
    ("lookahead all", &crate::decider::LOOKAHEAD_ALL),
];

/// The situation after the player to move plays `card`.
pub fn after(s: &Situation, card: Card) -> Result<Situation, SolveError> {
    let g = s.game().map_err(|e| SolveError::Replay(e.to_string()))?;
    if !g.legal_moves().contains(card) {
        return Err(SolveError::Replay(format!("{card} is not a legal move for Player{}", g.to_play() + 1)));
    }
    let mut next = s.clone();
    match next.played.last_mut() {
        Some(t) if t.cards.len() < g.n => t.cards.push(card),
        _ => next.played.push(PlayedTrick { leader: g.leader, cards: vec![card] }),
    }
    Ok(next)
}

/// The game at the last trick boundary (the trick in progress taken back).
fn boundary(g: &Game) -> Game {
    let mut b = g.clone();
    while !b.current.is_empty() {
        b.undo();
    }
    b
}

/// Each task's status at the last trick boundary.
pub fn task_status(g: &Game, tasks: &Resolved) -> Vec<Status> {
    let b = boundary(g);
    tasks.iter().map(|&(k, o)| k.eval(&b, o)).collect()
}

/// The legal moves as each of [`ORDERS`] ranks them, best first.
pub fn rankings(g: &Game, tasks: &Resolved) -> Vec<Vec<Card>> {
    let b = boundary(g);
    let kinds: Vec<_> = tasks.iter().map(|&(k, _)| k).collect();
    let (mut alive, mut done) = (0u128, 0u128);
    for (t, &(k, o)) in tasks.iter().enumerate() {
        let bit = 1u128 << (5 * t + o);
        match k.eval(&b, o) {
            Status::Failed => {}
            Status::Done => {
                alive |= bit;
                done |= bit;
            }
            Status::Open => alive |= bit,
        }
    }
    let pos = Position { game: g, kinds: &kinds, alive, done };
    ORDERS
        .iter()
        .map(|(_, d)| {
            let mut moves: Vec<Card> = g.legal_moves().iter().collect();
            d.order(&pos, &mut moves);
            moves
        })
        .collect()
}

/// Solve the position after `card`.
pub fn solve_move(s: &Situation, card: Card, opts: &Options) -> Result<(Verdict, u64), SolveError> {
    let next = after(s, card)?;
    let label = format!("move {card}");
    let (v, stats) = solver::solve(&next, &opts.labelled(&label))?;
    Ok((v, stats.nodes))
}

/// What happened when a move order played on by itself.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayOut {
    /// The cards it played, from the position on.
    pub line: Vec<Card>,
    /// Could the tasks still be fulfilled at the start? `None`: undecided.
    pub winnable: Option<bool>,
    /// Index in `line` of the move after which they no longer could.
    pub lost_at: Option<usize>,
    /// The moves that would have kept them possible there.
    pub alternatives: Vec<Card>,
    /// Some check after a move stayed undecided (node limit).
    pub undecided: bool,
    /// Task status at the end of the line.
    pub status: Vec<Status>,
}

/// Let move order `order` (index into [`ORDERS`]) play every seat from the
/// position to the end, checking after each move whether the tasks can still
/// be fulfilled (until the first move after which they cannot). Each choice
/// is a [`crate::trace::Event::Decision`]; `on_move` gets (index, card,
/// still winnable) as it goes. `None` if cancelled.
pub fn play_out(
    s: &Situation,
    order: usize,
    opts: &Options,
    on_move: Option<&dyn Fn(usize, Card, Option<bool>)>,
) -> Result<Option<PlayOut>, SolveError> {
    let cancelled = || opts.cancel.is_some_and(|c| c.load(Ordering::Relaxed));
    let (_, tasks) = solver::prepare(s)?;
    let verdict = |x: &Situation, label: &str| -> Result<Option<bool>, SolveError> {
        Ok(match solver::solve(x, &opts.labelled(label))?.0 {
            Verdict::Feasible(_) => Some(true),
            Verdict::Infeasible | Verdict::Redeal(_) => Some(false),
            _ => None,
        })
    };
    let winnable = verdict(s, "start")?;
    if cancelled() {
        return Ok(None);
    }
    let mut out = PlayOut { line: Vec::new(), winnable, lost_at: None, alternatives: Vec::new(), undecided: winnable.is_none(), status: Vec::new() };
    let mut still = winnable;
    let mut cur = s.clone();
    loop {
        let g = cur.game().map_err(|e| SolveError::Replay(e.to_string()))?;
        if g.is_over() {
            out.status = task_status(&g, &tasks);
            return Ok(Some(out));
        }
        let ranked = rankings(&g, &tasks).swap_remove(order);
        let card = ranked[0];
        let next = after(&cur, card)?;
        let i = out.line.len();
        // Check while it was still winnable (and decided).
        let after_v = if still == Some(true) {
            let label = format!("after move {} {card}", i + 1);
            let v = verdict(&next, &label)?;
            if cancelled() {
                return Ok(None);
            }
            match v {
                Some(false) => {
                    out.lost_at = Some(i);
                    for m in g.legal_moves().iter().filter(|&m| m != card) {
                        let label = format!("instead of {card}: {m}");
                        if verdict(&after(&cur, m)?, &label)? == Some(true) {
                            out.alternatives.push(m);
                        }
                    }
                    if cancelled() {
                        return Ok(None);
                    }
                }
                None => out.undecided = true,
                Some(true) => {}
            }
            still = v;
            match v {
                Some(true) => "feasible",
                Some(false) => "infeasible",
                None => "undecided",
            }
        } else {
            ""
        };
        if let Some(t) = opts.trace {
            let label = format!("{} · move {} · Player{}", ORDERS[order].0, i + 1, g.to_play() + 1);
            t.decision(&label, &cur, &ranked, card, after_v);
        }
        if let Some(f) = on_move {
            f(i, card, still);
        }
        out.line.push(card);
        cur = next;
    }
}

/// How the move orders do in one position: the legal moves, which of them
/// win, and per order the rank (1 = its first choice) of its first winning
/// move — `None` if no move wins, or a move ranked before it is undecided.
#[derive(Debug, Clone, PartialEq)]
pub struct PositionStats {
    pub legal: usize,
    pub winning: Vec<Card>,
    pub open: usize,
    pub first_win: Vec<Option<usize>>,
}

pub fn position_stats(s: &Situation, opts: &Options) -> Result<Option<PositionStats>, SolveError> {
    let (g, tasks) = solver::prepare(s)?;
    let legal: Vec<Card> = g.legal_moves().iter().collect();
    let mut win = Vec::new();
    let mut open = Vec::new();
    for &m in &legal {
        match solve_move(s, m, opts)?.0 {
            Verdict::Feasible(_) => win.push(m),
            Verdict::Cancelled => return Ok(None),
            Verdict::Undecided => open.push(m),
            _ => {}
        }
    }
    let first_win = rankings(&g, &tasks)
        .iter()
        .map(|r| {
            let k = r.iter().position(|m| win.contains(m))?;
            (!r[..k].iter().any(|m| open.contains(m))).then_some(k + 1)
        })
        .collect();
    Ok(Some(PositionStats { legal: legal.len(), winning: win, open: open.len(), first_win }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notation;

    fn situation(tasks: &str) -> Situation {
        notation::parse(&format!(
            "crew 1\nplayers: 4\nPlayer1: P2 P8 B4 B9 G1 G6 Y3 Y7 S2 S4\nPlayer2: P1 P5 B1 B2 B6 G4 G9 Y2 Y6 S1\nPlayer3: P3 P6 B3 B8 G2 G5 G8 Y1 Y8 S3\nPlayer4: rest\ntasks:\n{tasks}\n"
        ))
        .unwrap()
    }

    #[test]
    fn after_appends_to_the_trick_in_progress() {
        let s = situation("Player2: win B4");
        let g = s.game().unwrap();
        let lead = g.legal_moves().iter().next().unwrap();
        let one = after(&s, lead).unwrap();
        assert_eq!(one.played.len(), 1);
        assert_eq!(one.played[0].cards, vec![lead]);
        let second = one.game().unwrap().legal_moves().iter().next().unwrap();
        let two = after(&one, second).unwrap();
        assert_eq!(two.played.len(), 1);
        assert_eq!(two.played[0].cards.len(), 2);
        assert!(after(&s, Card::parse("S1").unwrap()).is_err(), "Player1 does not hold S1");
    }

    #[test]
    fn rankings_are_permutations_of_the_legal_moves() {
        let s = situation("Player2: win B4\nPlayer3: tricks =2");
        let (g, tasks) = solver::prepare(&s).unwrap();
        let mut legal: Vec<Card> = g.legal_moves().iter().collect();
        legal.sort();
        for r in rankings(&g, &tasks) {
            let mut r = r.clone();
            r.sort();
            assert_eq!(r, legal);
        }
        assert_eq!(task_status(&g, &tasks), vec![Status::Open, Status::Open]);
    }

    #[test]
    fn position_stats_agree_with_solving_each_move() {
        let s = situation("Player2: win B4\nPlayer3: tricks =2");
        let st = position_stats(&s, &Options::default()).unwrap().unwrap();
        assert_eq!(st.legal, 10);
        assert_eq!(st.open, 0);
        for m in s.game().unwrap().legal_moves().iter() {
            let wins = matches!(solve_move(&s, m, &Options::default()).unwrap().0, Verdict::Feasible(_));
            assert_eq!(wins, st.winning.contains(&m), "{m}");
        }
        assert!(!st.winning.is_empty());
        for (k, r) in st.first_win.iter().enumerate() {
            assert!(r.is_some_and(|r| r >= 1 && r <= st.legal), "order {k}: {r:?}");
        }
    }

    #[test]
    fn play_out_reports_each_choice_as_a_decision() {
        use crate::trace::{Event, Trace};
        let s = situation("Player2: win B4\nPlayer3: tricks =2");
        let n = std::cell::Cell::new(0);
        let sink = |e: &Event| {
            if let Event::Decision { ranked, chosen, .. } = e {
                assert_eq!(ranked[0], *chosen);
                n.set(n.get() + 1);
            }
        };
        let opts = Options { trace: Some(Trace::new(&sink)), ..Options::default() };
        let p = play_out(&s, 2, &opts, None).unwrap().unwrap();
        assert_eq!(n.get(), p.line.len());
        // Deck order gives up B4 in this deal (move 10; B6 would have kept it).
        let d = play_out(&s, 0, &Options::default(), None).unwrap().unwrap();
        assert_eq!((d.lost_at, d.alternatives), (Some(9), vec![Card::parse("B6").unwrap()]));
    }

    #[test]
    fn play_out_finds_where_it_goes_wrong() {
        // Feasible at the start; deck order plays its lowest cards and gives
        // up B4 at some point, or wins: either way the report is consistent.
        let s = situation("Player2: win B4\nPlayer3: tricks =2");
        for order in 0..ORDERS.len() {
            let p = play_out(&s, order, &Options::default(), None).unwrap().unwrap();
            assert_eq!(p.winnable, Some(true));
            assert_eq!(p.line.len(), 40);
            let fulfilled = p.status.iter().all(|x| *x == Status::Done);
            match p.lost_at {
                None => assert!(fulfilled, "order {order}: never lost but not fulfilled"),
                Some(i) => {
                    assert!(!fulfilled);
                    // Each alternative keeps the tasks possible at that point.
                    let mut cur = s.clone();
                    for &c in &p.line[..i] {
                        cur = after(&cur, c).unwrap();
                    }
                    for &m in &p.alternatives {
                        assert!(matches!(solver::solve(&after(&cur, m).unwrap(), &Options::default()).unwrap().0, Verdict::Feasible(_)));
                    }
                    assert!(!p.alternatives.is_empty());
                }
            }
        }
    }
}
