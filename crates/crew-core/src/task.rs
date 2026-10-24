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
                } else if over || others().any(|q| !g.won[q].intersect(*s).is_empty()) {
                    Failed
                } else {
                    Open
                }
            }
            Kind::WinNone(s) => {
                if !won.intersect(*s).is_empty() {
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
                let feasible = !over
                    && !mine.is_empty()
                    && match containing {
                        None => true,
                        Some(cv) if cv == value => unplayed.intersect(CardSet::colour_value(*cv)).len() >= 2,
                        Some(cv) => !unplayed.intersect(CardSet::colour_value(*cv)).is_empty(),
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
                if my > k || my + r < k {
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
                    let avail = unplayed.intersect(s.cards()).len();
                    let k = k as u32;
                    if have + if over { 0 } else { avail } < k || (*exact && have > k) {
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
                    || others().any(|q| g.won[q].contains(target))
                    || (over && !my_subs.contains(target))
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
                Some(t) => {
                    let w = t.winner as usize;
                    let cw = g.trick_counts[w];
                    if (0..g.n).any(|q| q != w && g.trick_counts[q] >= cw) {
                        Failed
                    } else if over {
                        Done
                    } else {
                        Open
                    }
                }
            },
            Kind::BalancedValue(v) => {
                let set = CardSet::colour_value(*v);
                let counts: Vec<u32> = (0..g.n).map(|q| g.won[q].intersect(set).len()).collect();
                let (mx, mn) = (*counts.iter().max().unwrap(), *counts.iter().min().unwrap());
                if mx >= mn + 2 {
                    Failed
                } else if over || unplayed.intersect(set).is_empty() {
                    Done
                } else {
                    Open
                }
            }
        }
    }
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
