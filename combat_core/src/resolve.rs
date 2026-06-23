//! `resolve_tick` — activation, contact, and effect application for one tick, in the exact order
//! the spec lays out (§7). Every place two things could happen "at once" is broken by the strict
//! total order in `PORTING.md` §4: priority class (desc), faction order, actor id, instance id.

use crate::actor::ActorState;
use crate::config::TempoModel;
use crate::events::{Event, FizzleReason};
use crate::ids::{ActorId, FactionId, InstanceId};
use crate::moves::Effect;
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

/// Resolve one instance's contact (spec §7, with the continuous spatial model): apply any movement
/// first (slide along the line to the target), then — if the move has landing effects — gate them
/// on the target carrying the required tag and being **within reach** (else it whiffs), do the
/// interrupt payoff, and apply the landing effects in order.
fn resolve_contact(sim: &mut Sim, id: InstanceId, tick: Tick) {
    let inst = *sim.timeline().get(id).expect("live instance");
    let attacker = inst.actor;
    let target = inst.target;

    // Snapshot the move's static data, releasing the library borrow before any mutation.
    let (effects, requires, reach, accuracy) = {
        let Some(def) = sim.library().get(inst.mv) else {
            return;
        };
        (
            def.effects.clone(),
            def.requires_tag,
            def.reach,
            def.accuracy,
        )
    };
    let has_landing = effects.iter().any(|e| e.lands_on_target());
    let has_movement = effects.iter().any(|e| !e.lands_on_target());
    let has_damage = effects.iter().any(|e| matches!(e, Effect::Damage { .. }));

    // Every effect (landing or movement) needs a valid target — to land on, or to aim the line at.
    let valid_target = target.is_some_and(|t| sim.actor(t).is_some_and(|a| a.targetable()));
    if (has_landing || has_movement) && !valid_target {
        sim.emit(Event::ActionFizzled {
            instance: id,
            reason: FizzleReason::NoValidTarget,
        });
        return;
    }

    // 1. Movement first, so a move can close the gap and *then* strike.
    for e in &effects {
        match *e {
            Effect::Approach { distance } => {
                slide(sim, attacker, target, distance, true, tick);
            }
            Effect::Withdraw { distance } => {
                slide(sim, attacker, target, distance, false, tick);
            }
            _ => {}
        }
    }

    if !has_landing {
        return;
    }
    let t = target.expect("validated above");

    // 2a. Required-tag gate.
    if let Some(req) = requires
        && sim.windows().active(t, req, tick).is_none()
    {
        sim.emit(Event::ActionFizzled {
            instance: id,
            reason: FizzleReason::MissingTag,
        });
        return;
    }

    // 2b. Reach gate — the whiff. Measured *after* movement, so a lunge that closed the gap lands.
    let in_reach = {
        let ap = sim.actor(attacker).map(|a| a.pos);
        let tp = sim.actor(t).map(|a| a.pos);
        matches!((ap, tp), (Some(ap), Some(tp)) if ap.within(tp, reach))
    };
    if !in_reach {
        sim.emit(Event::ActionFizzled {
            instance: id,
            reason: FizzleReason::OutOfReach,
        });
        return;
    }

    // 2c. To-hit gate (the WWN check) — only for damaging moves, only when enabled. A failed check
    // is a clean miss (the target evaded); a wide margin is a *strong* hit that crits.
    let mut crit_mult = crate::tick::Fixed::ONE;
    if sim.config().wwn_checks && has_damage {
        let cfg = *sim.config();
        let evasion = sim.actor(t).map(|a| a.evasion).unwrap_or(0);
        let margin = accuracy + cfg.to_hit_base - evasion;
        if margin < 0 {
            sim.emit(Event::ActionFizzled {
                instance: id,
                reason: FizzleReason::Missed,
            });
            return;
        }
        if margin >= cfg.strong_margin {
            crit_mult = cfg.strong_mult;
        }
    }

    // Interrupt payoff: a contact landed on a target mid-startup (unarmored) cancels its wind-up
    // *and stuns* the target.
    if let Some(victim) = sim.timeline().live_of(t).map(|i| i.id) {
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
            let stun = sim.config().interrupt_stagger;
            if stun > 0 {
                let until = tick + stun as u64;
                if let Some(a) = sim.actors.get_mut(&t) {
                    a.state = ActorState::Staggered { until };
                }
                sim.emit(Event::ActorStaggered { actor: t, until });
            }
        }
    }

    // 3. Landing effects in listed order (damage scaled by any crit).
    for e in &effects {
        if e.lands_on_target() {
            apply_effect(sim, id, attacker, t, *e, crit_mult, tick);
        }
    }

    // Downed check after all effects.
    if sim.actor(t).is_some_and(|a| a.alive() && a.vitals.hp <= 0) {
        sim.down_actor(t, tick);
    }
}

/// Slide `actor` along the 1D line to/from `target` by `distance`, emitting `Moved`.
fn slide(
    sim: &mut Sim,
    actor: ActorId,
    target: Option<ActorId>,
    distance: crate::tick::Fixed,
    toward: bool,
    tick: Tick,
) {
    let Some(t) = target else { return };
    let (Some(from), Some(to_anchor)) =
        (sim.actor(actor).map(|a| a.pos), sim.actor(t).map(|a| a.pos))
    else {
        return;
    };
    let dest = if toward {
        from.step_toward(to_anchor, distance)
    } else {
        from.step_away(to_anchor, distance)
    };
    if let Some(a) = sim.actors.get_mut(&actor) {
        a.pos = dest;
    }
    sim.emit(Event::Moved {
        actor,
        to: dest,
        tick,
    });
}

fn apply_effect(
    sim: &mut Sim,
    instance: InstanceId,
    attacker: ActorId,
    t: ActorId,
    effect: Effect,
    crit_mult: crate::tick::Fixed,
    tick: Tick,
) {
    match effect {
        Effect::Damage { amount } => {
            let exposed = sim.windows().active(t, WindowTag::Exposed, tick).is_some();
            let mut dmg = amount;
            if exposed {
                let mult = sim.config().exposed_damage_mult;
                let bounty = sim.config().tempo_on_window_hit;
                award_faction_tempo(sim, attacker, bounty);
                dmg = mult.scale_int(dmg);
            }
            dmg = crit_mult.scale_int(dmg);
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
        // Movement effects are handled in `resolve_contact` (before the reach gate), not here.
        Effect::Approach { .. } | Effect::Withdraw { .. } => {}
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
