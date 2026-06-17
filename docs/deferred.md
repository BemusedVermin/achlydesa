# Deferred work & scope notes

A running backlog of the **honest limitations** and **deliberately-deferred refinements**
behind the systems built so far — the "v1 depth" caveats, captured here so they're
revisitable rather than lost in chat. Each item is *what's simplified* and *what the
fuller version would take*. Nothing here is a bug; these are conscious scope lines.

Grouped by system. Items marked **(blocks X)** gate something else.

---

## Narrative director — v2 multi-thread drama manager built; the player layer deferred
The capstone is a **multi-thread drama manager** (`agents/src/director.rs` + `beats.rs` +
`data/beats.ron` + `data/moods.ron`, **v2 2026-06**, behind `Setup::director`). (The first
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

## Ecology & fauna
- **Predator–prey coexistence is empirically tuned, not derived.** The Liebig productivity +
  herd aggregation + spatial refuge + patient-predator parameters were found by experiment to
  give a persistent oscillation; they aren't proven stable across all worlds/seeds, and a very
  harsh world could still collapse a tier.
- **Only two trophic levels of fauna.** The design doc's **apex/migratory** and **scavenger**
  tiers are unbuilt (just `Herbivore` + `Carnivore`).
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

*Last updated 2026-06. Done since the reviews: smart-object affordances, per-agent discovery,
learn-a-trade, specialties, price smoothing, V&V harness, ODD doc, the predator layer + Q10 +
soil-carbon feedback + Liebig (→ coexistence), the full faction system (governance, multi-
membership, laws, enforcement, tribute, command, opinion, war), and the **narrative director**
— now a **v2 multi-thread drama manager** (Polti-grounded storylet beats run through
groom→climax→fall **threads**; objective = drama [stakes × attachment × reversal] × novelty ÷
resistance; **manufactured prominence** as attachment; register rotation with emergent betrayal
dominance; collisions; broadened registers + awe/hope/love moods; `staged(t)` joy + suffering
alongside `gratuitous(t)`; relentless protagonist re-casting with persistent prominence;
**no disarm button** — liberation is emergent precondition-starvation behind an impact floor
*and the deniability rule*, the freed world telling an omnipotent director **0 beats**).*
