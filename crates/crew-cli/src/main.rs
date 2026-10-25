//! `crew` command line.
//!
//! ```text
//! crew solve FILE [--no-cache]   solve every situation in a .crew file
//! ```

use crew_core::analysis::{self, DraftOptions};
use crew_core::card::Card;
use crew_core::notation;
use crew_core::situation::{Owner, Situation};
use crew_core::solver::{self, Options, Verdict};
use std::process::ExitCode;
use std::time::Instant;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("solve") => cmd_solve(&args[1..]),
        Some("bench") => cmd_bench(&args[1..]),
        _ => Err(USAGE.to_string()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

const USAGE: &str = "usage:
  crew solve FILE [--analyse] [--all-drafts] [--limit N] [--no-cache]
                                 solve every situation in a .crew file; tasks with
                                 owner ? are searched over all legal drafts (sampled
                                 above N drafts); --all-drafts also checks other drafts;
                                 --analyse adds critical decisions, smallest conflict
                                 and point of no return
  crew bench [--players N] [--target SUM] [--count K] [--seed S] [--timeout SECS] [--save FILE] [-v]
                                 random deals, tasks drawn to the target sum, random legal draft
  crew bench --load FILE [--timeout SECS] [-v]
                                 the same timing over the situations in FILE; with owner ?
                                 the draft search (verdict over all drafts) is timed";

fn flag<T: std::str::FromStr>(args: &[String], name: &str, default: T) -> Result<T, String> {
    match args.iter().position(|a| a == name) {
        None => Ok(default),
        Some(i) => args
            .get(i + 1)
            .and_then(|v| v.parse().ok())
            .ok_or_else(|| format!("{name} needs a value\n{USAGE}")),
    }
}

/// Run `f` with a wall-clock limit; the cancel flag is set when it runs out.
fn with_timeout<R>(secs: f64, f: impl FnOnce(&std::sync::atomic::AtomicBool) -> R) -> (R, f64) {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::time::Duration;
    let cancel = AtomicBool::new(false);
    let start = Instant::now();
    let result = std::thread::scope(|scope| {
        let (tx, rx) = mpsc::channel::<()>();
        let cancel_ref = &cancel;
        scope.spawn(move || {
            if rx.recv_timeout(Duration::from_secs_f64(secs)).is_err() {
                cancel_ref.store(true, Ordering::Relaxed);
            }
        });
        let r = f(&cancel);
        let _ = tx.send(());
        r
    });
    (result, start.elapsed().as_secs_f64())
}

/// Verdict label and node count (0 where the search does not count nodes).
fn timed_verdict(s: &Situation, secs: f64) -> ((&'static str, u64), f64) {
    if s.tasks.iter().any(|a| a.owner == Owner::Unknown) {
        return with_timeout(secs, |cancel| {
            let opts = DraftOptions { cancel: Some(cancel), ..DraftOptions::default() };
            match analysis::draft_search(s, &opts) {
                Ok(r) => match r.any_works {
                    Some(true) => ("feasible", 0),
                    Some(false) => ("infeasible", 0),
                    None => ("timeout", 0),
                },
                Err(_) => ("error", 0),
            }
        });
    }
    with_timeout(secs, |cancel| match solver::solve(s, &Options { cancel: Some(cancel), ..Options::default() }) {
        Ok((Verdict::Feasible(_), st)) => ("feasible", st.nodes),
        Ok((Verdict::Infeasible, st)) => ("infeasible", st.nodes),
        Ok((Verdict::Redeal(_), _)) => ("re-deal", 0),
        Ok((Verdict::Cancelled, st)) => ("timeout", st.nodes),
        Err(_) => ("error", 0),
    })
}

fn cmd_bench(args: &[String]) -> Result<(), String> {
    use crew_core::catalogue::catalogue;
    use crew_core::draft;
    use crew_core::game::Game;
    use crew_core::rng::Rng;
    use crew_core::situation::{Assignment, Owner};

    let players: usize = flag(args, "--players", 4)?;
    let target: u32 = flag(args, "--target", 7)?;
    let count: usize = flag(args, "--count", 20)?;
    let seed: u64 = flag(args, "--seed", 1)?;
    let timeout: f64 = flag(args, "--timeout", 10.0)?;
    let save: Option<String> = args.iter().position(|a| a == "--save").and_then(|i| args.get(i + 1).cloned());
    let verbose = args.iter().any(|a| a == "-v");

    let load: Option<String> = args.iter().position(|a| a == "--load").and_then(|i| args.get(i + 1).cloned());
    let loaded = match &load {
        Some(path) => {
            let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
            Some(notation::parse_all(&text).map_err(|e| format!("{path}: {e}"))?)
        }
        None => None,
    };
    let count = loaded.as_ref().map_or(count, |l| l.len());

    let mut rng = Rng::new(seed);
    let mut saved = Vec::new();
    let mut rows: Vec<(&'static str, f64)> = Vec::new();
    let (mut total_secs, mut total_nodes) = (0.0, 0u64);
    for i in 0..count {
        let s = match &loaded {
            Some(list) => list[i].clone(),
            None => {
                let hands = Game::random_hands(players, &mut rng);
                let captain = Game::new(&hands).unwrap().captain as usize;
                let cards = draft::draw_tasks(catalogue(), players, target, &mut rng).ok_or("could not draw tasks")?;
                let mut tasks: Vec<Assignment> =
                    cards.into_iter().map(|card| Assignment { card, owner: Owner::Unknown }).collect();
                let owners = draft::random_draft(&tasks, players, captain, &mut rng).ok_or("no legal draft")?;
                for (a, o) in tasks.iter_mut().zip(owners) {
                    a.owner = o;
                }
                Situation {
                    title: Some(format!("bench seed {seed} #{}", i + 1)),
                    target: Some(target),
                    hands,
                    tasks,
                    ..Situation::default()
                }
            }
        };
        let ((label, nodes), secs) = timed_verdict(&s, timeout);
        total_secs += secs;
        total_nodes += nodes;
        if verbose {
            let ids: Vec<&str> = s.tasks.iter().map(|a| a.card.id.as_str()).collect();
            println!("#{:<3} {:<10} {:>8.3} s {:>12} nodes  {}", i + 1, label, secs, nodes, ids.join(" | "));
        }
        rows.push((label, secs));
        saved.push(s);
    }
    match &load {
        Some(path) => println!("{path}: {count} situations, timeout {timeout} s"),
        None => println!("{players} players, target {target}, {count} missions, seed {seed}, timeout {timeout} s"),
    }
    println!("  total {total_secs:.1} s, {total_nodes} nodes");
    for label in ["feasible", "infeasible", "re-deal", "timeout", "error"] {
        let mut t: Vec<f64> = rows.iter().filter(|r| r.0 == label).map(|r| r.1).collect();
        if t.is_empty() {
            continue;
        }
        t.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let pct = |p: f64| t[((t.len() - 1) as f64 * p).round() as usize];
        println!(
            "  {label:<10} {:>4}   median {:.3} s   p90 {:.3} s   max {:.3} s",
            t.len(),
            pct(0.5),
            pct(0.9),
            t[t.len() - 1]
        );
    }
    if let Some(path) = save {
        std::fs::write(&path, notation::write_all(&saved)).map_err(|e| format!("{path}: {e}"))?;
        println!("saved to {path}");
    }
    Ok(())
}

fn owner_name(o: &Owner) -> String {
    match o {
        Owner::Player(p) => format!("Player{}", p + 1),
        Owner::Unknown => "?".into(),
        Owner::Crew => "crew".into(),
    }
}

fn print_drafts(s: &Situation, reference: Option<Vec<Owner>>, limit: u64) {
    let start = Instant::now();
    let opts = DraftOptions { reference, limit, ..DraftOptions::default() };
    match analysis::draft_search(s, &opts) {
        Err(e) => println!("error: {e}"),
        Ok(r) => {
            let secs = start.elapsed().as_secs_f64();
            match r.any_works {
                Some(true) => println!("FEASIBLE with some draft"),
                Some(false) => println!("NOT FEASIBLE with any draft"),
                None => println!("CANCELLED"),
            }
            let pct = if r.checked > 0 { 100.0 * r.working as f64 / r.checked as f64 } else { 0.0 };
            if r.sampled {
                println!("about {pct:.0}% of {} legal drafts work ({} sampled)", r.total, r.checked);
            } else {
                println!("{} of {} legal drafts work", r.working, r.total);
            }
            for &(i, p) in &r.impossible_pairs {
                println!("  impossible on its own: Player{} · {}", p + 1, s.tasks[i].card.id);
            }
            let shown = match (&r.closest, &r.example) {
                (Some((owners, dist, line)), _) => {
                    println!("smallest fix ({dist} task(s) change owner):");
                    Some((owners, line))
                }
                (None, Some((owners, line))) => {
                    println!("a working draft:");
                    Some((owners, line))
                }
                _ => None,
            };
            if let Some((owners, _)) = shown {
                for (a, o) in s.tasks.iter().zip(owners) {
                    println!("  {:<8} {}", format!("{}:", owner_name(o)), a.card.id);
                }
            }
            if let Some((owners, line)) = shown {
                let mut d = s.clone();
                for (a, o) in d.tasks.iter_mut().zip(owners) {
                    a.owner = *o;
                }
                println!("winning line for that draft:");
                print_line(&d, line);
            }
            println!("{secs:.3} s");
        }
    }
}

fn cmd_solve(args: &[String]) -> Result<(), String> {
    let file = args.iter().find(|a| !a.starts_with("--")).ok_or(USAGE)?;
    let use_cache = !args.iter().any(|a| a == "--no-cache");
    let all_drafts = args.iter().any(|a| a == "--all-drafts");
    let analyse = args.iter().any(|a| a == "--analyse");
    let limit: u64 = flag(args, "--limit", 2000)?;
    let text = std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
    let list = notation::parse_all(&text).map_err(|e| format!("{file}: {e}"))?;
    for (i, s) in list.iter().enumerate() {
        if list.len() > 1 {
            println!("--- situation {} ---", i + 1);
        }
        if let Some(t) = &s.title {
            println!("{t}");
        }
        if s.tasks.iter().any(|a| a.owner == Owner::Unknown) {
            print_drafts(s, None, limit);
            continue;
        }
        let start = Instant::now();
        let opts = Options { use_cache, ..Options::default() };
        match solver::solve(s, &opts) {
            Err(e) => println!("error: {e}"),
            Ok((v, stats)) => {
                let secs = start.elapsed().as_secs_f64();
                match &v {
                    Verdict::Feasible(line) => {
                        println!("FEASIBLE");
                        print_line(s, line);
                        if analyse {
                            if let Ok(Some(t)) = analysis::tightness(s, line, None) {
                                println!(
                                    "critical decisions: {} of {} choices; tightness {:.0}%",
                                    t.critical,
                                    t.choices,
                                    100.0 * t.tightness
                                );
                            }
                        }
                    }
                    Verdict::Infeasible => {
                        println!("NOT FEASIBLE");
                        if analyse {
                            if let Ok(Some(c)) = analysis::smallest_conflict(s, None) {
                                if c.len() == 1 {
                                    println!("impossible on its own:");
                                } else {
                                    println!("smallest conflict ({} tasks):", c.len());
                                }
                                for i in c {
                                    let a = &s.tasks[i];
                                    println!("  {} · {}", owner_name(&a.owner), a.card.id);
                                }
                            }
                            if !s.played.is_empty() {
                                match analysis::point_of_no_return(s, None) {
                                    Ok(Some(analysis::NoReturn::FromTheStart)) => println!("never winnable from the deal"),
                                    Ok(Some(analysis::NoReturn::At { trick, player, card, alternatives })) => {
                                        let alts: Vec<String> = alternatives.iter().map(|c| c.to_string()).collect();
                                        println!(
                                            "lost in trick {} when Player{} played {card}; would have kept a winning line: {}",
                                            trick + 1,
                                            player + 1,
                                            if alts.is_empty() { "none".into() } else { alts.join(" ") }
                                        );
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    Verdict::Redeal(id) => println!("RE-DEAL (task '{id}')"),
                    Verdict::Cancelled => println!("CANCELLED"),
                }
                println!(
                    "{:.3} s, {} nodes, {} cache hits, {} cached positions",
                    secs, stats.nodes, stats.cache_hits, stats.cache_size
                );
            }
        }
        if all_drafts {
            println!("other drafts:");
            let reference: Vec<Owner> = s.tasks.iter().map(|a| a.owner).collect();
            let mut open = s.clone();
            for a in open.tasks.iter_mut().filter(|a| a.owner != Owner::Crew) {
                a.owner = Owner::Unknown;
            }
            print_drafts(&open, Some(reference), limit);
        }
    }
    Ok(())
}

/// Print a winning line trick by trick, continuing from the situation's position.
fn print_line(s: &Situation, line: &[Card]) {
    let mut g = s.game().expect("situation was validated");
    let mut trick_cards: Vec<String> = Vec::new();
    let mut leader = g.leader;
    for &c in line {
        if g.current.is_empty() {
            leader = g.leader;
        }
        trick_cards.push(c.to_string());
        if g.play(c) {
            let t = g.tricks.last().unwrap();
            println!(
                "  trick{} Player{}: {}  -> Player{}",
                g.tricks.len(),
                leader + 1,
                trick_cards.join(" "),
                t.winner + 1
            );
            trick_cards.clear();
        }
    }
    if g.tricks.len() < g.total_tricks() {
        println!("  (all tasks done after trick {}; the rest does not matter)", g.tricks.len());
    }
}
