# Generating HD-2D assets with ComfyUI

A practical recipe for producing the sprites (and, if you want, textures) the HD-2D shift needs, with
a **local ComfyUI**. The game already loads real files over the placeholders — author a PNG, save it
to the catalogued path (`assets/sprites/…`, `assets/textures/…`), and it shows up next run. No code
change. See `docs/hd2d_assets.md` for the full slot list.

The two hard requirements that make a generated sprite actually drop in cleanly:
1. **Native transparency** (RGBA), with a **hard, clean alpha edge** — the cel outline traces the
   alpha and the material masks at 0.5, so feathered cutouts / background halos look wrong.
2. **Feet at the bottom edge**, character centered, single **front-facing** pose.

---

## Quick start — load the ready workflow
A ready-to-load graph ships at **`docs/comfyui/sprite_workflow.json`** (SDXL base + Pixel Art XL LoRA
+ LayerDiffuse — picked to be the most reliable sprite-with-transparency stack). Starting from zero:

1. **Install LayerDiffuse** — ComfyUI **Manager → Custom Nodes Manager → `ComfyUI-layerdiffuse` →
   Install → restart.** (It downloads its transparency weights to `models/layer_model/` on first run.)
2. **Download the two model files** (Manager → Model Manager can do both, or by hand):
   - `sd_xl_base_1.0.safetensors` → `ComfyUI/models/checkpoints/`
     — [stabilityai/stable-diffusion-xl-base-1.0](https://huggingface.co/stabilityai/stable-diffusion-xl-base-1.0)
   - `pixel-art-xl.safetensors` → `ComfyUI/models/loras/`
     — [nerijs/pixel-art-xl](https://huggingface.co/nerijs/pixel-art-xl)
3. **Load the workflow** — ComfyUI **Workflow → Open** (or drag `sprite_workflow.json` onto the canvas).
4. Edit the **positive prompt** (the character description), hit **Queue Prompt**. The result saves to
   `ComfyUI/output/townsfolk_*.png` **with transparency**.
5. **Crop so the feet touch the bottom edge** (and center it), save as `assets/sprites/townsfolk.png`,
   run `cargo run -p app`, enter a settlement.

If a node loads **red**, its dropdown filename doesn't match what you downloaded — just re-pick it.
**Tuning:** too pixelated → lower the LoRA strength (node *Load LoRA*, ~0.6) or swap the checkpoint for
an illustration SDXL model; for a matching *set* of townsfolk, set the KSampler control to **fixed**
and change only the character words. Manual node-by-node build is below if you'd rather wire it yourself.

## Queryable — generate anything from the command line
To generate *any* character (not just townsfolk) without touching the GUI, drive ComfyUI over its HTTP
API with **`tools/comfy_gen.py`** (Python standard library only — nothing to `pip install`). With
ComfyUI running:

```sh
python tools/comfy_gen.py --name blacksmith --prompt "a burly blacksmith in a leather apron"
python tools/comfy_gen.py --name avatar     --prompt "a hooded traveller with a short sword"
python tools/comfy_gen.py --name child      --prompt "a small barefoot village child" --seed 7
```

It posts the API-format graph (`docs/comfyui/sprite_workflow_api.json`) to `/prompt`, waits, saves the
transparent PNG to `assets/sprites/<name>.png`, and **auto-reframes it feet-at-base** — trims the
transparent margins and stands the figure centered on the bottom edge (nearest-neighbour, so pixels
stay crisp) so it sits on the ground in-game with no manual crop. (Needs Pillow, which the ComfyUI
Python env already has; `--no-frame` to skip.) The constant sprite framing is prepended to the prompt
automatically; `--raw` sends it verbatim. Flags: `--seed`, `--negative`, `--workflow <path>`,
`--out <dir>`; the `COMFY_URL` env var points at a non-default server.

**The API is three calls:** `POST /prompt {"prompt": <graph>}` queues and returns a `prompt_id`;
`GET /history/<prompt_id>` reports the outputs once it's done; `GET /view?filename=…` returns the PNG.
The graph is just the workflow with the prompt / seed / filename swapped — so the same mechanism makes
the avatar, future per-archetype townsfolk, enemies, fauna, anything.

> Tweaked the GUI workflow? Re-export its API form (ComfyUI **Settings → enable Dev mode → Save (API
> Format)**) and pass it with `--workflow`. The script locates the prompt/seed/save nodes
> *structurally*, so changed node ids don't matter.

## Characters — the sprite workflow (manual build)

### Install
- **ComfyUI-layerdiffuse** (`huchenlei/ComfyUI-layerdiffuse`, via the ComfyUI Manager) — generates an
  **RGBA** image with real transparency, so no background removal / `rembg` step and no halo. This is
  the key node.
- A **pixel-art style** on top of your base model — either:
  - SDXL + the **Pixel Art XL** LoRA (`nerijs/pixel-art-xl`), or
  - a pixel-art **checkpoint** from Civitai (search "pixel art" / "pixel"),
  - or, for a softer HD-2D painterly look, skip the pixel LoRA and use a clean illustration model and
    downscale in post (below).

### Graph
```
Load Checkpoint ─┬─► (Load LoRA: pixel-art) ─► CLIP Text Encode (positive) ─┐
                 └────────────────────────────► CLIP Text Encode (negative) ─┤
Layer Diffuse Apply  (model in, "SDXL, Conv Injection" or the SD1.5 variant) │
        └─► KSampler ◄── Empty Latent (portrait, e.g. 832×1216) ◄────────────┘
                └─► VAE Decode ─► Layer Diffuse Decode (RGBA) ─► Save Image (PNG = keeps alpha)
```
(LayerDiffuse routes the model through "Apply" before the sampler and recovers the alpha in "Decode
(RGBA)". Follow the node's example workflow if the wiring differs for your version.)

### Prompt
- **Positive:** `full body character, front view, standing idle, simple clean design, jrpg town
  sprite, hd-2d, [a weathered peasant farmer in homespun / a young woman in a market dress / a hooded
  traveller], plain, centered`
- **Negative:** `multiple views, sprite sheet, turntable, cropped, sitting, extra limbs, blurry,
  text, watermark, busy background`

### Settings
- **Portrait latent** (taller than wide) so the full body fits with headroom — e.g. **832 × 1216**
  (SDXL). Steps ~28, CFG ~6–7.
- **Fix the seed** and keep the framing prompt constant; change only the character description to get a
  consistent set of townsfolk that share scale/pose.

### Post (so feet sit on the ground)
- **Crop + pad** the RGBA so the character's **feet touch the bottom edge** and it's **centered
  horizontally** (the quad's base is the ground line). A ComfyUI image-crop node works, or any editor.
- *Optional pixel crunch:* `Image Scale` down to ~128 px tall with **nearest**, then back up to 448
  with **nearest** — crisp chunky pixels. (The Pixel Art XL LoRA mostly does this already.)
- **Save as PNG** (preserves alpha). Name it `avatar.png` / `townsfolk.png` → `assets/sprites/`.

---

## Textures — fastest path is *not* generation

For grass / stone / slate, **download CC0 albedo** from **[ambientCG](https://ambientcg.com/)** or
**[Poly Haven](https://polyhaven.com/textures)** — tileable, no attribution, better than generating.
Grab the *Color/Diffuse* map, resize to ~512², save as `ground_grass.png` / `plaza_stone.png` /
`slate_face.png` in `assets/textures/`.

If you'd rather generate them in ComfyUI:
- Install a **seamless-tiling** node (e.g. `spinagon/ComfyUI-seamless-tiling`, which patches conv
  padding to circular so the output tiles), enable it before the sampler.
- **Prompt:** `seamless tileable [lush grass / worn cobblestone / dark slate stone] texture, top-down,
  flat even lighting, no shadows, game texture`. Render **512×512**, save PNG.
- Keep it **mid-value, low-contrast** — the cel pass adds the banding/contrast; a busy texture fights it.

---

## Try one first
Make **one** `townsfolk.png` and grab **one** `ground_grass.png`, drop them in, and run
`cargo run -p app` (then enter a settlement). We'll look at the real thing on a known-good scene before
you batch the rest — easier to dial the prompt/scale against a live target than to guess.

---

## Troubleshooting

**`'JoinImageWithAlpha' object has no attribute 'join_image_with_alpha'`** (raised by
`LayeredDiffusionDecodeRGBA`). A known incompatibility between ComfyUI-layerdiffuse and current
ComfyUI: a ComfyUI refactor changed the built-in `JoinImageWithAlpha` node, so layerdiffuse's call to
it breaks ([ComfyUI issue #10766](https://github.com/Comfy-Org/ComfyUI/issues/10766)). It's the node
pair being out of sync — not your workflow.

1. First, **Manager → Update All → restart ComfyUI** (it may already be fixed upstream).
2. If it persists, **patch one method** in `custom_nodes/ComfyUI-layerdiffuse/layered_diffusion.py` —
   in class `LayeredDiffusionDecodeRGBA`, replace the `decode` body's `JoinImageWithAlpha()…` return
   with the inline join (reproducing the node's old result — the two mask inversions cancel to the
   original mask as the alpha channel):
   ```python
   def decode(self, samples, images, sd_version: str, sub_batch_size: int):
       image, mask = super().decode(samples, images, sd_version, sub_batch_size)
       # JoinImageWithAlpha's instance method was refactored away in current ComfyUI; inline it.
       return (torch.cat((image[..., :3], mask.unsqueeze(-1)), dim=-1),)
   ```
   `torch` is already imported at the top of that file. (An extension update will overwrite this — fine
   as a stop-gap.)
3. Robust long-term alternative: drop LayerDiffuse and cut the background with a **rembg / RMBG** node
   instead (generate the character on a flat background, then remove it). Trades native alpha for an
   extra node, but doesn't depend on the layerdiffuse↔ComfyUI version pairing.
