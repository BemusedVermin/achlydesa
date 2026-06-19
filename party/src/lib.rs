//! The **party layer**: recruited NPCs that travel with the avatar as a stack.
//!
//! Deliberately tiny and self-contained — just the roster, its tunables, and the pure
//! disposition→difficulty helper. The recruit *orchestration* (reading an NPC's disposition
//! and the avatar's social skill, running the deterministic check, and stamping the member
//! with the `agent_core` `Suspended` + `Follower` seams) lives in the `agents` assembler,
//! which is the one crate that has both `agent_core` and `rpg` in scope.

use bevy_ecs::prelude::*;

/// Marks a recruited party member — an ordinary NPC that now follows the avatar. It keeps all
/// its own components (economy, personality, WWN stats), but the assembler also stamps it with
/// the `agent_core` `Suspended` marker (so it skips its own planning) and `Follower` (so it
/// moves with the avatar). Removing those three un-recruits it.
#[derive(Component, Clone, Copy, Debug)]
pub struct PartyMember {
    /// The tick it joined.
    pub since: u64,
}

/// The avatar's party roster, in recruit order (deterministic — never iterate a set here).
#[derive(Resource, Clone, Debug, Default)]
pub struct Party {
    pub members: Vec<Entity>,
}

impl Party {
    pub fn len(&self) -> usize {
        self.members.len()
    }
    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }
    pub fn contains(&self, e: Entity) -> bool {
        self.members.contains(&e)
    }
    pub fn push(&mut self, e: Entity) {
        if !self.contains(e) {
            self.members.push(e);
        }
    }
    pub fn remove(&mut self, e: Entity) {
        self.members.retain(|&m| m != e);
    }
}

/// Knobs for recruitment.
#[derive(Resource, Clone, Copy, Debug)]
pub struct PartyConfig {
    /// Base difficulty of a recruit check against a neutral NPC (the WWN ladder: 6/8/10/12…).
    pub recruit_difficulty: i32,
    /// How strongly the NPC's opinion of the avatar (−1..1) shifts that difficulty — a friendly
    /// soul is easier to recruit, a hostile one harder.
    pub disposition_weight: f32,
    /// Cap on party size. `0` = no limit.
    pub max_size: usize,
}

impl Default for PartyConfig {
    fn default() -> Self {
        Self { recruit_difficulty: 8, disposition_weight: 4.0, max_size: 0 }
    }
}

/// The effective recruit difficulty for an NPC whose opinion of the avatar is `opinion`
/// (−1..1). Pure + deterministic: a friendlier soul lowers the number, a hostile one raises it.
pub fn disposition_difficulty(cfg: &PartyConfig, opinion: f32) -> i32 {
    cfg.recruit_difficulty - (opinion * cfg.disposition_weight).round() as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disposition_eases_with_a_warm_opinion() {
        let cfg = PartyConfig::default(); // base 8, weight 4
        assert_eq!(disposition_difficulty(&cfg, 0.0), 8);
        assert_eq!(disposition_difficulty(&cfg, 1.0), 4, "a beloved hero recruits easily");
        assert_eq!(disposition_difficulty(&cfg, -1.0), 12, "a hated one struggles");
    }

    #[test]
    fn roster_keeps_order_and_dedups() {
        let mut w = World::new();
        let (a, b) = (w.spawn_empty().id(), w.spawn_empty().id());
        let mut p = Party::default();
        p.push(a);
        p.push(b);
        p.push(a); // already in — ignored
        assert_eq!(p.members, vec![a, b]);
        p.remove(a);
        assert_eq!(p.members, vec![b]);
    }
}
