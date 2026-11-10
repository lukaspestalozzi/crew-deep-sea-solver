//! Task drafts: which player takes which task.
//!
//! Standard draft: the captain picks first, then clockwise, one task each
//! round until all are taken. With `t` tasks and `p` players, the first
//! `t mod p` seats from the captain take one task more. With fewer tasks than
//! players, each player takes at most one. The captain may not take the
//! "compared with the captain" tasks.

use crate::catalogue::TaskCard;
use crate::rng::Rng;
use crate::situation::{Assignment, Owner};

/// How many tasks each seat takes, or `None` if any seat may take 0 or 1
/// (fewer tasks than players).
pub fn seat_counts(tasks: usize, players: usize, captain: usize) -> Option<Vec<usize>> {
    if tasks < players {
        return None;
    }
    let (base, extra) = (tasks / players, tasks % players);
    let mut v = vec![base; players];
    for i in 0..extra {
        v[(captain + i) % players] += 1;
    }
    Some(v)
}

/// Capacity left per seat for the unknown tasks, given the fixed owners.
/// `None` if the fixed owners already break the draft rule.
pub(crate) fn capacities(tasks: &[Assignment], players: usize, captain: usize) -> Option<Vec<usize>> {
    let draftable: Vec<&Assignment> = tasks.iter().filter(|a| a.owner != Owner::Crew).collect();
    let mut fixed = vec![0usize; players];
    for a in &draftable {
        if let Owner::Player(p) = a.owner {
            fixed[p as usize] += 1;
        }
    }
    let target = seat_counts(draftable.len(), players, captain).unwrap_or_else(|| vec![1; players]);
    let mut cap = Vec::with_capacity(players);
    for p in 0..players {
        cap.push(target[p].checked_sub(fixed[p])?);
    }
    Some(cap)
}

/// Every legal way to give the `?` tasks an owner. Each draft is the full
/// owner list, parallel to `tasks`. Calls `f` for each; stop early by
/// returning `false`. Returns the number of drafts visited.
pub fn for_each_draft(
    tasks: &[Assignment],
    players: usize,
    captain: usize,
    f: impl FnMut(&[Owner]) -> bool,
) -> u64 {
    for_each_draft_filtered(tasks, players, captain, &|_, _| true, f)
}

/// As [`for_each_draft`], skipping drafts that give task `i` to player `p`
/// when `allowed(i, p)` is false.
pub fn for_each_draft_filtered(
    tasks: &[Assignment],
    players: usize,
    captain: usize,
    allowed: &dyn Fn(usize, usize) -> bool,
    mut f: impl FnMut(&[Owner]) -> bool,
) -> u64 {
    let Some(mut cap) = capacities(tasks, players, captain) else { return 0 };
    let mut owners: Vec<Owner> = tasks.iter().map(|a| a.owner).collect();
    let open: Vec<usize> = (0..tasks.len()).filter(|&i| tasks[i].owner == Owner::Unknown).collect();
    let mut count = 0;
    let mut stop = false;
    rec(tasks, &open, 0, captain, allowed, &mut cap, &mut owners, &mut count, &mut stop, &mut f);
    count
}

#[allow(clippy::too_many_arguments)]
fn rec(
    tasks: &[Assignment],
    open: &[usize],
    k: usize,
    captain: usize,
    allowed: &dyn Fn(usize, usize) -> bool,
    cap: &mut [usize],
    owners: &mut [Owner],
    count: &mut u64,
    stop: &mut bool,
    f: &mut impl FnMut(&[Owner]) -> bool,
) {
    if *stop {
        return;
    }
    if k == open.len() {
        *count += 1;
        if !f(owners) {
            *stop = true;
        }
        return;
    }
    let i = open[k];
    for p in 0..cap.len() {
        if cap[p] == 0 || (p == captain && !tasks[i].card.captain_allowed()) || !allowed(i, p) {
            continue;
        }
        cap[p] -= 1;
        owners[i] = Owner::Player(p as u8);
        rec(tasks, open, k + 1, captain, allowed, cap, owners, count, stop, f);
        cap[p] += 1;
        if *stop {
            break;
        }
    }
    owners[i] = Owner::Unknown;
}

/// Number of legal drafts (saturating at `u64::MAX`). Counted without
/// enumerating: the captain takes `j` of the tasks he may take, the other
/// tasks are spread over the other seats within their capacities.
pub fn count_drafts(tasks: &[Assignment], players: usize, captain: usize) -> u64 {
    let Some(cap) = capacities(tasks, players, captain) else { return 0 };
    let open: Vec<&Assignment> = tasks.iter().filter(|a| a.owner == Owner::Unknown).collect();
    let n = open.len();
    let free = open.iter().filter(|a| a.card.captain_allowed()).count();
    // binom[a][b], saturating.
    let mut binom = vec![vec![0u128; n + 1]; n + 1];
    for a in 0..=n {
        binom[a][0] = 1;
        for b in 1..=a {
            binom[a][b] = binom[a - 1][b - 1].saturating_add(if b < a { binom[a - 1][b] } else { 0 });
        }
    }
    // ways[m]: distinct tasks to give out among the seats handled so far.
    let mut ways = vec![0u128; n + 1];
    ways[0] = 1;
    for p in (0..players).filter(|&p| p != captain) {
        let mut next = vec![0u128; n + 1];
        for m in 0..=n {
            for k in 0..=cap[p].min(m) {
                next[m] = next[m].saturating_add(binom[m][k].saturating_mul(ways[m - k]));
            }
        }
        ways = next;
    }
    let mut total = 0u128;
    for j in 0..=cap[captain].min(free) {
        total = total.saturating_add(binom[free][j].saturating_mul(ways[n - j]));
    }
    u64::try_from(total).unwrap_or(u64::MAX)
}

/// Do the fixed owners fit the standard draft: no seat holds more tasks than
/// it would draft, and the captain holds no task he may not take?
pub fn owners_fit_draft(tasks: &[Assignment], players: usize, captain: usize) -> bool {
    capacities(tasks, players, captain).is_some()
        && !tasks.iter().any(|a| a.owner == Owner::Player(captain as u8) && !a.card.captain_allowed())
}

/// A uniformly random legal draft (by rejection), or `None` if there is none.
pub fn random_draft(tasks: &[Assignment], players: usize, captain: usize, rng: &mut Rng) -> Option<Vec<Owner>> {
    let cap = capacities(tasks, players, captain)?;
    let open: Vec<usize> = (0..tasks.len()).filter(|&i| tasks[i].owner == Owner::Unknown).collect();
    let mut slots: Vec<usize> = (0..players).flat_map(|p| std::iter::repeat(p).take(cap[p])).collect();
    if slots.len() < open.len() {
        return None;
    }
    for _ in 0..1000 {
        rng.shuffle(&mut slots);
        let mut owners: Vec<Owner> = tasks.iter().map(|a| a.owner).collect();
        let mut ok = true;
        for (j, &i) in open.iter().enumerate() {
            let p = slots[j];
            if p == captain && !tasks[i].card.captain_allowed() {
                ok = false;
                break;
            }
            owners[i] = Owner::Player(p as u8);
        }
        if ok {
            return Some(owners);
        }
    }
    None
}

/// Draw task cards at random until their difficulty sums to exactly `target`
/// for this player count (cards that would overshoot are skipped). Rule cards
/// are never drawn.
pub fn draw_tasks(catalogue: &'static [TaskCard], players: usize, target: u32, rng: &mut Rng) -> Option<Vec<&'static TaskCard>> {
    for _ in 0..100 {
        let mut deck: Vec<&TaskCard> = catalogue.iter().filter(|t| !t.is_rule()).collect();
        rng.shuffle(&mut deck);
        let mut sum = 0;
        let mut out = Vec::new();
        for t in deck {
            let d = t.difficulty_for(players) as u32;
            if sum + d <= target {
                sum += d;
                out.push(t);
            }
            if sum == target {
                return Some(out);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalogue::find;

    fn tasks(spec: &[(&str, Owner)]) -> Vec<Assignment> {
        spec.iter().map(|(id, o)| Assignment { card: find(id).unwrap(), owner: *o }).collect()
    }

    #[test]
    fn seat_counts_start_at_captain() {
        assert_eq!(seat_counts(6, 4, 2), Some(vec![1, 1, 2, 2]));
        assert_eq!(seat_counts(8, 4, 0), Some(vec![2, 2, 2, 2]));
        assert_eq!(seat_counts(3, 4, 0), None);
    }

    #[test]
    fn counts_match_multinomials() {
        use Owner::Unknown as U;
        // 4 tasks, 4 players, 1 each: 4! = 24.
        let t = tasks(&[("win B4", U), ("win P9 Y8", U), ("first-trick", U), ("tricks =2", U)]);
        assert_eq!(count_drafts(&t, 4, 0), 24);
        // Captain may not take "more than captain": 3 * 3! = 18.
        let t = tasks(&[("win B4", U), ("win P9 Y8", U), ("first-trick", U), ("more than captain", U)]);
        assert_eq!(count_drafts(&t, 4, 0), 18);
        // 2 tasks, 4 players: ordered pairs of distinct players = 12.
        let t = tasks(&[("win B4", U), ("first-trick", U)]);
        assert_eq!(count_drafts(&t, 4, 0), 12);
        // One fixed owner, 3 unknown among the other 3 seats: 3! = 6.
        let t = tasks(&[("win B4", Owner::Player(1)), ("win P9 Y8", U), ("first-trick", U), ("tricks =2", U)]);
        assert_eq!(count_drafts(&t, 4, 0), 6);
        // Fixed owners that break the rule: Player2 takes two of four tasks.
        let t = tasks(&[("win B4", Owner::Player(1)), ("win P9 Y8", Owner::Player(1)), ("first-trick", U), ("tricks =2", U)]);
        assert_eq!(count_drafts(&t, 4, 0), 0);
    }

    #[test]
    fn count_matches_enumeration_on_random_configurations() {
        let cat: Vec<&'static TaskCard> = crate::catalogue::catalogue().iter().collect();
        let restricted: Vec<_> = cat.iter().filter(|t| !t.captain_allowed()).copied().collect();
        let rules: Vec<_> = cat.iter().filter(|t| t.is_rule()).copied().collect();
        let plain: Vec<_> = cat.iter().filter(|t| !t.is_rule() && t.captain_allowed()).copied().collect();
        let mut rng = Rng::new(42);
        let mut nonzero = 0;
        for _ in 0..600 {
            let players = 3 + rng.below(3);
            let captain = rng.below(players);
            let n = rng.below(10);
            let mut pool = plain.clone();
            rng.shuffle(&mut pool);
            let mut rpool = restricted.clone();
            rng.shuffle(&mut rpool);
            let mut t: Vec<Assignment> = Vec::new();
            for i in 0..n {
                let card = if i < rpool.len() && rng.below(4) == 0 { rpool[i] } else { pool[i] };
                let owner = match rng.below(6) {
                    0 => Owner::Player(rng.below(players) as u8),
                    _ => Owner::Unknown,
                };
                t.push(Assignment { card, owner });
            }
            for _ in 0..rng.below(3) {
                t.push(Assignment { card: rules[rng.below(rules.len())], owner: Owner::Crew });
            }
            let expected = for_each_draft(&t, players, captain, |_| true);
            let got = count_drafts(&t, players, captain);
            assert_eq!(got, expected, "players {players} captain {captain} tasks {:?}", t.iter().map(|a| (&a.card.id, a.owner)).collect::<Vec<_>>());
            nonzero += (got > 0) as usize;
        }
        assert!(nonzero > 200, "{nonzero}");
    }

    #[test]
    fn count_is_fast_and_saturates() {
        use Owner::Unknown as U;
        let cat: Vec<_> = crate::catalogue::catalogue().iter().filter(|t| !t.is_rule() && t.captain_allowed()).take(21).collect();
        let t: Vec<Assignment> = cat.iter().map(|c| Assignment { card: c, owner: U }).collect();
        // 21 tasks, 3 players, 7 each: 21! / (7!)^3.
        assert_eq!(count_drafts(&t, 3, 0), 399_072_960);
        let t: Vec<Assignment> = crate::catalogue::catalogue().iter().filter(|t| !t.is_rule() && t.captain_allowed()).take(96).map(|c| Assignment { card: c, owner: U }).collect();
        assert_eq!(count_drafts(&t, 3, 0), u64::MAX);
    }

    #[test]
    fn owners_that_break_the_draft_are_detected() {
        use Owner::Unknown as U;
        let t = tasks(&[("win B4", Owner::Player(2)), ("win P9 Y8", Owner::Player(2)), ("first-trick", Owner::Player(3)), ("tricks =2", Owner::Player(3))]);
        assert!(!owners_fit_draft(&t, 4, 0));
        let t = tasks(&[("win B4", Owner::Player(2)), ("win P9 Y8", Owner::Player(1)), ("first-trick", Owner::Player(3)), ("tricks =2", Owner::Player(0))]);
        assert!(owners_fit_draft(&t, 4, 0));
        // The captain holds a task he may not take.
        let t = tasks(&[("more than captain", Owner::Player(0)), ("win P9 Y8", U), ("first-trick", U), ("tricks =2", U)]);
        assert!(!owners_fit_draft(&t, 4, 0));
        assert!(owners_fit_draft(&t, 4, 1));
    }

    #[test]
    fn random_draft_is_legal() {
        use Owner::Unknown as U;
        let t = tasks(&[("win B4", U), ("win P9 Y8", U), ("first-trick", U), ("more than captain", U), ("tricks =2", U)]);
        let mut rng = Rng::new(3);
        for _ in 0..50 {
            let d = random_draft(&t, 4, 0, &mut rng).unwrap();
            let mut per = [0; 4];
            for (i, o) in d.iter().enumerate() {
                let Owner::Player(p) = o else { panic!() };
                per[*p as usize] += 1;
                assert!(!(i == 3 && *p == 0));
            }
            assert_eq!(per, [2, 1, 1, 1]);
        }
    }
}
