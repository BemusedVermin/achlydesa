//! The combat bridge — the seam between the living-world sim and the headless `combat_core`
//! engine (`docs/combat-integration.md`).
//!
//! `combat_core` is pure and engine-agnostic; this module is what turns *world entities* into a
//! fight and writes the *outcome* back. It is gated by [`Setup::combat`](crate::Setup::combat):
//! with combat off none of its resources exist, so a world is byte-identical to one before the
//! layer (the off-by-default invariant). Combat is **player-paced**, so the live
//! [`combat_core::Sim`] is owned and driven by the caller (the app), exactly like a conversation —
//! the bridge only *builds* an [`Encounter`] and later *applies* its result.
//!
//! v1 simplifications (see the design doc): move kits are authored in code here (RON later);
//! "HP" is a combat-local [`Health`] derived from Constitution and persisted between fights; only
//! death writes back to the broader world. The avatar party is the Player faction; everyone else
//! is the Enemy faction driven by `combat_core::StubAi`.

use crate::{Coord, fauna, people};
use agent_core::Position;
use bevy_ecs::entity::Entity;
use bevy_ecs::prelude::*;
use combat_core as cc;
use serde::Deserialize;

// ── Move catalogue (RON-authored) ────────────────────────────────────────────────────────────
// The catalogue and per-archetype kits are authored in `assets/data/combat.ron` and compiled into
// `combat_core` MoveDefs here. Repositioning verbs are generated (one per zone), not authored.

/// Repositioning moves are `REPOSITION_BASE + zone`.
const REPOSITION_BASE: u32 = 10;
/// Number of abstract zones in the positioning model (Left / Center / Right).
pub const ZONE_COUNT: u8 = 3;
/// Everyone starts engaged in the centre zone.
pub const CENTER_ZONE: u8 = 1;

/// The move id that repositions to `zone` (for the position map's direction controls).
pub fn reposition_move(zone: u8) -> cc::MoveId {
    cc::MoveId(REPOSITION_BASE + zone.min(ZONE_COUNT - 1) as u32)
}

/// One authored move, compiled into a [`cc::MoveDef`] through the builder seam.
#[derive(Clone, Debug, Deserialize)]
struct MoveSpec {
    id: u32,
    name: String,
    startup: u32,
    active: u32,
    recovery: u32,
    #[serde(default)]
    priority: u8,
    #[serde(default)]
    armored: bool,
    /// Gated to the attacker's zone when zone-gating is on.
    #[serde(default)]
    melee: bool,
    #[serde(default)]
    damage: i32,
    /// `LineKnockback` ticks (0 = none).
    #[serde(default)]
    knockback: u32,
    /// `OpenWindow(Exposed)` duration (0 = none).
    #[serde(default)]
    expose: u32,
    #[serde(default)]
    requires_exposed: bool,
    #[serde(default)]
    tempo_cost: i32,
}

impl MoveSpec {
    fn compile(&self, melee_range: cc::ZoneReq) -> cc::MoveDef {
        use cc::{Effect, Fixed, MoveDef, MoveId, WindowTag, ZoneReq};
        let mut b = MoveDef::builder(MoveId(self.id), self.name.clone())
            .frames(self.startup, self.active, self.recovery)
            .priority(self.priority)
            .tempo_cost(self.tempo_cost)
            .range(if self.melee {
                melee_range
            } else {
                ZoneReq::AnyZone
            });
        if self.armored {
            b = b.armored();
        }
        if self.knockback > 0 {
            b = b.effect(Effect::LineKnockback {
                ticks: self.knockback,
            });
        }
        if self.expose > 0 {
            b = b.effect(Effect::OpenWindow {
                tag: WindowTag::Exposed,
                duration: self.expose,
                magnitude: Fixed::ZERO,
            });
        }
        if self.damage != 0 {
            b = b.damage(self.damage);
        }
        if self.requires_exposed {
            b = b.requires(WindowTag::Exposed);
        }
        b.build()
    }
}

#[derive(Clone, Debug, Deserialize)]
struct ArchetypeSpec {
    name: String,
    kit: Vec<u32>,
}

/// The RON-authored combat content: the move catalogue, the default action-tray kit, and the
/// per-archetype kits. Inserted as a resource when the combat layer is on.
#[derive(Resource, Clone, Debug, Deserialize)]
pub struct CombatContent {
    tray: Vec<u32>,
    moves: Vec<MoveSpec>,
    archetypes: Vec<ArchetypeSpec>,
}

impl CombatContent {
    /// The content baked in at compile time (via `config`), parsed from `assets/data/combat.ron`.
    pub fn bundled() -> Self {
        config::Config::bundled()
            .load(config::Asset::Combat)
            .expect("bundled combat.ron parses")
    }

    /// Compile the catalogue into a move library, appending the generated reposition verbs. Melee
    /// moves are zone-gated when `gating` is set.
    fn library(&self, gating: bool) -> cc::MoveLibrary {
        let melee = if gating {
            cc::ZoneReq::SameZone
        } else {
            cc::ZoneReq::AnyZone
        };
        let mut defs: Vec<cc::MoveDef> = self.moves.iter().map(|m| m.compile(melee)).collect();
        for zone in 0..ZONE_COUNT {
            defs.push(
                cc::MoveDef::builder(cc::MoveId(REPOSITION_BASE + zone as u32), "Reposition")
                    .frames(2, 1, 1)
                    .priority(1)
                    .reposition(zone)
                    .build(),
            );
        }
        cc::MoveLibrary::from_defs(defs)
    }

    /// The kit for `archetype` (falling back to the action-tray kit for an unknown one).
    fn kit_of(&self, archetype: &str) -> Vec<cc::MoveId> {
        self.archetypes
            .iter()
            .find(|a| a.name == archetype)
            .map(|a| &a.kit)
            .unwrap_or(&self.tray)
            .iter()
            .map(|&id| cc::MoveId(id))
            .collect()
    }
}

/// Which kit a body fights with: the avatar and party are adventurers, predators bull in, prey
/// kite, everyone else is a soldier.
fn archetype_of(world: &World, e: Entity, is_player: bool) -> &'static str {
    if is_player {
        "adventurer"
    } else if world.get::<fauna::Carnivore>(e).is_some() {
        "predator"
    } else if world.get::<fauna::Herbivore>(e).is_some() {
        "prey"
    } else {
        "soldier"
    }
}

// ── Tunables / resources / components ────────────────────────────────────────────────────────

/// Combat tunables. Stored as a resource when [`Setup::combat`](crate::Setup::combat) is on.
#[derive(Resource, Clone, Copy, Debug)]
pub struct CombatConfig {
    /// The headless engine's own knob surface.
    pub engine: cc::Config,
    /// HP a Constitution-neutral body fields.
    pub base_hp: i32,
    /// HP added (or removed) per point of Constitution modifier.
    pub hp_per_con: i32,
    /// Tempo each Player-faction combatant (avatar + party) spawns with.
    pub party_tempo: i32,
    /// Tempo an *elite* enemy spawns with (a named NPC); mooks/beasts spawn with 0.
    pub elite_tempo: i32,
    /// Gate the heavy melee moves to the attacker's zone (close in to land them). Off makes every
    /// move reach any zone — the position map then only displays, never gates.
    pub zone_gating: bool,
    /// Ticks of overworld time per 1 HP mended out of combat (0 disables regen). Combat freezes the
    /// world, so this only heals between fights.
    pub regen_period: u64,
}

impl Default for CombatConfig {
    fn default() -> Self {
        Self {
            engine: cc::Config::default(),
            base_hp: 20,
            hp_per_con: 6,
            party_tempo: 8,
            elite_tempo: 6,
            zone_gating: true,
            regen_period: 25,
        }
    }
}

/// Out-of-combat healing: bodies that have fought slowly mend. Added to the schedule only when the
/// combat layer is on (so a combat-off world never sees it), and — because combat freezes the world
/// — it ticks only between fights. The counter is system-local, so it perturbs nothing else.
pub fn regen_health(
    cfg: Res<CombatConfig>,
    mut counter: Local<u64>,
    mut bodies: Query<&mut Health>,
) {
    if cfg.regen_period == 0 {
        return;
    }
    *counter += 1;
    if *counter % cfg.regen_period != 0 {
        return;
    }
    for mut h in &mut bodies {
        if h.hp < h.max {
            h.hp += 1;
        }
    }
}

/// Persistent combat health for any body that has fought. Created on demand at an encounter's
/// start (so worldgen stays byte-identical) and carried between fights; death is the only thing
/// that writes back to the wider world.
#[derive(Component, Clone, Copy, Debug)]
pub struct Health {
    pub hp: i32,
    pub max: i32,
}

/// Determinism bookkeeping for the combat layer: the base stream seed and a monotonic encounter
/// counter, so each fight seeds its `Sim` reproducibly and independently of every other layer.
#[derive(Resource, Clone, Copy, Debug)]
pub struct CombatState {
    pub seed: u64,
    pub encounters: u64,
}

/// One combatant's identity in a fight, for the UI to label portraits/tokens.
#[derive(Clone, Debug)]
pub struct Combatant {
    pub actor: cc::ActorId,
    pub entity: Entity,
    pub name: String,
    pub is_player_side: bool,
    /// True for the avatar itself (player_actors[0]).
    pub is_avatar: bool,
}

/// A live fight: the engine `Sim` (driven by the caller) plus the mapping back to world entities.
pub struct Encounter {
    pub sim: cc::Sim,
    pub combatants: Vec<Combatant>,
}

impl Encounter {
    /// The combatant record for an engine actor id.
    pub fn of(&self, actor: cc::ActorId) -> Option<&Combatant> {
        self.combatants.iter().find(|c| c.actor == actor)
    }
    /// Player-faction engine actor ids, in roster order (avatar first).
    pub fn player_actors(&self) -> impl Iterator<Item = cc::ActorId> + '_ {
        self.combatants
            .iter()
            .filter(|c| c.is_player_side)
            .map(|c| c.actor)
    }
}

/// What a finished fight did to the world.
#[derive(Clone, Debug)]
pub struct Resolution {
    pub outcome: cc::Outcome,
    /// World entities that fell (enemies were despawned; the avatar is only flagged).
    pub downed: Vec<Entity>,
    /// The avatar went down — the caller decides what game-over means.
    pub avatar_down: bool,
    /// The player faction won.
    pub victory: bool,
}

// ── Stat derivation ──────────────────────────────────────────────────────────────────────────

fn con_mod(world: &World, e: Entity) -> i32 {
    world
        .get::<rpg::Abilities>(e)
        .map(|a| a.modifier(rpg::CON))
        .unwrap_or(0)
}

fn max_hp(world: &World, e: Entity, cfg: &CombatConfig) -> i32 {
    (cfg.base_hp + con_mod(world, e) * cfg.hp_per_con).max(1)
}

/// The avatar's full combat HP — exposed so `spawn_player` can seed its [`Health`] up front.
pub fn avatar_max_hp(world: &World, e: Entity, cfg: &CombatConfig) -> i32 {
    max_hp(world, e, cfg)
}

/// Current HP: a persisted [`Health`] (clamped to the derived max) if the body has fought before,
/// else full.
fn current_hp(world: &World, e: Entity, max: i32) -> i32 {
    world
        .get::<Health>(e)
        .map(|h| h.hp.clamp(0, max))
        .unwrap_or(max)
}

fn is_npc(world: &World, e: Entity) -> bool {
    world.get::<people::Npc>(e).is_some()
}

fn is_beast(world: &World, e: Entity) -> bool {
    world.get::<fauna::Carnivore>(e).is_some() || world.get::<fauna::Herbivore>(e).is_some()
}

fn name_of(world: &World, e: Entity) -> String {
    if is_npc(world, e) {
        crate::dialogue::display_name(world, e)
    } else if is_beast(world, e) {
        if world.get::<fauna::Carnivore>(e).is_some() {
            "Predator".to_string()
        } else {
            "Beast".to_string()
        }
    } else {
        "You".to_string()
    }
}

// ── Hostility / adjacency detection ──────────────────────────────────────────────────────────

/// Bodies (NPCs and fauna) on a tile adjacent to the avatar, excluding the avatar and its party.
/// These are the legal targets of the player-initiated *Attack* verb. `width` is the world's
/// cylinder width (for wrapped distance), supplied by the caller.
pub(crate) fn adjacent_bodies(
    world: &mut World,
    avatar: Entity,
    roster: &[Entity],
    width: i32,
) -> Vec<Entity> {
    let Some(at) = world.get::<Position>(avatar).map(|p| p.0) else {
        return Vec::new();
    };
    let candidates: Vec<Entity> = {
        let mut q = world.query::<(Entity, &Position)>();
        q.iter(world)
            .filter(|(e, p)| {
                *e != avatar && !roster.contains(e) && crate::wrapped_dist(at, p.0, width) <= 1
            })
            .map(|(e, _)| e)
            .collect()
    };
    let mut found: Vec<Entity> = candidates
        .into_iter()
        .filter(|&e| is_npc(world, e) || is_beast(world, e))
        .collect();
    found.sort_by_key(|e| e.to_bits());
    found
}

/// Whether `e` would set upon the avatar unprovoked: a predator, or an NPC bearing a grievance
/// against the avatar.
fn is_hostile(world: &World, e: Entity, avatar: Entity) -> bool {
    if world.get::<fauna::Carnivore>(e).is_some() {
        return true;
    }
    world
        .get::<people::Grievance>(e)
        .is_some_and(|g| g.0 == avatar)
}

// ── Encounter construction ───────────────────────────────────────────────────────────────────

/// Build a fight: the avatar and `roster` (Player faction) against `enemies` (Enemy faction).
/// Seeds the engine from the combat stream so the encounter is reproducible.
pub(crate) fn build_encounter(
    world: &mut World,
    cfg: &CombatConfig,
    content: &CombatContent,
    seed: u64,
    avatar: Entity,
    roster: &[Entity],
    enemies: &[Entity],
) -> Encounter {
    let mut sim = cc::Sim::new(cfg.engine, content.library(cfg.zone_gating), seed);
    let mut combatants = Vec::new();
    let mut next = 0u32;

    let mut enroll = |sim: &mut cc::Sim, e: Entity, faction: u32, tempo: i32, is_avatar: bool| {
        let max = max_hp(world, e, cfg);
        let hp = current_hp(world, e, max);
        let id = cc::ActorId(next);
        next += 1;
        let kit = content.kit_of(archetype_of(world, e, faction == 0));
        sim.add_actor(
            cc::Actor {
                id,
                faction: cc::FactionId(faction),
                vitals: cc::Vitals { hp, max_hp: max },
                tempo,
                next_ready_tick: cc::Tick(0),
                state: cc::ActorState::Idle,
                foresight_horizon: 0,
                zone: CENTER_ZONE,
            },
            kit,
        );
        combatants.push(Combatant {
            actor: id,
            entity: e,
            name: name_of(world, e),
            is_player_side: faction == 0,
            is_avatar,
        });
    };

    enroll(&mut sim, avatar, 0, cfg.party_tempo, true);
    for &m in roster {
        enroll(&mut sim, m, 0, cfg.party_tempo, false);
    }
    for &en in enemies {
        // Named NPCs fight as elites (they hold Tempo); beasts and the nameless are mooks.
        let tempo = if is_npc(world, en) {
            cfg.elite_tempo
        } else {
            0
        };
        enroll(&mut sim, en, 1, tempo, false);
    }

    Encounter { sim, combatants }
}

/// Apply a finished fight to the world: persist survivors' HP, despawn fallen enemies, and report
/// whether the avatar fell.
pub(crate) fn apply_outcome(world: &mut World, enc: &Encounter) -> Resolution {
    let outcome = enc.sim.outcome().unwrap_or(cc::Outcome::Stalemate);
    let mut downed = Vec::new();
    let mut avatar_down = false;

    for c in &enc.combatants {
        let Some(actor) = enc.sim.actor(c.actor) else {
            continue;
        };
        let fell = matches!(actor.state, cc::ActorState::Down) || actor.vitals.hp <= 0;
        if fell {
            if c.is_avatar {
                avatar_down = true;
            } else {
                downed.push(c.entity);
            }
            continue;
        }
        // Survivor: persist HP back onto the body (present == still has a Position).
        let (hp, max) = (actor.vitals.hp.max(0), actor.vitals.max_hp);
        if let Some(mut h) = world.get_mut::<Health>(c.entity) {
            h.hp = hp;
            h.max = max;
        } else if world.get::<Position>(c.entity).is_some() {
            world.entity_mut(c.entity).insert(Health { hp, max });
        }
    }

    // Fallen enemies leave the world. (The avatar is left standing for the caller to handle.)
    for &e in &downed {
        if world.get::<Position>(e).is_some() {
            world.despawn(e);
        }
    }

    let victory =
        matches!(outcome, cc::Outcome::Victory { faction } if faction == cc::FactionId::PLAYER);
    Resolution {
        outcome,
        downed,
        avatar_down,
        victory,
    }
}

/// Determine ambushers among the avatar's adjacent bodies — the hostiles that would spring a fight
/// when the avatar steps next to them.
pub(crate) fn ambushers(
    world: &mut World,
    avatar: Entity,
    roster: &[Entity],
    width: i32,
) -> Vec<Entity> {
    adjacent_bodies(world, avatar, roster, width)
        .into_iter()
        .filter(|&e| is_hostile(world, e, avatar))
        .collect()
}

/// Bodies adjacent to the avatar, paired with their tile — for the UI's "attack which?" prompt.
pub(crate) fn adjacent_with_pos(
    world: &mut World,
    avatar: Entity,
    roster: &[Entity],
    width: i32,
) -> Vec<(Entity, Coord)> {
    adjacent_bodies(world, avatar, roster, width)
        .into_iter()
        .filter_map(|e| world.get::<Position>(e).map(|p| (e, p.0)))
        .collect()
}
