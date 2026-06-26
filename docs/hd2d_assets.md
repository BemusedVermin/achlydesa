# HD-2D asset catalogue

The running list of **sprites and textures to author** for the Star Ocean: The Second Story R-style
HD-2D shift. The chosen direction (see the design call):

- **Hybrid render** — keep the cel/toon shading + ink outline on the 3-D environments; composite
  **2-D character sprites** into them, add **bloom** (later DoF / tilt-shift).
- **Single-facing billboards** — one sprite per character, always turned to face the camera. (Can be
  upgraded to 4-/8-direction later without changing the scene code.)
- **Re-skin the hex overworld** as HD-2D later; **the town/POI scene is first**.

Characters are **procedurally generated** — every soul is a distinct full-body pixel figure composed
in code (`app/src/sprites.rs::procedural_body_sprite`), seeded from its identity, with the clothing
biased by archetype. No art asset is needed (and the dialogue portraits are likewise procedural
busts, `app/src/portraits.rs`). Environment surfaces still keep their flat cel colour until a texture
is supplied (textures are the next proc-gen target). A real authored sprite/texture can still drop in
over the procedural default by filename.

## Drop-in
The game already **loads a real file over its placeholder automatically** — author the PNG, save it
under the catalogued path, run. No code change. Detected by exact filename; a missing file falls back
to the placeholder.
- Character sprites → **`assets/sprites/`** (`avatar.png`, `townsfolk.png`) — see its README.
- Environment textures → **`assets/textures/`** (`ground_grass.png`, `plaza_stone.png`,
  `slate_face.png`) — see its README.
- How to generate them locally with ComfyUI: **`docs/comfyui_pipeline.md`**.

## Conventions
- **Character sprites**: PNG, RGBA, transparent background, character standing upright and centred
  horizontally, **feet at the bottom edge** (the quad's base sits on the ground). Suggested size
  **~256 × 448 px** (or pixel-art ~96 × 168 upscaled) — tune once we see one in-scene. Lit subtly by
  the scene; the cel outline traces the alpha edge, so a clean silhouette matters.
- **Environment textures**: tileable PNG/JPG, sRGB. They layer *under* the cel pass, so mid-value,
  low-contrast reads best (the toon banding adds the contrast). Suggested **512²** tileable.

## Character sprites (town/POI scene — first)
| Slot | Used for | Facings | Status |
|------|----------|---------|--------|
| `avatar` | the player figure | 1 (billboard) | **procedural** (`avatar.png` overrides) |
| residents | every resident in a settlement scene | 1 | **procedural, per-soul** (each a distinct figure) |

_Deferred (later scenes): per-archetype refinement (more pronounced smith/noble/priest silhouettes),
fauna species sprites (overworld), enemy/combatant sprites (combat arena), the recruited-companion
roster. A `townsfolk.png` no longer applies — residents are individually generated._

## Environment textures (town/POI scene)
| Slot | Used for | Status |
|------|----------|--------|
| `ground_grass` | the settlement commons (ground disc) | placeholder (flat green) |
| `plaza_stone` | the paved central plaza | placeholder (flat grey) |
| `wall_wattle`, `wall_stone` | hut / house / hall walls | placeholder (procedural prop colour) |
| `roof_thatch`, `roof_slate` | building roofs | placeholder (procedural prop colour) |
| `slate_face` | the readable stone slate | placeholder (flat dark) |

## Post-processing
| Effect | Status |
|--------|--------|
| Bloom | **deferred** — needs HDR, but a secondary HDR scene camera composited over the LDR overworld through the custom outline pass renders black; the render graph has to be sorted first. The outline is already format-aware (LDR/HDR) for when it lands. |
| Depth-of-field | deferred |
| Tilt-shift | deferred |

## Deferred — overworld re-skin
Sprite avatar hopping hex tiles on a textured 3-D board; tileable **terrain textures per biome**
(grass / desert / tundra / forest floor / rock / water), and the bloom/DoF treatment carried over.
Catalogued in full when that work starts.
