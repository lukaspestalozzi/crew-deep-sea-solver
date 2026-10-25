//! Move deciders: rank the legal cards of the player to play.
//!
//! The solver tries moves in the decider's order, so a good decider finds
//! winning lines sooner (it never changes a verdict). A self-playing bot can
//! use the same interface and play the first card. Deciders see the full
//! game; a hidden-hand bot would build a [`Position`] from its own view.

use crate::card::{Card, CardSet, Suit};
use crate::game::{winning_position, Game};
use crate::task::{Kind, TrickProp};

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
/// beating a winner who wants the trick, feed it the cards it needs, and play
/// cards that fit its trick properties. Ties: win cheaply, lose expensively;
/// with a non-zero `salt`, ties are broken at random (one salt per restart).
pub struct TaskAware {
    pub salt: u64,
}

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
            Kind::MostTricks | Kind::MoreThanAllOthers => 2,
            Kind::VsCaptain(crate::task::Cmp::More) => 1,
            Kind::Trick(prop) if g.current.iter().all(|&c| fits(prop, c)) && !matches!(prop, TrickProp::CardInLastTrick(_)) => 2,
            Kind::NoTwoInARow if won_prev => -3,
            Kind::InARow { .. } if won_prev => 1,
            _ => 0,
        };
    }
    d
}

/// Does card `c` suit a trick with this property?
fn fits(prop: &TrickProp, c: Card) -> bool {
    let v = c.value();
    match prop {
        TrickProp::AllBelow(b) => !c.is_sub() && v < *b,
        TrickProp::AllAbove(b) => !c.is_sub() && v > *b,
        TrickProp::AllEven => !c.is_sub() && v % 2 == 0,
        TrickProp::AllOdd => !c.is_sub() && v % 2 == 1,
        TrickProp::SumAbove(_) => !c.is_sub() && v >= 6,
        TrickProp::SumBelow(_) => !c.is_sub() && v <= 3,
        TrickProp::SumIn(_) => !c.is_sub() && (4..=8).contains(&v),
        TrickProp::SameCount(a, b) => c.suit() == *a || c.suit() == *b,
        TrickProp::CardWonBySub(x) => c == *x || c.is_sub(),
        TrickProp::CardInLastTrick(_) => true,
    }
}

/// Cards player `p` wants in the tricks it wins (open tasks that need them)
/// and cards it must not win.
fn card_wishes(pos: &Position, p: usize) -> (CardSet, CardSet) {
    let g = pos.game;
    let won = g.won[p];
    let (mut need, mut avoid) = (CardSet::EMPTY, CardSet::EMPTY);
    let at_least = |set: CardSet, k: u8, exact: bool, need: &mut CardSet, avoid: &mut CardSet| {
        let have = won.intersect(set).len() as u8;
        if have < k {
            *need = need.union(set);
        } else if exact {
            *avoid = avoid.union(set);
        }
    };
    for (t, k) in pos.kinds.iter().enumerate() {
        if pos.open_owners(t) & (1 << p) == 0 {
            continue;
        }
        match k {
            Kind::WinCards(s) => need = need.union(*s),
            Kind::WinNone(s) => avoid = avoid.union(*s),
            Kind::SuitCounts { counts, exact } => {
                for &(su, n) in counts {
                    at_least(su.cards(), n, *exact, &mut need, &mut avoid);
                }
            }
            Kind::ValueCount { value, n, exact } => at_least(CardSet::colour_value(*value), *n, *exact, &mut need, &mut avoid),
            Kind::MoreOf(a, b) => {
                need = need.union(a.cards());
                avoid = avoid.union(b.cards());
            }
            Kind::SameOf(a, b) => need = need.union(a.cards()).union(b.cards()),
            Kind::AllOfAColour => {
                let others = (0..g.n).filter(|&q| q != p).fold(CardSet::EMPTY, |acc, q| acc.union(g.won[q]));
                for su in Suit::COLOURS {
                    if others.intersect(su.cards()).is_empty() {
                        need = need.union(su.cards());
                    }
                }
            }
            Kind::OnlySub(v) => {
                let target = Card::new(Suit::Sub, *v);
                need.insert(target);
                avoid = avoid.union(Suit::Sub.cards().minus(CardSet::single(target)));
            }
            Kind::Trick(TrickProp::CardWonBySub(c)) => need.insert(*c),
            _ => {}
        }
    }
    (need.minus(avoid), avoid)
}

/// Trick-property tasks still open for player `p`.
fn trick_props<'a>(pos: &'a Position, p: usize) -> impl Iterator<Item = &'a TrickProp> + 'a {
    pos.kinds.iter().enumerate().filter_map(move |(t, k)| match k {
        Kind::Trick(prop) if pos.open_owners(t) & (1 << p) != 0 => Some(prop),
        _ => None,
    })
}

/// +2 per trick property of `owner` that the trick so far still has and `c`
/// keeps, −2 per one it would break.
fn fit_score(pos: &Position, owner: usize, c: Card) -> i32 {
    let current = &pos.game.current;
    trick_props(pos, owner)
        .filter(|prop| current.iter().all(|&x| fits(prop, x)))
        .map(|prop| if fits(prop, c) { 2 } else { -2 })
        .sum()
}

/// +3 if `c` is a card the player needs, -3 if one it must not win.
fn wish(w: &(crate::card::CardSet, crate::card::CardSet), c: Card) -> i32 {
    3 * w.0.contains(c) as i32 - 3 * w.1.contains(c) as i32
}

impl MoveDecider for TaskAware {
    fn order(&self, pos: &Position, moves: &mut [Card]) {
        if moves.len() < 2 {
            return;
        }
        let g = pos.game;
        let me = g.to_play();
        let leading = g.current.is_empty();
        let base = desire(pos, me);
        let my_wishes = card_wishes(pos, me);
        // Current winner of the trick and how much it wants the trick.
        let (winner, winner_desire) = if leading {
            (me, 0)
        } else {
            let w = (g.leader as usize + winning_position(&g.current)) % g.n;
            (w, desire(pos, w))
        };
        let winner_wishes = if winner == me { my_wishes } else { card_wishes(pos, winner) };
        let mut buf = [Card::from_index(0); 6];
        let n = g.current.len();
        buf[..n].copy_from_slice(&g.current);
        let mut scored: [(i32, Card); 14] = [(0, Card::from_index(0)); 14];
        for (i, &m) in moves.iter().enumerate() {
            buf[n] = m;
            let wins = leading || winning_position(&buf[..=n]) == n;
            let want = base + wish(&my_wishes, m) + if wins { fit_score(pos, me, m) } else { 0 };
            let mut score = if wins { want * 10 } else { -want * 10 };
            if !leading && winner != me {
                if wins {
                    // Taking the trick from a winner who wants it, or who must lose it.
                    score -= winner_desire * 5;
                } else {
                    // Feeding the winner a card it needs (or must not get),
                    // and one that suits its trick properties.
                    score += (wish(&winner_wishes, m) + fit_score(pos, winner, m)) * 5;
                }
            }
            let tie = if self.salt == 0 {
                // Win cheaply, lose expensively.
                let v = m.value() as i32 + if m.is_sub() { 10 } else { 0 };
                if wins { -v } else { v }
            } else {
                let h = (self.salt ^ (m.index() as u64) << 8 ^ (g.tricks.len() as u64) << 16 ^ (n as u64) << 24)
                    .wrapping_mul(0x9E37_79B9_7F4A_7C15);
                (h >> 59) as i32
            };
            score = score * 32 + tie;
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
pub static TASK_AWARE: TaskAware = TaskAware { salt: 0 };

/// Task-aware orders with random tie-breaks, one per restart round.
pub static TASK_AWARE_RANDOM: [TaskAware; 8] = [
    TaskAware { salt: 0x243F_6A88 },
    TaskAware { salt: 0x85A3_08D3 },
    TaskAware { salt: 0x1319_8A2E },
    TaskAware { salt: 0x0370_7344 },
    TaskAware { salt: 0xA409_3822 },
    TaskAware { salt: 0x299F_31D0 },
    TaskAware { salt: 0x082E_FA98 },
    TaskAware { salt: 0xEC4E_6C89 },
];
