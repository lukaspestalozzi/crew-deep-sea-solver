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
fn capacities(tasks: &[Assignment], players: usize, captain: usize) -> Option<Vec<usize>> {
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
    mut f: impl FnMut(&[Owner]) -> bool,
) -> u64 {
    let Some(mut cap) = capacities(tasks, players, captain) else { return 0 };
    let mut owners: Vec<Owner> = tasks.iter().map(|a| a.owner).collect();
    let open: Vec<usize> = (0..tasks.len()).filter(|&i| tasks[i].owner == Owner::Unknown).collect();
    let mut count = 0;
    let mut stop = false;
    rec(tasks, &open, 0, captain, &mut cap, &mut owners, &mut count, &mut stop, &mut f);
    count
}

#[allow(clippy::too_many_arguments)]
fn rec(
    tasks: &[Assignment],
    open: &[usize],
    k: usize,
    captain: usize,
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
        if cap[p] == 0 || (p == captain && !tasks[i].card.captain_allowed()) {
            continue;
        }
        cap[p] -= 1;
        owners[i] = Owner::Player(p as u8);
        rec(tasks, open, k + 1, captain, cap, owners, count, stop, f);
        cap[p] += 1;
        if *stop {
            break;
        }
    }
    owners[i] = Owner::Unknown;
}

/// Number of legal drafts.
pub fn count_drafts(tasks: &[Assignment], players: usize, captain: usize) -> u64 {
    for_each_draft(tasks, players, captain, |_| true)
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
