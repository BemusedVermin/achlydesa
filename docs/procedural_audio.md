# Procedural audio & music for achlydesa

*A research synthesis and design recommendation.*

**Emotional target:** a sustained **melancholy** — slow, sparse, dreamlike — punctuated by rare
moments of **awe** (sublime, transcendent swells) that the hidden narrative director earns and times.

**Scope of this doc:** Part 1 surveys the state of the art (generative/adaptive *music* and
procedural *audio*/DSP). Part 2 is the craft of melancholy-with-awe. Part 3 picks a **pure-Rust,
in-engine** stack. Part 4 is the architecture that plugs into achlydesa's authoritative,
byte-deterministic sim **without breaking a single invariant**. Part 5 is a phased recommendation.

> Sourcing note. Findings below were gathered by a multi-source research pass and adversarially
> fact-checked (claims needed independent corroboration to survive). Inline citations point at the
> surviving sources; the **Sources** section lists them. Two honesty flags carried through:
> (a) the claim that affect-driven generative music *measurably improves perceived immersion* over a
> static soundtrack was **refuted** — so the generative architectures below are cited as viable
> *design patterns*, not as proof of a better player experience; (b) crate versions are current as of
> early–mid 2026 and **will drift** — re-check before pinning.

---

## Part 1 — The state of the art

### 1.1 Two paradigms of game music

Peer-reviewed surveys (Plut & Pasquier, *Entertainment Computing*; the Generative-Music-as-PCG
work) draw a clean line. Most shipped game music is **human-composed and played back linearly**;
the adaptive/generative field that extends it splits into two camps:

- **Re-arrangement of pre-composed material** — the *industry* path. Take authored stems and
  recombine them at runtime. Two sub-techniques (below). Battle-tested, cheap, low-risk; adds large
  variation at negligible cost.
- **Real-time algorithmic composition** — the *academic* frontier. Generate the notes themselves at
  runtime. Expressive and infinitely variable, but repeatedly described in the literature as "too
  experimental or computationally expensive" for mainstream adoption, and critiqued on *aesthetic
  coherence* rather than on whether it fits structurally.

**Why adapt at all?** Games are non-linear: the order of events, their outcomes, and session length
all vary, so a fixed loop becomes repetitive and "cannot scale well in today's complex and nonlinear
game narratives" (Plans & Morelli, IEEE TCIAIG 2012). achlydesa's emergent, agent-driven world is an
extreme case of exactly this non-linearity — which argues *for* adaptivity but **not necessarily for
full generative composition.** The proven middle ground is adaptive layering built from segmented
stems.

> **Recommendation R1.** Build the music layer on **stem re-arrangement first** (§1.2). Treat
> from-scratch algorithmic composition (§1.3) as an optional later experiment, not the foundation.

### 1.2 Adaptive stem techniques (the proven core)

- **Vertical layering** — stack several instrumental layers that share a bar grid and key, then
  *volume-mix* them in and out. Sparse drone alone at rest; fade in pads, then a melodic figure,
  then a swell, as intensity rises. No re-timing needed — layers are always in sync.
- **Horizontal re-sequencing** — branch/re-order *segments* of music over time (intro → A → B →
  climax → fall), choosing the next segment from game state, usually transitioning on bar/beat
  boundaries so cuts land musically.

These compose: layer vertically *within* a segment, re-sequence horizontally *between* segments. This
is the workhorse of adaptive scoring and maps directly onto achlydesa's director phases
(`Setup → Rising → Climax → Fall`, see Part 4).

### 1.3 Generative / algorithmic composition (the frontier, as a pattern)

- **Multi-agent composition driven by an affect model.** Hutchings & McCormack's *Adaptive Music
  System* (IEEE *Transactions on Games*, 2020) integrates "cognitive models of knowledge organisation
  and emotional affect … with multi-modal, multi-agent composition." Separate harmony / melody /
  percussion agents each propose material; a "leader" is elected by confidence; an explicit **affect
  vector** steers the whole thing. This is the closest published analogue to achlydesa's design: a
  hidden director that already tracks tension/phase/mood can feed an affect signal to an audio layer
  that *selects and weights* musical material. **Cite as an architecture pattern only** — the
  companion claim that it improved player-reported immersion did not survive verification.
- **Experience-driven procedural music.** Plans & Morelli propose generating music "according to user
  gameplay metrics," with a generator that "reacts to the excitement of the game." This validates the
  core mechanism achlydesa needs: **a scalar/vector signal → musical intensity, density, timbre.** In
  our case the signal is the director's existing `tension_now`/phase/register data, read-only.

### 1.4 Procedural audio / DSP synthesis

The sound-design counterpart to generative music. Avanzini (Springer, 2022) frames it as **"sound as
a process, rather than sound as data"** — synthesized at runtime from programmatic rules and live
input, versus sample pipelines whose decisions are "cast in stone" offline. Two stated advantages
matter directly for a large emergent world:

1. **Adaptability/interactivity** — "ever-changing sonic results in response to real-time control."
2. **Flexibility** — *one parametrized model covers a whole class of sounds*, instead of an
   ever-growing sample library "needed … for complex virtual worlds."

Building blocks relevant here: **drones** (detuned oscillators + filtered noise + long reverb),
**granular/atmospheric textures** (clouds of short grains), and **modal/physically-based SFX**
(struck/resonant bodies). An existence proof that this can be **simulation-driven inside a game
engine**: Su & Joslin (ACM SIGGRAPH MIG '19) compute per-frame "motion events" from real-time
geometric analysis in Unreal and drive **granular/concatenative** synthesis from them — i.e. audio
that reads per-frame sim/geometry state. (Their grains are *retargeted recordings* — a hybrid that
mixes recorded material with procedural control, relevant if we want organic textures without
pure-from-scratch DSP.)

> **Recommendation R2.** For achlydesa's scale, favor **in-engine synthesized ambience** (a
> parametrized drone/texture model) over a sprawling sample library. Keep sampled one-shots only
> where synthesis is uneconomical (specific UI clicks, footsteps).

---

## Part 2 — Achieving melancholy-with-awe

The emotional arc is a **dynamic-range problem**: melancholy must be the wide, quiet, patient
*floor*, so that awe reads as a genuine *event*. If everything is lush, awe has nowhere to go.

**The melancholy floor (default, ~95% of the time)**
- **Harmony:** minor or modal (Aeolian/Dorian/Phrygian); avoid bright leading-tone resolutions.
  Suspended/open voicings (no third, or added 2nds/4ths) read as unresolved and dreamlike.
- **Tempo & rhythm:** slow, often *pulseless*. Let a drone breathe rather than march. Rhythmic
  vagueness is itself melancholic.
- **Timbre:** dark and filtered. Low-passed pads, pink/brown noise beds, a single sparse melodic
  voice with long decay. Tape-like imperfection (slow detune/wow) suits the dream-purgatory tone.
- **Space:** long reverb tails and slow swells imply a vast, indifferent space — the Demiurge's
  hollow cosmos. Sparse events drowning in reverb feel lonely.
- **Density:** *one* idea at a time. Silence is an instrument.

**The awe swell (rare, director-earned)**
- Awe is built from **contrast and motion**, not just volume: introduce *harmonic width* (open
  fifths/octaves spreading outward, a slow Picardy-third lift or a modal brightening), *register
  expansion* (add a very low pedal *and* a high shimmer simultaneously), and a *rising swell* into a
  sustained, reverb-soaked plateau.
- **Reserve material for it.** Keep at least one layer (a choir/pad "awe channel", a high shimmer,
  the longest reverb) **completely silent** during melancholy so its first entrance *is* the awe. A
  layer you've never heard is the cheapest, strongest awe cue you have.
- **Earn it with the director.** Awe lands hardest right after sustained low affect — the sublime is
  potent because the floor was bleak. Tie swells to the director's **Climax** phase on the
  *transcendent* registers (`Wonder`, `Triumph`, `Reunion`, `Redemption`, `Relief`), especially on a
  `collision` (two threads peaking at once — see Part 4), and let them decay back into the floor.

**Reference points** worth a listen for the target affect: drone/ambient game scores built from
generative layers (e.g. *No Man's Sky*'s generative system by 65daysofstatic) and "drones of dread"
suspense-scoring craft articles — the same toolkit (sustained drones, sparse motifs, reserved
swells) serves both dread and awe; the *register* of the harmony is what flips it.

---

## Part 3 — The pure-Rust, in-engine stack

All recommendations honor the **pure-Rust / in-engine DSP** appetite — no Wwise/FMOD. The workspace
currently has **zero audio code** (confirmed: no `kira`/`rodio`/`cpal`/`fundsp`/`bevy_audio`
dependency, no asset files), so this is a clean greenfield addition.

| Crate | Role | Why / trade-offs |
|---|---|---|
| **FunDSP** | the **synthesis / DSP brain** | Pure-Rust audio DSP+synthesis. Expresses audio graphs as *typed Rust* via operators (`>>` pipe, `&` bus, `^` branch, `|` stack, `+` sum, `*` multiply) with **compile-time connectivity checking** and zero-cost, monomorphized/inlined graphs. No native middleware; `no_std`-capable. Ships exactly what the drone/awe palette needs: bandlimited saw/square/triangle/pulse/Hammond oscillators, `white()`/`pink()`/`brown()` noise, delay/multitap/flanger/chorus, and **32-channel FDN reverbs**. **Caveat:** *no* dedicated granular grain-scheduler opcode — true granular clouds need approximation via multitap/delay/modulation, a separate granular crate, or a hand-rolled scheduler. |
| **Kira** (via **`bevy_kira_audio`**) | game-audio **playback / mixing** | Backend-agnostic library "to create expressive audio for games": a mixer with effects, **tweens with multiple easings**, a clock for bar-accurate timing, spatial audio. The Bevy plugin's compatibility table maps **Bevy 0.18 → `bevy_kira_audio` 0.25**, and it explicitly aims to "replace or update `bevy_audio`." Crucially it offers **channel-based playback** — each channel independently controls volume/pan/speed/pause — which *is* the primitive for **vertical layering**, plus `fade_in`/`fade_out` tweens for crossfades. |
| **`cpal`** | OS audio I/O floor | Low-level cross-platform Rust audio I/O; **Kira already uses it by default** (`CpalBackend`). You rarely touch it directly. Kira also ships a device-free **`MockBackend`** useful for offline/CI rendering of the audio layer. |
| **`rodio`** | *alternative* DSP path | Trait-based `Source` (sample-level `Iterator`) you can implement for custom in-engine DSP chains. Viable **fallback** to Kira if we'd rather feed FunDSP through a custom `Source` directly. Trade-off: changing parameters mid-playback needs custom `Source` types; Kira's channel/tween model is a better fit for *adaptive music*. |

> **Recommendation R3.** **Kira + FunDSP** is the spine: **FunDSP synthesizes** the drone bed and
> atmospheric textures (and any DSP on stems); **Kira mixes and crossfades** layers via channels and
> tweens; **`cpal`** is the I/O floor underneath. Keep **`rodio`** in mind only as a fallback if the
> FunDSP→Kira buffer glue proves awkward (see Open Questions).

**On determinism of the audio itself.** FunDSP documents a *deterministic pseudorandom phase* (seeded
from network structure + node location) and **seedable noise** (`noise().seed(1)`). That is *phase*
reproducibility, **not** bit-exact cross-platform float output (it uses f32/f64). **This is fine** —
see Part 4: achlydesa's hard byte-determinism requirement is on the **simulation**, which must stay
audio-free; the audio layer lives on the non-deterministic Bevy front-end and only needs to (a) never
write back to sim state and (b) be *reproducible enough* on one platform given a run seed. Do **not**
advertise the audio output as byte-reproducible across machines.

---

## Part 4 — Architecture: plugging into achlydesa without breaking it

The whole design reduces to **one rule: a one-way, read-only seam.** The deterministic sim *publishes*
affect; the front-end audio layer *reads* it and makes sound; **nothing flows back.** achlydesa
already has a working template for exactly this shape — the `voice` integration — and the audio layer
should mirror it part-for-part.

### 4.1 What the sim already exposes (no sim changes required to start)

The narrative director runs **inside** the deterministic sim and writes its drama state every tick.
All of it is already readable from the front-end via `Simulation` getters (`agents/src/lib.rs`):

| Signal | Getter | Use for audio |
|---|---|---|
| Ambient dramatic tension around the protagonist | `director_tension() -> f32` (`director.rs: tension_now`) | **Master intensity** → layer volumes, filter cutoff, swell amount |
| Running drama threads | `director_threads() -> &[Thread]` | Each `Thread` carries `spine: Register`, `phase: Phase` (`Setup/Rising/Climax/Fall`), `heat`, `ripeness`, `climaxed` → choose musical *register* & detect imminent climax |
| Per-beat cadence log | `director_cadence() -> &[Cadence]` | `Cadence { tick, beat, register, phase, lead_prominence, collision }` → fire **stingers/swells** on new beats; `collision == true` is your strongest **awe** trigger |
| Beats fired so far | `director_beats_fired() -> usize` | Edge-detect "a new beat happened this tick" cheaply |
| Authored suffering this tick / total | `gratuitous_now`, `gratuitous_total()` | Deepen the melancholy floor |
| Protagonist prominence | `director_prominence(e) -> f32` | Spatial/voice focus |

`Register` includes the **melancholy-leaning** values (`Loss`, `Persecution`, `Sacrifice`,
`Betrayal`, `Disaster`) and the **awe-leaning** ones (`Wonder`, `Triumph`, `Reunion`, `Redemption`,
`Relief`). `Phase::Climax` on an awe-leaning register is the canonical "swell now" condition.

> **Recommendation R4 (optional, tiny).** The cleanest readable handle today is `tension_now` +
> threads + cadence. The director *also* internally computes protagonist mood `high()`/`low()`
> (joy/hope/love/awe/rapture/elation vs anger/sorrow/fear/despair/dread/foreboding via `MoodIds`),
> but that isn't exposed. Consider adding **one read-only getter** that returns a small
> **`AudioAffect { intensity, valence, awe }`** snapshot computed *inside* the sim each tick. It keeps
> the front-end dumb and the affect mapping authoritative. This is the single most useful sim-side
> addition; everything else is front-end-only.

### 4.2 The seam — mirror the `voice` precedent exactly

The `voice` crate is the proven "optional Bevy-side layer reads sim, never writes back" pattern. Copy
its shape:

| Concern | `voice` precedent (`app/src/main.rs`, `voice/src/lib.rs`) | Audio equivalent |
|---|---|---|
| **Feature gate** | `voice = ["dep:voice"]` in `app/Cargo.toml`, default-on; `--no-default-features` compiles candle out | `audio = ["dep:audio"]`, default-on; `--no-default-features` compiles the DSP/playback stack out |
| **Bridge module** | `mod voice_bridge` with a `#[cfg(feature="voice")]` impl and a `#[cfg(not)]` no-op impl behind one identical public surface | `mod audio_bridge` — same dual-impl trick, so the no-audio build still compiles and is silent |
| **Background thread** | `thread::spawn("voice")` owns the model; main thread holds `mpsc` channel ends | audio output/synthesis runs off the main thread anyway (Kira/`cpal` own the audio callback thread) |
| **Per-frame drain** | a Bevy system calls `g.voice.poll()` — non-blocking `try_recv` | an `update_audio` system reads sim affect and pushes target params to the bridge each frame |
| **No write-back** | voice results land in a *display* struct (`game.convo`); the only path to sim state is an explicit classify branch *outside* `sim.step()` | audio **only reads** `game.sim.director_*()`; it never calls any `sim.*_mut()`. Period. |

Concretely, the front-end loop already steps the sim by hand — `drive_sim` runs `g.sim.step()` once
per hex the player enters (`TICK_DT = 0.12 s/tick`), and every render system reads sim state through
`NonSend<Game>` on the single main thread. **Insert one `update_audio` system into that same `.chain()`,
right after `drive_sim`.** It reads `game.sim.director_tension()` / `director_threads()` /
`director_cadence()`, maps them to an affect vector, and sets target layer volumes / FunDSP parameters
on the audio bridge. Because it runs after the step and only reads, there is no aliasing and no
write-back.

### 4.3 How awe reacts to drama beats — without touching determinism

```
                          DETERMINISTIC SIM (Bevy-free, byte-identical)
   ┌──────────────────────────────────────────────────────────────────────┐
   │  director_step (each tick): writes tension_now, cadence[], threads[]    │
   │  • gated by cfg.enabled → early-return (audio reads only what exists)   │
   └───────────────────────────────┬────────────────────────────────────────┘
                                    │  read-only getters (Simulation::director_*)
                                    ▼
            FRONT-END AUDIO LAYER (Bevy `app` only — never writes sim)
   ┌──────────────────────────────────────────────────────────────────────┐
   │  update_audio system (after drive_sim, NonSend<Game>):                  │
   │   affect = map(tension, threads.phase/register/heat, cadence.collision) │
   │   ── melancholy floor ─────────────────────────────────────────────┐   │
   │   Kira channel "drone"   ← FunDSP detuned osc + pink/brown + LP + FDN│   │
   │   Kira channel "pad"     ← faded by affect.intensity                 │   │
   │   ── reserved for awe ──────────────────────────────────────────────┤   │
   │   Kira channel "swell"   ← SILENT until Phase::Climax on awe register │   │
   │   Kira channel "shimmer" ← multi-second fade_in tween on a collision  │   │
   └──────────────────────────────────────────────────────────────────────┘
```

- **Melancholy floor** = a FunDSP graph (detuned saw/triangle pair + `pink()`/`brown()` noise →
  lowpass → long FDN reverb), held at low volume, modulated slowly by `director_tension()`.
- **Rising** = vertical layering: as tension/`heat` climbs through `Phase::Rising`, Kira tween-fades
  the `pad`/`figure` channels up.
- **Awe** = the `swell`/`shimmer` channels, kept *silent* until a `Thread` hits `Phase::Climax` on an
  awe register (or a `Cadence.collision`), then brought in with a multi-second `fade_in`
  (`AudioTween` + easing) and let decay back into the floor on `Phase::Fall`.

### 4.4 Preserving the invariants (the checklist)

- **Off by default = byte-identical.** The audio layer is `app`-only and reads sim state; it cannot
  perturb the sim. Even the *sim-side* `AudioAffect` getter (R4), if added, only *reads* existing
  director fields and computes a value — it advances no RNG and mutates nothing.
- **Early-return when disabled.** If R4's getter is added, follow the director's own idiom (zero the
  scratch field, then `if !cfg.enabled { return; }` at the top) so a disabled build is bit-identical.
  Front-end audio gating is via the `audio` cargo feature + a runtime `Setup`-style toggle.
- **Separate, derived RNG streams.** If the audio layer ever needs randomness (e.g. seeding FunDSP
  textures reproducibly per run), it must **not** pull from any existing stream. Mirror the director's
  convention: a dedicated `SplitMix64` seeded as `run_seed ^ <distinct audio constant>`. Since audio
  lives on the front-end this never touches sim determinism, but keeping the discipline makes the
  audio itself reproducible per run.
- **Bevy-free sim.** Nothing in this design adds an audio dependency to `sim`/`config`/`game_sim`/
  `agents`/`voice`. FunDSP/Kira/`cpal` live **only** in `app` (and an optional new `audio`-style
  module/crate that `app` owns), exactly like the full Bevy engine does today.

---

## Part 5 — Recommendation & phased roadmap

A staged path that front-loads emotional payoff and defers the risky parts:

**Phase 0 — Skeleton (low risk).** Add `bevy_kira_audio` (0.25, Bevy 0.18) behind a default-on
`audio` cargo feature with a `mod audio_bridge` dual-impl mirroring `voice_bridge`. Play a single
static drone loop. Proves the seam, the feature gate, and the silent no-audio build. *(Per R3, voice.)*

**Phase 1 — Melancholy floor, synthesized (the heart).** Replace the loop with a **FunDSP** drone
graph (detuned osc + `pink()`/`brown()` → lowpass → FDN reverb). One Kira channel. Modulate cutoff/
volume from `director_tension()`. This alone delivers most of the mood. *(R2.)*

**Phase 2 — Vertical layering + awe.** Split into Kira channels (drone / pad / figure / **swell** /
**shimmer**). Drive channel volumes from the affect mapping; keep swell/shimmer **silent** until
`Phase::Climax` on an awe-leaning `Register` (and on `Cadence.collision`), brought in with multi-second
`fade_in` tweens. This is melancholy-with-awe working end to end. *(R1, R4.)*

**Phase 3 — Horizontal re-sequencing & richer textures.** Use Kira's clock for bar-accurate segment
transitions tied to phase changes; add granular/atmospheric textures (separate granular crate or a
hand-rolled grain scheduler, since FunDSP lacks a grain opcode). Optional.

**Phase 4 — Generative experiment (deferred, optional).** Only if Phases 1–3 plateau: prototype
affect-driven *algorithmic* motif generation à la the multi-agent AMS pattern, fed by the `AudioAffect`
vector. Treat as research, not foundation — and remember the immersion-benefit claim is unproven.

> **Bottom line.** Stand up the **Kira + FunDSP** seam mirroring `voice`; spend your effort on a
> **synthesized melancholy drone floor driven by `director_tension()`**, with **reserved swell/shimmer
> channels** that the director's **Climax-phase awe beats** fade in. That single arc — bleak patient
> floor, rare earned swell — *is* "melancholy with moments of awe," and it costs almost nothing
> against the determinism invariants because the sim never hears a note.

---

## Open questions

1. **What affect should the sim expose?** R4 proposes a small read-only `AudioAffect { intensity,
   valence, awe }` getter computed inside the sim from the director's existing
   `tension_now`/phase/register/mood data — vs. the front-end deriving everything from the current
   getters. Needs a look at whether `MoodIds.high()/low()` should be surfaced. *(This depends on
   director internals not fully audited here.)*
2. **FunDSP → Kira buffer glue.** Can FunDSP-rendered buffers be streamed into Kira's mixer in real
   time (custom Kira sound), or is it cleaner to run FunDSP inside a custom **`rodio` `Source`**? The
   concrete buffer-level integration is unverified.
3. **CPU budget / RT-thread safety.** No source quantified the cost of FunDSP drone+texture synthesis
   plus Kira mixing alongside Bevy rendering for *this* hand-driven `sim.step()` loop. Profile early.
4. **True granular textures.** Is a dedicated granular crate (or a hand-rolled grain scheduler) worth
   the dependency over FunDSP's delay/multitap approximations — and can it be made reproducible under
   the same seeding discipline?

## Caveats

- **Versions drift.** FunDSP ~0.23.0; Kira / `bevy_kira_audio` 0.25 targeting Bevy 0.18 (published
  2026-01-14). Re-check the `bevy_kira_audio` compatibility table before pinning.
- **Audio determinism is *phase*-level, not byte-exact across platforms.** Acceptable because the
  determinism invariant is on the sim, which stays audio-free. Don't claim cross-platform byte-exact
  audio.
- **Generative music ≠ proven immersion win.** The empirical claim that affect-aware generative music
  improves perceived immersion over a static soundtrack was **refuted (0–3)**. Cite the AMS / experience-
  driven work as *viable design patterns*, not as evidence of a better experience.
- **"Industry = lower risk" rests partly on a split vote** (2–1) and on critiques aimed at aesthetic
  coherence rather than structural fit. The phasing above hedges by making generative composition the
  last, optional phase.
- **Su & Joslin's "procedural" sound is concatenative** (retargeted recordings), a hybrid — relevant
  if we want organic textures without pure from-scratch DSP.

## Sources

Survived adversarial verification (3-vote; 2/3 refutes to kill). Quality tags: *primary* = peer-
reviewed / official docs.

**Music — adaptive & generative**
- Plut & Pasquier, *Generative Music in Video Games: State of the Art, Challenges, and Prospects*,
  *Entertainment Computing* (Art. 100337). — academia.edu/83151810
- *Generative Music in Digital Games — Application and Evaluation of PCG Principles* (ResearchGate
  342159490).
- Plans & Morelli, *Experience-Driven Procedural Music Generation for Games*, IEEE TCIAIG 4(3), 2012
  (ResearchGate 260583920).
- Hutchings & McCormack, *Adaptive Music Composition for Games*, IEEE *Transactions on Games* 12,
  2020 — arxiv.org/pdf/1907.01154. *(architecture only; immersion claim refuted)*
- *The Game Audio Co.* — vertical layering vs. horizontal re-sequencing (practitioner).

**Procedural audio / DSP**
- Avanzini, *Procedural Modeling of Interactive Sound Sources in VR*, Springer, 2022 —
  avanzini.di.unimi.it/downloads/publications/avanzini_inbook22a.pdf.
- Su & Joslin, *Procedural Sound Generation for Soft Bodies in Video Games*, ACM SIGGRAPH MIG '19 —
  dl.acm.org/doi/fullHtml/10.1145/3359566.3360068.
- Designing Sound — *Procedural Audio* interview with Andy Farnell.

**Craft — melancholy + awe**
- Game Developer — *Composing Video Game Music to Build Suspense* (Pt. 1 *Ominous Ambience*, Pt. 4
  *Drones of Dread*).
- Game Informer — *65daysofstatic on creating No Man's Sky's generative soundtrack*.

**Rust audio/DSP crates**
- FunDSP — github.com/SamiPerttu/fundsp ; lib.rs/crates/fundsp.
- Kira — crates.io/crates/kira ; github.com/tesselode/kira.
- `bevy_kira_audio` — github.com/NiklasEi/bevy_kira_audio.
- `rodio` — docs.rs/rodio. `cpal` — github.com/RustAudio/cpal.

**achlydesa code seams** (this repo)
- Director surface: `agents/src/director.rs` (`Director`, `Thread`, `Cadence`, `Phase`, `Register`,
  `MoodIds`); getters in `agents/src/lib.rs` (`director_tension`, `director_threads`,
  `director_cadence`, …).
- Sim driving: `app/src/main.rs` (`Game` `NonSend` resource; `drive_sim` → `sim.step()`;
  `TICK_DT`).
- Optional-layer template: `mod voice_bridge` in `app/src/main.rs`; `voice/src/lib.rs`; the
  `TextGen` seam in `agents/src/dialogue.rs`; `Setup` toggles + early-return idiom in
  `agents/src/lib.rs`.
