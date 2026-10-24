//! `crew` command line.
//!
//! ```text
//! crew solve FILE [--no-cache]   solve every situation in a .crew file
//! ```

use crew_core::card::Card;
use crew_core::notation;
use crew_core::situation::Situation;
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
  crew solve FILE [--no-cache]   solve every situation in a .crew file
  crew bench [--players N] [--target SUM] [--count K] [--seed S] [--timeout SECS] [--save FILE] [-v]
                                 random deals, tasks drawn to the target sum, random legal draft";

fn flag<T: std::str::FromStr>(args: &[String], name: &str, default: T) -> Result<T, String> {
    match args.iter().position(|a| a == name) {
        None => Ok(default),
        Some(i) => args
            .get(i + 1)
            .and_then(|v| v.parse().ok())
            .ok_or_else(|| format!("{name} needs a value\n{USAGE}")),
    }
}

/// Solve with a wall-clock limit; the solver is cancelled when it runs out.
fn solve_with_timeout(s: &Situation, secs: f64) -> (Result<(Verdict, solver::Stats), solver::SolveError>, f64) {
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
        let r = solver::solve(s, &Options { cancel: Some(&cancel), ..Options::default() });
        let _ = tx.send(());
        r
    });
    (result, start.elapsed().as_secs_f64())
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

    let mut rng = Rng::new(seed);
    let mut saved = Vec::new();
    let mut rows: Vec<(&'static str, f64)> = Vec::new();
    for i in 0..count {
        let hands = Game::random_hands(players, &mut rng);
        let captain = Game::new(&hands).unwrap().captain as usize;
        let cards = draft::draw_tasks(catalogue(), players, target, &mut rng).ok_or("could not draw tasks")?;
        let mut tasks: Vec<Assignment> = cards.into_iter().map(|card| Assignment { card, owner: Owner::Unknown }).collect();
        let owners = draft::random_draft(&tasks, players, captain, &mut rng).ok_or("no legal draft")?;
        for (a, o) in tasks.iter_mut().zip(owners) {
            a.owner = o;
        }
        let s = Situation {
            title: Some(format!("bench seed {seed} #{}", i + 1)),
            target: Some(target),
            hands,
            tasks,
            ..Situation::default()
        };
        let (res, secs) = solve_with_timeout(&s, timeout);
        let (label, nodes) = match &res {
            Ok((Verdict::Feasible(_), st)) => ("feasible", st.nodes),
            Ok((Verdict::Infeasible, st)) => ("infeasible", st.nodes),
            Ok((Verdict::Redeal(_), _)) => ("re-deal", 0),
            Ok((Verdict::Cancelled, st)) => ("timeout", st.nodes),
            Err(_) => ("error", 0),
        };
        if verbose {
            let ids: Vec<&str> = s.tasks.iter().map(|a| a.card.id.as_str()).collect();
            println!("#{:<3} {:<10} {:>8.3} s {:>12} nodes  {}", i + 1, label, secs, nodes, ids.join(" | "));
        }
        rows.push((label, secs));
        saved.push(s);
    }
    println!("{players} players, target {target}, {count} missions, seed {seed}, timeout {timeout} s");
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

fn cmd_solve(args: &[String]) -> Result<(), String> {
    let file = args.iter().find(|a| !a.starts_with("--")).ok_or(USAGE)?;
    let use_cache = !args.iter().any(|a| a == "--no-cache");
    let text = std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
    let list = notation::parse_all(&text).map_err(|e| format!("{file}: {e}"))?;
    for (i, s) in list.iter().enumerate() {
        if list.len() > 1 {
            println!("--- situation {} ---", i + 1);
        }
        if let Some(t) = &s.title {
            println!("{t}");
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
                    }
                    Verdict::Infeasible => println!("NOT FEASIBLE"),
                    Verdict::Redeal(id) => println!("RE-DEAL (task '{id}')"),
                    Verdict::Cancelled => println!("CANCELLED"),
                }
                println!(
                    "{:.3} s, {} nodes, {} cache hits, {} cached positions",
                    secs, stats.nodes, stats.cache_hits, stats.cache_size
                );
            }
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
