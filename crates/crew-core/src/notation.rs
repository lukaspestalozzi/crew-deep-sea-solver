//! The `crew 1` text format: parse and write situations.
//!
//! ```text
//! crew 1
//! title: Mission 13
//! players: 4
//! Player1: P2 P8 B4 B9 G1 G6 Y3 Y7 S2 S4
//! Player2: ...
//! Player4: rest
//! tasks:
//! Player2: win B4
//! ?:       win P9 Y8
//! crew:    rule M12 no-lead pink sub
//! played:
//! trick1 Player1: G6 G4 G2 G3
//! ```
//! Several situations in one text are separated by a line `---`.

use crate::card::{Card, CardSet};
use crate::catalogue;
use crate::situation::{Assignment, Owner, PlayedTrick, Situation};
use std::fmt;

pub const VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// 1-based line number in the whole text.
    pub line: usize,
    pub msg: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.msg)
    }
}

/// Parse one or more situations separated by `---` lines.
pub fn parse_all(text: &str) -> Result<Vec<Situation>, ParseError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut out = Vec::new();
    let mut chunk: Vec<(usize, &str)> = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if line.trim() == "---" {
            out.push(parse_lines(&chunk)?);
            chunk.clear();
        } else {
            chunk.push((i + 1, line));
        }
    }
    if chunk.iter().any(|(_, l)| !strip(l).is_empty()) || out.is_empty() {
        out.push(parse_lines(&chunk)?);
    }
    Ok(out)
}

/// Parse exactly one situation.
pub fn parse(text: &str) -> Result<Situation, ParseError> {
    let mut all = parse_all(text)?;
    if all.len() != 1 {
        return Err(ParseError { line: 1, msg: format!("expected one situation, found {}", all.len()) });
    }
    Ok(all.remove(0))
}

/// The line without its comment. A `title:` line keeps any `#`: titles are free text.
fn strip(line: &str) -> &str {
    let t = line.trim();
    if t.starts_with("title:") {
        return t;
    }
    line.split('#').next().unwrap().trim()
}

/// The most task cards a situation may have (the solver's limit).
pub const MAX_TASKS: usize = 25;

fn player_index(s: &str) -> Option<u8> {
    let n: u8 = s.strip_prefix("Player")?.parse().ok()?;
    (1..=5).contains(&n).then(|| n - 1)
}

enum Section {
    Header,
    Tasks,
    Played,
}

fn parse_lines(lines: &[(usize, &str)]) -> Result<Situation, ParseError> {
    let mut it = lines.iter().map(|(n, l)| (*n, strip(l))).filter(|(_, l)| !l.is_empty());
    let (first_line, first) = it.next().ok_or(ParseError { line: lines.first().map_or(1, |l| l.0), msg: "empty situation".into() })?;
    let err = |line: usize, msg: String| ParseError { line, msg };
    match first.strip_prefix("crew ").map(str::trim) {
        Some(v) if v == VERSION.to_string() => {}
        Some(v) => return Err(err(first_line, format!("format version {v} is not supported (this app reads version {VERSION})"))),
        None => return Err(err(first_line, "the first line must be 'crew 1'".into())),
    }

    let mut s = Situation::default();
    let mut players: Option<(usize, usize)> = None; // (count, line)
    let mut hands: Vec<(usize, u8, Option<CardSet>)> = Vec::new(); // (line, player, cards or rest)
    let mut section = Section::Header;

    for (ln, line) in it {
        if line == "tasks:" {
            section = Section::Tasks;
            continue;
        }
        if line == "played:" {
            section = Section::Played;
            continue;
        }
        let (key, val) = line
            .split_once(':')
            .map(|(k, v)| (k.trim(), v.trim()))
            .ok_or_else(|| err(ln, format!("expected 'key: value', got '{line}'")))?;
        match section {
            Section::Header => match key {
                "title" => s.title = Some(val.to_string()),
                "mission" => s.mission = Some(val.parse().map_err(|_| err(ln, format!("mission must be a number, got '{val}'")))?),
                "target" => s.target = Some(val.parse().map_err(|_| err(ln, format!("target must be a number, got '{val}'")))?),
                "sealed" => {
                    s.sealed = match val {
                        "yes" => true,
                        "no" => false,
                        _ => return Err(err(ln, format!("sealed must be yes or no, got '{val}'"))),
                    }
                }
                "players" => {
                    let n: usize = val.parse().map_err(|_| err(ln, format!("players must be 3, 4 or 5, got '{val}'")))?;
                    if !(3..=5).contains(&n) {
                        return Err(err(ln, format!("players must be 3, 4 or 5, got {n}")));
                    }
                    players = Some((n, ln));
                }
                k => {
                    let p = player_index(k).ok_or_else(|| err(ln, format!("unknown key '{k}'")))?;
                    if hands.iter().any(|h| h.1 == p) {
                        return Err(err(ln, format!("{k} is listed twice")));
                    }
                    let cards = if val == "rest" {
                        None
                    } else {
                        let mut set = CardSet::EMPTY;
                        for tok in val.split_whitespace() {
                            let c = Card::parse(tok).ok_or_else(|| err(ln, format!("'{tok}' is not a card")))?;
                            if set.contains(c) {
                                return Err(err(ln, format!("{c} appears twice")));
                            }
                            set.insert(c);
                        }
                        Some(set)
                    };
                    hands.push((ln, p, cards));
                }
            },
            Section::Tasks => {
                let owner = match key {
                    "?" => Owner::Unknown,
                    "crew" => Owner::Crew,
                    k => Owner::Player(player_index(k).ok_or_else(|| err(ln, format!("unknown owner '{k}'")))?),
                };
                let card = catalogue::resolve(val).map_err(|e| err(ln, e))?;
                if card.is_rule() != (owner == Owner::Crew) {
                    return Err(err(
                        ln,
                        if card.is_rule() {
                            format!("'{val}' is a rule card; its owner must be 'crew'")
                        } else {
                            format!("'{val}' is a task card; 'crew' is only for rule cards")
                        },
                    ));
                }
                if s.tasks.iter().any(|a| a.card.id == card.id) {
                    return Err(err(ln, format!("'{}' is listed twice; the deck has one of each card", card.id)));
                }
                if s.tasks.len() >= MAX_TASKS {
                    return Err(err(ln, format!("at most {MAX_TASKS} tasks per situation")));
                }
                s.tasks.push(Assignment { card, owner });
            }
            Section::Played => {
                let (num, leader) = key
                    .split_once(' ')
                    .ok_or_else(|| err(ln, format!("expected 'trickN PlayerK: cards', got '{key}'")))?;
                let n: usize = num
                    .strip_prefix("trick")
                    .and_then(|x| x.parse().ok())
                    .ok_or_else(|| err(ln, format!("expected 'trickN', got '{num}'")))?;
                if n != s.played.len() + 1 {
                    return Err(err(ln, format!("expected trick{}, got trick{n}", s.played.len() + 1)));
                }
                let leader = player_index(leader.trim()).ok_or_else(|| err(ln, format!("unknown player '{}'", leader.trim())))?;
                let cards = val
                    .split_whitespace()
                    .map(|t| Card::parse(t).ok_or_else(|| err(ln, format!("'{t}' is not a card"))))
                    .collect::<Result<Vec<_>, _>>()?;
                s.played.push(PlayedTrick { leader, cards });
            }
        }
    }

    // Hands.
    let (n, players_line) = players.ok_or_else(|| err(first_line, "missing 'players:' line".into()))?;
    if hands.len() != n {
        return Err(err(players_line, format!("players: {n}, but {} hands are listed", hands.len())));
    }
    if let Some(h) = hands.iter().find(|h| h.1 as usize >= n) {
        return Err(err(h.0, format!("Player{} does not exist with {n} players", h.1 + 1)));
    }
    let rests: Vec<_> = hands.iter().filter(|h| h.2.is_none()).collect();
    if rests.len() > 1 {
        return Err(err(rests[1].0, "only one player may hold 'rest'".into()));
    }
    let mut owner_of = [None::<(u8, usize)>; 40];
    let mut taken = CardSet::EMPTY;
    for &(ln, p, ref cards) in &hands {
        if let Some(cards) = cards {
            for c in cards.iter() {
                if let Some((q, _)) = owner_of[c.index() as usize] {
                    return Err(err(ln, format!("{c} appears twice, Player{} and Player{}", q + 1, p + 1)));
                }
                owner_of[c.index() as usize] = Some((p, ln));
            }
            taken = taken.union(*cards);
        }
    }
    let mut ordered = vec![CardSet::EMPTY; n];
    for &(_, p, ref cards) in &hands {
        ordered[p as usize] = cards.unwrap_or(CardSet::FULL.minus(taken));
    }
    s.hands = ordered;
    if let Err(e) = crate::game::Game::new(&s.hands) {
        return Err(err(players_line, e.to_string()));
    }
    for a in &s.tasks {
        if let Owner::Player(p) = a.owner {
            if p as usize >= n {
                return Err(err(players_line, format!("task '{}' is owned by Player{}, who does not exist", a.card.id, p + 1)));
            }
        }
    }
    Ok(s)
}

/// Write a situation in canonical form (all hands explicit).
pub fn write(s: &Situation) -> String {
    let mut out = format!("crew {VERSION}\n");
    if let Some(t) = &s.title {
        // One line, and nothing a reader would take for a comment or separator.
        let t: String = t.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
        out += &format!("title: {}\n", t.trim());
    }
    if let Some(m) = s.mission {
        out += &format!("mission: {m}\n");
    }
    if let Some(t) = s.target {
        out += &format!("target: {t}\n");
    }
    out += &format!("players: {}\n", s.players());
    out += &format!("sealed: {}\n\n", if s.sealed { "yes" } else { "no" });
    for (p, h) in s.hands.iter().enumerate() {
        let cards: Vec<String> = h.iter().map(|c| c.to_string()).collect();
        out += &format!("Player{}: {}\n", p + 1, cards.join(" "));
    }
    if !s.tasks.is_empty() {
        out += "\ntasks:\n";
        for a in &s.tasks {
            let owner = match a.owner {
                Owner::Player(p) => format!("Player{}", p + 1),
                Owner::Unknown => "?".into(),
                Owner::Crew => "crew".into(),
            };
            out += &format!("{:<8} {}\n", format!("{owner}:"), a.card.id);
        }
    }
    if !s.played.is_empty() {
        out += "\nplayed:\n";
        for (i, t) in s.played.iter().enumerate() {
            let cards: Vec<String> = t.cards.iter().map(|c| c.to_string()).collect();
            out += &format!("trick{} Player{}: {}\n", i + 1, t.leader + 1, cards.join(" "));
        }
    }
    out
}

/// Write several situations separated by `---`.
pub fn write_all(list: &[Situation]) -> String {
    list.iter().map(write).collect::<Vec<_>>().join("---\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "crew 1
title: Mission 13
mission: 13
target: 8
players: 4
sealed: no

Player1: P2 P8 B4 B9 G1 G6 Y3 Y7 S2 S4   # captain
Player2: P1 P5 B1 B2 B6 G4 G9 Y2 Y6 S1
Player3: P3 P6 B3 B8 G2 G5 G8 Y1 Y8 S3
Player4: rest

tasks:
Player2: win B4
?:       win P9 Y8
Player1: first-trick
?:       tricks =2
crew:    rule M12 no-lead pink sub

played:
trick1 Player1: G6 G4 G2 G3
trick2 Player1: B4 B6 B3 B5
";

    #[test]
    fn parses_the_spec_example() {
        let s = parse(SAMPLE).unwrap();
        assert_eq!(s.players(), 4);
        assert_eq!(s.title.as_deref(), Some("Mission 13"));
        assert_eq!(s.hands[3].len(), 10);
        assert_eq!(s.tasks.len(), 5);
        assert_eq!(s.tasks[1].owner, Owner::Unknown);
        assert_eq!(s.tasks[4].owner, Owner::Crew);
        assert_eq!(s.difficulty_sum(), 1 + 2 + 1 + 2);
        let g = s.game().unwrap();
        assert_eq!(g.tricks.len(), 2);
        assert_eq!(g.leader, 1); // Player2 won B4 with B6
    }

    #[test]
    fn write_then_parse_roundtrips() {
        let s = parse(SAMPLE).unwrap();
        let text = write(&s);
        let back = parse(&text).unwrap();
        assert_eq!(write(&back), text);
        let two = parse_all(&write_all(&[s.clone(), back])).unwrap();
        assert_eq!(two.len(), 2);
    }

    #[test]
    fn errors_name_the_line() {
        let dup = SAMPLE.replace("Player3: P3", "Player3: Y2 P3");
        let e = parse(&dup).unwrap_err();
        assert_eq!(e.to_string(), "line 10: Y2 appears twice, Player2 and Player3");
        let bad_task = SAMPLE.replace("win B4", "win B44");
        assert_eq!(parse(&bad_task).unwrap_err().line, 14);
        let bad_version = SAMPLE.replace("crew 1", "crew 2");
        assert!(parse(&bad_version).unwrap_err().msg.contains("version 2"));
        let wrong_leader = SAMPLE.replace("trick2 Player1", "trick2 Player3");
        let s = parse(&wrong_leader).unwrap();
        assert_eq!(s.game().unwrap_err().to_string(), "trick2: Player3 leads, but Player1 won the previous trick");
    }

    #[test]
    fn titles_roundtrip_with_hash_and_without_injection() {
        let mut s = parse(SAMPLE).unwrap();
        s.title = Some("bench seed 1 #1".into());
        let back = parse(&write(&s)).unwrap();
        assert_eq!(back.title.as_deref(), Some("bench seed 1 #1"));
        s.title = Some("a\n---\nb\r\tc".into());
        let text = write(&s);
        assert_eq!(parse_all(&text).unwrap().len(), 1);
        assert_eq!(parse(&text).unwrap().title.as_deref(), Some("a --- b  c"));
        // Other lines still have comments.
        assert_eq!(parse(&SAMPLE.replace("title: Mission 13", "title: M # x")).unwrap().title.as_deref(), Some("M # x"));
    }

    #[test]
    fn accepts_a_byte_order_mark() {
        let s = parse(&format!("\u{feff}{SAMPLE}")).unwrap();
        assert_eq!(s.players(), 4);
        assert_eq!(parse_all(&format!("\u{feff}{SAMPLE}---\n{SAMPLE}")).unwrap().len(), 2);
    }

    #[test]
    fn rejects_a_task_listed_twice() {
        let dup = SAMPLE.replace("Player1: first-trick", "Player1: win B4");
        let e = parse(&dup).unwrap_err();
        assert_eq!(e.line, 16);
        assert!(e.msg.contains("twice"), "{}", e.msg);
        // Same card through a differently written ID.
        let dup = SAMPLE.replace("?:       tricks =2", "Player3: win  P9 Y8");
        assert!(parse(&dup).unwrap_err().msg.contains("twice"));
    }

    #[test]
    fn rejects_more_than_25_tasks() {
        let ids: Vec<String> = crate::catalogue::catalogue().iter().filter(|t| !t.is_rule()).take(26).map(|t| t.id.clone()).collect();
        let header = "crew 1\nplayers: 4\nPlayer1: P2 P8 B4 B9 G1 G6 Y3 Y7 S2 S4\nPlayer2: P1 P5 B1 B2 B6 G4 G9 Y2 Y6 S1\nPlayer3: P3 P6 B3 B8 G2 G5 G8 Y1 Y8 S3\nPlayer4: rest\ntasks:\n";
        let text = |n: usize| header.to_string() + &ids[..n].iter().map(|i| format!("?: {i}\n")).collect::<String>();
        assert_eq!(parse(&text(25)).unwrap().tasks.len(), 25);
        let e = parse(&text(26)).unwrap_err();
        assert_eq!(e.line, 7 + 26);
        assert!(e.msg.contains("at most 25"), "{}", e.msg);
    }
}
