# HD-2D asset catalogue

The running list of **sprites and textures to author** for the Star Ocean: The Second Story R-style
HD-2D shift. The chosen direction (see the design call):

- **Hybrid render** — keep the cel/toon shading + ink outline on the 3-D environments; composite
  **2-D character sprites** into them, add **bloom** (later DoF / tilt-shift).
- **Single-facing billboards** — one sprite per character, always turned to face the camera. (Can be
  upgraded to 4-/8-direction later without changing the scene code.)
- **Re-skin the hex overworld** as HD-2D later; **the town/POI scene is first**.

Until each asset exists the game uses a **placeholder**: characters render as a procedurally-drawn
pawn silhouette (`app/src/sprites.rs::placeholder_silhouette`), tinted per role; environment surfaces
keep their flat cel colour. Each real asset drops in by swapping one texture handle — no scene code
changes. **Status: `placeholder` until you replace it.**

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
| `avatar` | the player figure (gold-tinted placeholder) | 1 (billboard) | placeholder |
| `townsfolk` | every resident in a settlement scene | 1 | placeholder (one shared silhouette, cool tint) |

_Deferred (later scenes): per-archetype townsfolk (farmer / smith / noble / priest / child), fauna
species sprites (overworld), enemy/combatant sprites (combat arena), the recruited-companion roster._

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
