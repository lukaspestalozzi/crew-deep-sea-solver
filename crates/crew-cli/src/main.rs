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
        Some("orders") => cmd_orders(&args[1..]),
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
  crew solve FILE [--analyse] [--all-drafts] [--count-drafts] [--limit N] [--no-cache] [--dfs] [--best-first] [--deck-order] [--zobrist] [--trace OUT]
                                 solve every situation in a .crew file; tasks with
                                 owner ? are searched over all legal drafts at once;
                                 --count-drafts also counts the drafts that work (sampled
                                 above N drafts); --all-drafts also checks other drafts;
                                 --analyse adds critical decisions, smallest conflict
                                 and point of no return; --dfs, --best-first,
                                 --deck-order and --zobrist switch strategy (default:
                                 restarts alternating move orders), move order and hashing
  crew bench [--players N] [--target SUM] [--count K] [--seed S] [--timeout SECS] [--save FILE] [-v]
                                 random deals, tasks drawn to the target sum, random legal draft
  crew bench --load FILE [--timeout SECS] [-v]
                                 the same timing over the situations in FILE; with owner ?
                                 the verdict over all drafts is timed
                                 (bench also takes --dfs, --best-first, --deck-order, --zobrist, --no-cache, --trace OUT)
  crew orders [--players N] [--target SUM] [--count K] [--seed S] [--budget NODES] [--effort NODES] [--threads T]
                                 compare the move orders: random missions with a random
                                 draft; at every position with a choice along a winning
                                 line, how often each order's first choice wins and the
                                 mean rank of its first winning move (leads and follows
                                 apart); and the positions a depth-first search needs with
                                 each order (at most --effort per mission)";

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
fn timed_verdict(s: &Situation, secs: f64, base: &Options) -> ((&'static str, u64), f64) {
    with_timeout(secs, |cancel| {
        let opts = Options { cancel: Some(cancel), ..base.clone() };
        let r = if s.tasks.iter().any(|a| a.owner == Owner::Unknown) {
            solver::solve_drafts(s, &opts).map(|(v, _, st)| (v, st))
        } else {
            solver::solve(s, &opts)
        };
        match r {
            Ok((Verdict::Feasible(_), st)) => ("feasible", st.nodes),
            Ok((Verdict::Infeasible, st)) => ("infeasible", st.nodes),
            Ok((Verdict::Redeal(_), _)) => ("re-deal", 0),
            Ok((Verdict::Cancelled | Verdict::Undecided, st)) => ("timeout", st.nodes),
            Err(_) => ("error", 0),
        }
    })
}

/// Search options from the command line flags.
/// `--trace FILE`: every search as JSON lines (see crew_core::trace), with
/// "t" in milliseconds since the start.
static TRACE: std::sync::OnceLock<std::sync::Mutex<(std::fs::File, Instant)>> = std::sync::OnceLock::new();

fn trace_sink(e: &crew_core::trace::Event) {
    use std::io::Write;
    if let Some(m) = TRACE.get() {
        let mut g = m.lock().unwrap();
        let json = e.to_json();
        let ms = g.1.elapsed().as_secs_f64() * 1000.0;
        let _ = writeln!(g.0, "{},\"t\":{ms:.1}}}", &json[..json.len() - 1]);
    }
}

fn cli_trace() -> Option<crew_core::trace::Trace<'static>> {
    TRACE.get().map(|_| crew_core::trace::Trace::new(&trace_sink))
}

fn search_options(args: &[String]) -> Options<'static> {
    let mut o = Options::default();
    if let Some(path) = args.iter().position(|a| a == "--trace").and_then(|i| args.get(i + 1)) {
        match std::fs::File::create(path) {
            Ok(f) => {
                let _ = TRACE.set(std::sync::Mutex::new((f, Instant::now())));
            }
            Err(e) => eprintln!("--trace {path}: {e}"),
        }
    }
    o.trace = cli_trace();
    if args.iter().any(|a| a == "--no-cache") {
        o.use_cache = false;
    }
    if args.iter().any(|a| a == "--deck-order") {
        o.decider = &crew_core::decider::DECK_ORDER;
    }
    if args.iter().any(|a| a == "--task-aware") {
        o.decider = &crew_core::decider::TASK_AWARE;
    }
    if args.iter().any(|a| a == "--lookahead-all") {
        o.decider = &crew_core::decider::LOOKAHEAD_ALL;
    }
    if args.iter().any(|a| a == "--zobrist") {
        o.zobrist = true;
    }
    if args.iter().any(|a| a == "--dfs") {
        o.strategy = solver::Strategy::Dfs;
    }
    if args.iter().any(|a| a == "--best-first") {
        o.strategy = solver::Strategy::BestFirst { max_positions: 2_000_000 };
    }
    o
}

/// A random mission: deal, task cards drawn to `target`, a random legal draft.
fn random_mission(players: usize, target: u32, rng: &mut crew_core::rng::Rng, title: String) -> Result<Situation, String> {
    use crew_core::catalogue::catalogue;
    use crew_core::draft;
    use crew_core::game::Game;
    use crew_core::situation::Assignment;
    let hands = Game::random_hands(players, rng);
    let captain = Game::new(&hands).unwrap().captain as usize;
    let cards = draft::draw_tasks(catalogue(), players, target, rng).ok_or("could not draw tasks")?;
    let mut tasks: Vec<Assignment> = cards.into_iter().map(|card| Assignment { card, owner: Owner::Unknown }).collect();
    let owners = draft::random_draft(&tasks, players, captain, rng).ok_or("no legal draft")?;
    for (a, o) in tasks.iter_mut().zip(owners) {
        a.owner = o;
    }
    Ok(Situation { title: Some(title), target: Some(target), hands, tasks, ..Situation::default() })
}

/// What one mission gives the move-order comparison.
#[derive(Default)]
struct OrderRows {
    /// Per position with a choice and a decided winning move: (lead?, some
    /// move loses?, first winning rank per order).
    ranks: Vec<(bool, bool, Vec<Option<usize>>)>,
    /// Positions a depth-first search needed per order; `None`: over the limit.
    effort: Vec<Option<u64>>,
    feasible: Option<bool>,
}

fn order_rows(s: &Situation, budget: u64, effort: u64) -> OrderRows {
    use crew_core::position::{self, ORDERS};
    let mut out = OrderRows::default();
    for (_, d) in ORDERS {
        let o = Options { decider: d, strategy: solver::Strategy::Dfs, node_limit: Some(effort), ..Options::default() };
        out.effort.push(match solver::solve(s, &o) {
            Ok((Verdict::Feasible(_) | Verdict::Infeasible, st)) => Some(st.nodes),
            _ => None,
        });
    }
    let Ok((v, _)) = solver::solve(s, &Options { node_limit: Some(effort.saturating_mul(4)), ..Options::default() }) else { return out };
    out.feasible = match v {
        Verdict::Feasible(_) => Some(true),
        Verdict::Infeasible => Some(false),
        _ => None,
    };
    let Verdict::Feasible(line) = v else { return out };
    let opts = Options { node_limit: Some(budget), ..Options::default() };
    let mut cur = s.clone();
    for c in line {
        let Ok(g) = cur.game() else { break };
        if g.legal_moves().len() > 1 {
            if let Ok(Some(st)) = position::position_stats(&cur, &opts) {
                if !st.winning.is_empty() {
                    let critical = st.winning.len() + st.open < st.legal;
                    out.ranks.push((g.current.is_empty(), critical, st.first_win));
                }
            }
        }
        match position::after(&cur, c) {
            Ok(n) => cur = n,
            Err(_) => break,
        }
    }
    out
}

fn cmd_orders(args: &[String]) -> Result<(), String> {
    use crew_core::position::ORDERS;
    use crew_core::rng::Rng;
    let players: usize = flag(args, "--players", 4)?;
    let target: u32 = flag(args, "--target", 7)?;
    let count: usize = flag(args, "--count", 20)?;
    let seed: u64 = flag(args, "--seed", 1)?;
    let budget: u64 = flag(args, "--budget", 2_000_000)?;
    let effort: u64 = flag(args, "--effort", 5_000_000)?;
    let threads: usize = flag(args, "--threads", std::thread::available_parallelism().map_or(1, |n| n.get()))?;
    let mut rng = Rng::new(seed);
    let missions: Vec<Situation> =
        (0..count).map(|i| random_mission(players, target, &mut rng, format!("orders seed {seed} #{}", i + 1))).collect::<Result<_, _>>()?;
    let start = Instant::now();
    let rows: Vec<OrderRows> = std::thread::scope(|sc| {
        let hs: Vec<_> = (0..threads.max(1))
            .map(|t| {
                let missions = &missions;
                sc.spawn(move || missions.iter().enumerate().filter(|(i, _)| i % threads.max(1) == t).map(|(i, s)| (i, order_rows(s, budget, effort))).collect::<Vec<_>>())
            })
            .collect();
        let mut all: Vec<(usize, OrderRows)> = hs.into_iter().flat_map(|h| h.join().unwrap()).collect();
        all.sort_by_key(|x| x.0);
        all.into_iter().map(|x| x.1).collect()
    });
    let feasible = rows.iter().filter(|r| r.feasible == Some(true)).count();
    let infeasible = rows.iter().filter(|r| r.feasible == Some(false)).count();
    println!(
        "{players} players, target {target}, {count} missions (seed {seed}): {feasible} feasible, {infeasible} infeasible, {} undecided; {:.1} s",
        count - feasible - infeasible,
        start.elapsed().as_secs_f64()
    );
    let groups = [
        ("all positions", None, false),
        ("critical (some move loses)", None, true),
        ("critical leads", Some(true), true),
        ("critical follows", Some(false), true),
    ];
    for (name, lead, critical) in groups {
        let pos: Vec<&Vec<Option<usize>>> = rows
            .iter()
            .flat_map(|r| r.ranks.iter())
            .filter(|(l, c, _)| lead.is_none_or(|x| x == *l) && (!critical || *c))
            .map(|(_, _, r)| r)
            .collect();
        println!("{name}: {} positions with a choice", pos.len());
        for (k, (oname, _)) in ORDERS.iter().enumerate() {
            let known: Vec<usize> = pos.iter().filter_map(|r| r[k]).collect();
            let top = known.iter().filter(|&&r| r == 1).count();
            let mean = known.iter().sum::<usize>() as f64 / known.len().max(1) as f64;
            println!(
                "  {oname:<11} first choice wins {:>5.1}%   mean first winning rank {mean:.2}   ({} counted)",
                100.0 * top as f64 / known.len().max(1) as f64,
                known.len()
            );
        }
    }
    println!("depth-first search, positions per mission (limit {effort}):");
    for (k, (oname, _)) in ORDERS.iter().enumerate() {
        let mut done: Vec<u64> = rows.iter().filter_map(|r| r.effort[k]).collect();
        done.sort();
        let over = rows.len() - done.len();
        let median = done.get(done.len() / 2).copied().unwrap_or(0);
        let total: u64 = rows.iter().map(|r| r.effort[k].unwrap_or(effort)).sum();
        println!("  {oname:<11} total {total:>12}   median {median:>9}   over the limit {over}");
    }
    Ok(())
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
    let search_opts = search_options(args);

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
        let ((label, nodes), secs) = timed_verdict(&s, timeout, &search_opts);
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

/// Who takes which task and which tasks share a player, if anything stands out.
fn print_relations(s: &Situation, r: &analysis::DraftRelations) {
    let id = |t: usize| s.tasks[t].card.id.as_str();
    let lines: Vec<String> = r
        .must
        .iter()
        .map(|&(t, o)| format!("must:  {} · {}", owner_name(&o), id(t)))
        .chain(r.never.iter().map(|&(t, o)| format!("never: {} · {}", owner_name(&o), id(t))))
        .chain(r.always_together.iter().map(|&(a, b)| format!("always same player: {} + {}", id(a), id(b))))
        .chain(r.never_together.iter().map(|&(a, b)| format!("never same player:  {} + {}", id(a), id(b))))
        .chain(r.tendencies.iter().map(|&(a, b, w, all)| {
            format!("same player in {:.0}% of working drafts ({:.0}% of all): {} + {}", 100.0 * w, 100.0 * all, id(a), id(b))
        }))
        .collect();
    if !lines.is_empty() {
        println!("who takes what (working drafts compared with all drafts):");
        for l in lines {
            println!("  {l}");
        }
    }
}

fn print_drafts(s: &Situation, reference: Option<Vec<Owner>>, limit: u64) {
    let start = Instant::now();
    let opts = DraftOptions { reference, limit, trace: cli_trace(), ..DraftOptions::default() };
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
            if r.undecided > 0 {
                println!("{} undecided (node limit per draft)", r.undecided);
            }
            for &(i, p) in &r.impossible_pairs {
                println!("  impossible on its own: Player{} · {}", p + 1, s.tasks[i].card.id);
            }
            let mut rel = analysis::draft_relations(&r.drafts);
            // Already listed as impossible on its own.
            rel.never.retain(|&(t, o)| !matches!(o, Owner::Player(p) if r.impossible_pairs.contains(&(t, p as usize))));
            print_relations(s, &rel);
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
    let opts = search_options(args);
    let count = args.iter().any(|a| a == "--count-drafts");
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
            let start = Instant::now();
            match solver::solve_drafts(s, &opts) {
                Err(e) => println!("error: {e}"),
                Ok((v, owners, stats)) => {
                    match (&v, owners) {
                        (Verdict::Feasible(line), Some(owners)) => {
                            println!("FEASIBLE with this draft:");
                            let mut d = s.clone();
                            for (a, o) in d.tasks.iter_mut().zip(owners) {
                                println!("  {:<8} {}", format!("{}:", owner_name(&o)), a.card.id);
                                a.owner = o;
                            }
                            print_line(&d, line);
                        }
                        (Verdict::Infeasible, _) => println!("NOT FEASIBLE with any draft"),
                        (Verdict::Redeal(id), _) => println!("RE-DEAL (task '{id}')"),
                        (v, _) => println!("{v:?}"),
                    }
                    println!(
                        "{:.3} s, {} nodes, {} cache hits, {} cached positions",
                        start.elapsed().as_secs_f64(),
                        stats.nodes,
                        stats.cache_hits,
                        stats.cache_size
                    );
                }
            }
            if count {
                print_drafts(s, None, limit);
            }
            continue;
        }
        let start = Instant::now();
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
                    Verdict::Undecided => println!("UNDECIDED (node limit)"),
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
