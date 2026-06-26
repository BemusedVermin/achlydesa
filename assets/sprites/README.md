# Character sprites — drop-in

Save an authored sprite here under its exact filename and it **replaces its placeholder on the next
run** — no code change. A missing file falls back to the procedural pawn placeholder.

| File | Used for | Placeholder tint |
|------|----------|------------------|
| `avatar.png` | the player figure | gold |
| `townsfolk.png` | every settlement resident | cool blue-grey |

## Specs
- **PNG, RGBA, transparent background.** The cel ink-outline traces the alpha edge and the material
  alpha-masks at 0.5, so use a **hard, clean alpha** — a feathered cutout or a background halo will
  outline wrong.
- Character **upright, centered, feet at the bottom edge** (the quad's base sits on the ground).
- A single **front-facing** pose for now (billboards turn to face the camera). ~**256 × 448 px** (or
  pixel-art at a smaller size upscaled with nearest-neighbour).

How to generate them with ComfyUI: **`docs/comfyui_pipeline.md`**. Full catalogue: `docs/hd2d_assets.md`.
