# Environment textures — drop-in

These surfaces are **procedural by default** — generated from noise in `app/src/textures.rs` (tileable
grass, plaza cobblestone, slate). Save a tileable texture here under its exact filename and it
**overrides the procedural one on the next run** — no code change. A missing file just uses the
procedural default.

| File | Surface | Procedural default |
|------|---------|--------------------|
| `ground_grass.png` | the settlement commons (ground disc) | FBM grass |
| `plaza_stone.png` | the paved central plaza | Worley cobblestone |
| `slate_face.png` | the readable stone slate | stratified slate |

## Specs
- **Tileable PNG, sRGB, ~512².**
- The cel pass bands the lighting and adds the contrast, so **mid-value, low-contrast albedo** reads
  best — a busy or high-contrast texture fights the toon banding.
- Only the **albedo / diffuse** is used (no normal/roughness maps yet).

Fastest source for these is CC0 libraries — **[ambientCG](https://ambientcg.com/)** or
**[Poly Haven](https://polyhaven.com/textures)** — grab the albedo, no attribution required. Building
wall/roof textures are **deferred** (the procedural building meshes need UV mapping first).
