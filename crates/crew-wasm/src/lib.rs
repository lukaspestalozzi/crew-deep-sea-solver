//! WebAssembly bindings. Every function takes and returns JSON strings, and
//! situations travel in the `crew 1` text format.
//!
//! Results are `{"ok": ...}` or `{"error": {"line"?: n, "msg": "..."}}`.
//! Long calls accept an optional progress callback; cancel by terminating
//! the Web Worker that runs them.

use crew_core::analysis::{self, DraftOptions, NoReturn};
use crew_core::card::Card;
use crew_core::catalogue::catalogue;
use crew_core::notation;
use crew_core::situation::{Assignment, Owner, PlayedTrick, Situation};
use crew_core::solver::{self, Options, Verdict};
use crew_core::task::{Kind, TrickProp};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use wasm_bindgen::prelude::*;

// ---------------------------------------------------------------- DTOs

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SituationDto {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub mission: Option<u32>,
    #[serde(default)]
    pub target: Option<u32>,
    #[serde(default)]
    pub sealed: bool,
    /// Cards per player, `Player1` first.
    pub hands: Vec<Vec<String>>,
    #[serde(default)]
    pub tasks: Vec<TaskDto>,
    #[serde(default)]
    pub played: Vec<TrickDto>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct TaskDto {
    pub id: String,
    /// "Player1".."Player5", "?" or "crew".
    pub owner: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct TrickDto {
    /// 0-based player index.
    pub leader: u8,
    pub cards: Vec<String>,
}

fn owner_str(o: Owner) -> String {
    match o {
        Owner::Player(p) => format!("Player{}", p + 1),
        Owner::Unknown => "?".into(),
        Owner::Crew => "crew".into(),
    }
}

fn owner_from(s: &str) -> Result<Owner, String> {
    match s {
        "?" => Ok(Owner::Unknown),
        "crew" => Ok(Owner::Crew),
        _ => s
            .strip_prefix("Player")
            .and_then(|n| n.parse::<u8>().ok())
            .filter(|n| (1..=5).contains(n))
            .map(|n| Owner::Player(n - 1))
            .ok_or_else(|| format!("unknown owner '{s}'")),
    }
}

fn card(s: &str) -> Result<Card, String> {
    Card::parse(s).ok_or_else(|| format!("'{s}' is not a card"))
}

pub fn to_dto(s: &Situation) -> SituationDto {
    SituationDto {
        title: s.title.clone(),
        mission: s.mission,
        target: s.target,
        sealed: s.sealed,
        hands: s.hands.iter().map(|h| h.iter().map(|c| c.to_string()).collect()).collect(),
        tasks: s.tasks.iter().map(|a| TaskDto { id: a.card.id.clone(), owner: owner_str(a.owner) }).collect(),
        played: s
            .played
            .iter()
            .map(|t| TrickDto { leader: t.leader, cards: t.cards.iter().map(|c| c.to_string()).collect() })
            .collect(),
    }
}

/// Builds a situation without checking the deal; [`Situation::game`] checks it.
pub fn from_dto(d: &SituationDto) -> Result<Situation, String> {
    let hands = d
        .hands
        .iter()
        .map(|h| h.iter().map(|c| card(c)).collect::<Result<_, _>>())
        .collect::<Result<Vec<_>, _>>()?;
    let tasks = d
        .tasks
        .iter()
        .map(|t| {
            let c = crew_core::catalogue::find(&t.id).ok_or_else(|| format!("unknown task '{}'", t.id))?;
            Ok(Assignment { card: c, owner: owner_from(&t.owner)? })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let played = d
        .played
        .iter()
        .map(|t| Ok(PlayedTrick { leader: t.leader, cards: t.cards.iter().map(|c| card(c)).collect::<Result<_, String>>()? }))
        .collect::<Result<Vec<_>, String>>()?;
    Ok(Situation {
        title: d.title.clone(),
        mission: d.mission,
        target: d.target,
        sealed: d.sealed,
        hands,
        tasks,
        played,
    })
}

fn ok(v: Value) -> String {
    json!({ "ok": v }).to_string()
}

fn err(msg: impl ToString) -> String {
    json!({ "error": { "msg": msg.to_string() } }).to_string()
}

/// Parse a situation given as text.
fn situation(text: &str) -> Result<Situation, String> {
    notation::parse(text).map_err(|e| e.to_string())
}

/// The tricks of a line played on from the situation's position.
fn line_tricks(s: &Situation, line: &[Card]) -> Value {
    let Ok(mut g) = s.game() else { return json!([]) };
    let mut out = Vec::new();
    let mut leader = g.leader;
    let mut cards: Vec<String> = g.current.iter().map(|c| c.to_string()).collect();
    for &c in line {
        if g.current.is_empty() {
            leader = g.leader;
        }
        cards.push(c.to_string());
        if g.play(c) {
            let t = g.tricks.last().unwrap();
            out.push(json!({ "n": g.tricks.len(), "leader": leader, "cards": cards, "winner": t.winner }));
            cards = Vec::new();
        }
    }
    json!(out)
}

/// The specific cards a task names, for display as chips ("all 3s" = the four 3s).
fn named_cards(kind: &Kind) -> Vec<Card> {
    match kind {
        Kind::WinCards(set) => set.iter().collect(),
        Kind::Trick(TrickProp::CardWonBySub(c) | TrickProp::CardInLastTrick(c)) | Kind::LastCardOfGame(c) => vec![*c],
        Kind::OnlySub(v) => card(&format!("S{v}")).into_iter().collect(),
        _ => Vec::new(),
    }
}

/// Cache size per worker when `parts` workers run at once: the default split
/// between them (at least 500 000 positions), so memory stays bounded.
fn cache_share(parts: u32) -> usize {
    (Options::default().cache_limit / parts.max(1) as usize).max(500_000)
}

fn progress_fn(f: &JsValue) -> Option<js_sys::Function> {
    f.dyn_ref::<js_sys::Function>().cloned()
}

// ---------------------------------------------------------------- API

/// All task and rule cards.
#[wasm_bindgen]
pub fn catalogue_json() -> String {
    ok(json!(catalogue().iter().map(card_json).collect::<Vec<_>>()))
}

/// Any task by ID: an official card, or a generated one for a parametric ID.
#[wasm_bindgen]
pub fn task_info(id: &str) -> String {
    match crew_core::catalogue::resolve(id) {
        Ok(t) => ok(card_json(t)),
        Err(e) => err(e),
    }
}

fn card_json(t: &crew_core::catalogue::TaskCard) -> Value {
    json!({
        "id": t.id,
        "category": t.category,
        "difficulty": t.difficulty,
        "text_en": t.text_en,
        "text_de": t.text_de,
        "doubtful": t.doubtful,
        "note": t.note,
        "rule": t.is_rule(),
        "captain_allowed": t.captain_allowed(),
        "cards": named_cards(&t.kind).iter().map(|c| c.to_string()).collect::<Vec<_>>(),
        "official": t.official,
    })
}

/// Parse text with one or more situations into DTOs.
#[wasm_bindgen]
pub fn parse(text: &str) -> String {
    match notation::parse_all(text) {
        Ok(list) => ok(json!(list.iter().map(to_dto).collect::<Vec<_>>())),
        Err(e) => json!({ "error": { "line": e.line, "msg": e.msg } }).to_string(),
    }
}

/// Write DTOs (a JSON array) as text.
#[wasm_bindgen]
pub fn write(dtos_json: &str) -> String {
    let dtos: Vec<SituationDto> = match serde_json::from_str(dtos_json) {
        Ok(d) => d,
        Err(e) => return err(e),
    };
    match dtos.iter().map(from_dto).collect::<Result<Vec<_>, _>>() {
        Ok(list) => ok(json!(notation::write_all(&list))),
        Err(e) => err(e),
    }
}

/// Check a situation: returns captain, difficulty sum and position, or the problem.
#[wasm_bindgen]
pub fn check(text: &str) -> String {
    let s = match situation(text) {
        Ok(s) => s,
        Err(e) => return err(e),
    };
    match s.game() {
        Ok(g) => ok(json!({
            "captain": g.captain,
            "difficulty": s.difficulty_sum(),
            "tricks_played": g.tricks.len(),
            "to_play": g.to_play(),
            "redeal": solver::redeal(&s),
            "drafts": crew_core::draft::count_drafts(&s.tasks, g.n, g.captain as usize),
        })),
        Err(e) => err(e),
    }
}

/// Feasibility for the entered owners (all fixed).
#[wasm_bindgen]
pub fn solve(text: &str, progress: JsValue) -> String {
    let cb = progress_fn(&progress);
    let report = |n: u64| {
        if let Some(f) = &cb {
            let _ = f.call1(&JsValue::NULL, &JsValue::from_f64(n as f64));
        }
    };
    solve_impl(text, Some(&report))
}

pub fn solve_impl(text: &str, progress: Option<&dyn Fn(u64)>) -> String {
    let s = match situation(text) {
        Ok(s) => s,
        Err(e) => return err(e),
    };
    let opts = Options { progress, ..Options::default() };
    match solver::solve(&s, &opts) {
        Err(e) => err(e),
        Ok((v, stats)) => {
            let stats = json!({ "nodes": stats.nodes, "cache_hits": stats.cache_hits });
            ok(match v {
                Verdict::Feasible(line) => json!({
                    "verdict": "feasible",
                    "line": line.iter().map(|c| c.to_string()).collect::<Vec<_>>(),
                    "tricks": line_tricks(&s, &line),
                    "stats": stats
                }),
                Verdict::Infeasible => json!({ "verdict": "infeasible", "stats": stats }),
                Verdict::Redeal(id) => json!({ "verdict": "redeal", "task": id }),
                Verdict::Cancelled => json!({ "verdict": "cancelled" }),
                Verdict::Undecided => json!({ "verdict": "undecided" }),
            })
        }
    }
}

/// Feasibility over all drafts of the `?` tasks in one search: the verdict,
/// and for a feasible one the owners of a working draft and its line.
#[wasm_bindgen]
pub fn solve_drafts(text: &str, progress: JsValue) -> String {
    let cb = progress_fn(&progress);
    let report = |n: u64| {
        if let Some(f) = &cb {
            let _ = f.call1(&JsValue::NULL, &JsValue::from_f64(n as f64));
        }
    };
    solve_drafts_impl(text, Some(&report))
}

pub fn solve_drafts_impl(text: &str, progress: Option<&dyn Fn(u64)>) -> String {
    let s = match situation(text) {
        Ok(s) => s,
        Err(e) => return err(e),
    };
    let opts = Options { progress, ..Options::default() };
    match solver::solve_drafts(&s, &opts) {
        Err(e) => err(e),
        Ok((v, owners, stats)) => {
            let stats = json!({ "nodes": stats.nodes, "cache_hits": stats.cache_hits });
            ok(match (v, owners) {
                (Verdict::Feasible(line), Some(owners)) => {
                    let mut d = s.clone();
                    for (a, o) in d.tasks.iter_mut().zip(&owners) {
                        a.owner = *o;
                    }
                    json!({
                        "verdict": "feasible",
                        "owners": owners.iter().map(|&o| owner_str(o)).collect::<Vec<_>>(),
                        "line": line.iter().map(|c| c.to_string()).collect::<Vec<_>>(),
                        "tricks": line_tricks(&d, &line),
                        "stats": stats
                    })
                }
                (Verdict::Infeasible, _) => json!({ "verdict": "infeasible", "stats": stats }),
                (Verdict::Redeal(id), _) => json!({ "verdict": "redeal", "task": id }),
                (Verdict::Undecided, _) => json!({ "verdict": "undecided" }),
                _ => json!({ "verdict": "cancelled" }),
            })
        }
    }
}

/// Search drafts for tasks with owner `?`. With `all_drafts`, every task
/// card is reassigned and the entered owners are the reference for the
/// smallest fix; this asks whether another draft would have worked, so it
/// starts from the deal and ignores played tricks.
#[wasm_bindgen]
pub fn drafts(text: &str, all_drafts: bool, limit: u32, part: u32, parts: u32, progress: JsValue) -> String {
    let cb = progress_fn(&progress);
    // Called with (checked, planned, working so far, drafts still open).
    let report = |checked: u64, planned: u64, working: u64, open: u64| {
        if let Some(f) = &cb {
            let args = js_sys::Array::of4(
                &JsValue::from_f64(checked as f64),
                &JsValue::from_f64(planned as f64),
                &JsValue::from_f64(working as f64),
                &JsValue::from_f64(open as f64),
            );
            let _ = f.apply(&JsValue::NULL, &args);
        }
    };
    drafts_impl(text, all_drafts, limit, (part as usize, parts as usize), Some(&report))
}

pub fn drafts_impl(
    text: &str,
    all_drafts: bool,
    limit: u32,
    part: (usize, usize),
    progress: Option<&dyn Fn(u64, u64, u64, u64)>,
) -> String {
    let mut s = match situation(text) {
        Ok(s) => s,
        Err(e) => return err(e),
    };
    let mut reference = None;
    if all_drafts {
        s.played.clear();
        reference = Some(s.tasks.iter().map(|a| a.owner).collect::<Vec<_>>());
        for a in s.tasks.iter_mut().filter(|a| a.owner != Owner::Crew) {
            a.owner = Owner::Unknown;
        }
    }
    let opts = DraftOptions {
        limit: limit as u64,
        reference,
        progress,
        part,
        cache_limit: cache_share(part.1 as u32),
        ..DraftOptions::default()
    };
    match analysis::draft_search(&s, &opts) {
        Err(e) => err(e),
        Ok(r) => {
            let with_owners = |owners: &[Owner], line: &[Card]| {
                let mut d = s.clone();
                for (a, o) in d.tasks.iter_mut().zip(owners) {
                    a.owner = *o;
                }
                json!({
                    "owners": owners.iter().map(|&o| owner_str(o)).collect::<Vec<_>>(),
                    "line": line.iter().map(|c| c.to_string()).collect::<Vec<_>>(),
                    "tricks": line_tricks(&d, line),
                })
            };
            ok(json!({
                "total": r.total,
                "checked": r.checked,
                "working": r.working,
                "undecided": r.undecided,
                "sampled": r.sampled,
                "any_works": r.any_works,
                "example": r.example.as_ref().map(|(o, l)| with_owners(o, l)),
                "closest": r.closest.as_ref().map(|(o, d, l)| {
                    let mut v = with_owners(o, l);
                    v["distance"] = json!(d);
                    v
                }),
                "impossible_pairs": r.impossible_pairs.iter().map(|&(t, p)| json!({ "task": t, "player": p })).collect::<Vec<_>>(),
            }))
        }
    }
}

/// Smallest conflicting task set from the deal (task indices), or null.
#[wasm_bindgen]
pub fn conflict(text: &str) -> String {
    match situation(text) {
        Err(e) => err(e),
        Ok(s) => match analysis::smallest_conflict(&s, None) {
            Ok(c) => ok(json!(c)),
            Err(e) => err(e),
        },
    }
}

/// Where a reconstructed or logged game became unwinnable.
#[wasm_bindgen]
pub fn no_return(text: &str) -> String {
    match situation(text) {
        Err(e) => err(e),
        Ok(s) => match analysis::point_of_no_return(&s, None) {
            Ok(None) => ok(Value::Null),
            Ok(Some(NoReturn::FromTheStart)) => ok(json!({ "kind": "from_start" })),
            Ok(Some(NoReturn::StillWinnable)) => ok(json!({ "kind": "still_winnable" })),
            Ok(Some(NoReturn::At { trick, player, card, alternatives })) => ok(json!({
                "kind": "at",
                "trick": trick,
                "player": player,
                "card": card.to_string(),
                "alternatives": alternatives.iter().map(|c| c.to_string()).collect::<Vec<_>>(),
            })),
            Err(e) => err(e),
        },
    }
}

/// Critical decisions and tightness along a winning line (cards as text).
#[wasm_bindgen]
pub fn tightness(text: &str, line_json: &str) -> String {
    let s = match situation(text) {
        Ok(s) => s,
        Err(e) => return err(e),
    };
    let line: Vec<String> = match serde_json::from_str(line_json) {
        Ok(l) => l,
        Err(e) => return err(e),
    };
    let line = match line.iter().map(|c| card(c)).collect::<Result<Vec<_>, _>>() {
        Ok(l) => l,
        Err(e) => return err(e),
    };
    match analysis::tightness(&s, &line, None) {
        Ok(Some(t)) => ok(json!({ "critical": t.critical, "choices": t.choices, "tightness": t.tightness })),
        Ok(None) => ok(Value::Null),
        Err(e) => err(e),
    }
}

/// Share of random deals on which the situation's task cards can be fulfilled.
#[wasm_bindgen]
pub fn deal_robustness(text: &str, deals: u32, seed: u32, part: u32, parts: u32, progress: JsValue) -> String {
    let cb = progress_fn(&progress);
    let report = |done: usize, total: usize| {
        if let Some(f) = &cb {
            let _ = f.call2(&JsValue::NULL, &JsValue::from_f64(done as f64), &JsValue::from_f64(total as f64));
        }
    };
    match situation(text) {
        Err(e) => err(e),
        Ok(s) => {
            let cards: Vec<_> = s.tasks.iter().filter(|a| !a.card.is_rule()).map(|a| a.card).collect();
            match analysis::deal_robustness(
                &cards,
                s.players(),
                deals as usize,
                true,
                seed as u64,
                (part as usize, parts as usize),
                &Options { node_limit: Some(2_000_000), cache_limit: cache_share(parts), ..Options::default() },
                Some(&report),
            ) {
                Some((good, undecided, total)) => ok(json!({ "solvable": good, "undecided": undecided, "deals": total })),
                None => ok(Value::Null),
            }
        }
    }
}

/// A random deal with task cards drawn to the difficulty `target` (owners `?`), as text.
#[wasm_bindgen]
pub fn random_situation(players: u32, target: u32, seed: u32) -> String {
    use crew_core::game::Game;
    use crew_core::rng::Rng;
    let players = players.clamp(3, 5) as usize;
    let mut rng = Rng::new(seed as u64 ^ 0x5EED);
    let hands = Game::random_hands(players, &mut rng);
    let Some(cards) = crew_core::draft::draw_tasks(catalogue(), players, target, &mut rng) else {
        return err(format!("could not draw tasks for difficulty {target}"));
    };
    let s = Situation {
        title: Some(format!("Random · {players} players · difficulty {target}")),
        target: Some(target),
        hands,
        tasks: cards.into_iter().map(|card| Assignment { card, owner: Owner::Unknown }).collect(),
        ..Situation::default()
    };
    ok(json!(notation::write(&s)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue_names_cards_for_chips() {
        let list: Value = serde_json::from_str(&catalogue_json()).unwrap();
        let cards = |id: &str| list["ok"].as_array().unwrap().iter().find(|t| t["id"] == id).unwrap()["cards"].clone();
        assert_eq!(cards("win all 3s"), json!(["P3", "B3", "G3", "Y3"]));
        assert_eq!(cards("win P7 with sub"), json!(["P7"]));
        assert_eq!(cards("only S2"), json!(["S2"]));
        assert_eq!(cards("no pink"), json!([]));
    }

    #[test]
    fn task_info_generates_parametric_tasks() {
        let g: Value = serde_json::from_str(&task_info("win P2 Y7")).unwrap();
        assert_eq!(g["ok"]["official"], false);
        assert_eq!(g["ok"]["cards"], json!(["P2", "Y7"]));
        let o: Value = serde_json::from_str(&task_info("win Y7 B6")).unwrap();
        assert_eq!((o["ok"]["id"].as_str(), o["ok"]["official"].as_bool()), (Some("win B6 Y7"), Some(true)));
        let e: Value = serde_json::from_str(&task_info("win X9")).unwrap();
        assert_eq!(e["error"]["msg"], "'X9' is not a card");
    }

    const TEXT: &str = "crew 1
players: 4
Player1: P2 P8 B4 B9 G1 G6 Y3 Y7 S2 S4
Player2: P1 P5 B1 B2 B6 G4 G9 Y2 Y6 S1
Player3: P3 P6 B3 B8 G2 G5 G8 Y1 Y8 S3
Player4: rest
tasks:
Player2: win B4
?: first-trick
";

    #[test]
    fn dto_roundtrip_through_text() {
        let parsed: Value = serde_json::from_str(&parse(TEXT)).unwrap();
        let dtos = parsed["ok"].to_string();
        let written: Value = serde_json::from_str(&write(&dtos)).unwrap();
        let back: Value = serde_json::from_str(&parse(written["ok"].as_str().unwrap())).unwrap();
        assert_eq!(parsed, back);
        assert_eq!(parsed["ok"][0]["tasks"][1]["owner"], "?");
    }

    #[test]
    fn solve_and_drafts_return_json() {
        let fixed = TEXT.replace("?: first-trick", "Player1: first-trick");
        let r: Value = serde_json::from_str(&solve_impl(&fixed, None)).unwrap();
        assert_eq!(r["ok"]["verdict"], "feasible");
        assert!(r["ok"]["tricks"].as_array().unwrap().len() >= 1);
        let d: Value = serde_json::from_str(&drafts_impl(TEXT, false, 2000, (0, 1), None)).unwrap();
        assert_eq!(d["ok"]["any_works"], true);
        assert!(d["ok"]["total"].as_u64().unwrap() > 0);
        let bad: Value = serde_json::from_str(&parse("crew 2")).unwrap();
        assert_eq!(bad["error"]["line"], 1);
    }

    #[test]
    fn solve_drafts_returns_owners_and_line() {
        let r: Value = serde_json::from_str(&solve_drafts_impl(TEXT, None)).unwrap();
        assert_eq!(r["ok"]["verdict"], "feasible");
        assert_eq!(r["ok"]["owners"][0], "Player2");
        assert!(r["ok"]["owners"][1].as_str().unwrap().starts_with("Player"));
        assert!(!r["ok"]["line"].as_array().unwrap().is_empty());
    }
}
