//! **Gossip** — rumours of the director's drama, spreading soul to soul and **decaying as they
//! travel** (`docs/narrative_surfacing.md` §3, the veil). When the director stages a beat, everyone
//! who witnessed it learns the truth firsthand (fidelity `1.0`); from there it passes between
//! co-located souls, each hop shaving the fidelity and — once it has worn thin enough — shedding the
//! particulars (who the counterpart was). What the player overhears from a soul beside it is *that
//! soul's* worn copy, so distant, much-told news arrives vague while a witness's account is sharp.
//!
//! **Deterministic & off-by-default.** State lives in the [`Gossip`] resource (no NPC component);
//! the spread uses no randomness — fidelity decay and detail-shedding are arithmetic + thresholds,
//! and the per-tile update is *order-independent* (each soul learns each rumour at the tile's best
//! fidelity × one hop's decay, kept only if it beats what it held), so the result is identical
//! regardless of map iteration order. With the director asleep nothing is ever seeded, so the map
//! stays empty and [`gossip_spread`] early-returns — byte-identical to a world before this layer.

use crate::data::RegisterId;
use crate::people::Npc;
use crate::{Position, Substrate};
use bevy_ecs::prelude::*;
use game_sim::Coord;
use std::collections::HashMap;

/// Fidelity a rumour keeps across one hop (each retelling loses the rest).
const HOP_DECAY: f32 = 0.78;
/// Fidelity a rumour loses each tick to staleness — old news fades from talk over a season or so.
const AGE_DECAY: f32 = 0.004;
/// Below this a rumour is forgotten (dropped from the holder).
const FLOOR: f32 = 0.08;
/// Below this fidelity the rumour has worn too thin to carry *who the counterpart was* — the
/// telephone-game detail loss (the `other` is dropped), so even a later sharpening lacks it.
const OTHER_DROP: f32 = 0.5;
/// How many rumours a soul carries at once (the loudest few; the weakest is shed past this).
const CAP: usize = 4;

/// One soul's belief about a beat the director staged — possibly worn from the truth. `Copy`, so the
/// spread can shuffle it cheaply. `event_id` identifies the originating beat (so a soul holds one
/// belief per event, and two tellings of the same event merge to the sharper).
#[derive(Clone, Copy, Debug)]
pub struct Rumor {
    pub event_id: u64,
    pub register: RegisterId,
    pub lead: Entity,
    /// The counterpart — shed once the rumour wears below [`OTHER_DROP`].
    pub other: Option<Entity>,
    pub place: Coord,
    pub fidelity: f32,
}

/// Who knows what — each soul's worn copies of the rumours it has heard. A resource (no component),
/// so a gossip-free world is byte-identical.
#[derive(Resource, Default)]
pub struct Gossip {
    by_soul: HashMap<Entity, Vec<Rumor>>,
}

impl Gossip {
    /// A soul learns a rumour firsthand or hears a sharper telling — keep the better of what it
    /// holds for that event. The seeding the director does for every witness of a fresh beat.
    pub fn witness(&mut self, who: Entity, r: Rumor) {
        let list = self.by_soul.entry(who).or_default();
        match list.iter_mut().find(|x| x.event_id == r.event_id) {
            Some(slot) => {
                if r.fidelity > slot.fidelity {
                    *slot = r;
                }
            }
            None => {
                list.push(r);
                cap(list);
            }
        }
    }

    /// The rumours a soul currently holds — what it would pass on, or tell the player.
    pub fn rumors_of(&self, who: Entity) -> &[Rumor] {
        self.by_soul.get(&who).map(Vec::as_slice).unwrap_or(&[])
    }
}

/// Keep only the loudest [`CAP`] rumours, shedding the weakest (lowest fidelity; ties by id, so the
/// drop is deterministic and "keep the top-K" is order-independent however the list was built).
fn cap(list: &mut Vec<Rumor>) {
    while list.len() > CAP {
        if let Some((i, _)) = list.iter().enumerate().min_by(|a, b| {
            a.1.fidelity
                .partial_cmp(&b.1.fidelity)
                .unwrap()
                .then(b.1.event_id.cmp(&a.1.event_id))
        }) {
            list.swap_remove(i);
        } else {
            break;
        }
    }
}

/// Each tick: fade every held rumour a little (and prune the spent), then let co-located souls share
/// — each learns each rumour on its tile at the tile's **best fidelity × one hop's decay**, kept only
/// if it beats what it already held, the `other` shed once worn thin. One hop per tick, deterministic
/// and order-independent. Early-returns while no rumour is abroad (the director asleep) — byte-identical.
pub(crate) fn gossip_spread(
    substrate: Res<Substrate>,
    mut gossip: ResMut<Gossip>,
    people: Query<(Entity, &Position), With<Npc>>,
) {
    if gossip.by_soul.is_empty() {
        return;
    }
    // 1. Staleness: fade all, drop the spent, forget souls left empty.
    for list in gossip.by_soul.values_mut() {
        for r in list.iter_mut() {
            r.fidelity -= AGE_DECAY;
        }
        list.retain(|r| r.fidelity > FLOOR);
    }
    gossip.by_soul.retain(|_, l| !l.is_empty());

    // 2. Spread among the co-located. Group souls by tile (a soul is on exactly one tile, so tiles
    // partition them — cross-tile order is irrelevant).
    let mut by_tile: HashMap<usize, Vec<Entity>> = HashMap::new();
    {
        let topo = substrate.0.topology();
        for (e, pos) in &people {
            by_tile.entry(topo.index_of(pos.0)).or_default().push(e);
        }
    }
    for souls in by_tile.into_values() {
        if souls.len() < 2 {
            continue;
        }
        // The sharpest telling of each event present on the tile (computed before any write, so
        // every soul learns from the same pre-hop state — exactly one hop this tick).
        let mut best: HashMap<u64, Rumor> = HashMap::new();
        for &s in &souls {
            for &r in gossip.rumors_of(s) {
                best.entry(r.event_id)
                    .and_modify(|b| {
                        if r.fidelity > b.fidelity {
                            *b = r;
                        }
                    })
                    .or_insert(r);
            }
        }
        if best.is_empty() {
            continue;
        }
        for &s in &souls {
            for b in best.values() {
                let mut learned = *b;
                learned.fidelity = b.fidelity * HOP_DECAY;
                if learned.fidelity <= FLOOR {
                    continue;
                }
                if learned.fidelity < OTHER_DROP {
                    learned.other = None; // worn too thin to carry who the counterpart was
                }
                let list = gossip.by_soul.entry(s).or_default();
                match list.iter_mut().find(|x| x.event_id == learned.event_id) {
                    Some(slot) => {
                        if learned.fidelity > slot.fidelity {
                            slot.fidelity = learned.fidelity;
                            if learned.other.is_none() {
                                slot.other = None;
                            }
                        }
                    }
                    None => {
                        list.push(learned);
                        cap(list);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use game_sim::Coord;

    fn r(id: u64, fid: f32) -> Rumor {
        Rumor {
            event_id: id,
            register: 0, // any register id; this test exercises fidelity/cap, not the register
            lead: Entity::PLACEHOLDER,
            other: None,
            place: Coord::new(0, 0),
            fidelity: fid,
        }
    }

    #[test]
    fn witness_keeps_the_sharper_telling_and_caps_to_the_loudest() {
        let mut w = World::new();
        let who = w.spawn_empty().id();
        let mut g = Gossip::default();
        // A sharper telling of the same event replaces a worn one; a weaker one is ignored.
        g.witness(who, r(1, 0.4));
        g.witness(who, r(1, 0.9));
        g.witness(who, r(1, 0.2));
        assert_eq!(g.rumors_of(who).len(), 1, "one belief per event");
        assert!(
            (g.rumors_of(who)[0].fidelity - 0.9).abs() < 1e-6,
            "the sharpest telling is kept"
        );

        // Past the cap, the weakest events are shed — the loudest few survive.
        for id in 2..=(CAP as u64 + 3) {
            g.witness(who, r(id, 0.1 * id as f32));
        }
        assert_eq!(g.rumors_of(who).len(), CAP, "carries only the loudest few");
        assert!(
            g.rumors_of(who).iter().any(|x| x.event_id == 1),
            "the sharp rumour (0.9) survives the cap"
        );
    }
}
