//! **Emergent dialogue** — the NPC's inner life, spoken (`docs/dialogue.md`).
//!
//! Dialogue here is **a new action modality for the brain the sim already has** —
//! *speaking is acting*. It splits into two layers (the design's load-bearing decision):
//!
//! - **Meaning is simulation.** A conversational [`Intent`] is authored data scored by the
//!   *same* IAUS utility ([`ai::score`]) that ranks goals — from the speaker's traits,
//!   mood, opinion of the listener, open grudges, and the director's pressure. The intent
//!   that fires is a product of *who the agent is and what has happened*, never a scripted
//!   branch; its [`Move`]s mutate the social state ([`Opinion`]/[`Mood`]/[`Grievance`]) and
//!   are part of the seeded tick. Emergent, deterministic, whole-population.
//! - **Surface is rendering.** The words are *generated*, never drawn from a phrasebook —
//!   composed by a [`Grammar`] (Dwarf-Fortress lineage: authored lexicon + rules, not
//!   lines), or, out of band, by a small language model ([`TextGen`]/[`SlmRealizer`]) for
//!   the one conversation in focus. The surface never feeds back into sim state, so a world
//!   with no model loaded is byte-identical to one with — the model is never load-bearing.
//!
//! Off by default and deterministic: all state lives in the [`Dialogue`] resource (no NPC
//! component), variety comes from a dedicated seeded [`SplitMix64`].

use crate::ai::{self, Consideration, Curve, Input};
use crate::chronicle::{Chronicle, EpisodeKind};
use crate::data::Registry;
use crate::factions::Opinion;
use crate::people::{Grievance, Mood, Needs, Npc, Personality};
use crate::{Position, Substrate};
use bevy_ecs::prelude::*;
use config::{Asset, Bundled};
use game_sim::{Coord, SplitMix64};
use serde::Deserialize;
use sim::Rng;
use smallvec::SmallVec;
use std::collections::HashMap;

/// A Searle-style illocutionary class — *what kind of thing* is being said. The grammar
/// and the moral colouring key off it; authored on each [`Intent`].
#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum SpeechAct {
    Greet,
    Accuse,
    Threaten,
    Plead,
    Confide,
    Console,
    Mourn,
    Boast,
    Praise,
    Reconcile,
    Recruit,
    Gossip,
    Deflect,
}

impl SpeechAct {
    /// The grammar key (lowercase) the surface generator composes from.
    pub fn key(self) -> &'static str {
        match self {
            SpeechAct::Greet => "greet",
            SpeechAct::Accuse => "accuse",
            SpeechAct::Threaten => "threaten",
            SpeechAct::Plead => "plead",
            SpeechAct::Confide => "confide",
            SpeechAct::Console => "console",
            SpeechAct::Mourn => "mourn",
            SpeechAct::Boast => "boast",
            SpeechAct::Praise => "praise",
            SpeechAct::Reconcile => "reconcile",
            SpeechAct::Recruit => "recruit",
            SpeechAct::Gossip => "gossip",
            SpeechAct::Deflect => "deflect",
        }
    }
}

/// The two roles in a conversational exchange. A [`Move`] names whose state it touches.
#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Party {
    Speaker,
    Listener,
}

/// What *saying* an intent does to the social fabric — the deterministic consequence, in
/// the same vocabulary as the director's beat effects but scoped to Speaker/Listener.
#[derive(Deserialize, Clone, Debug)]
pub enum Move {
    /// Move `who`'s opinion of `toward` by `delta` (clamped `-1..1`).
    Turn { who: Party, toward: Party, delta: f32 },
    /// Shift a mood of `who` by `delta` (clamped `0..1`).
    Stir { who: Party, mood: String, delta: f32 },
    /// Shift a trait of `who` by `delta` (clamped `0..1`).
    Sway { who: Party, trait_name: String, delta: f32 },
    /// `who` comes to bear `against` a grudge — words that harden into a feud.
    Grudge { who: Party, against: Party },
}

fn one() -> f32 {
    1.0
}

/// A conversational intent, resolved: a speech act, the IAUS considerations that make an
/// agent *want* to say it to a given listener, and what saying it does.
#[derive(Clone, Debug)]
pub struct Intent {
    pub id: String,
    pub act: SpeechAct,
    pub tags: Vec<String>,
    /// Scored by [`ai::score`] against the speaker→listener pair — the emergent "what
    /// would this person, feeling this way about you, want to say?"
    pub appeal: Vec<Consideration>,
    pub moves: Vec<Move>,
    pub weight: f32,
}

// --- Authored (RON) forms; trait/mood names resolved on load, mirroring goals.rs ---

#[derive(Deserialize)]
struct IntentDef {
    id: String,
    act: SpeechAct,
    #[serde(default)]
    tags: Vec<String>,
    appeal: Vec<ConsiderationDef>,
    #[serde(default)]
    moves: Vec<Move>,
    #[serde(default = "one")]
    weight: f32,
}

#[derive(Deserialize)]
struct ConsiderationDef {
    input: InputDef,
    curve: Curve,
}

/// The RON form of an [`Input`], extended with the dialogue layer's listener-relative axes.
#[derive(Deserialize)]
enum InputDef {
    Deficit,
    Trait(String),
    Mood(String),
    Sanction,
    OpinionOf,
    GrievanceAgainst,
    SharedHistory,
    Prominence,
}

impl ConsiderationDef {
    fn resolve(self, reg: &Registry) -> Result<Consideration, String> {
        let input = match self.input {
            InputDef::Deficit => Input::Deficit,
            InputDef::Trait(n) => Input::Trait(reg.trait_id(&n).ok_or_else(|| format!("unknown trait '{n}'"))?),
            InputDef::Mood(n) => Input::Mood(reg.mood_id(&n).ok_or_else(|| format!("unknown mood '{n}'"))?),
            InputDef::Sanction => Input::Sanction,
            InputDef::OpinionOf => Input::OpinionOf,
            InputDef::GrievanceAgainst => Input::GrievanceAgainst,
            InputDef::SharedHistory => Input::SharedHistory,
            InputDef::Prominence => Input::Prominence,
        };
        Ok(Consideration { input, curve: self.curve })
    }
}

/// The whole repertoire of conversational intents — authored in `assets/data/intents.ron`.
#[derive(Resource, Clone, Debug, Default)]
pub struct IntentBook(pub Vec<Intent>);

impl IntentBook {
    pub fn bundled() -> Self {
        Self::from_ron(Bundled::get(Asset::Intents), &Registry::bundled())
            .expect("bundled intents are valid")
    }

    pub fn from_ron(ron: &str, reg: &Registry) -> Result<Self, String> {
        let defs: Vec<IntentDef> = config::parse(ron).map_err(|e| e.to_string())?;
        let mut out = Vec::with_capacity(defs.len());
        for d in defs {
            // Validate move names eagerly, like the beat book does.
            for m in &d.moves {
                match m {
                    Move::Stir { mood, .. } => {
                        reg.mood_id(mood).ok_or_else(|| format!("intent '{}': unknown mood '{mood}'", d.id))?;
                    }
                    Move::Sway { trait_name, .. } => {
                        reg.trait_id(trait_name).ok_or_else(|| format!("intent '{}': unknown trait '{trait_name}'", d.id))?;
                    }
                    _ => {}
                }
            }
            let appeal =
                d.appeal.into_iter().map(|c| c.resolve(reg)).collect::<Result<Vec<_>, _>>().map_err(|e| format!("intent '{}': {e}", d.id))?;
            out.push(Intent { id: d.id, act: d.act, tags: d.tags, appeal, moves: d.moves, weight: d.weight });
        }
        Ok(IntentBook(out))
    }
}

// Dialogue knobs ([`DialogueConfig`]) live Bevy-free in the `config` crate;
// re-exported here and wrapped in an ECS-resource newtype.
pub use config::DialogueConfig;

/// ECS-resource handle for the [`DialogueConfig`] knobs. Derefs to the config.
#[derive(Resource, Clone, Debug)]
pub struct DialogueRes(pub DialogueConfig);

impl std::ops::Deref for DialogueRes {
    type Target = DialogueConfig;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// A remembered exchange — the cheap, symbolic episodic record the surface and the
/// `SharedHistory` axis read. No embeddings: relevance is set-membership + recency.
#[derive(Clone, Debug)]
pub struct MemRecord {
    pub tick: u64,
    pub partner: Entity,
    pub act: SpeechAct,
    pub register: String,
    pub importance: f32,
    /// Spaced-repetition strength: rises on recall, fades otherwise (Ebbinghaus).
    pub strength: f32,
}

/// One soul's dialogue memory and speaking cadence.
#[derive(Default)]
struct MemLog {
    records: Vec<MemRecord>,
    last_spoke: Option<u64>,
}

/// A told utterance — the meaning plan plus everything the surface generators need. The
/// grammar surface is filled in-tick; the SLM realizer (out of band) reads the card.
#[derive(Clone, Debug)]
pub struct Utterance {
    pub tick: u64,
    pub speaker: Entity,
    pub listener: Entity,
    pub intent: String,
    pub act: SpeechAct,
    /// Names, motive descriptors, mood word, relationship word, the referent — the
    /// "character card" the SLM prompt is assembled from (numbers already turned to words).
    pub speaker_name: String,
    pub listener_name: String,
    pub motive: SmallVec<[&'static str; 4]>,
    pub mood_word: &'static str,
    pub relation_word: &'static str,
    pub referent: Option<String>,
    /// The deterministic grammar realization — the always-available surface.
    pub surface: String,
    /// `true` if the director put these words in the speaker's mouth (a `Voice` beat).
    pub forced: bool,
}

/// The dialogue manager: memory, the seeded variety stream, the grammar, and the log.
#[derive(Resource)]
pub struct Dialogue {
    rng: SplitMix64,
    mem: HashMap<Entity, MemLog>,
    grammar: Grammar,
    /// Utterances the director has forced this tick (its `Voice` beats), drained by `converse`.
    forced: Vec<(Entity, Entity, String)>,
    /// The conversation, in order — `(tick, speaker, listener, intent, surface)` and more.
    pub log: Vec<Utterance>,
}

impl Dialogue {
    pub fn seeded(seed: u64) -> Self {
        Self { rng: SplitMix64::new(seed), mem: HashMap::new(), grammar: Grammar::bundled(), forced: Vec::new(), log: Vec::new() }
    }

    /// Queue an utterance the director is putting in `speaker`'s mouth (see `Effect::Voice`).
    pub fn force(&mut self, speaker: Entity, listener: Entity, intent: String) {
        self.forced.push((speaker, listener, intent));
    }

    /// Compose the grammar surface for an utterance (disjoint field borrow of grammar+rng).
    fn render(&mut self, act: SpeechAct, affect: &str, referent: &Option<String>, speaker: &str, listener: &str) -> String {
        self.grammar.realize(act, affect, referent, speaker, listener, &mut self.rng)
    }

    /// Salient shared history between `who` and `other`, `0..1` — the `SharedHistory` axis.
    fn shared_history(&self, who: Entity, other: Entity) -> f32 {
        let mass: f32 = self
            .mem
            .get(&who)
            .map(|l| l.records.iter().filter(|r| r.partner == other).map(|r| r.strength).sum())
            .unwrap_or(0.0);
        mass / (mass + 2.0)
    }
}

impl Default for Dialogue {
    fn default() -> Self {
        Self::seeded(0)
    }
}

// --- Numbers → words: the binning the surface and the SLM card read ---

/// The speaker's standing toward the listener, as a phrase (opinion `-1..1`).
fn relation_word(op: f32) -> &'static str {
    match op {
        x if x < -0.4 => "loathes",
        x if x < -0.1 => "resents",
        x if x < 0.1 => "is wary of",
        x if x < 0.4 => "warms to",
        _ => "is devoted to",
    }
}

/// The speaker's dominant feeling, as an adjective and a grammar affect bucket.
struct Affect {
    word: &'static str,
    bucket: &'static str,
}

struct MoodWords {
    // (mood id, adjective for the SLM card, grammar affect bucket). Sized for the table in
    // `resolve` below — keep the length ≥ that table's row count.
    ids: [(usize, &'static str, &'static str); 24],
    n: usize,
}

impl MoodWords {
    fn resolve(reg: &Registry) -> Self {
        // The bucket keys an optional `act/affect` grammar list; a missing list falls back to
        // the bare act (see `Grammar::realize`), so a mood may colour speech here without every
        // act authoring a line for it. Moods absent from the registry are skipped, so this stays
        // robust to content edits. Several feelings share a bucket on purpose — foreboding reads
        // as dread, nostalgia as longing, elation/gratitude as warmth.
        let table = [
            ("anger", "seething", "angry"),
            ("sorrow", "grieving", "grieving"),
            ("fear", "frightened", "afraid"),
            ("joy", "glad", "warm"),
            ("love", "tender", "warm"),
            ("hope", "hopeful", "warm"),
            ("awe", "awestruck", "calm"),
            ("calm", "calm", "calm"),
            // ── the achlydesan registers reach the surface too (the dream-purgatory's weather) ──
            ("dread", "dreading", "dreading"),
            ("foreboding", "uneasy", "dreading"),
            ("despair", "despairing", "despairing"),
            ("rapture", "enraptured", "enraptured"),
            ("longing", "yearning", "longing"),
            ("nostalgia", "wistful", "longing"),
            ("contempt", "contemptuous", "contemptuous"),
            ("guilt", "guilt-heavy", "guilty"),
            ("loneliness", "lonely", "lonely"),
            ("envy", "envious", "envious"),
            ("restlessness", "restless", "restless"),
            ("elation", "elated", "warm"),
            ("gratitude", "grateful", "warm"),
        ];
        let mut ids = [(0usize, "", ""); 24];
        let mut n = 0;
        for (name, adj, bucket) in table {
            if let Some(id) = reg.mood_id(name) {
                ids[n] = (id, adj, bucket);
                n += 1;
            }
        }
        MoodWords { ids, n }
    }

    fn dominant(&self, moods: &[f32]) -> Affect {
        let mut best = 0.15f32; // a feeling has to be felt to colour speech
        let mut out = Affect { word: "even", bucket: "plain" };
        for &(id, word, bucket) in &self.ids[..self.n] {
            let v = moods.get(id).copied().unwrap_or(0.0);
            if v > best {
                best = v;
                out = Affect { word, bucket };
            }
        }
        out
    }
}

/// The speaker's loudest motives, as adjectives (traits above a threshold).
fn motive_words(reg: &Registry, traits: &[f32]) -> SmallVec<[&'static str; 4]> {
    let table = [
        ("vengeance", "vengeful"),
        ("ambition", "ambitious"),
        ("greed", "grasping"),
        ("piety", "devout"),
        ("sociability", "warm-hearted"),
        ("forgiveness", "forgiving"),
        ("caution", "wary"),
        ("contentment", "content"),
        // ── achlydesan motives — a soul's standing toward its own captivity, and who it is ──
        ("gnosis", "awakened"),
        ("oblivion", "dream-drowned"),
        ("defiance", "defiant"),
        ("submission", "yielding"),
        ("devotion", "fervent"),
        ("curiosity", "curious"),
        ("wanderlust", "restless-footed"),
        ("cruelty", "cruel"),
        ("compassion", "tender-hearted"),
        ("pride", "proud"),
        ("zeal", "zealous"),
    ];
    let mut out: SmallVec<[(&'static str, f32); 8]> = SmallVec::new();
    for (name, word) in table {
        if let Some(id) = reg.trait_id(name)
            && let Some(&v) = traits.get(id)
            && v > 0.55
        {
            out.push((word, v));
        }
    }
    out.sort_by(|a, b| b.1.total_cmp(&a.1));
    out.into_iter().take(2).map(|(w, _)| w).collect()
}

/// A stable epithet for an entity — deterministic, so a soul is named the same all game.
fn name_of(grammar: &Grammar, e: Entity) -> String {
    let names = grammar.0.get("name").map(Vec::as_slice).unwrap_or(&[]);
    if names.is_empty() {
        format!("one-{}", e.to_bits())
    } else {
        names[(e.to_bits() as usize) % names.len()].clone()
    }
}

/// A living person's qualities, gathered owned so scoring is borrow-free (mirrors the director).
struct Cand {
    e: Entity,
    tile: usize,
    pos: Coord,
    traits: Vec<f32>,
    moods: Vec<f32>,
    op: HashMap<Entity, f32>,
    grudge: Option<Entity>,
}

/// The per-tick conversation loop: co-located souls speak when they have something worth
/// saying, the words emerging from who they are and what lies between them.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(crate) fn converse(
    mut commands: Commands,
    substrate: Res<Substrate>,
    cfg: Res<DialogueRes>,
    book: Res<IntentBook>,
    reg: Res<Registry>,
    director: Option<Res<crate::director::Director>>,
    mut dlg: ResMut<Dialogue>,
    mut people: Query<(Entity, &Position, &mut Personality, &mut Mood, &mut Opinion, &Needs, Option<&Grievance>), With<Npc>>,
    // Off-by-default Chronicle: a conversation that breeds a grudge is a story seed; recorded here.
    mut chronicle: Option<ResMut<crate::chronicle::Chronicle>>,
) {
    if !cfg.enabled {
        return;
    }
    let tick = substrate.0.tick();
    let moods_w = MoodWords::resolve(&reg);

    // --- Read pass: owned snapshot of every living soul, plus a tile index for proximity.
    let mut cands: Vec<Cand> = Vec::new();
    {
        let topo = substrate.0.topology();
        for (e, pos, pers, mood, op, _needs, gr) in people.iter() {
            cands.push(Cand {
                e,
                tile: topo.index_of(pos.0),
                pos: pos.0,
                traits: pers.0.clone(),
                moods: mood.0.clone(),
                op: op.0.clone(),
                grudge: gr.map(|g| g.0),
            });
        }
    }
    let idx_of: HashMap<Entity, usize> = cands.iter().enumerate().map(|(i, c)| (c.e, i)).collect();

    // --- Forget a little, everywhere (Ebbinghaus): trivia fades, the reopened wound stays.
    for log in dlg.mem.values_mut() {
        for r in &mut log.records {
            r.strength = (r.strength - cfg.forget_rate).max(0.0);
        }
        log.records.retain(|r| r.strength > 0.02);
    }

    // --- Drain the director's forced utterances first (its `Voice` beats are heard).
    let forced: Vec<(Entity, Entity, String)> = std::mem::take(&mut dlg.forced);
    let mut chosen: Vec<(Entity, Entity, usize)> = Vec::new(); // (speaker, listener, intent index)
    let mut forced_flag: Vec<bool> = Vec::new();
    for (sp, li, intent) in forced {
        if let Some(bi) = book.0.iter().position(|i| i.id == intent)
            && idx_of.contains_key(&sp)
            && idx_of.contains_key(&li)
        {
            chosen.push((sp, li, bi));
            forced_flag.push(true);
        }
    }

    // --- Emergent conversations: each soul, in turn, may speak to a co-located other.
    for ci in 0..cands.len() {
        let speaker = cands[ci].e;
        // Cadence: don't let the same mouth run every tick.
        if let Some(log) = dlg.mem.get(&speaker)
            && let Some(last) = log.last_spoke
            && tick.saturating_sub(last) < cfg.cooldown
        {
            continue;
        }
        let stile = cands[ci].tile;
        // Score every (listener, intent) and keep the most appealing thing worth saying.
        let mut best: Option<(f32, Entity, usize)> = None;
        for cj in 0..cands.len() {
            if cj == ci || cands[cj].tile != stile {
                continue;
            }
            let listener = cands[cj].e;
            let op = cands[ci].op.get(&listener).copied().unwrap_or(0.0);
            let grudge = if cands[ci].grudge == Some(listener) { 1.0 } else { 0.0 };
            let shared = dlg.shared_history(speaker, listener);
            let prom = director.as_ref().map(|d| (d.prominence_of(listener) / 4.0).min(1.0)).unwrap_or(0.0);
            for (bi, intent) in book.0.iter().enumerate() {
                let raw = ai::score(&intent.appeal, |input| match input {
                    Input::Deficit => 0.0,
                    Input::Trait(t) => cands[ci].traits.get(t).copied().unwrap_or(0.0),
                    Input::Mood(m) => cands[ci].moods.get(m).copied().unwrap_or(0.0),
                    Input::Sanction => 0.0,
                    Input::OpinionOf => (op + 1.0) * 0.5,
                    Input::GrievanceAgainst => grudge,
                    Input::SharedHistory => shared,
                    Input::Prominence => prom,
                });
                // Anti-repetition: damp an act just said to this listener.
                let echo = dlg
                    .mem
                    .get(&speaker)
                    .map(|l| l.records.iter().rev().take(3).any(|r| r.partner == listener && r.act == intent.act))
                    .unwrap_or(false);
                let score = raw * intent.weight * if echo { cfg.echo_penalty } else { 1.0 };
                if score >= cfg.appeal_floor && best.is_none_or(|(b, _, _)| score > b) {
                    best = Some((score, listener, bi));
                }
            }
        }
        if let Some((_, listener, bi)) = best {
            chosen.push((speaker, listener, bi));
            forced_flag.push(false);
        }
    }

    // --- Tell each chosen utterance: apply its moves, remember it, and render the words.
    for (k, (speaker, listener, bi)) in chosen.into_iter().enumerate() {
        let intent = &book.0[bi];
        let forced = forced_flag[k];
        let Some(&si) = idx_of.get(&speaker) else { continue };

        // Apply the social consequence (the canon — deterministic, in the tick).
        let mut made_grudge = false;
        for mv in &intent.moves {
            match mv {
                Move::Turn { who, toward, delta } => {
                    let (w, t) = (party(*who, speaker, listener), party(*toward, speaker, listener));
                    if w != t
                        && let Ok((.., mut op, _, _)) = people.get_mut(w)
                    {
                        let e = op.0.entry(t).or_insert(0.0);
                        *e = (*e + delta).clamp(-1.0, 1.0);
                    }
                }
                Move::Stir { who, mood, delta } => {
                    let w = party(*who, speaker, listener);
                    if let Some(mid) = reg.mood_id(mood)
                        && let Ok((.., mut m, _, _, _)) = people.get_mut(w)
                        && let Some(v) = m.0.get_mut(mid)
                    {
                        *v = (*v + delta).clamp(0.0, 1.0);
                    }
                }
                Move::Sway { who, trait_name, delta } => {
                    let w = party(*who, speaker, listener);
                    if let Some(tid) = reg.trait_id(trait_name)
                        && let Ok((.., mut pers, _, _, _, _)) = people.get_mut(w)
                        && let Some(v) = pers.0.get_mut(tid)
                    {
                        *v = (*v + delta).clamp(0.0, 1.0);
                    }
                }
                Move::Grudge { who, against } => {
                    let (w, a) = (party(*who, speaker, listener), party(*against, speaker, listener));
                    if w != a {
                        commands.entity(w).insert(Grievance(a));
                        made_grudge = true;
                        if let Some(c) = chronicle.as_deref_mut() {
                            c.record(tick, EpisodeKind::GrievanceFormed, [Some(w), Some(a), None], cands[si].pos, None, 0);
                        }
                    }
                }
            }
        }

        // Assemble the card (numbers → words) and choose a referent from memory.
        let op_sl = cands[si].op.get(&listener).copied().unwrap_or(0.0);
        let affect = moods_w.dominant(&cands[si].moods);
        let register = intent.tags.first().cloned().unwrap_or_else(|| intent.act.key().to_string());
        let referent = pick_referent(&dlg, speaker, listener, &register, cands[si].grudge == Some(listener));
        let speaker_name = name_of(&dlg.grammar, speaker);
        let listener_name = name_of(&dlg.grammar, listener);
        let motive = motive_words(&reg, &cands[si].traits);

        // Render the deterministic grammar surface.
        let surface = dlg.render(intent.act, affect.bucket, &referent, &speaker_name, &listener_name);

        // Remember it, for both souls (importance from the social weight it carried).
        let importance = (intent.weight + if made_grudge { 0.6 } else { 0.0 }).min(2.0);
        remember(&mut dlg, speaker, listener, intent.act, &register, importance, cfg.memory_cap);
        remember(&mut dlg, listener, speaker, intent.act, &register, importance * 0.8, cfg.memory_cap);
        dlg.mem.entry(speaker).or_default().last_spoke = Some(tick);

        dlg.log.push(Utterance {
            tick,
            speaker,
            listener,
            intent: intent.id.clone(),
            act: intent.act,
            speaker_name,
            listener_name,
            motive,
            mood_word: affect.word,
            relation_word: relation_word(op_sl),
            referent,
            surface,
            forced,
        });
    }
}

fn party(p: Party, speaker: Entity, listener: Entity) -> Entity {
    match p {
        Party::Speaker => speaker,
        Party::Listener => listener,
    }
}

/// Pick the most salient memory of the listener as the thing to bring up — mood-congruent,
/// recency-weighted; a standing grudge is the loudest referent of all.
fn pick_referent(dlg: &Dialogue, speaker: Entity, listener: Entity, register: &str, has_grudge: bool) -> Option<String> {
    if has_grudge {
        return Some("the wrong between us".to_string());
    }
    let log = dlg.mem.get(&speaker)?;
    let best = log
        .records
        .iter()
        .filter(|r| r.partner == listener)
        .max_by(|a, b| {
            let sa = a.strength * if a.register == register { 1.5 } else { 1.0 };
            let sb = b.strength * if b.register == register { 1.5 } else { 1.0 };
            sa.total_cmp(&sb)
        })?;
    Some(format!("what passed between us ({})", best.register))
}

fn remember(dlg: &mut Dialogue, who: Entity, partner: Entity, act: SpeechAct, register: &str, importance: f32, cap: usize) {
    // Recall reinforces (spaced repetition): bump an existing record with this partner+act.
    let log = dlg.mem.entry(who).or_default();
    if let Some(r) = log.records.iter_mut().find(|r| r.partner == partner && r.act == act) {
        r.strength = (r.strength + 0.5).min(3.0);
        return;
    }
    log.records.push(MemRecord { tick: 0, partner, act, register: register.to_string(), importance, strength: 1.0 });
    if log.records.len() > cap {
        // Forget the weakest.
        let weakest = log.records.iter().enumerate().min_by(|a, b| a.1.strength.total_cmp(&b.1.strength)).map(|(i, _)| i);
        if let Some(i) = weakest {
            log.records.remove(i);
        }
    }
}

// =====================================================================================
// Surface layer A — the generative grammar (deterministic, whole-population, the floor)
// =====================================================================================

/// A Tracery-style generative grammar: a map from symbol to productions. A production may
/// recurse into `#another_symbol#` and substitute `{slots}` from the utterance plan. The
/// *words* are composed from rules, never selected from a phrasebook.
#[derive(Clone, Debug, Default)]
pub struct Grammar(HashMap<String, Vec<String>>);

impl Grammar {
    pub fn bundled() -> Self {
        Self::from_ron(Bundled::get(Asset::Grammar)).expect("bundled grammar is valid RON")
    }

    pub fn from_ron(ron: &str) -> Result<Self, String> {
        config::parse(ron).map(Grammar).map_err(|e| e.to_string())
    }

    /// Compose a line for an act + affect, filling the listener/speaker/referent slots.
    /// Tries the `act/affect` symbol, falling back to the bare `act`.
    fn realize(
        &self,
        act: SpeechAct,
        affect: &str,
        referent: &Option<String>,
        speaker: &str,
        listener: &str,
        rng: &mut SplitMix64,
    ) -> String {
        let keyed = format!("{}/{}", act.key(), affect);
        let root = if self.0.contains_key(&keyed) { keyed } else { act.key().to_string() };
        let slots = |s: &str| -> Option<String> {
            match s {
                "speaker" => Some(speaker.to_string()),
                "listener" => Some(listener.to_string()),
                "referent" => Some(referent.clone().unwrap_or_else(|| "it".to_string())),
                _ => None,
            }
        };
        let out = self.expand(&root, &slots, rng, 0);
        // Tidy: capitalize, collapse spaces.
        let mut s = out.split_whitespace().collect::<Vec<_>>().join(" ");
        if let Some(c) = s.get_mut(0..1) {
            c.make_ascii_uppercase();
        }
        s
    }

    fn expand(&self, symbol: &str, slots: &impl Fn(&str) -> Option<String>, rng: &mut SplitMix64, depth: u8) -> String {
        if depth > 8 {
            return String::new();
        }
        if let Some(v) = slots(symbol) {
            return v;
        }
        let Some(prods) = self.0.get(symbol) else {
            return String::new();
        };
        if prods.is_empty() {
            return String::new();
        }
        let pick = (rng.next_u64() as usize) % prods.len();
        let template = &prods[pick];
        let mut out = String::new();
        let mut rest = template.as_str();
        while let Some(start) = rest.find('#') {
            out.push_str(&rest[..start]);
            let after = &rest[start + 1..];
            if let Some(end) = after.find('#') {
                let sym = &after[..end];
                out.push_str(&self.expand(sym, slots, rng, depth + 1));
                rest = &after[end + 1..];
            } else {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
        out.push_str(rest);
        out
    }
}

// =====================================================================================
// Surface layer B — the SLM realizer (optional, out of band, never load-bearing)
// =====================================================================================

/// A pluggable text generator — the seam a small on-device model plugs into. The host app
/// supplies the implementation (e.g. a `candle`/`llama.cpp` adapter); the simulation never
/// depends on it, so a build without one is byte-identical. See `docs/dialogue.md` §4b.
pub trait TextGen {
    /// Generate a line from a prompt, deterministically given the seed (so a replay can be
    /// reproduced). The seed is derived from the utterance's canonical state hash.
    fn generate(&self, prompt: &str, seed: u64) -> String;
}

/// Build the prompt the SLM rephrases from — the character card assembled from the
/// already-grounded utterance (numbers already turned to words; the model only voices it).
pub fn build_prompt(u: &Utterance) -> String {
    let motive = if u.motive.is_empty() { "unremarkable".to_string() } else { u.motive.join(", ") };
    let referent = u.referent.as_deref().unwrap_or("nothing in particular");
    format!(
        "You are {name}, a {motive} person. You feel {mood}. You {relation} {listener}. \
         You are about to {act} them, on the subject of {referent}. \
         Say one line, in character. Do not invent facts you were not given.\n\
         Draft: \"{draft}\"\n{name}:",
        name = u.speaker_name,
        motive = motive,
        mood = u.mood_word,
        relation = u.relation_word,
        listener = u.listener_name,
        act = u.act.key(),
        referent = referent,
        draft = u.surface,
    )
}

/// A small canonical hash of an utterance's *meaning* — the cache key, so a given exchange
/// renders the same words across runs and machines (reproducible display).
pub fn state_hash(u: &Utterance) -> u64 {
    // FNV-1a over the grounded fields (not the rendered surface — that's the output).
    let mut h: u64 = 0xcbf29ce484222325;
    let mut feed = |bytes: &[u8]| {
        for &b in bytes {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    };
    feed(u.intent.as_bytes());
    feed(u.speaker_name.as_bytes());
    feed(u.listener_name.as_bytes());
    feed(u.mood_word.as_bytes());
    feed(u.relation_word.as_bytes());
    feed(u.referent.as_deref().unwrap_or("").as_bytes());
    h
}

/// The optional SLM surface: cache by canonical state hash, generate on a miss, and fall
/// back to the grammar surface if the model is absent or its line fails a sanity check.
/// Runs **out of band** (foreground conversation only) — never inside the sim tick.
pub struct SlmRealizer<G: TextGen> {
    model: G,
    cache: HashMap<u64, String>,
}

impl<G: TextGen> SlmRealizer<G> {
    pub fn new(model: G) -> Self {
        Self { model, cache: HashMap::new() }
    }

    /// Render one foreground utterance with the model, cached and grammar-guarded.
    pub fn realize(&mut self, u: &Utterance) -> String {
        let key = state_hash(u);
        if let Some(hit) = self.cache.get(&key) {
            return hit.clone();
        }
        let line = self.model.generate(&build_prompt(u), key);
        let line = line.trim();
        // Guard: a model that returns nothing, or a wall of text, gets the grammar floor.
        let chosen = if line.is_empty() || line.len() > 240 { u.surface.clone() } else { line.to_string() };
        self.cache.insert(key, chosen.clone());
        chosen
    }
}

// =====================================================================================
// The player-avatar path (v2) — the player speaks with the SAME intent vocabulary
// =====================================================================================

/// Score the intents `speaker` would address to `listener`, most appealing first — the
/// emergent "what would this person, feeling this way about you, want to say?". This is the
/// **NPC** path (an NPC is its own mind); the player's path is [`repertoire`], which does
/// not score, because the *player* is the avatar's mind.
pub fn available(world: &World, speaker: Entity, listener: Entity) -> Vec<(String, f32)> {
    let book = world.resource::<IntentBook>();
    let reg = world.resource::<Registry>();
    let Some(traits) = world.get::<Personality>(speaker).map(|p| p.0.clone()) else { return Vec::new() };
    let moods = world.get::<Mood>(speaker).map(|m| m.0.clone()).unwrap_or_default();
    let op = world.get::<Opinion>(speaker).map(|o| o.of(listener)).unwrap_or(0.0);
    let grudge = if world.get::<Grievance>(speaker).is_some_and(|g| g.0 == listener) { 1.0 } else { 0.0 };
    let shared = world.resource::<Dialogue>().shared_history(speaker, listener);
    let prom = world.get_resource::<crate::director::Director>().map(|d| (d.prominence_of(listener) / 4.0).min(1.0)).unwrap_or(0.0);
    let _ = reg;
    let mut out: Vec<(String, f32)> = book
        .0
        .iter()
        .map(|intent| {
            let s = ai::score(&intent.appeal, |input| match input {
                Input::Deficit | Input::Sanction => 0.0,
                Input::Trait(t) => traits.get(t).copied().unwrap_or(0.0),
                Input::Mood(m) => moods.get(m).copied().unwrap_or(0.0),
                Input::OpinionOf => (op + 1.0) * 0.5,
                Input::GrievanceAgainst => grudge,
                Input::SharedHistory => shared,
                Input::Prominence => prom,
            }) * intent.weight;
            (intent.id.clone(), s)
        })
        .collect();
    out.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    out
}

/// The full repertoire of conversational verbs the **player** may choose from — every
/// authored intent, in authored order. The player is the avatar's mind (this is a
/// role-playing game): unlike an NPC, the player is *not* scored on what it "wants" to say
/// ([`available`]); it is simply offered the verbs and chooses the meaning. So the avatar
/// needs no traits, mood, or opinion to speak — the choosing is the human's.
pub fn repertoire(world: &World) -> Vec<String> {
    world.resource::<IntentBook>().0.iter().map(|i| i.id.clone()).collect()
}

/// `speaker` answers `listener` with its most-appealing intent that clears the appeal floor
/// — the same scoring and machinery as emergent speech, enacted now (an NPC replying to the
/// player who just addressed it). `None` if it has nothing worth saying. Cadence/cooldown
/// is ignored: the soul was spoken to, and answers.
pub fn reply(world: &mut World, speaker: Entity, listener: Entity) -> Option<Utterance> {
    let floor = world.resource::<DialogueRes>().appeal_floor;
    let (id, score) = available(world, speaker, listener).into_iter().next()?;
    if score < floor {
        return None;
    }
    perform(world, speaker, listener, &id)
}

/// The stable epithet an entity is known by in conversation — matches the dialogue log.
pub fn display_name(world: &World, e: Entity) -> String {
    name_of(&world.resource::<Dialogue>().grammar, e)
}

/// Apply just the social consequence of a conversational intent — the deterministic [`Move`]s
/// from `speaker` to `listener` (Turn/Stir/Sway/Grudge), mutating Opinion/Mood/Personality/
/// Grievance. No surface is rendered or remembered. Shared by [`perform`] and the player path
/// ([`Simulation::apply_conversational_intent`](crate::Simulation::apply_conversational_intent))
/// so free-text talk can move the social state through the *same* authored, deterministic
/// effects. Returns whether a grudge was made.
pub fn apply_moves(world: &mut World, speaker: Entity, listener: Entity, moves: &[Move]) -> bool {
    apply_moves_scaled(world, speaker, listener, moves, 1.0)
}

/// Apply an intent's [`Move`]s with their opinion/mood/trait deltas multiplied by `scale` — how
/// strongly the words land (e.g. the result of a speaker's persuasion-skill check). `scale == 1.0`
/// is the unscaled canon, so every existing caller (and a world without the RPG layer) is
/// byte-identical; `0.0` means the words move nothing and no grudge forms.
pub fn apply_moves_scaled(world: &mut World, speaker: Entity, listener: Entity, moves: &[Move], scale: f32) -> bool {
    let mut made_grudge = false;
    for mv in moves {
        match mv {
            Move::Turn { who, toward, delta } => {
                let (w, t) = (party(*who, speaker, listener), party(*toward, speaker, listener));
                if w != t
                    && let Some(mut op) = world.get_mut::<Opinion>(w)
                {
                    let e = op.0.entry(t).or_insert(0.0);
                    *e = (*e + delta * scale).clamp(-1.0, 1.0);
                }
            }
            Move::Stir { who, mood, delta } => {
                let w = party(*who, speaker, listener);
                let mid = world.resource::<Registry>().mood_id(mood);
                if let Some(mid) = mid
                    && let Some(mut m) = world.get_mut::<Mood>(w)
                    && let Some(v) = m.0.get_mut(mid)
                {
                    *v = (*v + delta * scale).clamp(0.0, 1.0);
                }
            }
            Move::Sway { who, trait_name, delta } => {
                let w = party(*who, speaker, listener);
                let tid = world.resource::<Registry>().trait_id(trait_name);
                if let Some(tid) = tid
                    && let Some(mut p) = world.get_mut::<Personality>(w)
                    && let Some(v) = p.0.get_mut(tid)
                {
                    *v = (*v + delta * scale).clamp(0.0, 1.0);
                }
            }
            Move::Grudge { who, against } => {
                let (w, a) = (party(*who, speaker, listener), party(*against, speaker, listener));
                if scale > 0.0 && w != a {
                    world.entity_mut(w).insert(Grievance(a));
                    made_grudge = true;
                    // The player is a part of the world: a grudge the avatar's words breed is
                    // recorded into the Chronicle, exactly as the emergent `converse` path records an
                    // NPC-bred one — so the sifter (and the director graft) perceive the *player's*
                    // own deeds too, not just the NPCs'. (This `apply_moves_scaled` is the immediate
                    // path, taken by the player's talk action and any direct caller; the schedule's
                    // `converse` system has its own tap, so there is no double-counting.) A no-op
                    // when the sift layer is off — no Chronicle resource — so it stays byte-identical.
                    let at = world.get::<Position>(speaker).map(|p| p.0);
                    let tick = world.resource::<Substrate>().0.tick();
                    if let Some(at) = at
                        && let Some(mut chron) = world.get_resource_mut::<Chronicle>()
                    {
                        chron.record(tick, EpisodeKind::GrievanceFormed, [Some(w), Some(a), None], at, None, 0);
                    }
                }
            }
        }
    }
    made_grudge
}

/// Enact one utterance immediately, at full strength — see [`perform_scaled`].
pub fn perform(world: &mut World, speaker: Entity, listener: Entity, intent_id: &str) -> Option<Utterance> {
    perform_scaled(world, speaker, listener, intent_id, 1.0)
}

/// Enact one utterance immediately — the player-avatar speaking, or any caller that wants the
/// result now rather than on the next `converse` tick. Applies the intent's moves (the
/// deterministic social consequence) **scaled by `scale`** — how strongly the words land, e.g.
/// the result of the speaker's persuasion check — then renders the surface, records the memory
/// for both souls, logs it, and returns it. `scale == 1.0` is the unscaled canon. Uses the
/// *same* machinery as emergent speech.
pub fn perform_scaled(world: &mut World, speaker: Entity, listener: Entity, intent_id: &str, scale: f32) -> Option<Utterance> {
    let bi = world.resource::<IntentBook>().0.iter().position(|i| i.id == intent_id)?;
    let intent = world.resource::<IntentBook>().0[bi].clone();
    let tick = world.resource::<Substrate>().0.tick();

    // Phase 1 — the speaker's pre-utterance snapshot, turned to words. The speaker may be
    // the player's avatar, which carries no traits or mood of its own — the player is its
    // mind. Absent components read as neutral: no fixed motive, an even affect. (The player
    // is offered the verbs and chooses; the sim never scores what the avatar "wants" to say.)
    let s_traits = world.get::<Personality>(speaker).map(|p| p.0.clone()).unwrap_or_default();
    let s_moods = world.get::<Mood>(speaker).map(|m| m.0.clone()).unwrap_or_default();
    let op_sl = world.get::<Opinion>(speaker).map(|o| o.of(listener)).unwrap_or(0.0);
    let has_grudge = world.get::<Grievance>(speaker).is_some_and(|g| g.0 == listener);
    let (affect_word, affect_bucket, motive, speaker_name, listener_name, referent, register) = {
        let reg = world.resource::<Registry>();
        let affect = MoodWords::resolve(reg).dominant(&s_moods);
        let motive = motive_words(reg, &s_traits);
        let register = intent.tags.first().cloned().unwrap_or_else(|| intent.act.key().to_string());
        let dlg = world.resource::<Dialogue>();
        let referent = pick_referent(dlg, speaker, listener, &register, has_grudge);
        (affect.word, affect.bucket, motive, name_of(&dlg.grammar, speaker), name_of(&dlg.grammar, listener), referent, register)
    };

    // Phase 2 — apply the moves (the canon consequence; mirrors `converse`), scaled by how
    // strongly the speaker's words land (the persuasion check; `1.0` = unscaled).
    let made_grudge = apply_moves_scaled(world, speaker, listener, &intent.moves, scale);

    // Phase 3 — render, remember, log.
    let cap = world.resource::<DialogueRes>().memory_cap;
    let utt = world.resource_scope::<Dialogue, Utterance>(|_w, mut dlg| {
        let surface = dlg.render(intent.act, affect_bucket, &referent, &speaker_name, &listener_name);
        let importance = (intent.weight + if made_grudge { 0.6 } else { 0.0 }).min(2.0);
        remember(&mut dlg, speaker, listener, intent.act, &register, importance, cap);
        remember(&mut dlg, listener, speaker, intent.act, &register, importance * 0.8, cap);
        dlg.mem.entry(speaker).or_default().last_spoke = Some(tick);
        let u = Utterance {
            tick,
            speaker,
            listener,
            intent: intent.id.clone(),
            act: intent.act,
            speaker_name: speaker_name.clone(),
            listener_name: listener_name.clone(),
            motive: motive.clone(),
            mood_word: affect_word,
            relation_word: relation_word(op_sl),
            referent: referent.clone(),
            surface,
            forced: false,
        };
        dlg.log.push(u.clone());
        u
    });
    Some(utt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_intents_and_grammar_load() {
        let book = IntentBook::bundled();
        assert!(book.0.len() >= 8, "the intent repertoire should be stocked, got {}", book.0.len());
        let g = Grammar::bundled();
        assert!(g.0.contains_key("accuse"), "the grammar needs at least the core acts");
        assert!(g.0.get("name").is_some_and(|n| !n.is_empty()), "the grammar needs a name list");
    }

    #[test]
    fn moves_scale_their_deltas() {
        // The persuasion-strength seam: a Turn move shifts opinion by `delta * scale`.
        let mut w = World::new();
        let speaker = w.spawn_empty().id();
        let listener = w.spawn(Opinion(Default::default())).id();
        let mv = [Move::Turn { who: Party::Listener, toward: Party::Speaker, delta: 0.4 }];

        apply_moves_scaled(&mut w, speaker, listener, &mv, 0.5);
        assert!((w.get::<Opinion>(listener).unwrap().of(speaker) - 0.2).abs() < 1e-6, "0.4 × 0.5");
        // A failed persuasion (scale 0) moves nothing further.
        apply_moves_scaled(&mut w, speaker, listener, &mv, 0.0);
        assert!((w.get::<Opinion>(listener).unwrap().of(speaker) - 0.2).abs() < 1e-6, "no change at scale 0");
        // Full strength is the unscaled canon, identical to `apply_moves`.
        apply_moves_scaled(&mut w, speaker, listener, &mv, 1.0);
        assert!((w.get::<Opinion>(listener).unwrap().of(speaker) - 0.6).abs() < 1e-6, "0.2 + 0.4");
    }

    #[test]
    fn an_unknown_mood_in_an_intent_is_rejected() {
        let ron = r#"[( id: "bad", act: Accuse, appeal: [(input: Mood("anger"), curve: Linear(m: 1.0, b: 0.0))],
            moves: [Stir(who: Speaker, mood: "ragemaxxing", delta: 0.1)] )]"#;
        assert!(IntentBook::from_ron(ron, &Registry::bundled()).is_err());
    }

    #[test]
    fn the_grammar_composes_and_is_deterministic() {
        let g = Grammar::bundled();
        let mut a = SplitMix64::new(7);
        let mut b = SplitMix64::new(7);
        let r = Some("the broken oath".to_string());
        let l1 = g.realize(SpeechAct::Accuse, "angry", &r, "Aldric", "Mira", &mut a);
        let l2 = g.realize(SpeechAct::Accuse, "angry", &r, "Aldric", "Mira", &mut b);
        assert_eq!(l1, l2, "same seed → same line");
        assert!(!l1.is_empty() && l1.contains("Mira"), "the line should address the listener: {l1:?}");
    }

    #[test]
    fn the_slm_seam_caches_and_falls_back() {
        // A fake generator stands in for an on-device model (the real seam is host-supplied).
        struct Loud;
        impl TextGen for Loud {
            fn generate(&self, _p: &str, _s: u64) -> String {
                "I will not forget what you did.".to_string()
            }
        }
        struct Empty;
        impl TextGen for Empty {
            fn generate(&self, _p: &str, _s: u64) -> String {
                String::new()
            }
        }
        let u = Utterance {
            tick: 1,
            speaker: Entity::PLACEHOLDER,
            listener: Entity::PLACEHOLDER,
            intent: "wound".into(),
            act: SpeechAct::Accuse,
            speaker_name: "Aldric".into(),
            listener_name: "Mira".into(),
            motive: SmallVec::new(),
            mood_word: "seething",
            relation_word: "resents",
            referent: Some("the broken oath".into()),
            surface: "You broke your oath, Mira.".into(),
            forced: false,
        };
        let mut loud = SlmRealizer::new(Loud);
        assert_eq!(loud.realize(&u), "I will not forget what you did.");
        assert_eq!(loud.realize(&u), "I will not forget what you did.", "second call is cached");
        // A model that returns nothing falls back to the deterministic grammar surface.
        let mut empty = SlmRealizer::new(Empty);
        assert_eq!(empty.realize(&u), "You broke your oath, Mira.");
    }
}
