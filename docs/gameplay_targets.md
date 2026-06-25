# Gameplay targets — what the simulation is *for*

> A living index of the experiences this project is trying to deliver, and where each one stands.
> The cosmology (the Demiurge, the Archons) is the *skin*; this file is about the *game*. Read it
> next to `perception_layer.md` (the layer being built now) and `narrative_surfacing.md`.

## The north star

Two games are the gameplay inspiration — explicitly **not** the Gnostic themes, which are a separate
authorial choice layered on top:

- **S.T.A.L.K.E.R.** — the **A-Life** simulation: factions that war and patrol, roving bandits,
  firefights you crest a hill *into* rather than being scripted to. A world indifferent to you that
  lives whether you watch or not.
- **Darklands** — the **grounded low-fantasy** RPG: a self-authored, skill-based party roving town to
  town, reputation across a believable society, deadly mundane stakes, **no chosen-one**.

Their shared DNA — and our north star — is **an indifferent, emergent, living world in which the
player is one small actor, not the protagonist of a scripted epic.** The substrate for this already
exists at world scale (factions, integer economy, feuds, migration, the three-tier agent spectrum);
the MDA analysis (2026-06) found the gap is not the *mechanics* but their **legibility** and the
**assembly of the avatar into the world** — plus one deliberate addition below.

## The added target: an *emotional* story — the world is the protagonist (NEW, 2026-06)

The third target, added alongside the two inspirations: **the main character is the world itself.**
The arc the player lives is their relationship to it — and the keystone choice (the Gnostic endgame)
is whether to **reject / free** the world. That choice must *cost* something. It lands emotionally
only if, by the time it is offered, the player has **bonded with souls who live there** — so freeing
the world is also leaving (or unmaking) the people who came to care for them.

This is a deliberate departure from STALKER/Darklands pure indifference: those worlds never ask you
to grieve them. Ours does. The two are reconciled by *where* the authorship sits — the world stays
indifferent and emergent moment to moment (STALKER), but the **whole** of it becomes something the
player can lose (the emotional story). Legibility is the bridge: you cannot grieve a world you cannot
read.

What already exists to build this on (verified in code, 2026-06):

- **`Bond(pub Entity)`** component (`agent_core/src/people.rs`) + **`Effect::Bond { who, to }`**
  (`director.rs`) — the attachment primitive. A soul can be bonded to the avatar.
- **`Effect::Free { who }`** and the **impact-floor silence** / "Gödel point" (`director.rs`) — a
  world made forgiving and free *starves the director* and it falls silent. The freed world is a
  real, reachable state, not a cutscene.
- **`Opinion`** of the avatar (read by `player_recruit`) and the **party** layer — souls already
  hold a directed disposition toward the avatar and can travel with it.

What is missing (the work this target names):

1. A **"reject / free the world"** player choice — a reachable keystone act, unsignposted (per
   `narrative_director.md`'s liberation path), that flips the world toward the freed state.
2. **NPC emotional reaction** to that act, concentrated on **bonded** souls — grief / longing /
   a plea — surfaced through the Perception Layer (a final readable sequence), not a popup.
3. **Bond growth through play** (proximity, shared danger, conversation, recruitment) so the bond is
   *earned*, and a **salience term** that makes bonded souls rise in every surface — so the world the
   player reads is visibly *their* world before they are asked to give it up.

The Perception Layer is the vehicle for #2 and #3; #1 is a small beat/verb on top of the existing
`Effect::Free` + impact-floor machinery. Bonds get a salience term in the Perception spine now (so
the rest follows cheaply later).

## Active target: the Perception Layer

**Status: implementing (2026-06).** See `perception_layer.md`. It makes the drama the sim already
stages *legible* — a prose log, a drama-map, a read-the-room scan, a combat timeline — all one
salience-ranked set of `Tell`s under different filters. It is first because it makes *all existing
content* felt at once, and because the emotional-story target is impossible without it. Its
`provenance` thread (Sim-grown vs. Director-authored) is also the Gnostic recognition mechanic.

## Noted targets (the MDA levers, not yet scheduled)

From the 2026-06 MDA analysis. Recorded here so they are not lost; ordered by leverage toward the
north star. None is started.

1. **Roving warbands / bandit bands + territory + the encounter.** *The* highest-leverage STALKER
   gap. A mobile group (faction war-party or masterless bandits) built on the **drifter tier** that
   moves, contests space, raids settlements/travelers, and that the avatar can **witness / avoid /
   fight / join**. Gives faction war a *visible body* (today war is bookkeeping: a casualty + champion
   grudges, no army, no front) and creates roving bandits (today absent). Pairs with a **territory**
   layer (faction influence that paints the map and shifts with war) and an **encounter trigger** that
   turns "a fight is happening at tile X" into something you walk up on. Reuses: drifters, the DANGER
   field (already flee-able), the lethal/persistent combat engine, the grievance machinery (motive).

2. **Assemble the avatar as an actor in the economy/society.** Today the avatar can talk, recruit,
   fight, explore — but has **no inventory, no money, no looting/trade, no jobs**. Both inspirations
   *are* a ground-level loop (Darklands: arrive, trade, take work, build reputation, move on). That
   loop is unassembled because the avatar is not yet an economic/social peer of the souls we simulate.

3. **Reputation the world reacts to.** Promote per-NPC `Opinion` into faction/settlement **standing**
   that gates prices, recruitment, hostility (bandits prey on the unknown; guards greet the renowned).
   Cheap on top of the existing opinion graph; large for both inspirations and for the emotional story.

4. **NPC survival + fauna-fear.** NPCs have no water/shelter-seeking goal and full-brain agents have
   no flee/fight-fauna operator (they walk past predators); survival is avatar/party-scoped. Restores
   "everyone here is mortal and faces the world you face" — the honesty STALKER immersion needs.

5. **Director posture as a per-run dial.** The director is structurally the *anti-A-Life* (a hidden
   author that grooms attachment then reverses it). For the STALKER feel, run it **off** (the
   substrate already produces feuds/wars/migration) or re-pointed to "stage faction/territory/scarcity
   pressure and let it play," reserving the groom→reverse thread machinery for the authored-Gnostic
   mode. The emotional-story target *wants* the authored mode; STALKER mode wants it demoted. Make it
   an explicit choice, not an accident.

6. **Character aging / lifecycle (Darklands texture).** Darklands' party grew old; that melancholy is
   load-bearing. Lower priority, but it rhymes with the world-as-protagonist theme (everything here is
   passing).

## Status at a glance

| Target | State |
|---|---|
| Perception Layer | **implementing** |
| Emotional story (world-as-protagonist) | **design recorded**; bond-salience hook landing with the Perception spine |
| Roving warbands + territory + encounter | noted |
| Avatar as economic/social actor | noted |
| Reputation the world reacts to | noted |
| NPC survival + fauna-fear | noted |
| Director posture dial | noted |
| Aging / lifecycle | noted |
