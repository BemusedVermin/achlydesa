//! Tick scheduling `σ`.

use crate::action::Action;
use crate::actor::Actor;
use crate::rng::Rng;
use crate::substrate::Substrate;

/// Runs one tick over the population.
pub trait Scheduler<S: Substrate> {
    fn tick(&mut self, actors: &mut [Box<dyn Actor<S>>], substrate: &mut S, rng: &mut dyn Rng);
}

/// Synchronous scheduler with conflict re-decision.
///
/// Round 1: every actor perceives the same start-of-tick state and submits an
/// intent — so a player action placed before the tick resolves on equal footing
/// with the AI. Contested claims are arbitrated (one winner per resource by
/// `Action::priority`, ties at random); losers see the updated state and decide
/// again. Repeats until everyone has acted or passed (`decide` returns `None`).
///
/// Losers are re-asked via `decide`, so a player-driven actor should return its
/// placed action while still viable and `None` once it is not.
pub struct Synchronous;

impl<S: Substrate> Scheduler<S> for Synchronous {
    fn tick(&mut self, actors: &mut [Box<dyn Actor<S>>], substrate: &mut S, rng: &mut dyn Rng) {
        let n = actors.len();
        let mut done = vec![false; n];

        // each round commits >= 1 actor (or the rest pass), so n+1 rounds suffice
        for _ in 0..=n {
            if done.iter().all(|&d| d) {
                break;
            }

            // Phase 1 (synchronous): perceive + decide against the current state
            let mut intents: Vec<(usize, Box<dyn Action<S>>)> = Vec::new();
            for i in 0..n {
                if done[i] {
                    continue;
                }
                let perception = actors[i].perceive(substrate);
                match actors[i].decide(&perception, rng) {
                    None => done[i] = true,
                    Some(action) => intents.push((i, action)),
                }
            }
            if intents.is_empty() {
                break;
            }

            // Phase 2: uncontested commit; contested arbitrate, losers retry
            let mut winners: Vec<(usize, Box<dyn Action<S>>)> = Vec::new();
            let mut contested: Vec<(usize, Box<dyn Action<S>>)> = Vec::new();
            for entry in intents {
                if entry.1.claim().is_some() {
                    contested.push(entry);
                } else {
                    winners.push(entry);
                }
            }
            while let Some(first) = contested.pop() {
                let claim = first.1.claim();
                let mut group = vec![first];
                let mut k = 0;
                while k < contested.len() {
                    if contested[k].1.claim() == claim {
                        group.push(contested.remove(k));
                    } else {
                        k += 1;
                    }
                }
                let win = pick_winner(&group, rng);
                winners.push(group.swap_remove(win));
            }

            for (i, action) in winners {
                action.apply(actors, i, substrate);
                done[i] = true;
            }
        }
    }
}

/// Index of the highest-priority entry, ties broken at random.
fn pick_winner<S: Substrate>(group: &[(usize, Box<dyn Action<S>>)], rng: &mut dyn Rng) -> usize {
    let top = group.iter().map(|(_, a)| a.priority()).max().unwrap();
    let candidates: Vec<usize> = group
        .iter()
        .enumerate()
        .filter(|(_, (_, a))| a.priority() == top)
        .map(|(idx, _)| idx)
        .collect();
    candidates[rng.gen_range(candidates.len())]
}
