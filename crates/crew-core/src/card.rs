//! Cards and card sets.
//!
//! A card is an index 0..40: colour cards are `suit * 9 + (value - 1)` for
//! pink, blue, green, yellow (0..36); submarines 1..4 are 36..40.
//! A [`CardSet`] is a 40-bit mask.

use std::fmt;

pub const NUM_CARDS: usize = 40;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Suit {
    Pink,
    Blue,
    Green,
    Yellow,
    Sub,
}

impl Suit {
    pub const COLOURS: [Suit; 4] = [Suit::Pink, Suit::Blue, Suit::Green, Suit::Yellow];
    pub const ALL: [Suit; 5] = [Suit::Pink, Suit::Blue, Suit::Green, Suit::Yellow, Suit::Sub];

    pub fn letter(self) -> char {
        match self {
            Suit::Pink => 'P',
            Suit::Blue => 'B',
            Suit::Green => 'G',
            Suit::Yellow => 'Y',
            Suit::Sub => 'S',
        }
    }

    pub fn from_letter(c: char) -> Option<Suit> {
        match c {
            'P' => Some(Suit::Pink),
            'B' => Some(Suit::Blue),
            'G' => Some(Suit::Green),
            'Y' => Some(Suit::Yellow),
            'S' => Some(Suit::Sub),
            _ => None,
        }
    }

    pub fn max_value(self) -> u8 {
        if self == Suit::Sub {
            4
        } else {
            9
        }
    }

    /// All cards of this suit.
    pub fn cards(self) -> CardSet {
        match self {
            Suit::Sub => CardSet(0b1111 << 36),
            s => CardSet(0x1FF << (s as u32 * 9)),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Card(u8);

impl Card {
    pub fn new(suit: Suit, value: u8) -> Card {
        assert!(value >= 1 && value <= suit.max_value(), "invalid value {value} for {suit:?}");
        match suit {
            Suit::Sub => Card(36 + value - 1),
            s => Card(s as u8 * 9 + value - 1),
        }
    }

    pub fn from_index(i: u8) -> Card {
        assert!((i as usize) < NUM_CARDS);
        Card(i)
    }

    pub fn index(self) -> u8 {
        self.0
    }

    pub fn suit(self) -> Suit {
        match self.0 / 9 {
            0 => Suit::Pink,
            1 => Suit::Blue,
            2 => Suit::Green,
            3 => Suit::Yellow,
            _ => Suit::Sub,
        }
    }

    pub fn value(self) -> u8 {
        if self.0 >= 36 {
            self.0 - 36 + 1
        } else {
            self.0 % 9 + 1
        }
    }

    pub fn is_sub(self) -> bool {
        self.0 >= 36
    }

    pub fn parse(s: &str) -> Option<Card> {
        let mut chars = s.chars();
        let suit = Suit::from_letter(chars.next()?)?;
        let rest: String = chars.collect();
        let value: u8 = rest.parse().ok()?;
        if value < 1 || value > suit.max_value() {
            return None;
        }
        Some(Card::new(suit, value))
    }

    pub fn all() -> impl Iterator<Item = Card> {
        (0..NUM_CARDS as u8).map(Card)
    }
}

impl fmt::Display for Card {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.suit().letter(), self.value())
    }
}

impl fmt::Debug for Card {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

/// A set of cards as a bitmask.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct CardSet(pub u64);

impl CardSet {
    pub const EMPTY: CardSet = CardSet(0);
    pub const FULL: CardSet = CardSet((1u64 << NUM_CARDS) - 1);

    pub fn single(c: Card) -> CardSet {
        CardSet(1u64 << c.0)
    }

    pub fn contains(self, c: Card) -> bool {
        self.0 & (1u64 << c.0) != 0
    }

    pub fn insert(&mut self, c: Card) {
        self.0 |= 1u64 << c.0;
    }

    pub fn remove(&mut self, c: Card) {
        self.0 &= !(1u64 << c.0);
    }

    pub fn len(self) -> u32 {
        self.0.count_ones()
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn union(self, o: CardSet) -> CardSet {
        CardSet(self.0 | o.0)
    }

    pub fn intersect(self, o: CardSet) -> CardSet {
        CardSet(self.0 & o.0)
    }

    pub fn minus(self, o: CardSet) -> CardSet {
        CardSet(self.0 & !o.0)
    }

    /// All colour cards (no submarines) with the given value.
    pub fn colour_value(value: u8) -> CardSet {
        let mut s = CardSet::EMPTY;
        for suit in Suit::COLOURS {
            s.insert(Card::new(suit, value));
        }
        s
    }

    pub fn iter(self) -> impl Iterator<Item = Card> {
        let mut bits = self.0;
        std::iter::from_fn(move || {
            if bits == 0 {
                None
            } else {
                let i = bits.trailing_zeros() as u8;
                bits &= bits - 1;
                Some(Card(i))
            }
        })
    }
}

impl FromIterator<Card> for CardSet {
    fn from_iter<T: IntoIterator<Item = Card>>(iter: T) -> Self {
        let mut s = CardSet::EMPTY;
        for c in iter {
            s.insert(c);
        }
        s
    }
}

impl fmt::Debug for CardSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let v: Vec<String> = self.iter().map(|c| c.to_string()).collect();
        write!(f, "{{{}}}", v.join(" "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_roundtrip_all_40() {
        let mut seen = CardSet::EMPTY;
        for c in Card::all() {
            let s = c.to_string();
            assert_eq!(Card::parse(&s), Some(c), "{s}");
            seen.insert(c);
        }
        assert_eq!(seen, CardSet::FULL);
        assert_eq!(Card::parse("S4").unwrap().value(), 4);
        assert_eq!(Card::parse("Y9").unwrap().suit(), Suit::Yellow);
        assert_eq!(Card::parse("S5"), None);
        assert_eq!(Card::parse("P0"), None);
        assert_eq!(Card::parse("X3"), None);
    }

    #[test]
    fn suit_sets_partition_the_deck() {
        let mut all = CardSet::EMPTY;
        for s in Suit::ALL {
            assert_eq!(s.cards().intersect(all), CardSet::EMPTY);
            all = all.union(s.cards());
            assert!(s.cards().iter().all(|c| c.suit() == s));
        }
        assert_eq!(all, CardSet::FULL);
        assert_eq!(CardSet::colour_value(5).len(), 4);
    }
}
