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
use agent_core::{Chronicle, EpisodeKind, Position, Provenance, Substrate};
use bevy_ecs::entity::Entity;
use bevy_ecs::prelude::*;
use combat_core as cc;
use serde::Deserialize;

// ── Move catalogue (RON-authored) ────────────────────────────────────────────────────────────
// The catalogue and per-archetype kits are authored in `assets/data/combat.ron` and compiled into
// `combat_core` MoveDefs here.

/// One authored move, compiled into a [`cc::MoveDef`] through the builder seam. Distances are in
/// whole world units (lifted to fixed-point on compile).
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
    /// How close the target must be for the landing effects to connect (beyond it: a whiff).
    #[serde(default = "default_reach")]
    reach: u32,
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
    /// Slide toward the target by this many units before the reach check (0 = none).
    #[serde(default)]
    approach: u32,
    /// Slide away from the target by this many units (0 = none).
    #[serde(default)]
    withdraw: u32,
    #[serde(default)]
    tempo_cost: i32,
    /// The governing WWN attribute ("STR"/"DEX"/"CON"/"INT"/"WIS"/"CHA"); its modifier feeds the
    /// to-hit accuracy and scales damage/reach. `None` = a flat move (no attribute).
    #[serde(default)]
    attr: Option<String>,
    /// The governing WWN skill (e.g. "Punch", "Stab", "Shoot"); its rank adds to accuracy.
    #[serde(default)]
    skill: Option<String>,
    /// Extra damage per point of the governing attribute's modifier (a heavy STR move scales hard).
    #[serde(default)]
    dmg_per_mod: i32,
    /// Extra reach (units) per point of the governing attribute's modifier (acrobatic/ranged moves).
    #[serde(default)]
    reach_per_mod: i32,
}

fn default_reach() -> u32 {
    2
}

fn attr_index(name: &str) -> Option<usize> {
    Some(match name {
        "STR" => rpg::STR,
        "DEX" => rpg::DEX,
        "CON" => rpg::CON,
        "INT" => rpg::INT,
        "WIS" => rpg::WIS,
        "CHA" => rpg::CHA,
        _ => return None,
    })
}

fn attr_mod(world: &World, e: Entity, attr: usize) -> i32 {
    world
        .get::<rpg::Abilities>(e)
        .map(|a| a.modifier(attr))
        .unwrap_or(0)
}

fn skill_rank(world: &World, e: Entity, skill: &str) -> i32 {
    let Some(data) = world.get_resource::<rpg::RpgData>() else {
        return 0;
    };
    let Some(id) = data.skill_id(skill) else {
        return 0;
    };
    world
        .get::<rpg::Proficiencies>(e)
        .map(|p| p.rank(id) as i32)
        .unwrap_or(0)
}

impl MoveSpec {
    fn compile(&self) -> cc::MoveDef {
        use cc::{Effect, Fixed, MoveDef, MoveId, WindowTag};
        let mut b = MoveDef::builder(MoveId(self.id), self.name.clone())
            .frames(self.startup, self.active, self.recovery)
            .priority(self.priority)
            .tempo_cost(self.tempo_cost)
            .reach(Fixed::from_int(self.reach as i32));
        if self.armored {
            b = b.armored();
        }
        if self.approach > 0 {
            b = b.approach(Fixed::from_int(self.approach as i32));
        }
        if self.withdraw > 0 {
            b = b.withdraw(Fixed::from_int(self.withdraw as i32));
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

    /// The base spec for move `id`.
    fn move_spec(&self, id: u32) -> Option<&MoveSpec> {
        self.moves.iter().find(|m| m.id == id)
    }

    /// The base move-id kit for `archetype` (falling back to the action-tray kit for an unknown one).
    fn kit_of(&self, archetype: &str) -> &[u32] {
        self.archetypes
            .iter()
            .find(|a| a.name == archetype)
            .map(|a| a.kit.as_slice())
            .unwrap_or(self.tray.as_slice())
    }
}

/// Compile a move *for a specific fighter*: lift the base spec, then fold in the governing
/// attribute modifier + skill rank — accuracy (the to-hit), scaled damage, and scaled reach — and
/// give it a per-fighter unique id so every combatant carries its own tuned copy.
fn scaled_move(spec: &MoveSpec, world: &World, e: Entity, unique_id: u32) -> cc::MoveDef {
    let am = spec
        .attr
        .as_deref()
        .and_then(attr_index)
        .map(|i| attr_mod(world, e, i))
        .unwrap_or(0);
    let sk = spec
        .skill
        .as_deref()
        .map(|s| skill_rank(world, e, s))
        .unwrap_or(0);
    let mut m = spec.compile();
    m.id = cc::MoveId(unique_id);
    m.accuracy = am + sk;
    m.reach = cc::Fixed::from_int((spec.reach as i32 + am * spec.reach_per_mod).max(1));
    for effect in m.effects.iter_mut() {
        if let cc::Effect::Damage { amount } = effect {
            *amount = (*amount + am * spec.dmg_per_mod).max(0);
        }
    }
    m
}

/// A fighter's evasion: a neutral attacker (accuracy 0) still connects on a neutral target
/// (margin 1), but a keen Dexterity makes you progressively harder to hit.
fn evasion_of(world: &World, e: Entity) -> i32 {
    6 + attr_mod(world, e, rpg::DEX) * 2
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
    /// Half the starting gap between the sides on the field: the party lines up at `-x`, the
    /// enemies at `+x`, this many world units out.
    pub field_half_width: i32,
    /// Vertical spacing between same-side combatants on the field.
    pub lane_gap: i32,
    /// Ticks of overworld time per 1 HP mended out of combat (0 disables regen). Combat freezes the
    /// world, so this only heals between fights.
    pub regen_period: u64,
}

impl Default for CombatConfig {
    fn default() -> Self {
        Self {
            // The game folds in the RPG: hits roll the WWN to-hit check, and a parry stuns.
            engine: cc::Config {
                wwn_checks: true,
                interrupt_stagger: 4,
                ..cc::Config::default()
            },
            base_hp: 20,
            hp_per_con: 6,
            party_tempo: 8,
            elite_tempo: 6,
            field_half_width: 8,
            lane_gap: 3,
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
    if !(*counter).is_multiple_of(cfg.regen_period) {
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
    /// This fighter's (scaled) move ids — so the UI can show its tray even when it isn't acting.
    pub moves: Vec<cc::MoveId>,
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
    // One slot per combatant, in id order: the party on the left edge, enemies on the right, each
    // side spread vertically and centred — a continuous field the moves close across.
    struct Slot {
        e: Entity,
        faction: u32,
        is_avatar: bool,
        tempo: i32,
        pos: cc::Pos,
    }
    let mut slots: Vec<Slot> = Vec::new();
    let players: Vec<(Entity, bool)> = std::iter::once((avatar, true))
        .chain(roster.iter().map(|&e| (e, false)))
        .collect();
    let np = players.len() as i32;
    for (i, &(e, is_avatar)) in players.iter().enumerate() {
        slots.push(Slot {
            e,
            faction: 0,
            is_avatar,
            tempo: cfg.party_tempo,
            pos: cc::Pos::from_ints(-cfg.field_half_width, lane_y(i as i32, np, cfg.lane_gap)),
        });
    }
    let ne = enemies.len() as i32;
    for (j, &en) in enemies.iter().enumerate() {
        slots.push(Slot {
            e: en,
            faction: 1,
            is_avatar: false,
            // Named NPCs fight as elites (they hold Tempo); beasts and the nameless are mooks.
            tempo: if is_npc(world, en) {
                cfg.elite_tempo
            } else {
                0
            },
            pos: cc::Pos::from_ints(cfg.field_half_width, lane_y(j as i32, ne, cfg.lane_gap)),
        });
    }

    // Build each fighter's kit as per-fighter *scaled* moves (folding in their RPG stats), with
    // unique move ids, and collect them all into one library.
    let mut defs: Vec<cc::MoveDef> = Vec::new();
    let mut kits: Vec<Vec<cc::MoveId>> = Vec::new();
    for (idx, slot) in slots.iter().enumerate() {
        let base_kit = content.kit_of(archetype_of(world, slot.e, slot.faction == 0));
        let mut kit = Vec::new();
        for &base_id in base_kit {
            if let Some(spec) = content.move_spec(base_id) {
                let uid = (idx as u32 + 1) * 100 + base_id;
                defs.push(scaled_move(spec, world, slot.e, uid));
                kit.push(cc::MoveId(uid));
            }
        }
        kits.push(kit);
    }

    let mut sim = cc::Sim::new(cfg.engine, cc::MoveLibrary::from_defs(defs), seed);
    let mut combatants = Vec::new();
    for (idx, slot) in slots.into_iter().enumerate() {
        let max = max_hp(world, slot.e, cfg);
        let hp = current_hp(world, slot.e, max);
        let id = cc::ActorId(idx as u32);
        sim.add_actor(
            cc::Actor {
                id,
                faction: cc::FactionId(slot.faction),
                vitals: cc::Vitals { hp, max_hp: max },
                tempo: slot.tempo,
                next_ready_tick: cc::Tick(0),
                state: cc::ActorState::Idle,
                foresight_horizon: 0,
                pos: slot.pos,
                evasion: evasion_of(world, slot.e),
            },
            kits[idx].clone(),
        );
        combatants.push(Combatant {
            actor: id,
            entity: slot.e,
            name: name_of(world, slot.e),
            is_player_side: slot.faction == 0,
            is_avatar: slot.is_avatar,
            moves: kits[idx].clone(),
        });
    }

    Encounter { sim, combatants }
}

/// The centred vertical offset for combatant `i` of `count` on one side.
fn lane_y(i: i32, count: i32, gap: i32) -> i32 {
    i * gap - (count - 1) * gap / 2
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

    // The fallen leave the world — but first record each to the Chronicle (when the sift/perception
    // layer is on), so a fight's dead feed the legibility surfaces (the prose log, the scan, the
    // drama-map) instead of vanishing silently. Only a slain **enemy** is the avatar's kill (a
    // `Killed` attributed to the avatar — the player's deed); a fallen **companion** (also
    // player-side) is a death the avatar did not deal, so it records as an unattributed `Death`, never
    // Killed-by-the-avatar. Entities and the place are captured *before* the despawn, the same
    // discipline the in-schedule taps use. A no-op (byte-identical) when the Chronicle is absent.
    let avatar = enc
        .combatants
        .iter()
        .find(|c| c.is_avatar)
        .map(|c| c.entity);
    let tick = world.resource::<Substrate>().0.tick();
    for &e in &downed {
        let Some(at) = world.get::<Position>(e).map(|p| p.0) else {
            continue; // already gone
        };
        let is_enemy = enc
            .combatants
            .iter()
            .find(|c| c.entity == e)
            .is_none_or(|c| !c.is_player_side);
        if let Some(mut chron) = world.get_resource_mut::<Chronicle>() {
            match (is_enemy, avatar) {
                (true, Some(slayer)) => chron.record(
                    tick,
                    EpisodeKind::Killed,
                    Provenance::Agent(slayer),
                    [Some(slayer), Some(e), None],
                    at,
                    None,
                    0,
                ),
                // A player-side fall (a companion), or no avatar in the fight (an autonomous clash):
                // a death the avatar did not deal — unattributed.
                _ => chron.record(
                    tick,
                    EpisodeKind::Death,
                    Provenance::Sim,
                    [Some(e), None, None],
                    at,
                    None,
                    0,
                ),
            }
        }
        world.despawn(e);
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
