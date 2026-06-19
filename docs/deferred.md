# Deferred work & scope notes

A running backlog of the **honest limitations** and **deliberately-deferred refinements**
behind the systems built so far — the "v1 depth" caveats, captured here so they're
revisitable rather than lost in chat. Each item is *what's simplified* and *what the
fuller version would take*. Nothing here is a bug; these are conscious scope lines.

Grouped by system. Items marked **(blocks X)** gate something else.

---

## RPG / survival / exploration overhaul (built 2026-06 — see `docs/rpg_survival_exploration.md`)
A large multi-crate build **modularised the agent layer** (`agent_core` + feature crates
`rpg`/`party`/`survival`/`travel`/`explore`, with `agents` the thin assembler) and added, all
gated off-by-default (byte-identical when off): the **WWN RPG layer** (attributes/skills/Foci/Edges/
saves on every NPC + the avatar), the **party** (recruit via a Convince/Lead check, travel as a
stack), **speech + world-interaction skills** (persuasion scales dialogue, Notice raises sight +
passive secret-spotting), the **survival layer** (per-tile thirst/warmth/stamina + tile-hunger), and
the **exploration layer** (cost-paced day-budget travel, a procedural road network, slope +
climbing-gear/boat edge gates, gear). The app world is now **US-scale** and `app` **owns world
generation** (`Simulation::from_world`). This supersedes the now-built items in *Planned game layers*
below (travel costs, roads, RPG & survival, world scale). What's deferred within it:
- **Survival isn't on world-wide.** Autonomous NPCs have no water/shelter-seeking GOAP goals, so a
  survival-on mostly-arid world depopulates to its green pockets (the survivors thrive). Needs an
  **NPC survival-AI** before it can be enabled for the whole population — it's a *player/party* layer
  until then. **(blocks: survival in the app)**
- **POI interaction and carts/paid-passage** need the avatar to be an **economic/needs actor** (it has
  no `Inventory`/money yet); invariant-safe payment needs the avatar's coins counted in `total_money()`.
  Shares the player-`Inventory` prerequisite already flagged for the maps slice.
- **App rendering at US-scale — mesh chunking + incremental markers.** The ground is *one merged
  mesh* that `rebuild_map` despawns and **fully rebuilds over every explored tile** (`build_ground_mesh`)
  on each reveal, re-uploading it to the GPU — O(explored), and the explored set only grows on a
  US-scale map. `rebuild_markers` likewise **despawned and respawned every marker each tick** (the
  *avatar* was split off 2026-06 into a persistent, camera-smoothed `AvatarFig`; the **NPC markers
  still rebuild** wholesale each tick), and three render systems each rebuild an O(explored) `HashSet`
  every tick. At the larger world these grow without bound; `cargo run -p app --release` makes it
  tolerable for now, but the **durable fix** is: **(1) mesh chunking** — tile the world into fixed
  blocks, one mesh per block, and rebuild only the *dirty* block(s) when new tiles are revealed;
  **(2) incremental marker reconciliation** — move/spawn/despawn only what changed (as `sync_fauna`
  already does for creatures) instead of a full rebuild; **(3) cache the explored set** rather than
  rebuilding it per-tick in three systems. If that still isn't enough, **decouple `sim.step()` from
  the render** (step on a worker, render the latest snapshot) — determinism is preserved (still
  single-threaded, just off the render thread). **Coarse/LOD off-view simulation is explicitly *not*
  the fix** — it would break the byte-identical determinism invariant and the authoritative living
  world (off-screen biomes/ecology/NPCs must keep evolving). **(blocks: smooth play once exploration
  is heavy)**
- **App render polish (other).** Roads and rivers aren't drawn yet (the `Roads` set + `surface_water`
  are queryable — `Simulation::road_tiles()` etc.). The character sheet (`C`) now surfaces RPG stats /
  archetype / skills / gear / vitals / party, but **travel cost** still isn't shown on the HUD, and
  recruited companions are listed on the sheet rather than rendered as a stack at the avatar's tile.
  Fog-of-war still bounds what's drawn.
- **Combat is deferred by design** — a **job-system** + **xianxia power tiers** (the `PowerTier` +
  grant-bundle hooks are reserved for it); combat-tagged Foci are authored but inert. **NPC↔NPC speech
  scaling** is also deferred (avatar speech only for now).
- **Economy ↔ WWN unification** (folding the economy `Skills` onto WWN Craft/Work/Trade) is deferred.
- **The 3 brittle narrative V&V tests stay red** pending a one-time rebaseline (carried from the biome
  work, not caused by this overhaul — every phase was verified byte-identical).

---

## Narrative director — v2 multi-thread drama manager built; the player layer deferred
The capstone is a **multi-thread drama manager** (`agents/src/director.rs` + `beats.rs` +
`assets/data/beats.ron` + `assets/data/moods.ron`, **v2 2026-06**, behind `Setup::director`). (The first
cut was an environmental-hazard generator — fire/predator pokes — which wasn't a *narrative*;
v1 followed real drama managers as Façade-style **beats** on a single tension arc; **v2**
replaced that selection core with a **thread-driven drama objective**.) `Γ` runs up to **three
threads at once**, each a **groom → climax → fall** arc, and **manufactures the audience's
attachment on purpose** (a persistent, per-soul *prominence* it grooms so a later reversal
devastates). It maximizes **drama = stakes × attachment × reversal** by the **cheapest novel
route** (`score = drama × novelty ÷ resistance`), **times climaxes onto highs** (and onto
each other — *collisions*), and **rotates registers freely** so betrayal dominates *because it
scores highest* (a trunk bonus), never by a rule. The metric generalized to **staged
experience** (joy counted, suffering heaviest) alongside the suffering-only `gratuitous`. It
**works people, factions, and the world** (grudges/traits/moods/opinions, faction taboos &
wars, famines, the *wonder* register wired to the `Features`/`Known` discovery layer), and
broadened beyond tragedy (romance/triumph/wonder/reunion/sacrifice/redemption + the moods
awe/hope/love). **Liberation has no button:** `Γ` is omnipotent but a *precondition engine*
with an **impact floor**, and the **deniability rule** (never tell a beat the world could not
have produced itself — a friend can't turn with no faction, a loved one can't fall with no
scarcity) means a *freed* world (provisioned/forgiving/stateless/unthroned) starves its
preconditions and it tells **0 beats** — proven by contrast in `director_demo` (volatile ~69
beats / 255 staged vs **freed 0**) and tests. What's still deferred:
- **The player model `P` is a stub.** The `Protagonist` proxy exists, prominence centres on
  it (and persists across its death — the player outlives the avatar), but there's **no
  intent inference and no §5.C poisoning** — `Γ` reads the protagonist's *situation* and
  *prominence*, not a model of how the player *plays*.
- **No interactive player yet — the machinery is proven only by contrast.** The player is to
  be an ordinary avatar with the *same* verbs as any NPC (no special actions); the freed
  state is reached through ordinary life. The headless ABM proves *world-state gates `Γ`*
  (freed silent vs volatile season); a real player driving a society to the freed attractor —
  and the **felt** manipulation + the reveal's payoff (the medium is the author) — is future.
- **Threads are mechanical, not written.** There is now explicit **thread state** (spine,
  pinned cast, phase, heat, trunk perpetuation) and a legible **cadence** record, but **no
  natural-language surface** — beats are situations, not authored scenes.
- **The beat catalog is a broad starter (~30), not exhaustive.** More of Polti's 36, more
  tone variants, and more **roles** beyond the current eight
  (Protagonist/Ally/Rival/Foe/Patron/Bystander/Lover/Mentor) would deepen the combinatorics.
- **Diversity is heuristic** (recency heat per id/tag/register + hard no-repeat). No
  novelty-search / quality-diversity; a depopulating late game still narrows the palette.
- **`gratuitous(t)`/`staged(t)` are attribution-based, not counterfactual** (per-effect
  affect magnitude + wake deaths). The exact `necessary(t)` (shadow-world diff) is
  deliberately not taken; "least resistance" *is* the alibi that hides the hand.
- **The impact floor & drama weights are global constants.** A per-world or adaptive floor
  would tune the freed/volatile boundary; the deniability gates currently carry the
  freed-world silence (the floor alone did not, once attachment was manufactured).
- **The hard open problem** (design §9): *permanence/irreversibility* — making the freed
  state one `Γ` truly cannot re-perturb — remains open; the self-perpetuating betrayal→
  vengeance trunk embodies the re-escalation pressure rather than resolving it.

---

## Factions
- **Law-as-appeal overlaps with enforcement.** A faction taboo now suppresses a member's
  goal *appeal* (reluctance) *and* its enforcers jail grudge-bearers. For the only current
  taboo (no-kill) these coincide; the appeal path's distinct value is generality to *future*
  faction laws that enforcement wouldn't catch. No issue, just redundant today.
- **Command is champion-dispatch only.** A faction at war drafts one champion (a granted
  `Grievance`). The deeper version — a leader steering members' *ordinary* plans/goals
  (muster, work the war economy, defend a tile) — needs the **planner to honour faction
  directives** (inject/weight goals from the faction), which is unbuilt.
- **Opinion graph is of leaders & rivals, not fully pairwise.** People hold directed
  opinions of the leaders they serve and the rivals they fight, not of every co-member.
  No **first-contact reaction roll** (the OSR-style initial opinion from personality fit).
- **Government is fixed by court kind.** No transitions (revolution monarchy→democracy,
  a coup, a council overthrown). Legislation is court-kind-driven, not *deliberated* by the
  government body.
- **Faction taboos limited to the no-kill act.** Only `temple`/`druid_circle`/`royal_court`
  legislate, and only against `avenge`. A richer law catalog (theft, heresy, trespass) and
  data-driven court→law mapping would broaden it.
- **Tribute goes to the head only** (even for oligarchies — the council doesn't share it),
  and enforcement targets only grudge-bearers (the no-kill case), not arbitrary law-breakers.

## Tile features & affordances
- **Live affordance use is opportunistic.** A well-fed economy rarely treks for wild forage,
  so the observer's `affordance_uses` can be low even though the capability is proven by the
  planner tests. Scarcity-driven scenarios exercise it more.
- **Discovery doesn't spread by word of mouth.** It's per-agent (each NPC's `Known`), grown
  only by *visiting*; no rumour/gossip propagation through the social graph.
- **The teach affordance is sought only in a corner.** Agents apprentice (learn a trade) only
  when their born calling can't serve a current goal — not as deliberate career planning.
- **`markets_on_settlements` is opt-in** (default off) so the tuned social/economy tests stay
  byte-identical; flipping it the default would need those tests re-validated.
- **Placement has no central-place/gravity spacing** beyond the community inhibition radius,
  and no "seed a thin history, then justify the ruins/courts" (Caves-of-Qud-style) layer —
  both flagged in the ABM review as ways to make exploration richer.

## Player discovery & knowledge
A **knowledge-gated discovery loop** (Outer-Wilds-style — knowledge *literally* gates progression)
is being layered onto the player avatar. **Slice 1 is built & tested:** features carry optional
`requires`/`reveals` lore facts (`assets/data/features.ron`, `#[serde(default)]` → existing content
untouched); the player holds a `PlayerKnowledge` resource (lore tokens + rumours); a **search** verb
(`Simulation::player_search`, one tick like *wait*) reveals undiscovered features on the tile whose
gate the held lore satisfies and harvests the lore they teach (`Features::search_at_index`); a
**journal** (J) lists places found + lore held; and the Look panel shows a *"you sense something / it
eludes you"* lure from `find_state`. It is **deterministic — knowledge, not dice, decides** — and
**off-by-default-identical** (empty until an avatar searches; `the_world_runs_with_no_player` still
passes). One authored gnostic chain proves it end-to-end (conventicle → names of the seven +
counter-cosmogony → archon throne / seer's seat → a password of the gate → gate of the seven → the
way beyond). What's deferred:
- **Maps & a player economy are unbuilt.** The avatar has no coin purse and there is no
  cartographer "buy a map" action (reveal a batch of nearby landmarks for coin). Decided: give the
  avatar an `Inventory` like any NPC; maps/services cost money; earning via exploration/quests is
  later. **(blocks: place-knowledge from anything but walking/searching)**
- **No rumours from people.** NPC dialogue does not yet yield place-rumours or lore — the
  `PlayerKnowledge::rumors` field exists but is never populated, the journal's rumour section is
  omitted, and the two-stage *rumoured → located* place model is unrealised. This is the
  player-facing half of the existing "discovery doesn't spread by word of mouth" gap above.
- **Authored gating is one chain, not broad.** Only the gnostic spine (conventicle/seer/archon/
  gate) carries `requires`/`reveals`; the cult of forgetting, the seven *distinct* Archons, the
  ruins of myth, etc. are ungated. The user asked for **broad** authoring — many more chains across
  `features.ron` — still to do (the mechanism is fully in place; this is content work).
- **Located-beyond-fog silhouettes are unbuilt.** A place revealed by a (future) map should beckon
  as a silhouette through the fog before you walk there; today every shown feature is gated on
  `player_explored`, so map-revealed places can't render ahead of the fog. Needs an authoritative
  `located` set in the player layer (planned for the maps slice).
- **"The way beyond" leads nowhere.** Completing the chain grants the `the_way_beyond` lore fact and
  nothing else — no ending, no state change, no payoff. The ascension/liberation it implies is
  unbuilt (and would want to tie into the director's freed-world story).
- **Search is current-tile only.** No searching adjacent tiles, no partial/luck reveals
  (deliberately — knowledge gates, not dice), no cost beyond the one tick; an empty search still
  spends the turn.
- **The player still can't *use* what it finds.** The "full interaction" option (rest at a shrine,
  forage a grove, learn a craft at a discovered guild) was scoped out of the first pass; feature
  affordances remain NPC-only for the avatar.
- **Richer landmark procgen deferred.** The agreed "stronger silhouettes, partial/ruined reveals,
  breadcrumbs, more variety per category" pass — making landmarks *pull the eye and reward
  investigation* — is after the loop (slice 5).

## Ecology & fauna
The substrate now classifies a tile's biome with the **38-zone Holdridge life-zone** system
(replacing the old 6-way `Pft`), from running **annual** climate averages (biotemperature +
annualised precipitation → PET/precip humidity provinces), and the **ecology is organised by
the biome** — each life zone sets its productivity ceiling, flammability and decomposition.
Fauna are **biome-specific**: an authored `assets/data/bestiary.ron` roster of species (habitat
formations, a biotemperature band, size / fecundity / herding, and a body `Form`) that spawn,
forage and breed in the biomes that suit them, rendered as procedurally-animated low-poly
creatures (`app/src/fauna_art.rs`). What's deferred:
- **Calibration is a first pass — done once, at the end.** The world is tuned for "vivid green
  islands in a harsh, otherworldly waste," but the **wastes can't yet sustain their fauna**
  (desert/grassland species starve — *Dune* wants its sandworms): they need a **hardiness /
  low-upkeep adaptation** so sparse biomes still hold life, and **habitat fidelity is loose**
  (out-of-habitat suitability `0.2` lets creatures drift into richer wrong biomes — likely
  `0.1`). Creature **mesh sizes & animation amplitudes** are unverified guesses to tune once
  seen in-app. **(blocks: the final V&V rebaseline + syncing `params.rs` defaults to the
  calibrated `params.ron`)**
- **Predator–prey coexistence is empirically tuned, not derived.** The Liebig productivity +
  herd aggregation + spatial refuge + patient-predator parameters were found by experiment to
  give a persistent oscillation; they aren't proven stable across all worlds/seeds, and a very
  harsh world could still collapse a tier.
- **Diet is still two-way; no apex/scavenger tiers.** Species now differentiate *within*
  `Herbivore` / `Carnivore` (habitat, size, gait), but the design doc's **apex/migratory** and
  **scavenger** trophic tiers are unbuilt.
- **Fauna are land-only.** No fish / marine life and no true flyers (drifters hover but stay
  over land), so the **ocean is lifeless** — pairs with the ocean-traversal gap below.
- **Soil chemistry is partial.** Q10 decomposition and a soil-carbon→carrying-capacity feedback
  are in, but **C:N stoichiometry** (nutrient release governed by litter C:N, immobilisation in
  cold/wet soils) is not.

## Substrate / climate (the geophysics review's open items)
- **No Coriolis term in the live wind** (`update_wind` is pure down-gradient flow), so the
  *dynamic* climate won't reproduce the zonal banding that worldgen's `prevailing_wind` bakes
  in — the two wind models are physically inconsistent. **(blocks: subtropical highs / desert
  belts at ~30°, and a self-consistent rain-shadow story.)** Highest-value substrate fix.
- **No land/sea thermal inertia** (`temp_relax` is the same over ocean and land) → no
  maritime-vs-continental climate or monsoon.
- **Pressure is purely thermal/elevation** — no dynamical highs/lows, so no subtropical highs.
- **Orographic precip is an additive fraction of column humidity**, not Smith–Barstad
  condensation-from-forced-ascent (the weakest-grounded precip term).
- **A richer moisture model** is the last coarse spot in the climate→ecology chain (the ecology
  review's note that the world is moisture-limited).
- Confirm `stream_power_m`/`stream_power_n` land near the literature m/n ≈ 0.5, and decay
  tectonic `stress` inland before it feeds lithology/ore.

## ABM rigour
- **Emergent balance is empirical.** Many results (coexistence, faction consolidation, the
  economy settling) are tuned by experiment; the V&V harness (`observe.rs`) checks *invariants*
  (money conserved, bounds) but does not yet run **parameter sweeps** or a docking/re-implementation
  check to distinguish robust emergence from seed-specific artefacts (Galán/Edmonds).
- **Some tests are property-based, not exact** (e.g. "predators thin a *dense* herd", polling for
  a war champion) because the emergent quantities are phase- and placement-sensitive.

---

# Planned game layers (forward-looking — not yet built)

Unlike the items above (limitations of *shipped* systems), these are whole layers the current
build is scaffolding *toward*, recorded so the direction is explicit. Several interlock —
travel costs, roads/rivers, survival, the economy and the world's scale are one connected design.

## A real UI (today's front-end is a debug interface)
The Bevy shell works but reads as a **developer view** onto the sim — text HUD panels, a colour
legend, floating labels, a plain text journal. A real UI pass is deferred: proper framing and
panels, iconography, readable typography, menus, and polished map / journal / inventory / quest
screens, with clear input affordances. Today's interface is for **inspecting** the simulation,
not yet for **playing** it.

## Ocean traversal — ship / airship / flight
The avatar walks land only (`people::MoveGraph` is land-only; ocean tiles block travel), so
**content across the sea is unreachable**. A vessel layer would open the map: boats / ships for
water, and later **airships or flight** to cross any terrain. Needs a water (and air) movement
graph, a vessel the avatar boards (built / bought / found), port features, and gating so the open
sea is a mid-game unlock rather than a starting affordance. **(blocks: cross-ocean content, a
complete world map)**

## Roads & rivers
Worldgen carves rivers (as surface water) and they shape the climate, but they are **neither
rendered nor used for movement**, and **roads don't exist** (both were skipped in the render
overhaul). The fuller version: generate a **road network** between settlements (the cheapest
travel), **render rivers**, and make a river both a **barrier** (needs a ford / bridge / boat) and
a **route** (fast downstream). Feeds travel costs and trade routes directly.

## Travel costs & world scale
Every land hex currently costs **one tick to cross**, flat — independent of terrain or slope — on
a small demo world (48×36). The intended model:
- **Hex = a day's travel.** Set the hex's in-diameter to the distance one can walk in a day on
  **forest** (baseline) terrain, so one hex ≈ one day's march on neutral ground; size the world to
  a continent of meaningful extent at that scale.
- **Terrain-dependent cost.** Crossing cost scales with the tile's formation / terrain — **roads
  cheapest**, grassland / open baseline, desert / dense forest / swamp / mountain costlier, **ocean
  impassable without a vessel**.
- **Slope / elevation cost.** Cost also depends on the **elevation difference** between adjacent
  tiles: a gentle rise is cheap, but a severe ascent (e.g. **~1000 m over ~0.2 mi**) demands
  **specialized gear** (climbing) or is impassable — gating mountain crossings.
- Feeds the survival layer (time & supplies spent travelling) and the RPG layer (gear unlocks
  routes). **(blocks: meaningful exploration; the survival / RPG layers)**

## JRPG-style conversation UI + procedural portraits
Conversation today is a text HUD panel (plus the optional on-device SLM voicing). A standard
**JRPG dialog window** is deferred — a framed box with the speaker's **name, portrait, and
typewritten text**. Alongside it, **procedurally-generated character sprites / portraits with
facial expressions** that reflect the soul's live state: the sim already tracks each person's
**mood** and **opinion of the player**, which would drive the expression (wary, warm, furious,
awed). Needs a procedural-face generator (parameterized by the agent's traits / appearance) with a
mood-keyed expression layer, and the window UI.

## RPG & survival layers (so exploration has stakes)
Exploration is movement + discovery only — walking A→B costs nothing and changes nothing about the
avatar. Two complementary layers:
- **Survival.** Give the avatar **needs** (hunger / fatigue / warmth — NPCs already carry a needs
  model it could share), drained by travel, time, and the harshness of the biome, restored by rest
  / forage / supplies. The otherworldly wastes and frozen belts become genuinely dangerous;
  provisioning matters.
- **RPG progression.** The avatar gains **capability** over a run (skills / gear / standing) that
  **unlocks routes** (climbing gear → mountains, a vessel → ocean), improves survival, and gates
  content — so discovery *rewards* growth. Composes with the **knowledge-gating already built**
  (knowledge *and* capability gate).
Both rest on a player **`Inventory` / economy** (already flagged for the maps slice) and the
travel-cost model.

## A real economic simulation & trade
The economy is an integer good / money system with markets, price smoothing, and NPC production /
trade (money conserved; death the only sink). A **real economic simulation** is deferred:
**supply / demand prices across a trade network**, caravans / shipping moving goods between
settlements along **roads / rivers / sea**, regional scarcity and specialization, and the **player
participating** (arbitrage, funding expeditions, running cargo). Ties into roads / rivers (routes),
travel costs (freight), and the player economy / RPG layer.

---

*Last updated 2026-06 — the **RPG / survival / exploration overhaul** (modular `agent_core` +
`rpg`/`party`/`survival`/`travel`/`explore` crates; WWN stats/skills/Foci/Edges, recruitable party,
speech + Notice skills, per-tile survival, cost-paced road travel with edge gates, a US-scale world;
its own section above). Done since the reviews: smart-object affordances, per-agent discovery,
learn-a-trade, specialties, price smoothing, V&V harness, ODD doc, the predator layer + Q10 +
soil-carbon feedback + Liebig (→ coexistence), the full faction system (governance, multi-
membership, laws, enforcement, tribute, command, opinion, war), and the **narrative director**
— now a **v2 multi-thread drama manager** (Polti-grounded storylet beats run through
groom→climax→fall **threads**; objective = drama [stakes × attachment × reversal] × novelty ÷
resistance; **manufactured prominence** as attachment; register rotation with emergent betrayal
dominance; collisions; broadened registers + awe/hope/love moods; `staged(t)` joy + suffering
alongside `gratuitous(t)`; relentless protagonist re-casting with persistent prominence;
**no disarm button** — liberation is emergent precondition-starvation behind an impact floor
*and the deniability rule*, the freed world telling an omnipotent director **0 beats**). Also: the
playable Bevy front-end (all-procedural natural terrain/props/buildings, compressed relief,
hover-inspect / right-click-travel, floating labels + legend) and **slice 1 of the knowledge-gated
discovery loop** (search verb, lore facts with `requires`/`reveals` gates, the discoveries journal,
the "you sense something" lure, and one authored gnostic chain). And the **biome overhaul**: the
38-zone **Holdridge life-zone** classifier (replacing `Pft`) from annual climate averages, a
**biome-organised ecology** (per-life-zone productivity / fire / decomposition), an **otherworldly
"Dune" calibration** + palette, **biome-specific fauna** (authored `bestiary.ron` species sorted
into their habitats), and **procedurally-animated low-poly creatures** rendered in the app
(sine-wave gait). The "Planned game layers" and "Ecology & fauna" sections above hold what remains.*
