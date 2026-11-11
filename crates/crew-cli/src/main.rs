//! `crew` command line.
//!
//! ```text
//! crew solve FILE [--no-cache]   solve every situation in a .crew file
//! ```
//!
//! Exit codes of `solve`: 1 if any situation had an error (or the arguments
//! were wrong), else 2 if any verdict is infeasible or a re-deal, else 3 if
//! any is undecided or cancelled, else 0.

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
        Some("bench") => cmd_bench(&args[1..]).map(|()| 0),
        Some("orders") => cmd_orders(&args[1..]).map(|()| 0),
        _ => Err(USAGE.to_string()),
    };
    match result {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

const USAGE: &str = "usage:
  crew solve FILE [--analyse] [--all-drafts] [--count-drafts] [--limit N] [--trace OUT] [search flags]
                                 solve every situation in a .crew file; tasks with
                                 owner ? are searched over all legal drafts at once;
                                 --count-drafts also counts the drafts that work (sampled
                                 above N drafts, N >= 1, default 2000; only for tasks with
                                 owner ?); --all-drafts also checks the other drafts, judged
                                 from the deal (only with fixed owners); --analyse adds
                                 critical decisions, smallest conflict and point of no
                                 return (only with fixed owners)
                                 exit code: 1 if any situation had an error, else 2 if any
                                 verdict is infeasible or a re-deal, else 3 if any is
                                 undecided or cancelled, else 0
  crew bench [--players N] [--target SUM] [--count K] [--seed S] [--timeout SECS] [--save FILE] [-v] [--trace OUT] [search flags]
                                 random deals, tasks drawn to the target sum, random legal
                                 draft; N is 3 to 5, SECS a positive number
  crew bench --load FILE [--timeout SECS] [-v] [--trace OUT] [search flags]
                                 the same timing over the situations in FILE; with owner ?
                                 the verdict over all drafts is timed
  crew orders [--players N] [--target SUM] [--count K] [--seed S] [--budget NODES] [--effort NODES] [--threads T]
                                 compare the move orders: random missions with a random
                                 draft; at every position with a choice along a winning
                                 line, how often each order's first choice wins and the
                                 mean rank of its first winning move (leads and follows
                                 apart); and the positions a depth-first search needs with
                                 each order (at most --effort per mission)
  search flags (solve, bench): --dfs, --best-first, --deck-order, --task-aware,
                                 --lookahead-all, --zobrist, --no-cache switch the search
                                 strategy, move order, hashing and position cache (default:
                                 restarts alternating move orders); --trace OUT writes every
                                 search as JSON lines";

/// The command line of one command: flags with a value, flags without, and
/// the arguments that are neither. Anything else is an error.
struct Args {
    values: Vec<(String, String)>,
    flags: Vec<String>,
    positional: Vec<String>,
}

/// Flags that choose the search, for `solve` and `bench`.
const SEARCH_FLAGS: &[&str] = &["--no-cache", "--dfs", "--best-first", "--deck-order", "--task-aware", "--lookahead-all", "--zobrist"];

fn parse_args(args: &[String], values: &[&str], flags: &[&str]) -> Result<Args, String> {
    let mut out = Args { values: Vec::new(), flags: Vec::new(), positional: Vec::new() };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if values.contains(&a.as_str()) {
            match it.next() {
                Some(v) if !v.starts_with("--") => out.values.push((a.clone(), v.clone())),
                _ => return Err(format!("{a} needs a value\n{USAGE}")),
            }
        } else if flags.contains(&a.as_str()) {
            out.flags.push(a.clone());
        } else if a.len() > 1 && a.starts_with('-') {
            return Err(format!("unknown flag '{a}'\n{USAGE}"));
        } else {
            out.positional.push(a.clone());
        }
    }
    Ok(out)
}

impl Args {
    fn has(&self, name: &str) -> bool {
        self.flags.iter().any(|f| f == name)
    }

    fn value(&self, name: &str) -> Option<&str> {
        self.values.iter().rev().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    /// The value of a flag as a number, or `default`.
    fn num<T: std::str::FromStr>(&self, name: &str, default: T) -> Result<T, String> {
        match self.value(name) {
            None => Ok(default),
            Some(v) => v.parse().map_err(|_| format!("{name}: '{v}' is not a valid number")),
        }
    }

    /// No arguments besides flags.
    fn no_positional(&self) -> Result<(), String> {
        match self.positional.first() {
            Some(p) => Err(format!("unexpected argument '{p}'\n{USAGE}")),
            None => Ok(()),
        }
    }
}

/// 3 to 5 players.
fn players_arg(a: &Args) -> Result<usize, String> {
    let n: usize = a.num("--players", 4)?;
    if (3..=5).contains(&n) {
        Ok(n)
    } else {
        Err(format!("--players: {n} is not 3, 4 or 5"))
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
            if rx.recv_timeout(Duration::try_from_secs_f64(secs).unwrap_or(Duration::MAX)).is_err() {
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

fn search_options(args: &Args) -> Result<Options<'static>, String> {
    let mut o = Options::default();
    if let Some(path) = args.value("--trace") {
        let f = std::fs::File::create(path).map_err(|e| format!("--trace {path}: {e}"))?;
        let _ = TRACE.set(std::sync::Mutex::new((f, Instant::now())));
    }
    o.trace = cli_trace();
    if args.has("--no-cache") {
        o.use_cache = false;
    }
    if args.has("--deck-order") {
        o.decider = &crew_core::decider::DECK_ORDER;
    }
    if args.has("--task-aware") {
        o.decider = &crew_core::decider::TASK_AWARE;
    }
    if args.has("--lookahead-all") {
        o.decider = &crew_core::decider::LOOKAHEAD_ALL;
    }
    if args.has("--zobrist") {
        o.zobrist = true;
    }
    if args.has("--dfs") {
        o.strategy = solver::Strategy::Dfs;
    }
    if args.has("--best-first") {
        o.strategy = solver::Strategy::BestFirst { max_positions: 2_000_000 };
    }
    Ok(o)
}

/// Task cards drawn to the difficulty `target`, at most [`notation::MAX_TASKS`] of them.
fn draw_tasks(players: usize, target: u32, rng: &mut crew_core::rng::Rng) -> Result<Vec<&'static crew_core::catalogue::TaskCard>, String> {
    let cards = crew_core::draft::draw_tasks(crew_core::catalogue::catalogue(), players, target, rng)
        .ok_or_else(|| format!("could not draw task cards to difficulty {target}"))?;
    if cards.len() > notation::MAX_TASKS {
        return Err(format!("difficulty {target} needs {} task cards; at most {} are allowed", cards.len(), notation::MAX_TASKS));
    }
    Ok(cards)
}

/// A random mission: deal, task cards drawn to `target`, a random legal draft.
fn random_mission(players: usize, target: u32, rng: &mut crew_core::rng::Rng, title: String) -> Result<Situation, String> {
    use crew_core::draft;
    use crew_core::game::Game;
    use crew_core::situation::Assignment;
    let hands = Game::random_hands(players, rng);
    let captain = Game::new(&hands).unwrap().captain as usize;
    let cards = draw_tasks(players, target, rng)?;
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
    let a = parse_args(args, &["--players", "--target", "--count", "--seed", "--budget", "--effort", "--threads"], &[])?;
    a.no_positional()?;
    let players = players_arg(&a)?;
    let target: u32 = a.num("--target", 7)?;
    let count: usize = a.num("--count", 20)?;
    let seed: u64 = a.num("--seed", 1)?;
    let budget: u64 = a.num("--budget", 2_000_000)?;
    let effort: u64 = a.num("--effort", 5_000_000)?;
    let threads: usize = a.num("--threads", std::thread::available_parallelism().map_or(1, |n| n.get()))?;
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
    use crew_core::draft;
    use crew_core::game::Game;
    use crew_core::rng::Rng;
    use crew_core::situation::{Assignment, Owner};

    let values = ["--players", "--target", "--count", "--seed", "--timeout", "--save", "--load", "--trace"];
    let a = parse_args(args, &values, &[SEARCH_FLAGS, &["-v"]].concat())?;
    a.no_positional()?;
    let players = players_arg(&a)?;
    let target: u32 = a.num("--target", 7)?;
    let count: usize = a.num("--count", 20)?;
    let seed: u64 = a.num("--seed", 1)?;
    let timeout: f64 = a.num("--timeout", 10.0)?;
    if !(timeout.is_finite() && timeout > 0.0 && timeout <= 1e9) {
        return Err(format!("--timeout: {timeout} is not a number of seconds between 0 and 1e9"));
    }
    let save: Option<String> = a.value("--save").map(String::from);
    let verbose = a.has("-v");
    let search_opts = search_options(&a)?;

    let load: Option<String> = a.value("--load").map(String::from);
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
                let cards = draw_tasks(players, target, &mut rng)?;
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

/// Who takes which task and which tasks share a player, if anything stands
/// out. With `sampled` the drafts are only a sample: what holds in all of
/// them is not proven for every working draft, and is worded so.
fn print_relations(s: &Situation, r: &analysis::DraftRelations, sampled: Option<usize>) {
    let id = |t: usize| s.tasks[t].card.id.as_str();
    let (must, never, always, never_together) = match sampled {
        None => ("must:  ", "never: ", "always same player: ", "never same player:  "),
        Some(_) => (
            "in all sampled working drafts:  ",
            "in none of the sampled working drafts:  ",
            "same player in all sampled working drafts: ",
            "different players in all sampled working drafts: ",
        ),
    };
    let lines: Vec<String> = r
        .must
        .iter()
        .map(|&(t, o)| format!("{must}{} · {}", owner_name(&o), id(t)))
        .chain(r.never.iter().map(|&(t, o)| format!("{never}{} · {}", owner_name(&o), id(t))))
        .chain(r.always_together.iter().map(|&(a, b)| format!("{always}{} + {}", id(a), id(b))))
        .chain(r.never_together.iter().map(|&(a, b)| format!("{never_together}{} + {}", id(a), id(b))))
        .chain(r.tendencies.iter().map(|&(a, b, w, all)| {
            format!("same player in {:.0}% of working drafts ({:.0}% of all): {} + {}", 100.0 * w, 100.0 * all, id(a), id(b))
        }))
        .collect();
    if !lines.is_empty() {
        match sampled {
            None => println!("who takes what (working drafts compared with all drafts):"),
            Some(n) => println!("who takes what, among the {n} sampled working drafts only (a sample, not every draft):"),
        }
        for l in lines {
            println!("  {l}");
        }
    }
}

/// Search the drafts and print the result; `false` if that was an error.
fn print_drafts(s: &Situation, reference: Option<Vec<Owner>>, limit: u64) -> bool {
    let start = Instant::now();
    let opts = DraftOptions { reference, limit, trace: cli_trace(), ..DraftOptions::default() };
    match analysis::draft_search(s, &opts) {
        Err(e) => {
            println!("error: {e}");
            false
        }
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
            if r.total == 0 {
                println!("no legal draft: the fixed owners, or the tasks the captain may not take, do not fit the draft rule");
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
            print_relations(s, &rel, r.sampled.then_some(r.working as usize));
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
            true
        }
    }
}

/// Exit code of one verdict (see the usage text).
fn verdict_code(v: &Verdict) -> u8 {
    match v {
        Verdict::Feasible(_) => 0,
        Verdict::Infeasible | Verdict::Redeal(_) => 2,
        Verdict::Cancelled | Verdict::Undecided => 3,
    }
}

fn cmd_solve(args: &[String]) -> Result<u8, String> {
    let a = parse_args(
        args,
        &["--limit", "--trace"],
        &[SEARCH_FLAGS, &["--analyse", "--all-drafts", "--count-drafts"]].concat(),
    )?;
    let file = match a.positional.as_slice() {
        [f] => f,
        [] => return Err(USAGE.to_string()),
        [_, extra, ..] => return Err(format!("unexpected argument '{extra}'\n{USAGE}")),
    };
    let opts = search_options(&a)?;
    let count = a.has("--count-drafts");
    let all_drafts = a.has("--all-drafts");
    let analyse = a.has("--analyse");
    let limit: u64 = a.num("--limit", 2000)?;
    if limit == 0 {
        return Err(format!("--limit: must be at least 1\n{USAGE}"));
    }
    let text = std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
    let list = notation::parse_all(&text).map_err(|e| format!("{file}: {e}"))?;
    // Worst outcome so far: 1 error, 2 infeasible or re-deal, 3 undecided or cancelled (the
    // code of the first kind in that order of priority).
    let (mut errors, mut lost, mut open) = (false, false, false);
    let mut tally = |code: u8| match code {
        2 => lost = true,
        3 => open = true,
        _ => {}
    };
    for (i, s) in list.iter().enumerate() {
        if list.len() > 1 {
            println!("--- situation {} ---", i + 1);
        }
        if let Some(t) = &s.title {
            println!("{t}");
        }
        if let Ok(g) = s.game() {
            if !crew_core::draft::owners_fit_draft(&s.tasks, g.n, g.captain as usize) {
                println!("warning: the fixed owners do not fit the standard draft (too many tasks for a player, or the captain holds a task he may not take); solving anyway");
            }
        }
        if s.tasks.iter().any(|a| a.owner == Owner::Unknown) {
            if analyse || all_drafts {
                let names: Vec<&str> = [(analyse, "--analyse"), (all_drafts, "--all-drafts")].iter().filter(|x| x.0).map(|x| x.1).collect();
                println!("note: {} ignored: this situation has tasks with owner ?, searched over all drafts (--count-drafts counts the ones that work)", names.join(" and "));
            }
            let start = Instant::now();
            match solver::solve_drafts(s, &opts) {
                Err(e) => {
                    println!("error: {e}");
                    errors = true;
                }
                Ok((v, owners, stats)) => {
                    tally(verdict_code(&v));
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
                errors |= !print_drafts(s, None, limit);
            }
            continue;
        }
        if count {
            println!("note: --count-drafts ignored: no task has owner ? (use --all-drafts to check the other drafts)");
        }
        let start = Instant::now();
        match solver::solve(s, &opts) {
            Err(e) => {
                println!("error: {e}");
                errors = true;
            }
            Ok((v, stats)) => {
                tally(verdict_code(&v));
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
            // Whether another draft would have worked is judged from the deal.
            let mut all = s.clone();
            all.played.clear();
            for a in all.tasks.iter_mut().filter(|a| a.owner != Owner::Crew) {
                a.owner = Owner::Unknown;
            }
            errors |= !print_drafts(&all, Some(reference), limit);
        }
    }
    Ok(if errors {
        1
    } else if lost {
        2
    } else if open {
        3
    } else {
        0
    })
}

/// Print a winning line trick by trick, continuing from the situation's position.
fn print_line(s: &Situation, line: &[Card]) {
    let mut g = s.game().expect("situation was validated");
    // The trick in progress already holds cards.
    let mut trick_cards: Vec<String> = g.current.iter().map(|c| c.to_string()).collect();
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
