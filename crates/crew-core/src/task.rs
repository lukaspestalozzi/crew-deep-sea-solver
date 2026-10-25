//! Task kinds and their evaluation.
//!
//! A task is *done* once it can no longer fail, *failed* once it can no longer
//! be met, otherwise *open*. Evaluation runs on a [`Game`] between tricks
//! (no trick in progress) and must be exact at the end of the game.

use crate::card::{Card, CardSet, Suit};
use crate::game::{Game, Trick};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Open,
    Done,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cmp {
    More,
    Fewer,
    Same,
}

/// A property a single trick must have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrickProp {
    /// Every card is a colour card with value below the bound.
    AllBelow(u8),
    /// Every card is a colour card with value above the bound.
    AllAbove(u8),
    AllEven,
    AllOdd,
    /// Colour cards only; sum strictly above the bound for 3/4/5 players.
    SumAbove([u8; 3]),
    /// Colour cards only; sum strictly below the bound for 3/4/5 players.
    SumBelow([u8; 3]),
    /// Colour cards only; sum is one of these values.
    SumIn(Vec<u8>),
    /// As many cards of one suit as of the other, at least one each.
    SameCount(Suit, Suit),
    /// The trick contains this card and is won by a submarine.
    CardWonBySub(Card),
    /// The last trick of the game contains this card.
    CardInLastTrick(Card),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// Win all these cards.
    WinCards(CardSet),
    /// Win none of these cards.
    WinNone(CardSet),
    /// Never lead a trick with a card from this set.
    NoLead(CardSet),
    /// Win a trick whose winning card is a colour card of `value`; with
    /// `containing`, the trick must also hold another colour card of that value.
    WinWith { value: u8, containing: Option<u8> },
    /// Compare own trick count with the captain's at the end.
    VsCaptain(Cmp),
    /// Win at least one trick with this property.
    Trick(TrickProp),
    /// Win none of the first k tricks.
    NoneOfFirst(u8),
    /// Win the first `first` tricks and/or the last trick; with `only`, no others.
    WinTricks { first: u8, last: bool, only: bool },
    /// Win exactly this many tricks.
    ExactTricks(u8),
    /// Predict the own trick count: always met under perfect information.
    Predict,
    /// Card counts per suit among the won cards (exact or at least).
    SuitCounts { counts: Vec<(Suit, u8)>, exact: bool },
    /// Number of won colour cards of a value (exact or at least).
    ValueCount { value: u8, n: u8, exact: bool },
    /// Win `n` tricks in a row; `exact`: exactly `n` tricks in total, all in one row.
    InARow { n: u8, exact: bool },
    /// Never win two tricks in a row.
    NoTwoInARow,
    /// Win submarine `v` and no other submarine.
    OnlySub(u8),
    /// Win all cards of at least one colour.
    AllOfAColour,
    /// More won cards of suit A than of suit B (B may be 0).
    MoreOf(Suit, Suit),
    /// As many won cards of suit A as of suit B, at least one each.
    SameOf(Suit, Suit),
    /// More tricks than each other player.
    MostTricks,
    /// More tricks than all other players together.
    MoreThanAllOthers,
    /// Fewer tricks than each other player.
    FewestTricks,

    // ---- Rule cards: apply to the whole crew ----
    /// Nobody leads a trick with a card from this set.
    CrewNoLead(CardSet),
    /// This card is the last card of the last trick.
    LastCardOfGame(Card),
    /// The winner of trick 1 has strictly more tricks than everyone else at all times.
    FirstWinnerAlwaysLeads,
    /// At no time does anyone hold two more won colour cards of `value` than anyone else.
    BalancedValue(u8),
}

impl Kind {
    pub fn is_rule(&self) -> bool {
        matches!(
            self,
            Kind::CrewNoLead(_) | Kind::LastCardOfGame(_) | Kind::FirstWinnerAlwaysLeads | Kind::BalancedValue(_)
        )
    }

    /// Evaluate for `owner` (ignored for rule cards). The game must be between tricks.
    pub fn eval(&self, g: &Game, owner: usize) -> Status {
        debug_assert!(g.current.is_empty());
        let over = g.is_over();
        let r = g.tricks_left() as i32;
        let won = g.won[owner];
        let unplayed = g.unplayed();
        let my = g.trick_counts[owner] as i32;
        let others = || (0..g.n).filter(move |&q| q != owner);
        use Status::*;
        match self {
            Kind::WinCards(s) => {
                if won.intersect(*s) == *s {
                    Done
                } else if over
                    || others().any(|q| !g.won[q].intersect(*s).is_empty())
                    || s.minus(won).iter().any(|c| !can_capture(g, owner, c))
                {
                    Failed
                } else {
                    Open
                }
            }
            Kind::WinNone(s) => {
                if !won.intersect(*s).is_empty() || !top_subs(g, owner).intersect(*s).is_empty() {
                    Failed
                } else if over || unplayed.intersect(*s).is_empty() {
                    Done
                } else {
                    Open
                }
            }
            Kind::NoLead(s) => {
                if g.tricks.iter().any(|t| t.leader as usize == owner && s.contains(t.cards[0])) {
                    Failed
                } else if over || g.hands[owner].intersect(*s).is_empty() {
                    Done
                } else {
                    Open
                }
            }
            Kind::WinWith { value, containing } => {
                let hit = |t: &Trick| {
                    let w = t.winning_card(g.n);
                    if w.is_sub() || w.value() != *value {
                        return false;
                    }
                    match containing {
                        None => true,
                        Some(cv) => t.cards.iter().any(|&c| c != w && !c.is_sub() && c.value() == *cv),
                    }
                };
                if won_tricks(g, owner).any(hit) {
                    return Done;
                }
                let mine = g.hands[owner].intersect(CardSet::colour_value(*value));
                // The contained card is another player's; it must not beat the
                // winning card, so a higher or equal one must be of another suit.
                let others_cards = unplayed.minus(g.hands[owner]);
                let feasible = !over
                    && !mine.is_empty()
                    && match containing {
                        None => true,
                        Some(cv) => mine.iter().any(|x| {
                            others_cards
                                .intersect(CardSet::colour_value(*cv))
                                .iter()
                                .any(|y| y.suit() != x.suit() || y.value() < x.value())
                        }),
                    };
                if feasible {
                    Open
                } else {
                    Failed
                }
            }
            Kind::VsCaptain(cmp) => {
                let cap = g.trick_counts[g.captain as usize] as i32;
                let (a, b) = (my, cap);
                let possible = match cmp {
                    Cmp::More => a + r > b,
                    Cmp::Fewer => b + r > a,
                    Cmp::Same => (a - b).abs() <= r,
                };
                if !possible {
                    Failed
                } else if over {
                    Done
                } else {
                    Open
                }
            }
            Kind::Trick(p) => {
                if won_tricks(g, owner).any(|t| trick_has(p, t, g)) {
                    return Done;
                }
                if over {
                    return Failed;
                }
                match p {
                    TrickProp::CardWonBySub(c) | TrickProp::CardInLastTrick(c) if !unplayed.contains(*c) => Failed,
                    TrickProp::CardWonBySub(_) if g.hands[owner].intersect(Suit::Sub.cards()).is_empty() => Failed,
                    // The owner plays one card per trick: not both the card and the submarine.
                    TrickProp::CardWonBySub(c) if g.hands[owner].contains(*c) => Failed,
                    _ if !trick_still_possible(p, g) => Failed,
                    _ => Open,
                }
            }
            Kind::NoneOfFirst(k) => {
                let k = *k as usize;
                if g.tricks.iter().take(k).any(|t| t.winner as usize == owner) {
                    Failed
                } else if g.tricks.len() >= k {
                    Done
                } else {
                    Open
                }
            }
            Kind::WinTricks { first, last, only } => {
                let total = g.total_tricks();
                let required = |i: usize| i < *first as usize || (*last && i == total - 1);
                for (i, t) in g.tricks.iter().enumerate() {
                    let mine = t.winner as usize == owner;
                    if required(i) != mine && (required(i) || *only) {
                        return Failed;
                    }
                }
                let req_done = (0..total).filter(|&i| required(i)).all(|i| i < g.tricks.len());
                if req_done && (!*only || over) {
                    Done
                } else {
                    Open
                }
            }
            Kind::ExactTricks(k) => {
                let k = *k as i32;
                if my + top_subs(g, owner).len() as i32 > k || my + r < k {
                    Failed
                } else if over {
                    Done
                } else {
                    Open
                }
            }
            Kind::Predict => Done,
            Kind::SuitCounts { counts, exact } => {
                let mut all_done = true;
                for &(s, k) in counts {
                    let have = won.intersect(s.cards()).len();
                    let mut avail = unplayed.intersect(s.cards()).len();
                    let mut forced = 0;
                    if s == Suit::Sub {
                        // Only capturable submarines count; the top ones are certain.
                        avail = unplayed.intersect(s.cards()).iter().filter(|&c| can_capture(g, owner, c)).count() as u32;
                        forced = top_subs(g, owner).len();
                    }
                    let k = k as u32;
                    if have + if over { 0 } else { avail } < k || (*exact && have + forced > k) {
                        return Failed;
                    }
                    let done = if *exact { have == k && (over || avail == 0) } else { have >= k };
                    all_done &= done;
                }
                if all_done {
                    Done
                } else {
                    Open
                }
            }
            Kind::ValueCount { value, n, exact } => {
                let set = CardSet::colour_value(*value);
                let have = won.intersect(set).len();
                let avail = if over { 0 } else { unplayed.intersect(set).len() };
                let k = *n as u32;
                if have + avail < k || (*exact && have > k) {
                    Failed
                } else if (*exact && have == k && avail == 0) || (!*exact && have >= k) {
                    Done
                } else {
                    Open
                }
            }
            Kind::InARow { n, exact } => {
                let n = *n as usize;
                let idx: Vec<usize> = won_indices(g, owner).collect();
                let played = g.tricks.len();
                if !*exact {
                    let (best, tail) = runs(&idx, played);
                    if best >= n {
                        Done
                    } else if over || (tail + r as usize) < n && (r as usize) < n {
                        Failed
                    } else {
                        Open
                    }
                } else {
                    let cnt = idx.len();
                    if cnt > n {
                        return Failed;
                    }
                    if cnt > 0 {
                        let contiguous = idx[cnt - 1] - idx[0] + 1 == cnt;
                        let ended = idx[cnt - 1] + 1 < played;
                        if !contiguous || (ended && cnt < n) || (cnt + r as usize) < n {
                            return Failed;
                        }
                    } else if (r as usize) < n {
                        return Failed;
                    }
                    if over {
                        Done
                    } else {
                        Open
                    }
                }
            }
            Kind::NoTwoInARow => {
                let idx: Vec<usize> = won_indices(g, owner).collect();
                if idx.windows(2).any(|w| w[1] == w[0] + 1) {
                    Failed
                } else if over {
                    Done
                } else {
                    Open
                }
            }
            Kind::OnlySub(v) => {
                let target = Card::new(Suit::Sub, *v);
                let my_subs = won.intersect(Suit::Sub.cards());
                if !my_subs.minus(CardSet::single(target)).is_empty()
                    || !top_subs(g, owner).minus(CardSet::single(target)).is_empty()
                    || others().any(|q| g.won[q].contains(target))
                    || (over && !my_subs.contains(target))
                    || (!my_subs.contains(target) && !can_capture(g, owner, target))
                    // Taking the target from another player needs a higher submarine
                    // of the owner, which the owner then wins too.
                    || (!my_subs.contains(target) && !g.hands[owner].contains(target))
                {
                    Failed
                } else if my_subs.contains(target) && (over || unplayed.intersect(Suit::Sub.cards()).is_empty()) {
                    Done
                } else {
                    Open
                }
            }
            Kind::AllOfAColour => {
                if Suit::COLOURS.iter().any(|s| won.intersect(s.cards()) == s.cards()) {
                    Done
                } else if over
                    || Suit::COLOURS
                        .iter()
                        .all(|s| others().any(|q| !g.won[q].intersect(s.cards()).is_empty()))
                {
                    Failed
                } else {
                    Open
                }
            }
            Kind::MoreOf(a, b) => {
                let (ha, hb) = (won.intersect(a.cards()).len() as i32, won.intersect(b.cards()).len() as i32);
                let (va, vb) = if over {
                    (0, 0)
                } else {
                    (unplayed.intersect(a.cards()).len() as i32, unplayed.intersect(b.cards()).len() as i32)
                };
                if ha + va <= hb {
                    Failed
                } else if ha > hb + vb {
                    Done
                } else {
                    Open
                }
            }
            Kind::SameOf(a, b) => {
                let (ha, hb) = (won.intersect(a.cards()).len() as i32, won.intersect(b.cards()).len() as i32);
                let (va, vb) = if over {
                    (0, 0)
                } else {
                    (unplayed.intersect(a.cards()).len() as i32, unplayed.intersect(b.cards()).len() as i32)
                };
                if ha + va < hb || hb + vb < ha || ha + va < 1 || hb + vb < 1 {
                    Failed
                } else if over {
                    Done
                } else {
                    Open
                }
            }
            Kind::MostTricks => {
                let max_other = others().map(|q| g.trick_counts[q] as i32).max().unwrap();
                if my + r <= max_other {
                    Failed
                } else if my > max_other + r {
                    Done
                } else {
                    Open
                }
            }
            Kind::MoreThanAllOthers => {
                let sum: i32 = others().map(|q| g.trick_counts[q] as i32).sum();
                if my + r <= sum {
                    Failed
                } else if my > sum + r {
                    Done
                } else {
                    Open
                }
            }
            Kind::FewestTricks => {
                if others().any(|q| my >= g.trick_counts[q] as i32 + r) {
                    Failed
                } else if over {
                    Done
                } else {
                    Open
                }
            }
            Kind::CrewNoLead(s) => {
                if g.tricks.iter().any(|t| s.contains(t.cards[0])) {
                    Failed
                } else if over || unplayed.intersect(*s).is_empty() {
                    Done
                } else {
                    Open
                }
            }
            Kind::LastCardOfGame(c) => {
                let total = g.total_tricks();
                let pos = g.tricks.iter().position(|t| t.cards.contains(c));
                match pos {
                    Some(i) if i == total - 1 && *g.tricks[i].cards.last().unwrap() == *c => Done,
                    Some(_) => Failed,
                    None if over => Failed,
                    None => Open,
                }
            }
            Kind::FirstWinnerAlwaysLeads => match g.tricks.first() {
                None => Open,
                Some(first) => {
                    // "At all times": check after every trick so far.
                    let w = first.winner as usize;
                    let mut counts = [0u8; crate::game::MAX_PLAYERS];
                    for t in &g.tricks {
                        counts[t.winner as usize] += 1;
                        if (0..g.n).any(|q| q != w && counts[q] >= counts[w]) {
                            return Failed;
                        }
                    }
                    if over {
                        Done
                    } else {
                        Open
                    }
                }
            },
            Kind::BalancedValue(v) => {
                // "At no time": check after every trick so far.
                let set = CardSet::colour_value(*v);
                let mut counts = [0u32; crate::game::MAX_PLAYERS];
                for t in &g.tricks {
                    counts[t.winner as usize] += t.set().intersect(set).len();
                    let (mx, mn) = (counts[..g.n].iter().max().unwrap(), counts[..g.n].iter().min().unwrap());
                    if *mx >= mn + 2 {
                        return Failed;
                    }
                }
                if over || unplayed.intersect(set).is_empty() {
                    Done
                } else {
                    Open
                }
            }
        }
    }
}

/// Cards of `s` in suits that `s` only partly covers: those are the cards that
/// `s` tells apart from other cards of the same suit.
fn partial(s: CardSet) -> CardSet {
    Suit::ALL.iter().fold(CardSet::EMPTY, |acc, su| {
        let part = s.intersect(su.cards());
        if part.is_empty() || part == su.cards() {
            acc
        } else {
            acc.union(part)
        }
    })
}

/// Value boundaries per suit (indexed by `Suit as usize`): bit `v - 1` set
/// means cards of value `v` and `v + 1` of that suit are told apart by some
/// task. Cards of a suit with no boundary between them are interchangeable.
pub type Boundaries = [u16; 5];

pub const NO_BOUNDARIES: Boundaries = [0; 5];

/// Boundaries on both sides of every card in `s`.
pub fn around(s: CardSet) -> Boundaries {
    let mut b = NO_BOUNDARIES;
    for c in s.iter() {
        let (i, v) = (c.suit() as usize, c.value() as u16);
        if v < c.suit().max_value() as u16 {
            b[i] |= 1 << (v - 1);
        }
        if v > 1 {
            b[i] |= 1 << (v - 2);
        }
    }
    b
}

/// A boundary between `v` and `v + 1` in every colour suit.
fn colour_cut(v: u8) -> Boundaries {
    let mut b = NO_BOUNDARIES;
    if (1..9).contains(&v) {
        for s in Suit::COLOURS {
            b[s as usize] = 1 << (v - 1);
        }
    }
    b
}

pub fn merge(a: Boundaries, b: Boundaries) -> Boundaries {
    let mut out = a;
    for i in 0..5 {
        out[i] |= b[i];
    }
    out
}

impl Kind {
    /// Where this task tells cards of the same suit apart (see [`Boundaries`]).
    pub fn boundaries(&self) -> Boundaries {
        let all_colour_values = [0xFF, 0xFF, 0xFF, 0xFF, 0];
        match self {
            Kind::WinCards(s) | Kind::WinNone(s) | Kind::NoLead(s) | Kind::CrewNoLead(s) => around(partial(*s)),
            Kind::WinWith { value, containing } => {
                around(CardSet::colour_value(*value).union(containing.map_or(CardSet::EMPTY, CardSet::colour_value)))
            }
            Kind::Trick(p) => match p {
                TrickProp::SameCount(..) => NO_BOUNDARIES,
                TrickProp::CardWonBySub(c) | TrickProp::CardInLastTrick(c) => around(CardSet::single(*c)),
                TrickProp::AllBelow(b) => colour_cut(b.saturating_sub(1)),
                TrickProp::AllAbove(b) => colour_cut(*b),
                TrickProp::AllEven | TrickProp::AllOdd | TrickProp::SumAbove(_) | TrickProp::SumBelow(_) | TrickProp::SumIn(_) => {
                    all_colour_values
                }
            },
            Kind::ValueCount { value, .. } | Kind::BalancedValue(value) => around(CardSet::colour_value(*value)),
            Kind::OnlySub(v) => around(CardSet::single(Card::new(Suit::Sub, *v))),
            Kind::LastCardOfGame(c) => around(CardSet::single(*c)),
            Kind::VsCaptain(_)
            | Kind::NoneOfFirst(_)
            | Kind::WinTricks { .. }
            | Kind::ExactTricks(_)
            | Kind::Predict
            | Kind::SuitCounts { .. }
            | Kind::InARow { .. }
            | Kind::NoTwoInARow
            | Kind::AllOfAColour
            | Kind::MoreOf(..)
            | Kind::SameOf(..)
            | Kind::MostTricks
            | Kind::MoreThanAllOthers
            | Kind::FewestTricks
            | Kind::FirstWinnerAlwaysLeads => NO_BOUNDARIES,
        }
    }

    /// Everything about the game's history that this task's future evaluation
    /// depends on, beyond the remaining hands, the leader and the trick number.
    /// Two positions with equal hands, leader and signatures of all tasks have
    /// the same outcome; the solver caches failed positions by this key.
    /// Only valid between tricks and for tasks that have not failed;
    /// `status` is this task's current evaluation.
    pub fn signature(&self, g: &Game, owner: usize, status: Status) -> u64 {
        let won = g.won[owner];
        let my = g.trick_counts[owner] as u64;
        let counts = || (0..g.n).fold(0u64, |acc, q| acc | (g.trick_counts[q] as u64) << (4 * q));
        let won_last = g.tricks.last().is_some_and(|t| t.winner as usize == owner);
        let status = status as u64;
        let info = match self {
            Kind::WinCards(s) | Kind::WinNone(s) => won.intersect(*s).0,
            Kind::OnlySub(_) => won.intersect(Suit::Sub.cards()).0,
            Kind::NoLead(_)
            | Kind::WinWith { .. }
            | Kind::Trick(_)
            | Kind::NoneOfFirst(_)
            | Kind::WinTricks { .. }
            | Kind::Predict
            | Kind::CrewNoLead(_)
            | Kind::LastCardOfGame(_) => 0,
            Kind::VsCaptain(_) => (my as i64 - g.trick_counts[g.captain as usize] as i64 + 64) as u64,
            Kind::ExactTricks(_) => my,
            Kind::SuitCounts { counts, .. } => counts
                .iter()
                .enumerate()
                .fold(0, |acc, (i, (s, _))| acc | (won.intersect(s.cards()).len() as u64) << (4 * i)),
            Kind::ValueCount { value, .. } => won.intersect(CardSet::colour_value(*value)).len() as u64,
            Kind::InARow { .. } => {
                let idx: Vec<usize> = won_indices(g, owner).collect();
                let (best, tail) = runs(&idx, g.tricks.len());
                my | (tail as u64) << 8 | (best as u64) << 16
            }
            Kind::NoTwoInARow => won_last as u64,
            Kind::AllOfAColour => Suit::COLOURS.iter().enumerate().fold(0, |acc, (i, s)| {
                let spoiled = (0..g.n).any(|q| q != owner && !g.won[q].intersect(s.cards()).is_empty());
                acc | (spoiled as u64) << i
            }),
            Kind::MoreOf(a, b) | Kind::SameOf(a, b) => {
                won.intersect(a.cards()).len() as u64 | (won.intersect(b.cards()).len() as u64) << 8
            }
            Kind::MostTricks | Kind::MoreThanAllOthers | Kind::FewestTricks => counts(),
            Kind::FirstWinnerAlwaysLeads => {
                counts() | g.tricks.first().map_or(7, |t| t.winner as u64) << 24
            }
            Kind::BalancedValue(v) => {
                let set = CardSet::colour_value(*v);
                (0..g.n).fold(0, |acc, q| acc | (g.won[q].intersect(set).len() as u64) << (4 * q))
            }
        };
        debug_assert!(info < 1 << 60);
        status << 60 | info
    }
}

/// Necessary condition for a future trick with property `p`: every player
/// contributes one card, so each hand must still hold a fitting card.
/// (Between tricks only; in a 3-player game the leftover card is included,
/// which keeps the check necessary.)
fn trick_still_possible(p: &TrickProp, g: &Game) -> bool {
    let colour_values = |p: usize| g.hands[p].minus(Suit::Sub.cards()).iter().map(|c| c.value() as u32);
    let players = 0..g.n;
    let ni = g.n - 3;
    match p {
        TrickProp::AllBelow(b) => players.clone().all(|q| colour_values(q).any(|v| v < *b as u32)),
        TrickProp::AllAbove(b) => players.clone().all(|q| colour_values(q).any(|v| v > *b as u32)),
        TrickProp::AllEven => players.clone().all(|q| colour_values(q).any(|v| v % 2 == 0)),
        TrickProp::AllOdd => players.clone().all(|q| colour_values(q).any(|v| v % 2 == 1)),
        TrickProp::SumAbove(_) | TrickProp::SumBelow(_) | TrickProp::SumIn(_) => {
            let mut lo = 0;
            let mut hi = 0;
            for q in players.clone() {
                match (colour_values(q).min(), colour_values(q).max()) {
                    (Some(a), Some(b)) => {
                        lo += a;
                        hi += b;
                    }
                    _ => return false,
                }
            }
            match p {
                TrickProp::SumAbove(b) => hi > b[ni] as u32,
                TrickProp::SumBelow(b) => lo < b[ni] as u32,
                TrickProp::SumIn(vals) => vals.iter().any(|&v| lo <= v as u32 && v as u32 <= hi),
                _ => unreachable!(),
            }
        }
        _ => true,
    }
}

/// A constraint on the final trick counts `f` (one per player).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CountRule {
    Exact(usize, u8),
    AtLeast(usize, u8),
    /// f[a] > f[b]
    Greater(usize, usize),
    /// f[a] == f[b]
    Equal(usize, usize),
    /// f[o] > f[q] for every other q
    Most(usize),
    /// f[o] < f[q] for every other q
    Fewest(usize),
    /// f[o] > sum of all others
    MoreThanAll(usize),
}

impl CountRule {
    pub fn holds(&self, f: &[u8]) -> bool {
        let others = |o: usize| (0..f.len()).filter(move |&q| q != o);
        match *self {
            CountRule::Exact(o, k) => f[o] == k,
            CountRule::AtLeast(o, k) => f[o] >= k,
            CountRule::Greater(a, b) => f[a] > f[b],
            CountRule::Equal(a, b) => f[a] == f[b],
            CountRule::Most(o) => others(o).all(|q| f[o] > f[q]),
            CountRule::Fewest(o) => others(o).all(|q| f[o] < f[q]),
            CountRule::MoreThanAll(o) => f[o] as u32 > others(o).map(|q| f[q] as u32).sum::<u32>(),
        }
    }
}

impl Kind {
    /// What this task implies about the final trick counts, if anything.
    pub fn count_rule(&self, g: &Game, owner: usize) -> Option<CountRule> {
        let cap = g.captain as usize;
        match self {
            Kind::ExactTricks(k) => Some(CountRule::Exact(owner, *k)),
            Kind::VsCaptain(Cmp::More) => Some(CountRule::Greater(owner, cap)),
            Kind::VsCaptain(Cmp::Fewer) => Some(CountRule::Greater(cap, owner)),
            Kind::VsCaptain(Cmp::Same) => Some(CountRule::Equal(owner, cap)),
            Kind::MostTricks => Some(CountRule::Most(owner)),
            Kind::FewestTricks => Some(CountRule::Fewest(owner)),
            Kind::MoreThanAllOthers => Some(CountRule::MoreThanAll(owner)),
            Kind::WinTricks { first, last, only: true } => Some(CountRule::Exact(owner, first + *last as u8)),
            Kind::WinTricks { first, last, only: false } => Some(CountRule::AtLeast(owner, first + *last as u8)),
            Kind::InARow { n, exact: true } => Some(CountRule::Exact(owner, *n)),
            Kind::InARow { n, exact: false } => Some(CountRule::AtLeast(owner, *n)),
            Kind::FirstWinnerAlwaysLeads => g.tricks.first().map(|t| CountRule::Most(t.winner as usize)),
            _ => None,
        }
    }
}

/// Is there a way to split the remaining tricks so that every rule holds?
/// Each player also keeps at least the tricks their top submarines will win.
pub fn counts_possible(g: &Game, rules: &[CountRule]) -> bool {
    any_final_counts(g, |f| rules.iter().all(|r| r.holds(f)))
}

/// Is there a possible split of the remaining tricks (final counts `f`, one per
/// player, each at least the current count plus the certain submarine wins)
/// for which `test(f)` holds?
pub fn any_final_counts(g: &Game, mut test: impl FnMut(&[u8]) -> bool) -> bool {
    let n = g.n;
    let r = g.tricks_left() as u8;
    let mut lo = [0u8; crate::game::MAX_PLAYERS];
    let mut need = 0u8;
    for p in 0..n {
        lo[p] = g.trick_counts[p] + top_subs(g, p).len() as u8;
        need += lo[p] - g.trick_counts[p];
    }
    if need > r {
        return false;
    }
    let mut f = lo;
    fn rec(p: usize, n: usize, left: u8, f: &mut [u8; crate::game::MAX_PLAYERS], test: &mut dyn FnMut(&[u8]) -> bool) -> bool {
        if p == n - 1 {
            f[p] += left;
            let ok = test(&f[..n]);
            f[p] -= left;
            return ok;
        }
        for extra in 0..=left {
            f[p] += extra;
            let ok = rec(p + 1, n, left - extra, f, test);
            f[p] -= extra;
            if ok {
                return true;
            }
        }
        false
    }
    rec(0, n, r - need, &mut f, &mut test)
}

/// What an open task still asks of its owner's card play, for the combined
/// checks across tasks: cards the owner must still win, cards it must not
/// win, sets of which it must win `k` more cards, and further tricks it must
/// win or lose.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Req {
    pub need: CardSet,
    pub avoid: CardSet,
    pub demands: [(CardSet, u8); 4],
    pub n_demands: usize,
    pub more_wins: u8,
    pub more_losses: u8,
    /// The owner must win all cards of one of these sets (e.g. a colour).
    pub alternatives: [CardSet; 4],
    pub n_alternatives: usize,
}

impl Req {
    fn demand(&mut self, set: CardSet, k: u8) {
        if self.n_demands < self.demands.len() {
            self.demands[self.n_demands] = (set, k);
            self.n_demands += 1;
        }
    }

    pub fn demands(&self) -> &[(CardSet, u8)] {
        &self.demands[..self.n_demands]
    }

    pub fn alternatives(&self) -> &[CardSet] {
        &self.alternatives[..self.n_alternatives]
    }
}

impl Kind {
    /// Requirements of this task for `owner`; only meaningful while it is open.
    pub fn requirements(&self, g: &Game, owner: usize) -> Req {
        let won = g.won[owner];
        let unplayed = g.unplayed();
        let mut r = Req::default();
        let count = |r: &mut Req, set: CardSet, k: u8, exact: bool| {
            let have = won.intersect(set).len() as u8;
            if have < k {
                r.demand(set.intersect(unplayed), k - have);
            } else if exact {
                r.avoid = r.avoid.union(set.intersect(unplayed));
            }
        };
        match self {
            Kind::WinCards(s) => r.need = s.minus(won).intersect(unplayed),
            Kind::WinNone(s) => r.avoid = s.intersect(unplayed),
            Kind::SuitCounts { counts, exact } => {
                for &(su, k) in counts {
                    count(&mut r, su.cards(), k, *exact);
                }
            }
            Kind::ValueCount { value, n, exact } => count(&mut r, CardSet::colour_value(*value), *n, *exact),
            Kind::OnlySub(v) => {
                let target = Card::new(Suit::Sub, *v);
                if !won.contains(target) {
                    r.need = CardSet::single(target).intersect(unplayed);
                }
                r.avoid = Suit::Sub.cards().minus(CardSet::single(target)).intersect(unplayed);
            }
            Kind::NoneOfFirst(k) => r.more_losses = (*k as usize).saturating_sub(g.tricks.len()) as u8,
            Kind::WinWith { .. } | Kind::Trick(_) => r.more_wins = 1,
            Kind::AllOfAColour => {
                let others: CardSet = (0..g.n).filter(|&q| q != owner).fold(CardSet::EMPTY, |a, q| a.union(g.won[q]));
                let mut best = u8::MAX;
                for s in Suit::COLOURS {
                    if others.intersect(s.cards()).is_empty() {
                        let rest = s.cards().intersect(unplayed);
                        r.alternatives[r.n_alternatives] = rest;
                        r.n_alternatives += 1;
                        let need = (0..g.n).map(|h| g.hands[h].intersect(rest).len() as u8).max().unwrap_or(0);
                        best = best.min(need);
                    }
                }
                if r.n_alternatives == 1 {
                    r.need = r.alternatives[0];
                    r.n_alternatives = 0;
                } else if best != u8::MAX {
                    r.more_wins = best;
                }
            }
            Kind::MoreOf(a, b) => {
                let (ha, hb) = (won.intersect(a.cards()).len() as u8, won.intersect(b.cards()).len() as u8);
                if ha <= hb {
                    r.demand(a.cards().intersect(unplayed), hb + 1 - ha);
                }
            }
            Kind::SameOf(a, b) => {
                let (ha, hb) = (won.intersect(a.cards()).len() as u8, won.intersect(b.cards()).len() as u8);
                if ha < hb.max(1) {
                    r.demand(a.cards().intersect(unplayed), hb.max(1) - ha);
                }
                if hb < ha.max(1) {
                    r.demand(b.cards().intersect(unplayed), ha.max(1) - hb);
                }
            }
            _ => {}
        }
        r
    }
}

/// Fewest tricks in which one player can collect `k` cards of `set`: each
/// trick holds one card per player. `None` if fewer than `k` such cards are in hands.
fn tricks_for(g: &Game, set: CardSet, k: u8) -> Option<u8> {
    let mut per = [0u8; crate::game::MAX_PLAYERS];
    for (h, x) in per.iter_mut().enumerate().take(g.n) {
        *x = g.hands[h].intersect(set).len() as u8;
    }
    if per.iter().map(|&x| x as u32).sum::<u32>() < k as u32 {
        return None;
    }
    (0..=g.tricks_left() as u8).find(|&m| per.iter().map(|&x| x.min(m) as u32).sum::<u32>() >= k as u32)
}

/// Bounds (lowest, highest) on player `p`'s final trick count implied by
/// these requirements of its open tasks together, or `None` if they
/// contradict each other or cannot be met:
/// - cards `p` must win that one player holds go into different tricks, all won by `p`;
/// - `k` more cards of a set need enough tricks (one card per player per trick);
/// - cards in `p`'s own hand that `p` must not win go into tricks `p` loses
///   (one card may stay unplayed when a hand has more cards than tricks left).
pub fn req_bounds<'a>(g: &Game, p: usize, reqs: impl Iterator<Item = &'a Req>) -> Option<(u8, u8)> {
    let r = g.tricks_left() as u8;
    let (mut need, mut avoid, mut wins, mut losses) = (CardSet::EMPTY, CardSet::EMPTY, 0u8, 0u8);
    for q in reqs {
        need = need.union(q.need);
        avoid = avoid.union(q.avoid);
        wins = wins.max(q.more_wins);
        losses = losses.max(q.more_losses);
        for &(set, k) in q.demands() {
            wins = wins.max(tricks_for(g, set, k)?);
        }
    }
    if !need.intersect(avoid).is_empty() {
        return None;
    }
    for h in 0..g.n {
        wins = wins.max(g.hands[h].intersect(need).len() as u8);
    }
    let hand = g.hands[p];
    let spare = (hand.len() as u8).saturating_sub(r);
    losses = losses.max((hand.intersect(avoid).len() as u8).saturating_sub(spare));
    let my = g.trick_counts[p];
    if wins + losses > r {
        return None;
    }
    Some((my + wins, my + r - losses))
}

pub fn top_subs_count(g: &Game, p: usize) -> u64 {
    top_subs(g, p).len() as u64
}

/// Submarines from the top that player `p` holds without a gap among the
/// remaining submarines. Each of them is certain to win its own trick
/// (one card per player per trick), so `p` is certain to win that many more
/// tricks, and certain to capture these submarines.
fn top_subs(g: &Game, p: usize) -> CardSet {
    let mut out = CardSet::EMPTY;
    let remaining = g.unplayed().intersect(Suit::Sub.cards());
    for v in (1..=4).rev() {
        let c = Card::new(Suit::Sub, v);
        if !remaining.contains(c) {
            continue;
        }
        if g.hands[p].contains(c) {
            out.insert(c);
        } else {
            break;
        }
    }
    out
}

/// Necessary condition for `owner` to still win the trick holding unplayed card `c`.
/// For a submarine held by someone else, the owner needs a higher submarine.
fn can_capture(g: &Game, owner: usize, c: Card) -> bool {
    let hand = g.hands[owner];
    if hand.contains(c) {
        return true;
    }
    let subs = Suit::Sub.cards();
    if c.is_sub() {
        let higher = subs.iter().filter(|s| s.value() > c.value()).collect::<CardSet>();
        return !hand.intersect(higher).is_empty();
    }
    // A colour card of another player ends in a trick the owner wins: led or
    // followed in its suit and beaten by a higher card of that suit or a
    // submarine from the owner, or discarded into a trick of another suit the
    // owner wins (which needs a card of another suit). The owner needs at
    // least one of these cards.
    let s = c.suit();
    hand.iter().any(|x| x.is_sub() || x.suit() != s || x.value() > c.value())
}

fn won_tricks<'a>(g: &'a Game, owner: usize) -> impl Iterator<Item = &'a Trick> + 'a {
    g.tricks.iter().filter(move |t| t.winner as usize == owner)
}

fn won_indices(g: &Game, owner: usize) -> impl Iterator<Item = usize> + '_ {
    g.tricks.iter().enumerate().filter(move |(_, t)| t.winner as usize == owner).map(|(i, _)| i)
}

/// Longest run of consecutive indices, and the run ending at the last played trick.
fn runs(idx: &[usize], played: usize) -> (usize, usize) {
    let (mut best, mut cur, mut prev): (usize, usize, Option<usize>) = (0, 0, None);
    for &i in idx {
        cur = if prev == Some(i.wrapping_sub(1)) && i > 0 { cur + 1 } else { 1 };
        best = best.max(cur);
        prev = Some(i);
    }
    let tail = if played > 0 && prev == Some(played - 1) { cur } else { 0 };
    (best, tail)
}

fn trick_has(p: &TrickProp, t: &Trick, g: &Game) -> bool {
    let no_subs = || t.cards.iter().all(|c| !c.is_sub());
    let sum = || t.cards.iter().map(|c| c.value() as u32).sum::<u32>();
    let count = |s: Suit| t.cards.iter().filter(|c| c.suit() == s).count();
    let ni = g.n - 3;
    match p {
        TrickProp::AllBelow(b) => no_subs() && t.cards.iter().all(|c| c.value() < *b),
        TrickProp::AllAbove(b) => no_subs() && t.cards.iter().all(|c| c.value() > *b),
        TrickProp::AllEven => no_subs() && t.cards.iter().all(|c| c.value() % 2 == 0),
        TrickProp::AllOdd => no_subs() && t.cards.iter().all(|c| c.value() % 2 == 1),
        TrickProp::SumAbove(b) => no_subs() && sum() > b[ni] as u32,
        TrickProp::SumBelow(b) => no_subs() && sum() < b[ni] as u32,
        TrickProp::SumIn(vals) => no_subs() && vals.contains(&(sum() as u8)),
        TrickProp::SameCount(a, b) => count(*a) >= 1 && count(*a) == count(*b),
        TrickProp::CardWonBySub(c) => t.cards.contains(c) && t.winning_card(g.n).is_sub(),
        TrickProp::CardInLastTrick(c) => {
            g.is_over() && std::ptr::eq(t, g.tricks.last().unwrap()) && t.cards.contains(c)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::tests_support::*;

    fn play_all(g: &mut Game, cards: &str) {
        for s in cards.split_whitespace() {
            g.play(c(s));
        }
    }

    #[test]
    fn win_cards_done_failed() {
        let mut g = Game::new(&mock_deal()).unwrap();
        let k = Kind::WinCards(cs("G4 G2"));
        assert_eq!(k.eval(&g, 0), Status::Open);
        play_all(&mut g, "G6 G4 G2 G3"); // Player1 wins
        assert_eq!(k.eval(&g, 0), Status::Done);
        assert_eq!(k.eval(&g, 1), Status::Failed);
    }

    #[test]
    fn win_none_and_no_lead() {
        let mut g = Game::new(&mock_deal()).unwrap();
        play_all(&mut g, "G6 G4 G2 G3");
        assert_eq!(Kind::WinNone(Suit::Green.cards()).eval(&g, 0), Status::Failed);
        assert_eq!(Kind::WinNone(Suit::Green.cards()).eval(&g, 1), Status::Open);
        assert_eq!(Kind::NoLead(Suit::Green.cards()).eval(&g, 0), Status::Failed);
        assert_eq!(Kind::NoLead(Suit::Pink.cards()).eval(&g, 0), Status::Open);
        assert_eq!(Kind::CrewNoLead(Suit::Green.cards()).eval(&g, 2), Status::Failed);
    }

    #[test]
    fn win_with_value() {
        let mut g = Game::new(&mock_deal()).unwrap();
        play_all(&mut g, "G6 G4 G2 G3"); // won with a 6, contains 4, 2, 3
        assert_eq!(Kind::WinWith { value: 6, containing: None }.eval(&g, 0), Status::Done);
        assert_eq!(Kind::WinWith { value: 6, containing: Some(3) }.eval(&g, 0), Status::Done);
        assert_eq!(Kind::WinWith { value: 6, containing: Some(6) }.eval(&g, 0), Status::Failed);
        // Player2 has no colour 7 at all.
        assert_eq!(Kind::WinWith { value: 7, containing: None }.eval(&g, 1), Status::Failed);
    }

    #[test]
    fn first_tricks_and_counts() {
        let mut g = Game::new(&mock_deal()).unwrap();
        play_all(&mut g, "G6 G4 G2 G3"); // Player1
        let first = Kind::WinTricks { first: 1, last: false, only: false };
        let only_first = Kind::WinTricks { first: 1, last: false, only: true };
        assert_eq!(first.eval(&g, 0), Status::Done);
        assert_eq!(only_first.eval(&g, 0), Status::Open);
        assert_eq!(first.eval(&g, 1), Status::Failed);
        assert_eq!(Kind::NoneOfFirst(3).eval(&g, 0), Status::Failed);
        assert_eq!(Kind::NoneOfFirst(1).eval(&g, 1), Status::Done);
        assert_eq!(Kind::ExactTricks(0).eval(&g, 0), Status::Failed);
        play_all(&mut g, "P2 P5 P6 P9"); // Player4
        assert_eq!(only_first.eval(&g, 0), Status::Open);
        assert_eq!(Kind::InARow { n: 2, exact: false }.eval(&g, 0), Status::Open);
    }

    /// Every early verdict (done or failed before the game ends) must match
    /// the end-of-game evaluation on every continuation. Exhaustive for short
    /// endings, sampled for longer ones.
    #[test]
    fn early_verdicts_are_sound() {
        use crate::catalogue::catalogue;
        use crate::rng::Rng;

        fn all_endings(g: &mut Game, f: &mut dyn FnMut(&Game)) {
            if g.is_over() {
                f(g);
                return;
            }
            for c in g.legal_moves().iter() {
                g.play(c);
                all_endings(g, f);
                g.undo();
            }
        }

        let mut rng = Rng::new(9);
        let cat = catalogue();
        let mut checked = 0;
        for case in 0..600 {
            let n = 3 + case % 3;
            let exhaustive = case % 2 == 0;
            let left = if exhaustive { if n == 3 { 4 } else { 3 } } else { 7 };
            let mut g = Game::new(&Game::random_hands(n, &mut rng)).unwrap();
            while g.tricks_left() > left || !g.current.is_empty() {
                let m: Vec<Card> = g.legal_moves().iter().collect();
                g.play(m[rng.below(m.len())]);
            }
            for t in cat {
                let owner = rng.below(n);
                let claim = t.kind.eval(&g, owner);
                if claim == Status::Open {
                    continue;
                }
                checked += 1;
                let mut check = |end: &Game| {
                    let fin = t.kind.eval(end, owner);
                    assert_eq!(fin, claim, "case {case}: {} for Player{} claimed {claim:?} early, ended {fin:?}", t.id, owner + 1);
                };
                if exhaustive {
                    all_endings(&mut g.clone(), &mut check);
                } else {
                    for _ in 0..200 {
                        let mut h = g.clone();
                        while !h.is_over() {
                            let m: Vec<Card> = h.legal_moves().iter().collect();
                            h.play(m[rng.below(m.len())]);
                        }
                        check(&h);
                    }
                }
            }
        }
        assert!(checked > 5000, "only {checked} early verdicts checked");
    }

    #[test]
    fn trick_properties() {
        let mut g = Game::new(&mock_deal()).unwrap();
        play_all(&mut g, "G6 G4 G2 G3"); // sum 15, all below 7, mixed parity
        let t = |p: TrickProp| Kind::Trick(p).eval(&g, 0);
        assert_eq!(t(TrickProp::AllBelow(7)), Status::Done);
        assert_eq!(t(TrickProp::SumBelow([8, 12, 16])), Status::Open); // 4 players: needs < 12
        assert_eq!(t(TrickProp::SumBelow([8, 16, 16])), Status::Done);
        assert_eq!(t(TrickProp::SumIn(vec![15])), Status::Done);
        assert_eq!(t(TrickProp::AllEven), Status::Open);
    }
}
