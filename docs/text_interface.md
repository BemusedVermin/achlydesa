# The text interface (terminal front-end)

Status: **design** (2026-06-27). Companion to [`prose_generation.md`](prose_generation.md), which
covers how the world is put into words. This doc covers the *client*: a terminal program that
renders that prose, takes typed commands, and drives the (unchanged) simulation one turn at a time.

## What changes, and what does not

We are replacing the Bevy 3D front-end (`app`) with a **terminal, text-first client** -- a
Zork-like adventure over the living, MUD-like simulation. The defining constraint of the codebase
holds: **only the front-end is engine-specific.** `sim`, `config`, `game_sim`, `agent_core`,
`agents`, `rpg`, `party`, `survival`, `travel`, `explore` are untouched. `agents::Simulation`
remains the authoritative model; the new client is, like `app` was, "a thin **view** over the
authoritative sim" -- it just draws with characters instead of meshes.

Decisions taken (owner, 2026-06-27):

- **Replace `app` entirely.** The text client is the sole front-end; the Bevy `app` crate and the
  `voice` SLM crate are retired (see [Workspace changes](#workspace-changes)).
- **Terminal TUI with panels** (ratatui + crossterm): a main prose pane plus side panels for map,
  journal, and status. Runs in any terminal; ASCII-only output (the existing convention -- fancy
  glyphs render as tofu).
- **Action-driven turns.** The world ticks when the player acts; long actions (travel, rest)
  advance many ticks at once. This keeps the deterministic single-threaded schedule intact -- no
  wall-clock thread racing the sim.
- **Pure parser for input** (under active investigation -- see [Input](#input-a-pure-parser)). The
  player types verb-noun-prep commands, Zork-style.
- **Six cardinal directions.** The world is hex and the worldbuilding holds six cardinal
  directions; the compass and movement commands present six, and we do **not** reconcile them to
  four or eight. The direction *names* come from the (forthcoming) worldbuilding; until then use
  placeholders.

## Screen layout

A four-region terminal layout (ratatui `Layout` constraints), ASCII throughout:

```
+--------------------------------------------------+-------------------+
|                                                  |  MAP              |
|  PROSE  (scrollback)                             |   . . # # .       |
|                                                  |    . @ # . .      |
|  The market is half-shuttered today. A smith     |   . . . ~ ~       |
|  counts coppers twice before he lets them go;    |    . . ~ ~ .      |
|  his cloak has been turned at the collar. Two    |                   |
|  women break off talking as you pass.            +-------------------+
|                                                  |  STATUS           |
|  > examine smith                                 |   thirst  ##....   |
|                                                  |   warmth  ####..   |
|                                                  |   here: Ash Hamlet |
+--------------------------------------------------+-------------------+
|  FEED  the betrayed one was seen leaving by the north road ...       |
+---------------------------------------------------------------------+
```

- **Prose pane** (primary, scrollable): scene descriptions, command echoes, parser replies,
  conversation -- the Zork scrollback. This is where all of [`prose_generation.md`](prose_generation.md)
  surfaces.
- **Map panel**: an ASCII hex minimap of explored tiles, fog of war, the avatar `@`, terrain by
  glyph. See [The map](#the-map-and-six-directions).
- **Status panel**: survival vitals (thirst/warmth/stamina, when the survival layer is on), current
  place, and a one-line "read the room" demeanour.
- **Feed**: a thin event ticker for ambient/"while you were busy" events (see [World time](#world-time-action-driven)).

Panels are toggled/expanded by command (`map`, `journal`) so the prose can go full-width when
reading -- the diegetic interfaces below take over the screen when opened.

## Input: a pure parser

Decided (owner, 2026-06-27): **pure parser everywhere -- including conversation -- with no menus.**
Discoverability comes from **highlighted keywords in the prose** (Morrowind-style), not from action
lists. Here is the case, the hard part, and how it is handled.

### Why a pure parser fits

- It is the authentic Zork-like feel the owner is after: typed `examine smith`, `go north`,
  `take lantern`, `ask aldric about the feud`.
- It gives verb x noun combinatorics and the serendipity of a discovered verb -- which a menu of
  pre-listed actions cannot.
- We already sit close to it: the avatar is a *body in the world* choosing actions from an unscored
  repertoire, with no attributes of its own -- a deep world model with a thin action surface. A
  parser is just a different intent-capture skin on that same model.

### The hard part: noun resolution in a proc-gen world

In hand-authored IF, the parser resolves a typed noun against per-object **synonym lists** the
author wrote. Our world is generated -- the smith, the hamlet, the feud all have procedurally
minted names and epithets, not authored synonyms. So the parser must resolve a typed noun against
the **entities currently in scope**, using what the sim already knows about each:

- **Scope** = what the avatar can presently refer to: things at the current tile, adjacent tiles in
  view, the avatar's inventory, and recently-mentioned entities (a short anaphora buffer so "him" /
  "the smith" / "it" resolve to the last salient referent -- the same discourse model the prose
  layer maintains, shared).
- **Matching** = each in-scope entity exposes its generated name, its epithet(s) (the director's
  "the Betrayed"), and its kind word ("smith", "slate", "road"). The parser tokenises the noun
  phrase and matches against those, preferring the most salient on a tie, and **disambiguates by
  asking** ("Which smith -- Aldric or the young one?") rather than guessing. This is the
  Dale & Reiter referring-expression machinery run in reverse: the same true attributes that *name*
  an entity in prose are what the player *types* to refer to it.

This is the crux that makes a pure parser viable here, and it is genuinely tractable because the
entities already carry the data. It is also why "highlight the interactive nouns" (below) matters
so much: the player should be typing words the prose just showed them.

### Guess-the-verb, mitigated

The parser's classic failure is the player knowing *what* to do but not the word for it. The
modern IF mitigations (Emily Short) all apply, and we adopt them as requirements:

- **A small, fixed, well-signposted verb set** (below), with the currently-valid verbs always
  discoverable (a `verbs` command and a help line; optionally listed in a panel).
- **Highlight interactive keywords** in the prose (a distinct ASCII treatment, e.g. CAPS or
  bracketing within our tofu-safe palette) so the player types words the game already used. This is
  the load-bearing discovery mechanism (Morrowind-style): both interactable *nouns* and conversation
  *topics* surface as highlighted words embedded in the fiction, never as a menu.
- **A forgiving parser**: synonyms, abbreviations (`x` = examine, `l` = look, `i` = inventory,
  bare directions), tolerant of articles and filler; on a near-miss, *suggest* ("Did you mean
  EXAMINE?") rather than a flat "I don't understand."
- **Every input is juicy**: even a failed parse returns something true and in-voice, never a dead
  error.

### Verb set (exploration-led, combat de-emphasised)

The exploration spine is small; this is a feature, not a limitation.

- **Move**: `go <dir>` and the six bare directions; `enter`/`exit` (settlements, courts, ruins,
  POIs). One hex per turn off-road; travel can be multi-turn (below).
- **Perceive** (the heart of exploration): `look`/`l` (re-describe the tile), `examine`/`x <thing>`
  (the workhorse -- NPCs, features, readables, terrain), `search` (reveal hidden/secret features --
  the discovery verb), `read <slate/inscription>`.
- **Manipulate** (only where the world uses it): `take`/`drop`, `give <thing> to <npc>`,
  `use <thing>`; lootable bodies and gear tie into the (coming) combat/loot economy.
- **Body/survival**: `eat`, `drink`, `rest`, `wait`/`z`.
- **Converse**: `talk to <npc>`, then the conversation verbs below.
- **Diegetic/meta**: `map`, `journal` (+ `note <text>`), `recall`/`recap`, `verbs`, `help`,
  `save`, `load`.

### Conversation under a pure parser

This is where the owner's favourite thing -- interacting with everyone -- meets the parser's
weakest point (the "guess-the-topic" tax of ask/tell). The design keeps it typed *and* discoverable,
and keeps faith with the "emergent, no phrasebook" rule:

- `talk to <npc>` opens a conversation; the prose pane becomes the exchange.
- The player addresses **intents/topics as typed verbs** -- `accuse him`, `mourn`, `reconcile`,
  `ask about the feud`, `tell her about the road` -- parsed against (a) the avatar's available
  **intent repertoire** (`Simulation::player_intents`, the same unscored vocabulary the choice UI
  used) and (b) **known topics** (lore tokens, named entities, shared memories the avatar actually
  holds). Meaning is still the IAUS-scored intent system; the parser is only a new way to *pick* an
  intent, replacing the menu.
- To defeat guess-the-topic, **topics surface as highlighted keywords in the prose itself**
  (Morrowind-style, owner's call): when an NPC's line or a scene mentions something you can pursue --
  a feud, a failed harvest, a debt -- that word is highlighted, and typing it raises it as a topic.
  No menu; the discoverable topics are *embedded in the fiction you are already reading*. Only topics
  the avatar has narrative reason to know are ever highlighted (Eric Eve's rule), and a reply can
  reveal new keywords (Threaded Conversation's directly/indirectly-follows: an answer opens fresh
  topics). A `recall` command can re-list the keywords currently in play as a backstop, but the
  primary surface is the highlighted word in context.
- NPC replies are generated by the prose layer; the director's `Effect::Voice` betrayals are heard
  here, and every line is a public act others in earshot can react to (the Versu model the dialogue
  layer already matches).

This resolves the one real tension cleanly: a pure-parser conversation would otherwise replace the
*choice from a visible repertoire* (which the owner enjoyed) with blind typing. The highlighted
keywords are the bridge -- the repertoire is visible *in the fiction*, so the player always has
something concrete to type, without conversation ever collapsing into a menu.

## The map and six directions

- **ASCII hex minimap.** Render explored tiles as offset rows of single glyphs (`.` open, `#`
  high/forest, `~` water, `^` mountain, `@` avatar, letters for settlements/POIs), fog = blank.
  Offset-row hex packing reads fine in a monospace grid; keep it ASCII for the tofu-safe font.
- **Full map screen.** `map` opens a larger, scrollable map over the prose pane -- a diegetic
  parchment the avatar carries, showing only what has been discovered (the fog/`Known` model).
- **Six-direction compass.** Movement and the "exits" line present six directions, named by the
  worldbuilding (placeholders until it lands). The hex topology already stores six neighbours per
  tile (`game_sim` `Topology::neighbors` returns the 6 links); we name those six and never collapse
  to N/S/E/W. A scene's exits are described in-voice ("a road runs toward <dir>, the river bends
  away toward <dir>").

## The journal (diegetic note-taking)

A journal the avatar keeps, opened with `journal`:

- **Auto-logged discoveries**: features found, NPCs met, lore tokens learned, quests taken, and
  notable feed events -- written in-voice, filed by place/person. This is where tagged prose lines
  pay off: each line carries machine-readable tags (`implies:feud`, `topic:harvest`), so the
  journal can file and cross-reference automatically without parsing English.
- **Player notes**: `note <text>` appends a free-text line; the player records suspicions and
  plans. (This is the Wolfean payoff -- the player is meant to *notice* and write down implications
  the prose only hinted at.)
- **Persistence**: the journal (auto + manual) is part of the save. Saves serialise the sim seed +
  the player's action log (deterministic replay) plus the journal text.

## World time (action-driven)

- **One action, one or many ticks.** A simple action (`examine`, `talk`) advances the world a
  single `sim.step()`; a long action (multi-hex travel, `rest`) advances many ticks in a loop,
  with the avatar's position/vitals updated each tick. The world is genuinely living -- NPCs pursue
  goals, trade, feud, and talk on every tick -- but it only moves when the player does, so reading
  is never interrupted and determinism is preserved (no background thread).
- **"While you were busy."** After a multi-tick action, the client diffs sim state since the action
  began and surfaces the salient changes through the **feed** and prose: who moved, what was
  overheard, a death, a betrayal, a price swing -- the MUD "you notice X happening" pattern, ranked
  by the same salience/surprise scoring the prose layer uses. `recap` re-reads the recent feed.
- This is the MUD feel the owner wants (a world that advances around you) without a real-time clock:
  the advancement is bulk-applied at action boundaries and *narrated*, rather than ticking live.

## The Simulation seam (what the client calls)

The client holds `agents::Simulation` (today the Bevy `app` holds it as a `NonSend` resource; the
TUI just owns it directly) and drives it through the existing public API -- no sim changes needed.
The loop:

```
let mut sim = build_world();          // Setup with all layers ON, like app did
sim.spawn_player(at);
loop {
    render(prose, map, status, feed); // from the read API below
    let cmd = parse(read_line());     // the pure parser
    match cmd {
        Move(dir)        => sim.player_travel_to(neighbor),   // may loop many ticks
        Look             => { /* re-render scene_at(pos) */ }
        Examine(target)  => { /* describe via prose layer */ }
        Talk(npc, intent)=> sim.player_talk(npc, intent),     // ticks once
        Wait             => sim.player_wait(),
        Search           => /* features.search_at(...) */,
        ...
    }
    // sim.step() is invoked by the action handlers (one or many), not on idle.
}
```

Read API the client renders from (already present per the codebase map; see `agents/src/lib.rs`):
`player_view`, `player_position`, `player_nearby_npcs`, `scene_at`, `souls_at`, `dialogue_log`,
`player_intents`, `overheard`, `tidings`, `display_name` / `npc_epithet` / `npc_situation`,
`player_quests`, and the `substrate()` tile queries. The action API: `step`, `player_travel_to`,
`player_wait`, `player_talk`, `player_counsel`, `search`, `learn_lore`.

The new prose layer (from [`prose_generation.md`](prose_generation.md)) sits between the read API
and the prose pane: it takes the structured `SceneView`/views and *narrates* them. The client never
formats raw facts itself -- it asks the prose layer for the line.

## Workspace changes

- **Add** a `tui` crate: `members = [... "tui"]`, depending on `agents`, `game_sim`, `config`, and
  `ratatui` + `crossterm`. Module sketch: `main` (loop), `render` (panels), `parser`
  (tokenise/resolve/disambiguate), `scope` (in-scope entity resolution), `map` (ASCII hex),
  `journal`, `convo` (the typed-intent conversation sub-mode).
- **Remove** `app` and `voice` from the workspace members and retire their code. With Bevy gone,
  the `[profile.dev]` "build dependencies at opt-level 3" note in the root `Cargo.toml` loses its
  main reason to exist (it existed so the large Bevy stack compiled once, optimised) -- it can be
  simplified, though it is harmless to keep.
- The SLM/`TextGen`/`SlmRealizer` seam in `agent_core/src/dialogue.rs` is removed along with
  `voice` (see `prose_generation.md` -- the no-hallucination mandate rules out an LLM re-voicer).

## Build order (suggested)

1. Stand up the `tui` crate: own a `Simulation`, render a bare prose pane + status from the read
   API, implement `look`/`go`/`examine`/`wait` against the existing (un-narrated) views. Prove the
   action-turn loop and determinism end-to-end before any prose work.
2. The pure parser: tokeniser, the in-scope entity resolver + disambiguation, the verb set,
   guess-the-verb mitigations.
3. The map panel + full map screen + six-direction compass.
4. The journal (auto-log + `note` + save/load).
5. The conversation sub-mode (typed intents + `topics`/`recall`), wired to `player_talk`.
6. The feed / "while you were busy" diff, then fold in the prose layer as it lands.

## Resolved (owner, 2026-06-27)

- **Pure parser everywhere, including conversation.** Discoverability via highlighted keywords in the
  prose (Morrowind-style), never a menu. (See [Conversation](#conversation-under-a-pure-parser).)
- **Save model: decide later** -- it does not block the front-end or prose work; pick when saves are
  actually built (seed+replay vs. full-state snapshot).

## Open questions (for the owner, before implementation)

1. **Direction names.** The six cardinal directions need names from the new worldbuilding. Provide
   them when ready; until then I will use neutral placeholders and a `dir1..dir6` mapping onto the
   topology's six neighbours.

## Prior art

**Parser craft / world model / conversation**
- The Zork/ZIL world model (rooms as objects, containment tree, verb x noun-synonym x preposition
  parser): https://blog.zarfhome.com/2025/01/the-visible-zorker .
- Inform 7 room-description assembly (locale priority, the "mentioned" flag, verbose/brief), the
  basis for our scene assembly: https://ganelson.github.io/inform-website/book/WI_18_28.html and
  .../WI_18_24.html .
- Emily Short, "So, Do We Need This Parser Thing Anyway?"
  https://emshort.blog/2010/06/07/so-do-we-need-this-parser-thing-anyway/ and "Writing
  Novice-friendly Parser Games" https://emshort.blog/2016/04/15/writing-novice-friendly-parser-games/
  -- the guess-the-verb problem and the mitigations we adopt.
- Emily Short, "Conversation" (the model taxonomy) https://emshort.blog/how-to-play/writing-if/my-articles/conversation/ ;
  Threaded Conversation (directly/indirectly-follows topic threading)
  https://github.com/i7/extensions/blob/master/Chris%20Conley/Threaded%20Conversation.i7x ; Versu
  (dialogue as a utility-planned public act) https://emshort.blog/2013/02/26/versu-conversation-implementation/ .

**Terminal UI**
- ratatui (maintained TUI framework) https://ratatui.rs/ + crossterm (terminal backend, input
  events) -- the recommended Rust stack: mature, MIT-licensed, multi-panel layout, scrollable text.

**Living-world / MUD presentation**
- The "event feed + room description" split and "you notice X happening" ambient messaging from MUD
  and roguelike practice -- surfaced here as the action-boundary diff ("while you were busy").
