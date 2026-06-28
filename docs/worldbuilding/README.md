# Worldbuilding — the Waxen World (v2)

The **worldbuilding + writing** companion to the v2 text-conversion design. The *architecture* canon
lives in two docs committed on the `v2` branch — read them first, and **do not edit them**:

- **[`../prose_generation.md`](../prose_generation.md)** — how the world is put into words: a no-LLM,
  never-false generative grammar that assembles authored fragments + Wolfean **tells** over real sim
  facts. The "never false" guarantee is structural.
- **[`../text_interface.md`](../text_interface.md)** — the terminal (ratatui) parser front-end the
  prose renders into; replaces the Bevy `app`.

Those docs repeatedly defer to "forthcoming worldbuilding" and call the fragment/tell corpus "the
product" without itemising it. **These three docs are that worldbuilding and that worklist:**

1. **[`waxen_world.md`](./waxen_world.md)** — *the canon.* The Voice Gallery (seven showpiece passages
   in the teller's voice) and the Part II generators (Seam-Stance, Faction, Society, History, People).
   The source; edit the other two, not this, unless the canon changes.
2. **[`style_guide.md`](./style_guide.md)** — *how the prose should sound.* The native-teller rule, the
   Gricean "flout Quantity, never Quality" justification, the tell-don't-name toolbox, the `distinctive`
   dial, focalisation, the diction bank (folk idiom vs. authoring labels), before→after transforms, and
   a pin-it checklist. This is the voice companion to `prose_generation.md`.
3. **[`content_catalog.md`](./content_catalog.md)** — *the corpus worklist.* The two prose surfaces to
   author into — **`tells.ron`** (the Wolfean tell libraries, the heart of the corpus) and the hardened
   **`grammar.ron`** — plus the agent_core meaning-data (registers, intents, beats), how the generators
   map onto them, and a plane-friendly flight plan. **Defers to the two v2 docs where they overlap.**

**The golden rule across all of these:** the cosmology is *never named* to the player. Lethe, Penury,
Caryatid, tallow, Corollary, the forgery — these are authoring labels and fact-kinds, not words a
character or the narrator ever says. The player only ever gets the folk idiom and the oblique tell.
`style_guide.md` §6 is the translation table.

**The reframing that matters (if you read nothing else):** you are **not** writing a description for
each location/NPC/item — that anti-pattern is exactly what the generative architecture exists to avoid.
You are writing **fact-keyed fragments and tells** the engine assembles per scene. Start on the plane
at `content_catalog.md` §0 (the correction) and §9 (the flight plan); the top job is `tells.ron`.
