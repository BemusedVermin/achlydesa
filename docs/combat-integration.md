# Combat — integration design

How the headless `combat_core` engine (built to `docs/combat-core-spec.md` / `combat_core/SPEC.md`)
plugs into the living-world sim and the Bevy front-end. The engine itself is pure and
self-contained; this doc is about the **seam** — encounter → fight → consequences — and the UI.

## Design decisions (locked)

| Question | Choice |
|---|---|
| Combat stakes | **Persistent & lethal.** Downed enemies die in the sim; the avatar and party take real HP/vitals damage that carries back to the world; the avatar can die. One fight advances the world like any other player action. |
| Encounter trigger | **Both.** A player-initiated *Attack* verb on an adjacent hostile, *and* an ambush when the avatar steps adjacent to a hostile (predator / an NPC with a `Grievance` against the avatar / a hostile faction). |
| Player control | **The whole party.** The player issues moves and edits for the avatar *and* each recruited companion. Only enemies are AI (`StubAi`). |
| Combat view | **Tactical timeline.** The fight reads-and-edits a "next few seconds" timeline ribbon; the Tempo verbs (Slow/Haste/Interrupt/Insert) are the core interaction. |

**UI constraint (from the mockup review):** the timeline ribbon is a *slim band* (docked along the
bottom of the Main Game Area), **not** the whole area. The rest of the Main Game Area is reserved
for the later 3D fight rendering.

## Three layers (respecting the crate boundaries)

```
combat_core   pure engine + combat_cli + golden vectors        (no bevy, no float, no I/O)
    ▲
agents        combat bridge — gated by `setup.combat`           (bevy_ecs only; off = byte-identical)
    ▲           · extract a Scenario from world entities (avatar + party vs adjacent hostiles)
    │           · detect ambushes; expose an "attack target" API
    │           · apply outcomes back (kill downed entities, persist HP/vitals)
    │           · dedicated xor-seeded RNG; re-exported through `agents`
    ▲
app           combat mode — a new state in the `Game` modal stack, reusing the HUD chrome
                · owns the live `combat_core::Sim` for the duration of a fight
                · right tray → Moves, bottom tray → combat stats, minimap → positioning map
                · Main Game Area → timeline ribbon band (rest reserved for future 3D)
                · collects the player's commands for the whole party; runs StubAi for enemies
                · on end, calls the bridge to write outcomes to the world
```

Why `combat_core` stays out of the world sim: combat is **interactive and player-paced**, so it is
not a per-tick schedule system. The app owns the live `Sim` (like a modal), the same way it already
owns conversation state. The world sim is perturbed by the *outcome* of a fight — a player-driven
mutation — exactly as it is by travel/talk/recruit, preserving the seed→outcome story.

## Actor sourcing (v1)

The combat layer does not yet have the WWN→frame-data bridge (a `combat_core` non-goal). For v1:

- **HP / vitals** come from the world: `rpg`/`survival` `Vitals` where present, else a faction
  default from the combat config. Persisted back on combat end.
- **Move kits** are authored content. Each combatant is assigned a small kit by a simple archetype
  tag; the kit catalogue is RON under `assets/` (parsed via `config`, like the rest of the data-
  driven content) and compiled into `MoveDef`s through the `MoveDef::builder` seam.
- **Tempo** for the player party and elite enemies comes from config; mooks spawn with 0.

## Determinism

Each encounter seeds its `combat_core::Sim` from `world_seed ^ COMBAT_CONST ^ encounter_id`, so a
given encounter is reproducible. The world sim's own determinism is unaffected: with `setup.combat`
off nothing changes (byte-identical), and with it on the only world mutations are the player-driven
outcomes written back through a deterministic `agents` API.

## Status

- [x] **Phase 1 — `combat_core` + `combat_cli`.** Engine, 5 golden scenarios, determinism +
  property tests, `PORTING.md`. Complete and green.
- [x] **Phase 2 — `agents` bridge.** `setup.combat`, scenario extraction, ambush detection, attack
  API, outcome write-back, RNG stream, re-export. Bridge smoke + off tests green.
- [x] **Phase 3 — `app` combat mode.** A combat overlay reusing the HUD chrome: the timeline-ribbon
  band, combatant roster with HP/Tempo, the move tray (which becomes the Slow/Haste/Interrupt/Insert
  edit verbs on a dilation turn), the field reserved for future 3D, and the combat log. Transition
  via the **G** Attack verb and predator/grudge ambush after a step; StubAi enemies; write-back and
  return on the result banner. Verified by headless screenshot (`ACHLYDESA_FIGHT` + `ACHLYDESA_SHOT`).

### The combat scene (rebuilt) — continuous field, its own screen

After review the combat presentation was rebuilt to the intended design:

- **Continuous 2D spatial model** (`combat_core::space`): actors have a `Pos`; a move targets a
  *person* and travels the 1D line to them, landing only **within reach** (else it whiffs).
  `Approach`/`Withdraw` close/open the gap along that line; movement resolves before the reach gate
  (a Lunge closes *and* strikes). This replaces the abstract zones — the spec's deferred "full
  spatial model". Determinism preserved (Fixed positions, integer sqrt). New golden:
  `whiff_and_lunge`.
- **A separate, opaque battle scene** (`app/combat.rs` + `app/combat_field.rs`): nothing of
  exploration shows through; the field is software-rasterised — tokens at continuous positions, a
  reach ring, the attack line drawn **cyan = in reach / red = whiff**, HP arcs, plus a legend.
- **Real-time burst playback that pauses at every decision.** The engine resolves to the next
  decision; the app plays the burst over real time (tokens glide on `Moved`, blows flash, the
  timeline playhead sweeps) then pauses — the player's decisions wait for input; a foe's chosen
  move is shown and auto-advances after a readable beat (skippable).
- **Move previews**: hovering/selecting a move shows its damage, effects, timing, reach, and
  whether it would land or whiff against the current target — before you commit. Edit verbs preview
  their effect too.

- **Dilating elite AI** (`combat_core::EliteAi`): enemies spend Tempo to interrupt/slow the
  player's line — the per-actor opposed economy is two-sided. The app drives enemies with it.

### Fighting-game ATB feel (reworked)

The flow was reworked to a thinking-player's fighting game:

- **ATB:** time flows in real time while moves play out (you're committed, frame-by-frame); it
  pauses **only on your own turns** — a readiness choice or a bullet-time *dilation* (spend Tempo to
  Slow/Interrupt a foe's committed move). Foe decisions resolve inline; no banners, no per-tick stop.
- **Always-on tray + cost preview:** the move tray is always shown (greyed off-turn, never blinks);
  selecting a move ghosts its startup/active/recovery onto the timeline (its cost) and the preview
  forecasts the WWN to-hit against the target.
- **Interrupts stun** (`combat_core::Config::interrupt_stagger`): a parry staggers the wind-up.
- **RPG-driven moves:** the bridge compiles each fighter's moves *per fighter*, folding the governing
  attribute modifier + skill rank into a WWN to-hit `accuracy` (`Config::wwn_checks`: accuracy +
  to_hit_base − target.evasion; a wide margin crits), scaled damage (`dmg_per_mod`) and reach
  (`reach_per_mod`). STR powers Shove/Strike, DEX powers Lunge/Loose and evasion. The check is
  deterministic (no dice), so a matchup reliably hits or misses by stats.
- **RON-authored content.** The move catalogue + per-archetype kits live in `assets/data/combat.ron`
  (sourced through `config`, parsed by the bridge into `MoveDef`s via the builder). Kits are picked
  by archetype: adventurer (avatar/party), soldier (NPCs), predator (carnivores), prey (herbivores).
- **Out-of-combat HP regen.** A `regen_health` schedule system (added only when the layer is on)
  mends `Health` one point per `CombatConfig::regen_period` overworld ticks — and combat freezes the
  world, so it only heals between fights.
- **3D fight figures.** Combatants render as capsule figures in two facing rows centred on the
  avatar (player blue, enemy red, the fallen toppled grey, a lift while mid-action) — the fight in
  the reserved field, beneath the timeline ribbon. The overworld avatar is hidden while fighting.

### Deferred (follow-ups)
- A real **game-over** when the avatar falls (today it shows a banner and returns to the overworld;
  the avatar is left at 0 HP rather than removed). *(Left to the user's call.)*
- The WWN→frame-data bridge (move stats compiled from the RPG layer rather than a fixed catalogue),
  animated figures, and a smarter AI that also uses the Feint→Riposte combo and repositions.
```
