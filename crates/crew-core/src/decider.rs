//! Move deciders: rank the legal cards of the player to play.
//!
//! The solver tries moves in the decider's order, so a good decider finds
//! winning lines sooner (it never changes a verdict). A self-playing bot can
//! use the same interface and play the first card. Deciders see the full
//! game; a hidden-hand bot would build a [`Position`] from its own view.

use crate::card::Card;
use crate::game::{winning_position, Game};
use crate::task::Kind;

/// What a decider may look at: the game and, per task, which players may
/// still own it (bit p = player p) and for whom it is already done.
pub struct Position<'a> {
    pub game: &'a Game,
    pub kinds: &'a [&'a Kind],
    /// Bits `5 * task + player`: that player can still fulfil that task.
    pub alive: u128,
    /// Bits `5 * task + player`: the task is done for that player.
    pub done: u128,
}

impl Position<'_> {
    /// Players for whom task `t` is still open (alive, not done).
    pub fn open_owners(&self, t: usize) -> u8 {
        ((self.alive & !self.done) >> (5 * t)) as u8 & 0x1F
    }
}

pub trait MoveDecider: Sync {
    /// Reorder `moves` (legal cards of the player to play), best first.
    fn order(&self, pos: &Position, moves: &mut [Card]);
}

/// Keeps the deck order (pink 1 … submarine 4): the reference.
pub struct DeckOrder;

impl MoveDecider for DeckOrder {
    fn order(&self, _: &Position, _: &mut [Card]) {}
}

/// Task-aware ordering: each player wants to win or lose the current trick
/// depending on its open tasks and the cards in the trick; partners avoid
/// beating a winner who wants the trick and feed it the cards it needs.
pub struct TaskAware;

/// How much player `p` wants to win the trick in progress (positive) or lose
/// it (negative), from the cards already in it.
fn desire(pos: &Position, p: usize) -> i32 {
    let g = pos.game;
    let trick_no = g.tricks.len() as u8; // 0-based number of the trick in progress
    let last = g.tricks_left() == 1;
    let in_trick: crate::card::CardSet = g.current.iter().copied().collect();
    let won_prev = g.tricks.last().is_some_and(|t| t.winner as usize == p);
    let mut d = 0;
    for (t, k) in pos.kinds.iter().enumerate() {
        if pos.open_owners(t) & (1 << p) == 0 {
            continue;
        }
        d += match k {
            Kind::WinCards(s) if !s.intersect(in_trick).is_empty() => 3,
            Kind::WinNone(s) if !s.intersect(in_trick).is_empty() => -3,
            Kind::NoneOfFirst(k) if trick_no < *k => -3,
            Kind::ExactTricks(n) => {
                if g.trick_counts[p] < *n {
                    1
                } else {
                    -3
                }
            }
            Kind::WinTricks { first, last: l, only } => {
                if trick_no < *first || (*l && last) {
                    3
                } else if *only {
                    -3
                } else {
                    0
                }
            }
            Kind::FewestTricks | Kind::VsCaptain(crate::task::Cmp::Fewer) => -1,
            Kind::MostTricks | Kind::MoreThanAllOthers | Kind::VsCaptain(crate::task::Cmp::More) => 1,
            Kind::NoTwoInARow if won_prev => -3,
            Kind::InARow { .. } if won_prev => 1,
            _ => 0,
        };
    }
    d
}

/// Extra wish of player `p` for card `c` landing in its trick: positive if a
/// task of `p` needs `c`, negative if it must not win `c`.
fn wants_card(pos: &Position, p: usize, c: Card) -> i32 {
    let mut d = 0;
    for (t, k) in pos.kinds.iter().enumerate() {
        if pos.open_owners(t) & (1 << p) == 0 {
            continue;
        }
        match k {
            Kind::WinCards(s) if s.contains(c) => d += 3,
            Kind::WinNone(s) if s.contains(c) => d -= 3,
            _ => {}
        }
    }
    d
}

impl MoveDecider for TaskAware {
    fn order(&self, pos: &Position, moves: &mut [Card]) {
        let g = pos.game;
        let me = g.to_play();
        let leading = g.current.is_empty();
        let base = desire(pos, me);
        // Current winner of the trick and how much it wants the trick.
        let (winner, winner_desire) = if leading {
            (me, 0)
        } else {
            let w = (g.leader as usize + winning_position(&g.current)) % g.n;
            (w, desire(pos, w))
        };
        let mut buf = [Card::from_index(0); 6];
        let n = g.current.len();
        buf[..n].copy_from_slice(&g.current);
        let mut scored: [(i32, Card); 14] = [(0, Card::from_index(0)); 14];
        for (i, &m) in moves.iter().enumerate() {
            buf[n] = m;
            let wins = leading || winning_position(&buf[..=n]) == n;
            let want = base + wants_card(pos, me, m);
            let mut score = if wins { want * 10 } else { -want * 10 };
            if !leading && winner != me {
                if wins {
                    // Taking the trick from a winner who wants it, or who must lose it.
                    score -= winner_desire * 5;
                } else {
                    // Feeding the winner a card it needs (or must not get).
                    score += wants_card(pos, winner, m) * 5;
                }
            }
            // Tie-breaks: win cheaply, lose expensively.
            let v = m.value() as i32 + if m.is_sub() { 10 } else { 0 };
            score = score * 32 + if wins { -v } else { v };
            scored[i] = (score, m);
        }
        let k = moves.len();
        scored[..k].sort_by(|a, b| b.0.cmp(&a.0));
        for (i, (_, m)) in scored[..k].iter().enumerate() {
            moves[i] = *m;
        }
    }
}

pub static DECK_ORDER: DeckOrder = DeckOrder;
pub static TASK_AWARE: TaskAware = TaskAware;
