//! `resolve_tick` — activation, contact, and effect application for one tick, in the exact order
//! the spec lays out (§7). Every place two things could happen "at once" is broken by the strict
//! total order in `PORTING.md` §4: priority class (desc), faction order, actor id, instance id.

use crate::actor::{ActorState, ZoneId};
use crate::config::TempoModel;
use crate::events::{Event, FizzleReason};
use crate::ids::{ActorId, FactionId, InstanceId};
use crate::moves::{Effect, ZoneReq};
use crate::sim::Sim;
use crate::tick::Tick;
use crate::timeline::{InstanceStatus, Phase};
use crate::windows::WindowTag;

/// Resolve everything due at `tick`, emitting events in resolution order.
pub(crate) fn resolve_tick(sim: &mut Sim, tick: Tick) {
    expire_windows(sim, tick);
    enter_startup(sim, tick);
    resolve_contacts(sim, tick);
    complete_actions(sim, tick);
    wake_staggered(sim, tick);
}

/// §7.1 — windows whose `end == tick` close.
fn expire_windows(sim: &mut Sim, tick: Tick) {
    for w in sim.windows.expire_at(tick) {
        sim.emit(Event::WindowClosed { window: w.id });
    }
}

/// §7.2 — phase-entry `ActionStarted` for any instance that begins startup exactly now and hasn't
/// already announced it. (The common path announces startup at commit time; this catches the rare
/// instance re-anchored into the future.)
fn enter_startup(sim: &mut Sim, tick: Tick) {
    let starts: Vec<InstanceId> = sim
        .timeline()
        .iter()
        .filter(|i| i.live() && !i.started && i.start_tick == tick)
        .map(|i| i.id)
        .collect();
    for id in starts {
        if let Some(inst) = sim.timeline.get_mut(id) {
            inst.started = true;
        }
        sim.emit(Event::ActionStarted { instance: id, tick });
    }
}

/// §7.3 — every instance entering its active frame this tick, resolved in strict order.
fn resolve_contacts(sim: &mut Sim, tick: Tick) {
    let mut cands: Vec<InstanceId> = sim
        .timeline()
        .iter()
        .filter(|i| i.live() && !i.active_emitted && i.active_start() == tick)
        .map(|i| i.id)
        .collect();
    cands.sort_by_key(|&id| contact_key(sim, id));

    for id in cands {
        // A higher-priority contact this tick may have cancelled the owner's action already.
        let still_live = sim.timeline().get(id).is_some_and(|i| i.live());
        if !still_live {
            continue;
        }
        if let Some(inst) = sim.timeline.get_mut(id) {
            inst.active_emitted = true;
            inst.status = InstanceStatus::Resolving;
        }
        sim.emit(Event::ActionActive { instance: id, tick });
        resolve_contact(sim, id, tick);
    }
}

/// The strict ordering key (spec §4): higher priority first, then Player faction before Enemy,
/// then ascending actor id, then ascending instance id.
fn contact_key(sim: &Sim, id: InstanceId) -> (core::cmp::Reverse<u8>, u32, u32, u32) {
    let inst = sim.timeline().get(id).expect("candidate exists");
    let pr = sim
        .library()
        .get(inst.mv)
        .map(|d| d.priority_class)
        .unwrap_or(0);
    let fac = sim.actor(inst.actor).map(|a| a.faction.0).unwrap_or(0);
    (core::cmp::Reverse(pr), fac, inst.actor.0, id.0)
}

#[inline]
fn zone_ok(range: ZoneReq, attacker: ZoneId, target: ZoneId) -> bool {
    match range {
        ZoneReq::SameZone => attacker == target,
        ZoneReq::AnyZone => true,
    }
}

/// Award `amount` Tempo to the side that just earned it (spec §9). Per-actor in the opposed model;
/// player-only in the shared-pool model.
fn award_faction_tempo(sim: &mut Sim, actor: ActorId, amount: i32) {
    if amount == 0 {
        return;
    }
    let is_player = sim.actor(actor).map(|a| a.faction) == Some(FactionId::PLAYER);
    match sim.config().tempo_model {
        TempoModel::PerActorOpposed => sim.change_tempo(actor, amount),
        TempoModel::SharedPlayerPool if is_player => sim.change_tempo(actor, amount),
        TempoModel::SharedPlayerPool => {}
    }
}

/// Resolve one instance's contact: validity, the interrupt payoff, then its effects in order.
fn resolve_contact(sim: &mut Sim, id: InstanceId, tick: Tick) {
    let inst = *sim.timeline().get(id).expect("live instance");
    let attacker = inst.actor;
    let target = inst.target;

    // Snapshot the move's static data, releasing the library borrow before any mutation.
    let (effects, requires, range) = {
        let Some(def) = sim.library().get(inst.mv) else {
            return;
        };
        (def.effects.clone(), def.requires_tag, def.range)
    };
    let needs_target = effects
        .iter()
        .any(|e| !matches!(e, Effect::Reposition { .. }));

    let attacker_zone = sim.actor(attacker).map(|a| a.zone).unwrap_or(0);
    let valid_target = match target {
        Some(t) => sim
            .actor(t)
            .is_some_and(|a| a.targetable() && zone_ok(range, attacker_zone, a.zone)),
        None => false,
    };

    if needs_target && !valid_target {
        sim.emit(Event::ActionFizzled {
            instance: id,
            reason: FizzleReason::NoValidTarget,
        });
        return;
    }

    if let Some(req) = requires {
        let carries = target.is_some_and(|t| sim.windows().active(t, req, tick).is_some());
        if !carries {
            sim.emit(Event::ActionFizzled {
                instance: id,
                reason: FizzleReason::MissingTag,
            });
            return;
        }
    }

    // Interrupt payoff: a contact landed on a target mid-startup (unarmored) cancels its wind-up.
    if let Some(t) = target.filter(|_| valid_target)
        && let Some(victim) = sim.timeline().live_of(t).map(|i| i.id)
    {
        let in_startup = sim
            .timeline()
            .get(victim)
            .is_some_and(|i| i.phase(tick) == Phase::Startup);
        let armored = sim
            .timeline()
            .get(victim)
            .and_then(|i| sim.library().get(i.mv))
            .map(|d| d.has_armor)
            .unwrap_or(true);
        if in_startup && !armored {
            sim.cancel_instance(victim, attacker);
            let bounty = sim.config().tempo_on_interrupt;
            award_faction_tempo(sim, attacker, bounty);
        }
    }

    // Effects in listed order.
    for e in &effects {
        apply_effect(sim, id, attacker, target, *e, tick);
    }

    // Downed check after all effects.
    if let Some(t) = target
        && sim.actor(t).is_some_and(|a| a.alive() && a.vitals.hp <= 0)
    {
        sim.down_actor(t, tick);
    }
}

fn apply_effect(
    sim: &mut Sim,
    instance: InstanceId,
    attacker: ActorId,
    target: Option<ActorId>,
    effect: Effect,
    tick: Tick,
) {
    match effect {
        Effect::Damage { amount } => {
            let Some(t) = target else { return };
            let exposed = sim.windows().active(t, WindowTag::Exposed, tick).is_some();
            let dmg = if exposed {
                let mult = sim.config().exposed_damage_mult;
                let bounty = sim.config().tempo_on_window_hit;
                award_faction_tempo(sim, attacker, bounty);
                mult.scale_int(amount)
            } else {
                amount
            };
            if let Some(a) = sim.actors.get_mut(&t) {
                a.vitals.hp -= dmg;
            }
            sim.emit(Event::Hit {
                instance,
                attacker,
                target: t,
                amount: dmg,
            });
        }
        Effect::LineKnockback { ticks } => {
            let Some(t) = target else { return };
            if let Some(victim) = sim.timeline().live_of(t).map(|i| i.id) {
                let new_start = sim
                    .timeline()
                    .get(victim)
                    .map(|i| i.start_tick + ticks as u64)
                    .unwrap_or(tick);
                if let Some(i) = sim.timeline.get_mut(victim) {
                    i.start_tick = new_start;
                }
                sim.emit(Event::LineShoved {
                    target: t,
                    instance: Some(victim),
                    ticks,
                });
            } else {
                if let Some(a) = sim.actors.get_mut(&t) {
                    a.next_ready_tick = a.next_ready_tick + ticks as u64;
                }
                sim.emit(Event::LineShoved {
                    target: t,
                    instance: None,
                    ticks,
                });
            }
        }
        Effect::OpenWindow {
            tag,
            duration,
            magnitude,
        } => {
            let Some(t) = target else { return };
            let end = tick + duration as u64;
            let wid = sim.windows.open(t, tag, tick, end, magnitude);
            sim.emit(Event::WindowOpened {
                window: wid,
                actor: t,
                tag,
                end,
            });
        }
        Effect::Stagger { ticks } => {
            let Some(t) = target else { return };
            // A staggered actor cannot continue an in-flight action.
            if let Some(victim) = sim.timeline().live_of(t).map(|i| i.id)
                && let Some(i) = sim.timeline.get_mut(victim)
            {
                i.status = InstanceStatus::Cancelled;
            }
            let until = tick + ticks as u64;
            if let Some(a) = sim.actors.get_mut(&t) {
                a.state = ActorState::Staggered { until };
            }
            sim.emit(Event::ActorStaggered { actor: t, until });
        }
        Effect::Reposition { zone } => {
            if let Some(a) = sim.actors.get_mut(&attacker) {
                a.zone = zone;
            }
        }
    }
}

/// §7.4 — instances whose recovery ends now complete cleanly and free their owner.
fn complete_actions(sim: &mut Sim, tick: Tick) {
    let dones: Vec<InstanceId> = sim
        .timeline()
        .iter()
        .filter(|i| i.status == InstanceStatus::Resolving && i.recovery_end() == tick)
        .map(|i| i.id)
        .collect();
    for id in dones {
        let owner = sim.timeline().get(id).map(|i| i.actor);
        if let Some(i) = sim.timeline.get_mut(id) {
            i.status = InstanceStatus::Resolved;
        }
        if let Some(owner) = owner
            && let Some(a) = sim.actors.get_mut(&owner)
            && a.state == ActorState::Committed(id)
        {
            a.state = ActorState::Idle;
            a.next_ready_tick = tick;
        }
        sim.emit(Event::ActionCompleted { instance: id, tick });
    }
}

/// §7.5 — staggered actors whose timer ends now return to idle.
fn wake_staggered(sim: &mut Sim, tick: Tick) {
    let wake: Vec<ActorId> = sim
        .actors()
        .filter(|a| matches!(a.state, ActorState::Staggered { until } if until == tick))
        .map(|a| a.id)
        .collect();
    for aid in wake {
        if let Some(a) = sim.actors.get_mut(&aid) {
            a.state = ActorState::Idle;
            a.next_ready_tick = tick;
        }
    }
}
