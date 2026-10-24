//! The task card catalogue, loaded from `data/tasks.toml`.

use crate::card::{Card, CardSet, Suit};
use crate::task::{Cmp, Kind, TrickProp};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// One task card (or rule card) of the catalogue.
#[derive(Debug, Clone)]
pub struct TaskCard {
    pub id: String,
    pub category: String,
    /// Difficulty for 3/4/5 players; `None` for rule cards.
    pub difficulty: Option<[u8; 3]>,
    pub text_en: String,
    pub text_de: String,
    pub doubtful: bool,
    pub note: Option<String>,
    /// The deal must be redone if any player holds all cards of one of these sets.
    pub redeal_if: Vec<CardSet>,
    pub kind: Kind,
    /// A card of the game (from `data/tasks.toml`), not a generated one.
    pub official: bool,
}

impl TaskCard {
    pub fn difficulty_for(&self, players: usize) -> u8 {
        self.difficulty.map(|d| d[players - 3]).unwrap_or(0)
    }

    pub fn is_rule(&self) -> bool {
        self.kind.is_rule()
    }

    /// May the captain take this task?
    pub fn captain_allowed(&self) -> bool {
        !matches!(self.kind, Kind::VsCaptain(_))
    }

    pub fn redeal_triggered(&self, hands: &[CardSet]) -> bool {
        self.redeal_if.iter().any(|set| hands.iter().any(|h| h.intersect(*set) == *set))
    }
}

/// All cards, in file order. Panics if the embedded data is invalid
/// (guarded by tests).
pub fn catalogue() -> &'static [TaskCard] {
    static CAT: OnceLock<Vec<TaskCard>> = OnceLock::new();
    CAT.get_or_init(|| parse(include_str!("../data/tasks.toml")).expect("invalid data/tasks.toml"))
}

/// Find a card by ID; whitespace is normalised.
pub fn find(id: &str) -> Option<&'static TaskCard> {
    resolve(id).ok()
}

/// An official card by ID; otherwise a generated card for a parametric ID
/// (the official card if one has the same task). Generated cards are
/// interned, so each lives once for the program's lifetime.
pub fn resolve(id: &str) -> Result<&'static TaskCard, String> {
    let norm = normalise(id);
    if let Some(t) = catalogue().iter().find(|t| t.id == norm) {
        return Ok(t);
    }
    let card = crate::generate::generate(&norm)?;
    if let Some(t) = catalogue().iter().find(|t| t.kind == card.kind) {
        return Ok(t);
    }
    static GENERATED: Mutex<Option<HashMap<String, &'static TaskCard>>> = Mutex::new(None);
    let mut map = GENERATED.lock().unwrap();
    let map = map.get_or_insert_with(HashMap::new);
    Ok(*map.entry(card.id.clone()).or_insert_with(|| Box::leak(Box::new(card))))
}

pub fn normalise(id: &str) -> String {
    id.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn parse(src: &str) -> Result<Vec<TaskCard>, String> {
    let file: File = toml::from_str(src).map_err(|e| e.to_string())?;
    let mut out = Vec::with_capacity(file.task.len());
    for raw in file.task {
        let id = raw.id.clone();
        let card = convert(raw).map_err(|e| format!("task '{id}': {e}"))?;
        if out.iter().any(|t: &TaskCard| t.id == card.id) {
            return Err(format!("duplicate task id '{}'", card.id));
        }
        out.push(card);
    }
    Ok(out)
}

#[derive(Deserialize)]
struct File {
    task: Vec<Raw>,
}

#[derive(Deserialize)]
struct Raw {
    id: String,
    category: String,
    #[serde(default)]
    difficulty: Option<[u8; 3]>,
    text_en: String,
    #[serde(default)]
    text_de: String,
    #[serde(default)]
    doubtful: bool,
    #[serde(default)]
    note: Option<String>,
    #[serde(default)]
    redeal_if: Vec<Vec<String>>,
    kind: KindDef,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum KindDef {
    WinCards { cards: Vec<String> },
    WinNone { cards: Vec<String> },
    NoLead { cards: Vec<String> },
    WinWith { value: u8, #[serde(default)] containing: Option<u8> },
    VsCaptain { cmp: String },
    Trick { prop: PropDef },
    NoneOfFirst { k: u8 },
    WinTricks {
        #[serde(default)]
        first: u8,
        #[serde(default)]
        last: bool,
        #[serde(default)]
        only: bool,
    },
    ExactTricks { n: u8 },
    Predict,
    SuitCounts { counts: Vec<(String, u8)>, exact: bool },
    ValueCount { value: u8, n: u8, exact: bool },
    InARow { n: u8, exact: bool },
    NoTwoInARow,
    OnlySub { v: u8 },
    AllOfAColour,
    MoreOf { a: String, b: String },
    SameOf { a: String, b: String },
    MostTricks,
    MoreThanAllOthers,
    FewestTricks,
    CrewNoLead { cards: Vec<String> },
    LastCardOfGame { card: String },
    FirstWinnerAlwaysLeads,
    BalancedValue { value: u8 },
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum PropDef {
    AllBelow { bound: u8 },
    AllAbove { bound: u8 },
    AllEven,
    AllOdd,
    SumAbove { bounds: [u8; 3] },
    SumBelow { bounds: [u8; 3] },
    SumIn { values: Vec<u8> },
    SameCount { a: String, b: String },
    CardWonBySub { card: String },
    CardInLastTrick { card: String },
}

fn suit(s: &str) -> Result<Suit, String> {
    let mut ch = s.chars();
    match (ch.next().and_then(Suit::from_letter), ch.next()) {
        (Some(su), None) => Ok(su),
        _ => Err(format!("not a suit letter: '{s}'")),
    }
}

fn card(s: &str) -> Result<Card, String> {
    Card::parse(s).ok_or_else(|| format!("not a card: '{s}'"))
}

/// A card ("B4"), a suit ("P"), or a colour value ("9").
fn selector(tok: &str) -> Result<CardSet, String> {
    if let Ok(v) = tok.parse::<u8>() {
        if (1..=9).contains(&v) {
            return Ok(CardSet::colour_value(v));
        }
        return Err(format!("value out of range: '{tok}'"));
    }
    if tok.len() == 1 {
        return suit(tok).map(Suit::cards);
    }
    card(tok).map(CardSet::single)
}

fn selectors(toks: &[String]) -> Result<CardSet, String> {
    toks.iter().try_fold(CardSet::EMPTY, |acc, t| Ok(acc.union(selector(t)?)))
}

fn convert(raw: Raw) -> Result<TaskCard, String> {
    let kind = match raw.kind {
        KindDef::WinCards { cards } => Kind::WinCards(selectors(&cards)?),
        KindDef::WinNone { cards } => Kind::WinNone(selectors(&cards)?),
        KindDef::NoLead { cards } => Kind::NoLead(selectors(&cards)?),
        KindDef::WinWith { value, containing } => Kind::WinWith { value, containing },
        KindDef::VsCaptain { cmp } => Kind::VsCaptain(match cmp.as_str() {
            "more" => Cmp::More,
            "fewer" => Cmp::Fewer,
            "same" => Cmp::Same,
            o => return Err(format!("unknown cmp '{o}'")),
        }),
        KindDef::Trick { prop } => Kind::Trick(match prop {
            PropDef::AllBelow { bound } => TrickProp::AllBelow(bound),
            PropDef::AllAbove { bound } => TrickProp::AllAbove(bound),
            PropDef::AllEven => TrickProp::AllEven,
            PropDef::AllOdd => TrickProp::AllOdd,
            PropDef::SumAbove { bounds } => TrickProp::SumAbove(bounds),
            PropDef::SumBelow { bounds } => TrickProp::SumBelow(bounds),
            PropDef::SumIn { values } => TrickProp::SumIn(values),
            PropDef::SameCount { a, b } => TrickProp::SameCount(suit(&a)?, suit(&b)?),
            PropDef::CardWonBySub { card: c } => TrickProp::CardWonBySub(card(&c)?),
            PropDef::CardInLastTrick { card: c } => TrickProp::CardInLastTrick(card(&c)?),
        }),
        KindDef::NoneOfFirst { k } => Kind::NoneOfFirst(k),
        KindDef::WinTricks { first, last, only } => Kind::WinTricks { first, last, only },
        KindDef::ExactTricks { n } => Kind::ExactTricks(n),
        KindDef::Predict => Kind::Predict,
        KindDef::SuitCounts { counts, exact } => Kind::SuitCounts {
            counts: counts.iter().map(|(s, k)| Ok((suit(s)?, *k))).collect::<Result<_, String>>()?,
            exact,
        },
        KindDef::ValueCount { value, n, exact } => Kind::ValueCount { value, n, exact },
        KindDef::InARow { n, exact } => Kind::InARow { n, exact },
        KindDef::NoTwoInARow => Kind::NoTwoInARow,
        KindDef::OnlySub { v } => Kind::OnlySub(v),
        KindDef::AllOfAColour => Kind::AllOfAColour,
        KindDef::MoreOf { a, b } => Kind::MoreOf(suit(&a)?, suit(&b)?),
        KindDef::SameOf { a, b } => Kind::SameOf(suit(&a)?, suit(&b)?),
        KindDef::MostTricks => Kind::MostTricks,
        KindDef::MoreThanAllOthers => Kind::MoreThanAllOthers,
        KindDef::FewestTricks => Kind::FewestTricks,
        KindDef::CrewNoLead { cards } => Kind::CrewNoLead(selectors(&cards)?),
        KindDef::LastCardOfGame { card: c } => Kind::LastCardOfGame(card(&c)?),
        KindDef::FirstWinnerAlwaysLeads => Kind::FirstWinnerAlwaysLeads,
        KindDef::BalancedValue { value } => Kind::BalancedValue(value),
    };
    if kind.is_rule() != (raw.category == "rule") {
        return Err("rule kinds must have category 'rule' and vice versa".into());
    }
    if kind.is_rule() != raw.difficulty.is_none() {
        return Err("task cards need a difficulty, rule cards none".into());
    }
    Ok(TaskCard {
        id: normalise(&raw.id),
        category: raw.category,
        difficulty: raw.difficulty,
        text_en: raw.text_en,
        text_de: raw.text_de,
        doubtful: raw.doubtful,
        note: raw.note,
        redeal_if: raw.redeal_if.iter().map(|s| selectors(s)).collect::<Result<_, _>>()?,
        kind,
        official: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn catalogue_has_96_tasks_in_the_expected_categories() {
        let cat = catalogue();
        let mut per: BTreeMap<&str, usize> = BTreeMap::new();
        for t in cat {
            *per.entry(t.category.as_str()).or_default() += 1;
        }
        let expected = [
            ("captain", 3),
            ("cards", 16),
            ("counts", 10),
            ("lead", 2),
            ("none", 11),
            ("not-first", 4),
            ("number", 5),
            ("other", 9),
            ("row", 5),
            ("rule", 5),
            ("trick", 12),
            ("tricks", 7),
            ("values", 5),
            ("with", 7),
        ];
        assert_eq!(per.into_iter().collect::<Vec<_>>(), expected.to_vec());
        assert_eq!(cat.iter().filter(|t| !t.is_rule()).count(), 96);
    }

    #[test]
    fn lookup_and_redeal() {
        let t = find("win  P9 Y8").unwrap();
        assert_eq!(t.difficulty_for(4), 2);
        assert!(!find("more than captain").unwrap().captain_allowed());
        let only1 = find("only S1").unwrap();
        let s = |x: &str| x.split_whitespace().map(|c| Card::parse(c).unwrap()).collect::<CardSet>();
        assert!(only1.redeal_triggered(&[s("S1 S4 P1"), s("S2 S3")]));
        assert!(!only1.redeal_triggered(&[s("S1 S2"), s("S3 S4")]));
        assert!(find("no such task").is_none());
    }
}
