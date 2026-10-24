//! Generated (parametric) task cards: any task of the official categories,
//! written as an ID such as `win P3 Y7`, `tricks =3` or `no pink green`.
//!
//! The ID grammar extends the official IDs; an ID whose task equals an
//! official card resolves to that card (see [`crate::catalogue::find`]).
//! Generated cards carry an *inferred* difficulty: a rule of thumb per
//! category, fitted by eye to the printed values of that category. Generated
//! cards have no re-deal clauses. Rule cards are not generated.

use crate::card::{Card, CardSet, Suit};
use crate::catalogue::TaskCard;
use crate::task::{Kind, TrickProp};

/// Parse a parametric task ID into a task card (not interned).
pub fn generate(id: &str) -> Result<TaskCard, String> {
    let toks: Vec<&str> = id.split_whitespace().collect();
    let (kind, category) = parse_kind(&toks)?;
    Ok(TaskCard {
        id: canonical_id(&kind),
        category: category.to_string(),
        difficulty: Some(infer(&kind)),
        text_en: text_en(&kind),
        text_de: String::new(),
        doubtful: false,
        note: None,
        redeal_if: Vec::new(),
        kind,
        official: false,
    })
}

// ---------------------------------------------------------------- parsing

fn num(tok: &str, lo: u8, hi: u8) -> Result<u8, String> {
    match tok.parse::<u8>() {
        Ok(v) if (lo..=hi).contains(&v) => Ok(v),
        _ => Err(format!("'{tok}' must be a number from {lo} to {hi}")),
    }
}

/// "3s" → 3.
fn plural_value(tok: &str) -> Result<u8, String> {
    tok.strip_suffix('s')
        .and_then(|v| v.parse::<u8>().ok())
        .filter(|v| (1..=9).contains(v))
        .ok_or_else(|| format!("'{tok}' must be a value like 3s"))
}

fn word_number(tok: &str) -> Option<u8> {
    ["one", "two", "three", "four"].iter().position(|w| *w == tok).map(|i| i as u8 + 1)
}

fn colour(tok: &str) -> Option<Suit> {
    match tok {
        "pink" => Some(Suit::Pink),
        "blue" => Some(Suit::Blue),
        "green" => Some(Suit::Green),
        "yellow" => Some(Suit::Yellow),
        _ => None,
    }
}

fn colour_or_err(tok: &str) -> Result<Suit, String> {
    colour(tok).ok_or_else(|| format!("'{tok}' must be pink, blue, green or yellow"))
}

fn card(tok: &str) -> Result<Card, String> {
    Card::parse(tok).ok_or_else(|| format!("'{tok}' is not a card"))
}

fn colour_card(tok: &str) -> Result<Card, String> {
    let c = card(tok)?;
    if c.is_sub() {
        return Err(format!("'{tok}' must be a colour card"));
    }
    Ok(c)
}

fn distinct_cards(toks: &[&str]) -> Result<CardSet, String> {
    let mut set = CardSet::EMPTY;
    for t in toks {
        let c = card(t)?;
        if set.contains(c) {
            return Err(format!("{t} appears twice"));
        }
        set.insert(c);
    }
    Ok(set)
}

/// "pink=yellow" → (Pink, Yellow), sorted.
fn colour_pair(tok: &str) -> Option<Result<(Suit, Suit), String>> {
    let (a, b) = tok.split_once('=')?;
    Some((|| {
        let (a, b) = (colour_or_err(a)?, colour_or_err(b)?);
        if a == b {
            return Err("the two colours must differ".to_string());
        }
        Ok((a.min(b), a.max(b)))
    })())
}

fn parse_kind(t: &[&str]) -> Result<(Kind, &'static str), String> {
    use Kind as K;
    let bad = || Err(format!("unknown task '{}'", t.join(" ")));
    Ok(match t {
        [] => return Err("empty task".into()),
        // ---- win with
        ["win", "with", v] => (K::WinWith { value: num(v, 1, 9)?, containing: None }, "with"),
        ["win", c, "with", "sub"] => (K::Trick(TrickProp::CardWonBySub(colour_card(c)?)), "trick"),
        ["win", w, "with", v] => (K::WinWith { value: num(v, 1, 9)?, containing: Some(num(w, 1, 9)?) }, "with"),
        ["win", c, "last"] => (K::Trick(TrickProp::CardInLastTrick(card(c)?)), "trick"),
        ["win", "all", v] => (K::WinCards(CardSet::colour_value(plural_value(v)?)), "cards"),
        ["win", cards @ ..] if !cards.is_empty() => (K::WinCards(distinct_cards(cards)?), "cards"),
        // ---- win none
        ["no", groups @ ..] if !groups.is_empty() => {
            let mut set = CardSet::EMPTY;
            for g in groups {
                let part = if let Some(s) = colour(g) {
                    s.cards()
                } else if *g == "subs" {
                    Suit::Sub.cards()
                } else {
                    CardSet::colour_value(plural_value(g).map_err(|_| format!("'{g}' must be a colour, subs, or a value like 3s"))?)
                };
                if !set.intersect(part).is_empty() {
                    return Err(format!("{g} overlaps an earlier part"));
                }
                set = set.union(part);
            }
            (K::WinNone(set), "none")
        }
        ["no-lead", cols @ ..] if !cols.is_empty() => {
            let mut set = CardSet::EMPTY;
            for c in cols {
                let s = colour_or_err(c)?;
                if !set.intersect(s.cards()).is_empty() {
                    return Err(format!("{c} appears twice"));
                }
                set = set.union(s.cards());
            }
            (K::NoLead(set), "lead")
        }
        // ---- trick properties
        ["trick", "below", n] => (K::Trick(TrickProp::AllBelow(num(n, 2, 9)?)), "trick"),
        ["trick", "above", n] => (K::Trick(TrickProp::AllAbove(num(n, 1, 8)?)), "trick"),
        ["trick", "even"] => (K::Trick(TrickProp::AllEven), "trick"),
        ["trick", "odd"] => (K::Trick(TrickProp::AllOdd), "trick"),
        ["trick", "sum", s] => {
            let prop = if let Some(n) = s.strip_prefix('>') {
                TrickProp::SumAbove([num(n, 2, 44)?; 3])
            } else if let Some(n) = s.strip_prefix('<') {
                TrickProp::SumBelow([num(n, 4, 46)?; 3])
            } else if let Some((a, b)) = s.split_once('-') {
                let (a, b) = (num(a, 3, 45)?, num(b, 3, 45)?);
                if a > b {
                    return Err(format!("'{s}': the lower bound comes first"));
                }
                TrickProp::SumIn((a..=b).collect())
            } else {
                return Err(format!("'{s}' must be >N, <N or A-B"));
            };
            (K::Trick(prop), "trick")
        }
        ["trick", pair] => match colour_pair(pair) {
            Some(p) => {
                let (a, b) = p?;
                (K::Trick(TrickProp::SameCount(a, b)), "trick")
            }
            None => return bad(),
        },
        // ---- specific tricks and trick counts
        ["none", "of", "first", k] => (K::NoneOfFirst(num(k, 1, 12)?), "not-first"),
        ["tricks", n] => match n.strip_prefix('=') {
            Some(n) => (K::ExactTricks(num(n, 0, 13)?), "number"),
            None => return Err(format!("'{n}' must be =N")),
        },
        ["first", n] => match n.strip_suffix("+last") {
            Some(n) => (K::WinTricks { first: num(n, 1, 12)?, last: true, only: false }, "tricks"),
            None => (K::WinTricks { first: num(n, 1, 13)?, last: false, only: false }, "tricks"),
        },
        ["only", "first", n] => (K::WinTricks { first: num(n, 1, 12)?, last: false, only: true }, "tricks"),
        ["only", s] if s.starts_with('S') => {
            let c = card(s)?;
            (K::OnlySub(c.value()), "other")
        }
        // ---- rows
        ["row", n] => (K::InARow { n: num(n, 2, 13)?, exact: false }, "row"),
        ["exactly", "row", n] => (K::InARow { n: num(n, 2, 13)?, exact: true }, "row"),
        // ---- value counts: "at least three 5s", "exactly two 9s"
        ["at", "least", w, v] | ["exactly", w, v] if word_number(w).is_some() => (
            K::ValueCount { value: plural_value(v)?, n: word_number(w).unwrap(), exact: t[0] == "exactly" },
            "values",
        ),
        // ---- suit counts: "exactly 1 pink 1 green", "at least 7 yellow", "exactly 2 subs"
        ["at", "least", rest @ ..] | ["exactly", rest @ ..] if !rest.is_empty() && rest.len() % 2 == 0 => {
            let mut counts: Vec<(Suit, u8)> = Vec::new();
            for pair in rest.chunks(2) {
                let (s, max) = match pair[1] {
                    "sub" | "subs" => (Suit::Sub, 4),
                    c => (colour_or_err(c)?, 9),
                };
                if counts.iter().any(|(x, _)| *x == s) {
                    return Err(format!("{} appears twice", pair[1]));
                }
                counts.push((s, num(pair[0], 1, max)?));
            }
            counts.sort();
            (K::SuitCounts { counts, exact: t[0] == "exactly" }, "counts")
        }
        // ---- comparisons between suits
        ["more", a, "than", b] => {
            let (a, b) = (colour_or_err(a)?, colour_or_err(b)?);
            if a == b {
                return Err("the two colours must differ".into());
            }
            (K::MoreOf(a, b), "other")
        }
        [pair] => match colour_pair(pair) {
            Some(p) => {
                let (a, b) = p?;
                (K::SameOf(a, b), "other")
            }
            None => return bad(),
        },
        _ => return bad(),
    })
}

// ---------------------------------------------------------------- canonical ID and text

fn colour_name(s: Suit) -> &'static str {
    match s {
        Suit::Pink => "pink",
        Suit::Blue => "blue",
        Suit::Green => "green",
        Suit::Yellow => "yellow",
        Suit::Sub => "sub",
    }
}

fn cards_str(set: CardSet) -> Vec<String> {
    set.iter().map(|c| c.to_string()).collect()
}

/// Colour cards of one value, if the set is exactly the four of them.
fn as_value(set: CardSet) -> Option<u8> {
    (1..=9).find(|&v| CardSet::colour_value(v) == set)
}

/// Splits a set into whole colours, subs and whole values (in that order).
fn groups(set: CardSet) -> Option<Vec<String>> {
    let mut rest = set;
    let mut out = Vec::new();
    for s in Suit::COLOURS {
        if rest.intersect(s.cards()) == s.cards() {
            out.push(colour_name(s).to_string());
            rest = rest.minus(s.cards());
        }
    }
    if rest.intersect(Suit::Sub.cards()) == Suit::Sub.cards() {
        out.push("subs".into());
        rest = rest.minus(Suit::Sub.cards());
    }
    for v in 1..=9 {
        let vs = CardSet::colour_value(v);
        if rest.intersect(vs) == vs {
            out.push(format!("{v}s"));
            rest = rest.minus(vs);
        }
    }
    rest.is_empty().then_some(out)
}

const WORDS: [&str; 4] = ["one", "two", "three", "four"];

pub fn canonical_id(kind: &Kind) -> String {
    use Kind as K;
    match kind {
        K::WinCards(set) => match as_value(*set) {
            Some(v) => format!("win all {v}s"),
            None => format!("win {}", cards_str(*set).join(" ")),
        },
        K::WinNone(set) => format!("no {}", groups(*set).unwrap_or_default().join(" ")),
        K::NoLead(set) => format!("no-lead {}", groups(*set).unwrap_or_default().join(" ")),
        K::WinWith { value, containing: None } => format!("win with {value}"),
        K::WinWith { value, containing: Some(w) } => format!("win {w} with {value}"),
        K::Trick(p) => match p {
            TrickProp::AllBelow(n) => format!("trick below {n}"),
            TrickProp::AllAbove(n) => format!("trick above {n}"),
            TrickProp::AllEven => "trick even".into(),
            TrickProp::AllOdd => "trick odd".into(),
            TrickProp::SumAbove(b) => format!("trick sum >{}", b[0]),
            TrickProp::SumBelow(b) => format!("trick sum <{}", b[0]),
            TrickProp::SumIn(v) => format!("trick sum {}-{}", v[0], v[v.len() - 1]),
            TrickProp::SameCount(a, b) => format!("trick {}={}", colour_name(*a), colour_name(*b)),
            TrickProp::CardWonBySub(c) => format!("win {c} with sub"),
            TrickProp::CardInLastTrick(c) => format!("win {c} last"),
        },
        K::NoneOfFirst(k) => format!("none of first {k}"),
        K::WinTricks { first, last, only } => match (only, last) {
            (false, false) => format!("first {first}"),
            (false, true) => format!("first {first}+last"),
            (true, false) => format!("only first {first}"),
            (true, true) => format!("only first {first}+last"),
        },
        K::ExactTricks(n) => format!("tricks ={n}"),
        K::SuitCounts { counts, exact } => {
            let parts: Vec<String> = counts
                .iter()
                .map(|(s, n)| match s {
                    Suit::Sub => format!("{n} {}", if *n == 1 { "sub" } else { "subs" }),
                    _ => format!("{n} {}", colour_name(*s)),
                })
                .collect();
            format!("{} {}", if *exact { "exactly" } else { "at least" }, parts.join(" "))
        }
        K::ValueCount { value, n, exact } => {
            format!("{} {} {value}s", if *exact { "exactly" } else { "at least" }, WORDS[*n as usize - 1])
        }
        K::InARow { n, exact } => format!("{}row {n}", if *exact { "exactly " } else { "" }),
        K::OnlySub(v) => format!("only S{v}"),
        K::MoreOf(a, b) => format!("more {} than {}", colour_name(*a), colour_name(*b)),
        K::SameOf(a, b) => format!("{}={}", colour_name(*a), colour_name(*b)),
        _ => String::new(),
    }
}

fn and_list(items: &[String], conj: &str) -> String {
    match items {
        [] => String::new(),
        [a] => a.clone(),
        [init @ .., last] => format!("{} {conj} {last}", init.join(", ")),
    }
}

fn card_name(c: Card) -> String {
    if c.is_sub() { format!("submarine {}", c.value()) } else { c.to_string() }
}

fn plural(n: u8, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn article(v: u8) -> &'static str {
    if v == 8 { "an" } else { "a" }
}

pub fn text_en(kind: &Kind) -> String {
    use Kind as K;
    let body = match kind {
        K::WinCards(set) => match as_value(*set) {
            Some(v) => format!("win all four {v}s"),
            None => format!("win {}", and_list(&set.iter().map(card_name).collect::<Vec<_>>(), "and")),
        },
        K::WinNone(set) => {
            let parts: Vec<String> = groups(*set)
                .unwrap_or_default()
                .into_iter()
                .map(|g| match g.as_str() {
                    "subs" => "no submarines".into(),
                    g if g.ends_with('s') && g.len() == 2 => format!("no {g}"),
                    g => format!("no {g} cards"),
                })
                .collect();
            format!("win {}", and_list(&parts, "and"))
        }
        K::NoLead(set) => format!("not open a trick with {}", and_list(&groups(*set).unwrap_or_default(), "or")),
        K::WinWith { value, containing: None } => format!("win a trick with {} {value}", article(*value)),
        K::WinWith { value, containing: Some(w) } if w == value => format!("win {} {w} with another {value}", article(*w)),
        K::WinWith { value, containing: Some(w) } => format!("win {} {w} with {} {value}", article(*w), article(*value)),
        K::Trick(p) => match p {
            TrickProp::AllBelow(n) => format!("win a trick whose cards are all below {n} (no submarines)"),
            TrickProp::AllAbove(n) => format!("win a trick whose cards are all above {n} (no submarines)"),
            TrickProp::AllEven => "win a trick of only even cards".into(),
            TrickProp::AllOdd => "win a trick of only odd cards".into(),
            TrickProp::SumAbove(b) => format!("win a trick with a total above {} (no submarines)", b[0]),
            TrickProp::SumBelow(b) => format!("win a trick with a total below {} (no submarines)", b[0]),
            TrickProp::SumIn(v) if v.len() == 1 => format!("win a trick with a total of {} (no submarines)", v[0]),
            TrickProp::SumIn(v) => format!("win a trick with a total from {} to {} (no submarines)", v[0], v[v.len() - 1]),
            TrickProp::SameCount(a, b) => {
                format!("win a trick with as many {} as {} cards (at least one each)", colour_name(*a), colour_name(*b))
            }
            TrickProp::CardWonBySub(c) => format!("win {c} with a submarine"),
            TrickProp::CardInLastTrick(c) => format!("win {} in the last trick", card_name(*c)),
        },
        K::NoneOfFirst(k) => format!("win none of the first {}", plural(*k, "trick", "tricks")),
        K::WinTricks { first, last, only } => {
            let firsts = if *first == 1 { "the first trick".to_string() } else { format!("the first {first} tricks") };
            match (only, last) {
                (true, _) => format!("win only {firsts}"),
                (false, true) => format!("win {firsts} and the last trick"),
                (false, false) => format!("win {firsts}"),
            }
        }
        K::ExactTricks(0) => "win no tricks".into(),
        K::ExactTricks(n) => format!("win exactly {}", plural(*n, "trick", "tricks")),
        K::SuitCounts { counts, exact } => {
            let parts: Vec<String> = counts
                .iter()
                .map(|(s, n)| match s {
                    Suit::Sub => plural(*n, "submarine", "submarines"),
                    _ => plural(*n, &format!("{} card", colour_name(*s)), &format!("{} cards", colour_name(*s))),
                })
                .collect();
            format!("win {} {}", if *exact { "exactly" } else { "at least" }, and_list(&parts, "and"))
        }
        K::ValueCount { value, n, exact } => {
            format!("win {} {} {value}s", if *exact { "exactly" } else { "at least" }, WORDS[*n as usize - 1])
        }
        K::InARow { n, exact: false } => format!("win {n} tricks in a row"),
        K::InARow { n, exact: true } => format!("win exactly {n} tricks, in a row"),
        K::OnlySub(v) => format!("win submarine {v} and no other submarine"),
        K::MoreOf(a, b) => format!(
            "win more {} than {} cards (0 {} allowed)",
            colour_name(*a),
            colour_name(*b),
            colour_name(*b)
        ),
        K::SameOf(a, b) => format!("win as many {} as {} cards (at least one each)", colour_name(*a), colour_name(*b)),
        _ => String::new(),
    };
    format!("I will {body}.")
}

// ---------------------------------------------------------------- inferred difficulty

/// Rule-of-thumb difficulty for 3/4/5 players, fitted by eye to the printed
/// values of the same category. Placeholder until measured values exist.
pub fn infer(kind: &Kind) -> [u8; 3] {
    use Kind as K;
    let d = |a: u8, b: u8, c: u8| [a, b, c];
    let raw = match kind {
        K::WinCards(set) => {
            let k = set.len() as u8;
            // 1 card: 1; 2 cards: 2 2 3; 3: 3 4 4; 4: 4 5 5; then +1 per card.
            d(k, k + (k >= 3) as u8, k + (k >= 2) as u8)
        }
        K::WinNone(set) => {
            let g = groups(*set).unwrap_or_default();
            let colours = g.iter().filter(|x| colour(x).is_some()).count() as u8;
            let subs = g.iter().any(|x| x == "subs") as u8;
            let values = g.len() as u8 - colours - subs;
            let v = (colours > 0) as u8 + colours + subs + values;
            d(v, v, v)
        }
        K::NoLead(set) => match groups(*set).unwrap_or_default().len() {
            1 => d(1, 1, 1),
            2 => d(2, 1, 1),
            3 => d(4, 3, 3),
            _ => d(5, 4, 4),
        },
        K::WinWith { value, containing } => {
            let base = match value {
                1 => d(4, 5, 6),
                2 | 3 => d(3, 4, 5),
                4 => d(3, 4, 4),
                5 => d(2, 3, 4),
                6 => d(2, 3, 3),
                7 => d(2, 2, 3),
                8 => d(1, 2, 2),
                _ => d(1, 1, 2),
            };
            match containing {
                // A higher card beaten by a lower one is harder.
                Some(w) if w > value => base.map(|x| x + 1),
                _ => base,
            }
        }
        K::Trick(p) => match p {
            TrickProp::AllBelow(_) | TrickProp::AllAbove(_) => {
                let allowed = match p {
                    TrickProp::AllBelow(n) => n - 1,
                    TrickProp::AllAbove(n) => 9 - n,
                    _ => unreachable!(),
                };
                match allowed {
                    8.. => d(1, 1, 2),
                    7 => d(1, 2, 2),
                    5 | 6 => d(2, 3, 3),
                    4 => d(2, 3, 4),
                    3 => d(3, 4, 5),
                    2 => d(4, 5, 6),
                    _ => d(5, 6, 7),
                }
            }
            TrickProp::AllEven => d(2, 5, 6),
            TrickProp::AllOdd => d(2, 4, 5),
            // Printed: above 23/28/31 and below 8/12/16 are 3 3 4; a step of 3 is one point.
            TrickProp::SumAbove(b) => {
                let r = [23i32, 28, 31];
                std::array::from_fn(|i| (3 + (i == 2) as i32 + (b[i] as i32 - r[i]) / 3).clamp(1, 9) as u8)
            }
            TrickProp::SumBelow(b) => {
                let r = [8i32, 12, 16];
                std::array::from_fn(|i| (3 + (i == 2) as i32 + (r[i] - b[i] as i32) / 3).clamp(1, 9) as u8)
            }
            TrickProp::SumIn(v) => match v.len() {
                1 => d(4, 4, 5),
                2..=4 => d(3, 3, 4),
                _ => d(2, 2, 3),
            },
            TrickProp::SameCount(..) => d(2, 3, 3),
            TrickProp::CardWonBySub(_) => d(3, 3, 3),
            TrickProp::CardInLastTrick(_) => d(3, 4, 5),
        },
        K::NoneOfFirst(k) => {
            let k = *k;
            d(
                1 + (k >= 5) as u8 + (k >= 7) as u8,
                1 + (k >= 3) as u8 + (k >= 5) as u8 + (k >= 8) as u8,
                1 + (k >= 3) as u8 + (k >= 4) as u8 + (k >= 6) as u8,
            )
        }
        K::WinTricks { first, last, only } => {
            let f = *first;
            let firsts = match f {
                1 => d(1, 1, 1),
                2 => d(1, 1, 2),
                _ => d(f - 1, f, f + 1),
            };
            match (only, last) {
                // Printed "only the first trick": 4 3 3.
                (true, _) => d(3 + f, 2 + f, 2 + f),
                (false, true) => [firsts[0] + 2, firsts[1] + 3, firsts[2] + 3],
                (false, false) => firsts,
            }
        }
        K::ExactTricks(n) => match n {
            0 => d(4, 3, 3),
            1 => d(3, 2, 2),
            2 => d(2, 2, 2),
            3 => d(2, 2, 3),
            n => d(2 + (n - 4), 3 + (n - 4), 5 + (n - 4)),
        },
        K::SuitCounts { counts, exact } => {
            let one = |s: Suit, n: u8| -> [u8; 3] {
                match (s, exact) {
                    (Suit::Sub, true) => match n {
                        1 => d(3, 3, 3),
                        2 => d(3, 3, 4),
                        3 => d(3, 4, 4),
                        _ => d(4, 5, 5),
                    },
                    (Suit::Sub, false) => match n {
                        1 => d(1, 1, 1),
                        2 => d(2, 2, 3),
                        3 => d(3, 4, 4),
                        _ => d(4, 5, 5),
                    },
                    (_, true) => if n == 1 { d(3, 3, 4) } else { d(3, 4, 4) },
                    (_, false) => match n {
                        1..=3 => d(1, 1, 1),
                        4 => d(1, 2, 2),
                        5 => d(2, 3, 3),
                        6 | 7 => d(3, 3, 3),
                        8 => d(4, 4, 4),
                        _ => d(4, 5, 5),
                    },
                }
            };
            let each: Vec<[u8; 3]> = counts.iter().map(|&(s, n)| one(s, n)).collect();
            let extra = each.len() as u8 - 1;
            std::array::from_fn(|i| each.iter().map(|x| x[i]).max().unwrap() + extra)
        }
        K::ValueCount { n, exact, .. } => match (n, exact) {
            (1, false) => d(1, 1, 1),
            (2, false) => d(2, 2, 2),
            (3, false) => d(3, 4, 5),
            (1, true) => d(2, 2, 3),
            (2, true) => d(2, 3, 3),
            (3, true) => d(3, 4, 4),
            _ => d(4, 5, 6),
        },
        K::InARow { n, exact: false } => match n {
            2 => d(1, 1, 1),
            3 => d(2, 3, 4),
            n => d(*n - 1, *n, *n + 1),
        },
        K::InARow { n, exact: true } => match n {
            2 => d(3, 3, 3),
            3 => d(3, 3, 4),
            n => d(*n, *n, *n + 1),
        },
        K::OnlySub(_) => d(3, 3, 3),
        K::MoreOf(..) => d(1, 1, 1),
        K::SameOf(..) => d(4, 4, 4),
        _ => d(1, 1, 1),
    };
    raw.map(|x| x.clamp(1, 9))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalogue::{catalogue, resolve};

    /// Official IDs the grammar does not cover (fixed cards without parameters,
    /// or parameters the grammar cannot express such as per-player sum bounds).
    const FIXED: [&str; 18] = [
        "more than captain",
        "fewer than captain",
        "same as captain",
        "trick sum high",
        "trick sum low",
        "first-trick",
        "last-trick",
        "first+last",
        "only first",
        "only last",
        "predict",
        "predict secret",
        "one of each colour",
        "no row",
        "all of a colour",
        "most tricks",
        "more than all others",
        "fewest tricks",
    ];

    #[test]
    fn official_ids_parse_to_their_own_task() {
        let mut uncovered: Vec<String> = Vec::new();
        for t in catalogue().iter().filter(|t| !t.is_rule()) {
            match generate(&t.id) {
                Ok(g) => assert_eq!(g.kind, t.kind, "'{}' parses to a different task", t.id),
                Err(_) if FIXED.contains(&t.id.as_str()) => {}
                Err(e) => uncovered.push(format!("{}: {e}", t.id)),
            }
        }
        assert!(uncovered.is_empty(), "official IDs outside the grammar: {uncovered:#?}");
    }

    #[test]
    fn resolve_prefers_official_and_interns_generated() {
        assert_eq!(resolve("win B3 B1 B2").unwrap().id, "win B1 B2 B3");
        assert_eq!(resolve("win Y7 B6").unwrap().id, "win B6 Y7");
        assert_eq!(resolve("no-lead pink blue yellow").unwrap().id, "no-lead pink yellow blue");
        assert_eq!(resolve("first 1").unwrap().id, "first-trick");

        let g = resolve("win Y7  P2").unwrap();
        assert!(!g.official);
        assert_eq!(g.id, "win P2 Y7");
        assert_eq!(g.text_en, "I will win P2 and Y7.");
        assert_eq!(g.difficulty, Some([2, 2, 3]));
        assert!(std::ptr::eq(g, resolve("win P2 Y7").unwrap()), "generated cards are interned");

        let t = resolve("tricks =3").unwrap();
        assert_eq!((t.text_en.as_str(), t.difficulty), ("I will win exactly 3 tricks.", Some([2, 2, 3])));

        assert_eq!(resolve("win P2 P2").unwrap_err(), "P2 appears twice");
        assert_eq!(resolve("tricks =20").unwrap_err(), "'20' must be a number from 0 to 13");
        assert!(resolve("fly to the moon").unwrap_err().starts_with("unknown task"));
    }

    #[test]
    fn every_branch_round_trips() {
        let ids = [
            "win S4", "win P1 B2 G3 Y4 S1", "win all 7s", "no blue", "no subs 4s", "no pink green yellow subs", "no 1s 2s",
            "no-lead blue", "no-lead pink blue green yellow", "win with 1", "win 9 with 2", "win 3 with 3",
            "win Y3 with sub", "win S2 last", "trick below 4", "trick above 8", "trick even", "trick odd",
            "trick sum >20", "trick sum <10", "trick sum 15-15", "trick sum 12-14", "trick blue=yellow",
            "none of first 7", "tricks =0", "tricks =6", "first 4", "first 2+last", "only first 3",
            "row 5", "exactly row 4", "at least 2 subs", "exactly 4 subs", "at least 3 blue 2 yellow",
            "exactly 1 pink 2 blue 1 sub", "at least one 1s", "exactly four 2s", "only S3", "only S4",
            "more green than pink", "blue=green",
        ];
        for id in ids {
            let g = generate(id).unwrap_or_else(|e| panic!("{id}: {e}"));
            let again = generate(&g.id).unwrap_or_else(|e| panic!("{id} → {}: {e}", g.id));
            assert_eq!(again.kind, g.kind, "{id} → {}", g.id);
            assert_eq!(again.id, g.id);
            assert!(g.text_en.len() > 12 && !g.text_en.contains("  "), "{id}: text '{}'", g.text_en);
        }
    }

    /// Inferred difficulty against the printed one, on official cards in the grammar.
    #[test]
    fn inferred_difficulty_is_close_to_printed() {
        let (mut n, mut err, mut worst) = (0, 0u32, (0u8, String::new()));
        for t in catalogue().iter().filter(|t| !t.is_rule()) {
            let Ok(g) = generate(&t.id) else { continue };
            let (a, b) = (g.difficulty.unwrap(), t.difficulty.unwrap());
            for i in 0..3 {
                let e = a[i].abs_diff(b[i]);
                err += e as u32;
                n += 1;
                if e > worst.0 {
                    worst = (e, format!("{} ({} players): inferred {}, printed {}", t.id, i + 3, a[i], b[i]));
                }
            }
        }
        let mean = err as f64 / n as f64;
        println!("mean |inferred - printed| = {mean:.2} over {n} values; worst: {}", worst.1);
        assert!(mean <= 0.5 && worst.0 <= 2, "mean {mean:.2}, worst {}", worst.1);
    }
}
