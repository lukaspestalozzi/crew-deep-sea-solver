//! Trick-taking rules of Mission Deep Sea.

use crate::card::{Card, CardSet, Suit, NUM_CARDS};
use crate::rng::Rng;
use std::fmt;

pub const MAX_PLAYERS: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DealError {
    PlayerCount(usize),
    Overlap(Card),
    Missing(CardSet),
    HandSize { player: usize, size: u32 },
}

impl fmt::Display for DealError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DealError::PlayerCount(n) => write!(f, "player count must be 3 to 5, got {n}"),
            DealError::Overlap(c) => write!(f, "{c} appears in more than one hand"),
            DealError::Missing(s) => write!(f, "cards not dealt: {s:?}"),
            DealError::HandSize { player, size } => {
                write!(f, "Player{} has {size} cards, which does not fit the deal", player + 1)
            }
        }
    }
}

/// A finished trick. `cards[i]` was played by player `(leader + i) % n`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trick {
    pub leader: u8,
    pub cards: Vec<Card>,
    pub winner: u8,
}

impl Trick {
    pub fn set(&self) -> CardSet {
        self.cards.iter().copied().collect()
    }

    /// The card that won the trick.
    pub fn winning_card(&self, n: usize) -> Card {
        let pos = (self.winner as usize + n - self.leader as usize) % n;
        self.cards[pos]
    }
}

/// Position in the cards of a trick that wins it.
pub fn winning_position(cards: &[Card]) -> usize {
    let led = cards[0].suit();
    let mut best = 0;
    for (i, &c) in cards.iter().enumerate().skip(1) {
        let b = cards[best];
        let better = if c.is_sub() {
            !b.is_sub() || c.value() > b.value()
        } else {
            !b.is_sub() && c.suit() == led && c.value() > b.value()
        };
        if better {
            best = i;
        }
    }
    best
}

#[derive(Debug, Clone)]
pub struct Game {
    pub n: usize,
    pub captain: u8,
    /// Original hands as dealt.
    pub dealt: [CardSet; MAX_PLAYERS],
    /// Cards still in hand.
    pub hands: [CardSet; MAX_PLAYERS],
    pub tricks: Vec<Trick>,
    /// Cards of the trick in progress, in play order.
    pub current: Vec<Card>,
    /// Leader of the trick in progress.
    pub leader: u8,
    pub won: [CardSet; MAX_PLAYERS],
    pub trick_counts: [u8; MAX_PLAYERS],
}

impl Game {
    pub fn new(hands: &[CardSet]) -> Result<Game, DealError> {
        let n = hands.len();
        if !(3..=MAX_PLAYERS).contains(&n) {
            return Err(DealError::PlayerCount(n));
        }
        let mut all = CardSet::EMPTY;
        for h in hands {
            if let Some(c) = all.intersect(*h).iter().next() {
                return Err(DealError::Overlap(c));
            }
            all = all.union(*h);
        }
        if all != CardSet::FULL {
            return Err(DealError::Missing(CardSet::FULL.minus(all)));
        }
        let base = (NUM_CARDS / n) as u32;
        let extra = (NUM_CARDS % n) as u32;
        let mut extras = 0;
        for (p, h) in hands.iter().enumerate() {
            let size = h.len();
            if size == base + 1 && extra > 0 {
                extras += 1;
            } else if size != base {
                return Err(DealError::HandSize { player: p, size });
            }
        }
        if extras != extra {
            // Only possible for 3 players: exactly one hand holds 14.
            let p = hands.iter().position(|h| h.len() != base).unwrap_or(0);
            return Err(DealError::HandSize { player: p, size: hands[p].len() });
        }
        let s4 = Card::new(Suit::Sub, 4);
        let captain = hands.iter().position(|h| h.contains(s4)).unwrap() as u8;
        let mut arr = [CardSet::EMPTY; MAX_PLAYERS];
        arr[..n].copy_from_slice(hands);
        Ok(Game {
            n,
            captain,
            dealt: arr,
            hands: arr,
            tricks: Vec::with_capacity(NUM_CARDS / n),
            current: Vec::with_capacity(n),
            leader: captain,
            won: [CardSet::EMPTY; MAX_PLAYERS],
            trick_counts: [0; MAX_PLAYERS],
        })
    }

    /// Random deal. The extra card in a 3-player game goes to a random seat.
    pub fn random_hands(n: usize, rng: &mut Rng) -> Vec<CardSet> {
        let mut cards: Vec<Card> = Card::all().collect();
        rng.shuffle(&mut cards);
        let start = rng.below(n);
        let mut hands = vec![CardSet::EMPTY; n];
        for (i, c) in cards.into_iter().enumerate() {
            hands[(start + i) % n].insert(c);
        }
        hands
    }

    pub fn total_tricks(&self) -> usize {
        NUM_CARDS / self.n
    }

    pub fn tricks_left(&self) -> usize {
        self.total_tricks() - self.tricks.len()
    }

    pub fn is_over(&self) -> bool {
        self.tricks.len() == self.total_tricks()
    }

    pub fn to_play(&self) -> usize {
        (self.leader as usize + self.current.len()) % self.n
    }

    pub fn legal_moves(&self) -> CardSet {
        if self.is_over() {
            return CardSet::EMPTY;
        }
        let hand = self.hands[self.to_play()];
        match self.current.first() {
            None => hand,
            Some(led) => {
                let follow = hand.intersect(led.suit().cards());
                if follow.is_empty() {
                    hand
                } else {
                    follow
                }
            }
        }
    }

    /// Play a card for the player to move. Returns true if this completed a trick.
    /// Panics on an illegal card; callers check `legal_moves` first.
    pub fn play(&mut self, card: Card) -> bool {
        assert!(self.legal_moves().contains(card), "illegal play {card}");
        let p = self.to_play();
        self.hands[p].remove(card);
        self.current.push(card);
        if self.current.len() < self.n {
            return false;
        }
        let pos = winning_position(&self.current);
        let winner = ((self.leader as usize + pos) % self.n) as u8;
        let cards = std::mem::take(&mut self.current);
        for &c in &cards {
            self.won[winner as usize].insert(c);
        }
        self.trick_counts[winner as usize] += 1;
        self.tricks.push(Trick { leader: self.leader, cards, winner });
        self.leader = winner;
        true
    }

    /// Undo the last card played.
    pub fn undo(&mut self) {
        if self.current.is_empty() {
            let t = self.tricks.pop().expect("nothing to undo");
            for &c in &t.cards {
                self.won[t.winner as usize].remove(c);
            }
            self.trick_counts[t.winner as usize] -= 1;
            self.leader = t.leader;
            self.current = t.cards;
        }
        let card = self.current.pop().unwrap();
        let p = self.to_play();
        self.hands[p].insert(card);
    }

    /// Cards not yet played by anyone (in hands).
    pub fn unplayed(&self) -> CardSet {
        self.hands[..self.n].iter().fold(CardSet::EMPTY, |a, h| a.union(*h))
    }
}

#[cfg(test)]
pub(crate) mod tests_support {
    use super::*;

    pub fn cs(s: &str) -> CardSet {
        s.split_whitespace().map(|t| Card::parse(t).unwrap()).collect()
    }

    pub fn c(s: &str) -> Card {
        Card::parse(s).unwrap()
    }

    /// Fixed 4-player deal used in the mockups. Player1 holds S4.
    pub fn mock_deal() -> Vec<CardSet> {
        vec![
            cs("P2 P8 B4 B9 G1 G6 Y3 Y7 S2 S4"),
            cs("P1 P5 B1 B2 B6 G4 G9 Y2 Y6 S1"),
            cs("P3 P6 B3 B8 G2 G5 G8 Y1 Y8 S3"),
            cs("P4 P7 P9 B5 B7 G3 G7 Y4 Y5 Y9"),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::tests_support::*;
    use super::*;

    #[test]
    fn deal_validation() {
        let g = Game::new(&mock_deal()).unwrap();
        assert_eq!(g.captain, 0);
        assert_eq!(g.leader, 0);
        let mut bad = mock_deal();
        bad[3].remove(c("Y9"));
        assert!(matches!(Game::new(&bad), Err(DealError::Missing(_))));
        bad[2].insert(c("Y9"));
        assert!(matches!(Game::new(&bad), Err(DealError::HandSize { player: 2, size: 11 })));
    }

    #[test]
    fn follow_suit_and_winner() {
        let mut g = Game::new(&mock_deal()).unwrap();
        // Player1 leads G6; Player2 must follow green.
        g.play(c("G6"));
        assert_eq!(g.legal_moves(), cs("G4 G9"));
        g.play(c("G4"));
        g.play(c("G2"));
        assert!(g.play(c("G3")));
        assert_eq!(g.tricks[0].winner, 0);
        assert_eq!(g.leader, 0);
        // Player1 leads P2; Player2 P5; Player3 P6; Player4 P9 wins.
        g.play(c("P2"));
        g.play(c("P5"));
        g.play(c("P6"));
        g.play(c("P9"));
        assert_eq!(g.tricks[1].winner, 3);
        assert_eq!(g.to_play(), 3);
    }

    #[test]
    fn submarine_beats_led_colour_and_off_suit_never_wins() {
        assert_eq!(winning_position(&[c("Y5"), c("Y9"), c("S1"), c("P9")]), 2);
        assert_eq!(winning_position(&[c("Y5"), c("P9"), c("B9")]), 0);
        assert_eq!(winning_position(&[c("S2"), c("S3"), c("Y9")]), 1);
    }

    #[test]
    fn void_player_may_play_anything() {
        let mut g = Game::new(&mock_deal()).unwrap();
        // Player1 leads Y3, Player2 Y2, Player3 Y1, Player4 Y9 wins.
        for s in ["Y3", "Y2", "Y1", "Y9"] {
            g.play(c(s));
        }
        // Player4 leads Y4; Player1 has Y7 left so must follow.
        g.play(c("Y4"));
        assert_eq!(g.legal_moves(), cs("Y7"));
        g.play(c("Y7"));
        // Player2 has Y6.
        g.play(c("Y6"));
        // Player3 has Y8.
        g.play(c("Y8"));
        // Player3 wins and leads Y? No: Y8 is highest -> Player3.
        assert_eq!(g.tricks[1].winner, 2);
        // Player3 leads; to test void: Player3 leads P3, Player4 follows pink.
        g.play(c("P3"));
        g.play(c("P4"));
        // Player1 (pink P2, P8) follows.
        g.play(c("P8"));
        g.play(c("P1"));
        assert_eq!(g.tricks[2].winner, 0);
        // Player1 leads Y? Player1 has no yellow left: lead B4; others follow blue.
        g.play(c("B4"));
        assert_eq!(g.legal_moves(), cs("B1 B2 B6"));
    }

    #[test]
    fn undo_restores_state() {
        let mut g = Game::new(&mock_deal()).unwrap();
        let before = (g.hands, g.leader, g.won, g.trick_counts);
        for s in ["G6", "G4", "G2", "G3", "P2"] {
            g.play(c(s));
        }
        for _ in 0..5 {
            g.undo();
        }
        assert_eq!((g.hands, g.leader, g.won, g.trick_counts), before);
        assert!(g.tricks.is_empty() && g.current.is_empty());
    }

    #[test]
    fn three_player_game_leaves_one_card() {
        let mut rng = Rng::new(7);
        let hands = Game::random_hands(3, &mut rng);
        let mut g = Game::new(&hands).unwrap();
        assert_eq!(g.total_tricks(), 13);
        while !g.is_over() {
            let m = g.legal_moves().iter().next().unwrap();
            g.play(m);
        }
        assert_eq!(g.unplayed().len(), 1);
        assert!(g.legal_moves().is_empty());
    }
}
