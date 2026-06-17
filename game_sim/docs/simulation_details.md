# Simulation Design

## Substrate

A hexagonal grid, wrapping east–west with polar caps north/south, so interior hexes have 6
neighbours. Each hex carries the qualities below. **Kind** says when a quality changes:
*static* (fixed at world-gen), *slow* (rare geological events), *dynamic* (every tick, in
`evolve`), *derived* (recomputed from others, not stored).

### Tile Qualities

#### Geological

| Quality | Kind | Description |
| --- | --- | --- |
| `elevation` | slow | Height above sea level. Raised by orogeny, lowered by erosion. |
| `plate` | static | Tectonic plate id. |
| `crust_type` | static | Continental or oceanic. |
| `boundary` | static | Plate boundary here: convergent / divergent / transform / none. |
| `lithology` | static | Bedrock class; gates minerals and soil. |
| `slope` | derived | Steepest elevation drop to a neighbour. |
| `volcanism` | slow | Magma proximity; erupts to add elevation and nutrients. |
| `seismicity` | derived | Quake hazard from boundary proximity. |
| `soil_depth` | slow | Regolith thickness (weathering minus erosion). |
| `minerals` | static | Ore / fuel richness. |

#### Climate (all dynamic)

| Quality | Description |
| --- | --- |
| `insolation` | Solar energy from latitude, axial tilt, season. |
| `temperature` | Insolation minus elevation lapse and albedo, smeared across neighbours. |
| `pressure` | From temperature and elevation. |
| `wind` | Vector down the pressure gradient. |
| `humidity` | Evaporation, carried along the wind. |
| `precipitation` | Rain where humid air lifts over windward slopes or cools. |
| `surface_water` | Rainfall routed downhill into rivers and lakes. |
| `snow_ice` | Cover that builds where it freezes. |

#### Ecosystem (all dynamic)

| Quality | Description |
| --- | --- |
| `pft` | Dominant plant type (forest / grass / shrub / tundra) fitting the climate. |
| `npp` | Plant growth from temperature, water, light, nutrients. |
| `plant_biomass` | Standing growth, minus grazing, fire, mortality. |
| `litter` | Dead biomass feeding the soil. |
| `soil_carbon` | Litter minus decomposition. |
| `soil_nutrients` | Weathering plus decomposition, minus plant uptake. |
| `herbivore_biomass` | Grazers on the plants (or modelled as Actors — see below). |
| `carnivore_biomass` | Predators on the grazers (or Actors). |

#### Derived

`slope`, `seismicity`, `leaf_area_index`, `albedo`, `evapotranspiration`, `biome`,
`carrying_capacity`, `species_richness`, `latitude`, plus `fire` (ignites on dry fuel, burns
biomass back into soil).

### Tile Features

Multiple can occupy a tile; suitability follows the tile qualities. Each belongs to one of
WWN's four point-of-interest categories and is fleshed out by two WWN tags (Enemies / Friends /
Complications / Things / Places).

> **Implemented** in the agent layer (`agents/src/features.rs`, catalog in
> `agents/data/features.ron`). A hex carries at most one feature per category, so up to four
> stack on a tile (a city, its royal court, the catacombs beneath, a wonder nearby). Each
> kind's *Favoured by* is authored as **suitability terms** — a tile `Signal` read through the
> same response `Curve` the agent utility scorer uses — combined as a compensated product, so
> "favoured by the land" is one algebra with "agent appeal". Placement is deterministic (its own
> RNG stream, so it never perturbs the economy's) and runs in two passes: communities first,
> with an **inhibition radius** that spaces settlements out instead of clumping them into the one
> best basin (the central-place insight); then courts / ruins / wilderness, which can read
> *remoteness* (hex distance to the nearest community) and honour `host` constraints (a royal
> court only inside a city). Discovery tiers are live: Landmarks are known on placement, an NPC
> searching a hex reveals its Hidden features, and Secrets stay latent. *Still ahead:* features
> advertising **affordances** into the NPC `decide` loop (the smart-object step that turns a POI
> from scenery into a place agents use), per-agent knowledge of discoveries, and seating the
> economy's markets in settlements by default (`Setup::markets_on_settlements`, opt-in today).

**Discovery tier** — how a feature is found on entering its hex:

- *Landmark* — seen automatically (the obvious structure or terrain).
- *Hidden* — found by spending a turn searching (a lair, a buried entrance).
- *Secret* — needs luck, insight, or a skill check (a vault, the true power behind a court).

The enumeration below is cross-genre (fantasy, sci-fi, post-collapse, weird). `Discovery` is the
typical tier; `Favoured by` is the qualities that make a tile suitable.

#### Communities (settlements)

| Feature | Discovery | Favoured by |
|---|---|---|
| Thorp / hamlet | Landmark | mild `temperature`, some `soil_nutrients`, water |
| Village | Landmark | rich `soil_nutrients`, `surface_water`, low `slope` |
| Town | Landmark | river/road junction, `minerals` nearby |
| City / metropolis | Landmark | fertile basin, navigable `surface_water`, coast |
| Fishing village | Landmark | coast or river mouth |
| Mining / logging camp | Landmark | high `minerals` / dense `forest` |
| Caravanserai / waystation | Landmark | arid, on a route between settlements |
| Monastery / abbey | Landmark | remote, defensible high `elevation` |
| Garrison / frontier fort | Landmark | border, high `slope` chokepoint |
| Nomad / refugee camp | Hidden | marginal grassland, low `carrying_capacity` |
| Dome colony / arcology | Landmark | hostile climate (extreme `temperature`) |
| Vault / underground shelter | Hidden | irradiated or buried; near a `ruin` |

#### Courts (powers & factions)

| Feature | Discovery | Favoured by |
|---|---|---|
| Royal court / seat of rule | Secret | inside a city |
| Baron's keep / warlord hold | Landmark | defensible high ground, border |
| Temple hierarchy / high cult | Hidden | inside a community or sacred site |
| Mage academy / guild | Hidden | city, or a remote tower |
| Thieves' guild / cartel | Secret | inside a city |
| Merchant consortium | Hidden | trade hub, port |
| Knightly / military order | Landmark | fortress, frontier |
| Druid circle / fey court | Secret | deep `forest`, wild site |
| Corporate HQ / AI core | Secret | arcology, station |
| Cult compound | Hidden | remote wilderness |

#### Ruins (adventure sites)

| Feature | Discovery | Favoured by |
|---|---|---|
| Ruined keep / castle | Landmark | old border, hilltop |
| Wizard's / broken tower | Landmark | remote, often wild |
| Forgotten / sunken temple | Hidden | jungle, swamp, underwater |
| Barrow / tomb / crypt | Hidden | old settled land, moor |
| Pyramid / great monument | Landmark | desert, open plain |
| Catacombs / buried city | Secret | under a present or past city |
| Ghost town / dead village | Landmark | abandoned farmland, plague land |
| Collapsed mine | Hidden | high `minerals`, mountains |
| Ancient battlefield | Hidden | open plain |
| Crashed starship / derelict | Landmark | anywhere; impact scar |
| Ruined factory / refinery | Landmark | post-collapse industrial belt |
| Military bunker complex | Hidden | hills, borders |
| Alien monolith ruin | Secret | remote, anomalous |

#### Wilderness locations (wonders, lairs, hazards)

| Feature | Discovery | Favoured by |
|---|---|---|
| Waterfall / rapids | Landmark | river over high `slope` |
| Hot springs / geysers | Landmark | high `volcanism` |
| Cave / cavern system | Hidden | limestone `lithology`, hills |
| Gorge / canyon | Landmark | river-cut high `slope` |
| Lone peak / volcano | Landmark | high `elevation`, `volcanism` |
| Crater | Landmark | impact / blast scar |
| Tar pit / sinkhole | Hidden | low ground, soft `lithology` |
| Oasis | Landmark | desert with `surface_water` |
| Ancient grove / great tree | Landmark | dense old `forest` |
| Standing stones / megalith | Landmark | moor, open plain |
| Beast lair / monster nest | Hidden | caves, dense `forest`, remote |
| Dragon roost / apex den | Hidden | mountain peak |
| Bandit / raider camp | Hidden | near roads, broken terrain |
| Magical / mutagenic anomaly | Secret | wild-magic or irradiated zone |
| Kaiju rest site / titan bones | Secret | remote, low population |

### Substrate update (`Φ`)

Each tick reads neighbour values from the old grid and writes a fresh one, then swaps — so a
quality never sees an already-updated neighbour. Three spatial moves over the 6 neighbours:
**diffuse** (spread evenly), **advect** (push along the wind), **flow** (send to the steepest
downhill neighbour). One tick, in order:

1. **Geology** (rare events only): at convergent boundaries, lift elevation and maybe erupt.
   Every tick, erode each hex toward its lowest neighbour and deposit the soil there.
2. **Climate**: set insolation from latitude and season; derive temperature, then diffuse it;
   take pressure from temperature and elevation, and wind from the pressure gradient; raise
   humidity by evaporation and advect it along the wind; drop rain where that air lifts over
   slopes or cools; route the rain downhill into rivers and lakes; lay snow where it freezes.
3. **Ecosystem**: from temperature, water, light, and nutrients, compute growth and add it to
   biomass (less what's grazed or burned); pick the plant type that fits; shed litter, which
   decomposes back into soil carbon and nutrients. Feed herbivores from plants and carnivores
   from herbivores, letting each drift toward better neighbouring tiles.
4. **Disturbance**: ignite fire where dry fuel is high, then recompute albedo and biome.

## Actor types

An **Actor** is any entity the player doesn't control. Two kinds: **fauna** (herds and packs,
driven by instinct) and **NPCs** (sentient folk, driven by needs *and* motivations). Both run
the same loop — `perceive` the local hex and nearby actors, `decide` an action, `interact` to
trade effects — and `claim` contested things so a tick stays consistent.

### Fauna

Herds and packs that move and feed on the substrate's fields. Rule of thumb: continuous,
everywhere dynamics (vegetation, soil, climate) stay substrate fields; things that decide and
compete for space become Actors. Each carries `size`, `energy`, `age`, and a hex, and closes the
trophic loop: grazing lowers `plant_biomass`, hunting sends a `Predation` effect that shrinks the
target herd.

| Actor | Role | Decides | Effect |
| --- | --- | --- | --- |
| Herbivore herd | grazer | graze, migrate to better range, breed, flee | eats `plant_biomass` |
| Carnivore pack | predator | hunt a weak herd, track prey, patrol, breed | shrinks the target herd |
| Apex / migratory | seasonal | long-range migration along the climate gradient | thins mid-level packs |
| Scavenger flock | optional | forage litter and carcasses | speeds soil return |

Contested forage or prey is resolved by `claim` + `priority` (size / hunger); the loser re-decides.

### NPCs

A DM normally voices NPCs; here they run themselves. Two drivers sit on top of the base loop:

- **Needs** — short-term meters that drain over time and set baseline behaviour (utility AI, as in
  *The Sims*): *sustenance, rest, safety, wealth, belonging, health*. The most urgent need pulls
  the NPC toward whatever action best relieves it.
- **Motivation** — a longer-term goal that biases choices once needs are met and that **can
  change** on events: *survival, wealth, power, status, knowledge, faith, family, revenge,
  freedom, order*. A betrayal flips a merchant to *revenge*; a promotion shifts *wealth* to
  *power*; chronic hunger collapses everything to *survival*.

**No classes — roles emerge.** There is no *baker* type; there is an agent that buys grain, bakes,
and sells bread because, given the local price and its skill, that pays best and feeds it. Every
NPC shares one action library and runs the same choice. This is the *Sims* smart-object model —
actions are **afforded** by tiles, features, and nearby actors, each advertising what it gives —
crossed with agent-based economics, where agents take the most profitable work their skill and the
market allow (as identical agents self-sort into trades in the AI Economist and Dwarf Fortress).

`decide`: collect the actions afforded here, score each by *need relief + expected payoff*
(money and goods feed the wealth need) × skill × motivation weight, take the best; with nothing
urgent, the motivation's goal-action wins. Performing an action trains its skill, so its future
utility climbs and the agent **specialises**; as a good floods the market its price falls, nudging
others toward different work — division of labour emerges with no role ever assigned. `interact`
applies social effects — `Trade`, `Hire`, `Command`, `Tax`, `Threaten`, `Befriend`, `Betray`,
`Convert` — that move needs, rewire relationships, and may rewrite the motivation.

**Action library** — universal; an NPC may attempt any action its location and skills afford:

| Group | Verbs | Afforded by | Pays in |
| --- | --- | --- | --- |
| Subsist | eat, drink, rest, heal | home, inn, food stock | need relief |
| Produce | farm, herd, fish, hunt, mine, fell | the matching tile + quality | raw goods |
| Craft | bake, smith, weave, brew, build | a workshop + inputs | finished goods |
| Exchange | buy, sell, haul, lend | market, road, caravan | money |
| Protect | guard, patrol, raid, war | a settlement or a target | pay, loot |
| Social | befriend, court, preach, scheme, beg, give | other actors, temple, court | belonging, alms, allies |
| Govern | tax, command, judge, build | an office in a court | power, revenue |

**Emergent archetypes** — recognisable loops the system settles into; labels, not types. The
earlier roster (smith, merchant, priest, lord, bandit…) is now an *output*, each just a loop:

| Archetype | The loop it runs |
| --- | --- |
| Farmer | farm a fertile tile → sell the surplus |
| Baker | buy grain → bake → sell bread |
| Merchant | buy cheap here → haul → sell dear there |
| Guard | hire on to a settlement → patrol for pay |
| Bandit | raid travellers when honest work pays less |
| Beggar | beg where alms flow (a rich or pious tile) when nothing else pays |
| Priest | preach at a temple → gather belonging, alms |
| Lord / monarch | hold an office → tax and command → fund power |

### Choosing an action (utility)

One rule drives fauna and NPCs alike: score every afforded action, take the best.

`score(a) = ( Σₙ urgency(n)·relief(a,n) + value(payoff a)·w_wealth + goalFit(a)·w_goal ) · skill(a) / cost(a)`

- **urgency(n)** climbs steeply as need *n* empties, so the most pressing need dominates (the
  utility-curve idea from *The Sims*).
- **relief(a, n)** is what the action advertises it restores to need *n* — the affordance's promise.
- **payoff(a)** is coin or goods gained; `value()` runs it through the wealth need, so a coin is
  worth more to the poor and "sell bread" shines exactly when money is short and bread sells well.
- **skill(a)** scales yield and speed and grows by doing — the specialisation engine.
- **cost(a)** is time + travel to the afforded hex + risk, so a cheap nearby option competes with a
  rich distant one (and `Migrate` is just an action with a travel cost).
- **w_wealth, w_goal** come from the motivation: a power-seeker weights office and command, a zealot
  weights preaching — whatever the trade.

Take the highest; below an idle threshold, pass (`decide` → `None`). A little softmax noise breaks
ties and keeps crowds from moving in lockstep. Contested actions reuse this score as
`Action::priority`, so whoever values a tile or job most wins the `claim`.

### Changing motivation

Motivation isn't fixed — it's a small weighted set of drives (*survival, wealth, power, status,
knowledge, faith, family, revenge, freedom, order*); the top one is "the motivation" that sets
`w_goal`. Events nudge the weights, and when a challenger overtakes the leader by a margin, the
motivation flips. Triggers come from three places — a need stuck low, an `interact` effect, or a
life event the driver raises:

| Event | Pushes toward |
| --- | --- |
| Chronic hunger / danger / sickness | survival (everything else collapses) |
| Robbed, betrayed, assaulted, kin slain | revenge (at the culprit) |
| Saved, sheltered, married, a child born | family / belonging (at the helper or kin) |
| Preached to, survived a disaster, grief | faith |
| Windfall, promotion, taking an office | power, status |
| Publicly shamed or out-ranked | status (restore face), or revenge |
| Taxed, conscripted, confined, enslaved | freedom |
| Crime, war, or disorder witnessed | order |
| Exposed to lore, mystery, a mentor | knowledge |

Mechanics: each event adds a delta scaled by its intensity and the agent's personality (a vindictive
trait amplifies *revenge*, a pious one *faith*), so the same blow lands differently on different
people. Drives **decay toward a personal baseline** each tick, so an unreinforced shift fades — a
grudge cools unless poked again. A **hysteresis margin** on the flip stops flicker, and **satiation**
(a fulfilled drive relaxes) lets the next rise, giving life-arcs: *wealth → power → status*, or the
slide to *survival* and *revenge* on ruin. Some drives are **directed** — revenge at a culprit,
family at kin, loyalty at a court — so they aim the utility scorer at a specific actor, not just a
goal type.

A flip needs no new code: the new dominant drive simply re-weights `w_goal`, so the same scorer now
favours different goal-actions — the betrayed baker starts scoring `scheme`, `hire`, and `raid`
against whoever wronged them.

### Personality

Three timescales stack: **needs** shift by the tick, **motivation** by life events, **personality**
barely at all — it is set at birth (inherited and learned from kin) and is the dial that makes the
same event land differently on two people. Two layers, from the Big Five and *Pendragon* / Dwarf
Fortress practice:

- **Temperament** — five continuous axes that bias the utility scorer and the motivation deltas:

| Axis | A high score shifts… |
| --- | --- |
| Openness | weights *knowledge*, exploration, switching trades; bigger knowledge deltas |
| Conscientiousness | faster skill gain, steady production over risk, weights *order* |
| Extraversion | weights social actions and *belonging*; acts on others more |
| Agreeableness | favours `befriend` / `give` / cooperate; low end → `betray`, `raid`, bigger *revenge* deltas |
| Neuroticism | weights the safety need, lowers risk tolerance, bigger *survival* / fear deltas |

- **Values** — what the agent holds dear (family, faith, honour, freedom, wealth, power — the Dwarf
  Fortress *beliefs* idea). Values set the **baseline** of the motivation vector — the baseline that
  drives decay toward — so personality decides which motivation an agent drifts back to once events
  fade. Named traits (Brave, Greedy, Pious — the CK / RimWorld style) are just labelled regions of
  this space, handy for authoring and display.

### Relationships & factions

NPCs carry a directed **opinion** of those they know — a score from −100 to +100 plus relation tags
(kin, friend, rival, lover, ally, liege / vassal, debtor), the *Crusader Kings* model. The directed
motivation drives from above (revenge-at, loyalty-to) are edges in this same graph.

- **First contact** with no history rolls a reaction (hostile → friendly) seeded by personality fit —
  shared values and high agreeableness skew friendly — the OSR reaction roll.
- **Update** — `Trade`, `give`, `Befriend` raise opinion; `Betray`, `raid`, `Tax`, `Threaten` lower
  it; it accumulates and decays toward the personality-compatibility baseline.
- **Effect** — opinion gates social-action success and utility: you `hire`, `Command`, or `Convert`
  allies easily, aim `betray` / `raid` at the disliked, and won't turn on a friend.

**Factions are higher-order Actors.** A cluster of mutually loyal NPCs around a court (a tile
feature) becomes a composite actor that runs the *same* loop one level up — the *Worlds / Stars
Without Number* faction turn:

- **Stats** — Force / Cunning / Wealth (war, intrigue, economy) plus cohesion and **assets** (tiles,
  settlements, troops, agents), fed by members: soldiers add Force, spies and scholars Cunning,
  merchants Wealth.
- **Goal** — a faction-level *motivation* (conquest, commerce, an intrigue coup, survival, converting
  the land); it `decide`s one action per turn toward it (expand, attack, seize a tile, build an
  asset, scheme), funded by members' taxes and tithes.
- **Lifecycle** — forms when loyalty clusters past a threshold, **splits or collapses** when internal
  opinion erodes or the leader dies, and sheds defectors (low opinion) that seed rival factions. The
  `Simulation` driver handles faction birth, merge, and death between ticks, as it does herds;
  factions `claim` tiles and offices and contend through the scheduler like any actor.

The recursion is the point: **a faction is an Actor whose members are Actors**, so politics needs no
new machinery — the same perceive / decide / interact / claim loop, scaled up.

How they interact with each other and the world:

- **Economy** — a chain of producers → crafters → traders → consumers. Prices track local supply
  (substrate `minerals`, `plant_biomass`) against demand (population needs); a drought that lowers
  `npp` raises food prices, leaving needs unmet → unrest and migration.
- **Society** — each NPC keeps relationship edges (kin / ally / rival) and a loyalty to a court
  (a tile feature). `Befriend` and `Betray` rewire these into factions.
- **Politics** — rulers issue directives that cascade down a hierarchy (a `Command` to
  subordinates) and reshape settlements: war mobilises soldiers, spawns raiders and refugees, and
  squeezes commoners' `wealth` and `safety` — looping back into migration and the substrate.
- **Contention** — NPCs `claim` jobs, market goods, a title in a court, or a mate; `priority` goes
  by rank / wealth / strength, and losers re-decide.

As with fauna, births, deaths, and role changes (a ruined farmer turning bandit, a guard promoted
to captain) alter the actor roster, so the `Simulation` driver applies them between ticks; an
`Observer` reports population, wealth, unrest, and faction power for balancing.

## Markets & prices

Payoff must be endogenous, or everyone bakes forever. Each settlement feature is a **market** with
a `stock` per good; prices form from local supply and demand and tie the economy to the land.

- **Price from stock** — a market prices each good by how full its larder is against a target:
  `price = base · target / max(stock, ε)`, clamped to a floor and ceiling. Plentiful → cheap,
  scarce → dear. `stock` is the running sum of supply minus demand, so this is supply-and-demand
  without measuring flows directly.
- **Supply & demand** — `Produce` / `Craft` / `sell` add to `stock`; `buy` removes it. Demand traces
  to needs: a hungry agent buys food, a baker buys grain (derived demand), so population needs set
  what each good is worth.
- **Yield from the land** — production scales with tile qualities: grain ∝ `soil_nutrients` · water ·
  `npp`, ore ∝ `minerals`. A drought lowers `npp` → less grain reaches the larder → `stock` falls →
  bread price climbs. This is the single channel from climate to economy.
- **Trade equalises space** — markets differ because local stocks differ; the `haul` action buys
  where a good is cheap and sells where it's dear, moving stock and pulling the two prices together
  minus transport cost over hex distance and road risk. The merchant archetype *is* that arbitrage,
  and the surviving price gap sets how far goods travel.
- **Back to behaviour** — when food outruns wages, the sustenance need goes unmet: `value(payoff)` of
  honest work drops while the utility of `raid`, `beg`, or `Migrate` rises — banditry, begging,
  emigration, unrest. The same scorer that picks "bake" picks "leave" or "rob" when the market turns.
- **Timing** — within a tick, trades adjust `stock`; at tick end each market recomputes prices from
  the new stock (written into the next grid like every dynamic field), and agents decide next tick
  against them.

The whole loop: **land → yield → stock → price → payoff → utility → action → land.**
