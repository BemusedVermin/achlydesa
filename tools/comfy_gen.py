#!/usr/bin/env python3
"""Query a local ComfyUI to generate a transparent character sprite from a prompt.

Makes the sprite workflow *queryable*: instead of editing the prompt in the ComfyUI GUI, drive it over
ComfyUI's HTTP API and get a PNG saved straight into `assets/`. Standard library only — no `pip`.

    # ComfyUI must be running (default http://127.0.0.1:8188).
    python tools/comfy_gen.py --name blacksmith --prompt "a burly blacksmith in a leather apron"
    python tools/comfy_gen.py --name avatar     --prompt "a young hooded traveller with a short sword"
    python tools/comfy_gen.py --name child      --prompt "a small barefoot village child" --seed 7

The constant framing (front view, full body, feet-visible, sprite style) is prepended automatically;
`--raw` sends the prompt verbatim instead. Override the server with COMFY_URL, the workflow with
--workflow, the output folder with --out. Crop the result so the feet touch the bottom edge before
using it in-game (see docs/comfyui_pipeline.md).
"""

import argparse
import json
import os
import pathlib
import random
import time
import urllib.parse
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parent.parent
COMFY = os.environ.get("COMFY_URL", "http://127.0.0.1:8188").rstrip("/")

FRAMING = (
    "full body character, front view, standing idle, simple clean design, "
    "jrpg town sprite, hd-2d pixel art, {desc}, plain, centered, full figure with feet visible"
)
DEFAULT_NEGATIVE = (
    "multiple views, sprite sheet, turnaround, cropped, sitting, extra limbs, "
    "blurry, jpeg artifacts, text, watermark, busy background, drop shadow on ground"
)


def _api(path, payload=None):
    url = f"{COMFY}{path}"
    if payload is None:
        with urllib.request.urlopen(url) as r:
            return json.load(r)
    data = json.dumps(payload).encode()
    req = urllib.request.Request(url, data=data, headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req) as r:
        return json.load(r)


def _find(wf, class_type):
    """The (single) node id of a given class — KSampler/SaveImage are unique in this graph."""
    ids = [nid for nid, n in wf.items() if n.get("class_type") == class_type]
    if len(ids) != 1:
        raise SystemExit(f"expected exactly one {class_type} node, found {len(ids)}")
    return ids[0]


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--name", required=True, help="output filename stem, e.g. blacksmith -> blacksmith.png")
    ap.add_argument("--prompt", required=True, help="what to draw, e.g. 'a burly blacksmith'")
    ap.add_argument("--negative", default=DEFAULT_NEGATIVE)
    ap.add_argument("--seed", type=int, default=None, help="fixed seed (default: random)")
    ap.add_argument("--raw", action="store_true", help="send --prompt verbatim, skip the sprite framing")
    ap.add_argument("--workflow", default=str(ROOT / "docs" / "comfyui" / "sprite_workflow_api.json"))
    ap.add_argument("--out", default=str(ROOT / "assets" / "sprites"))
    args = ap.parse_args()

    wf = json.loads(pathlib.Path(args.workflow).read_text())

    # Locate the variable nodes structurally, so a re-exported workflow (different ids) still works.
    ksampler = _find(wf, "KSampler")
    save = _find(wf, "SaveImage")
    pos_node = wf[ksampler]["inputs"]["positive"][0]
    neg_node = wf[ksampler]["inputs"]["negative"][0]

    wf[pos_node]["inputs"]["text"] = args.prompt if args.raw else FRAMING.format(desc=args.prompt)
    wf[neg_node]["inputs"]["text"] = args.negative
    wf[ksampler]["inputs"]["seed"] = args.seed if args.seed is not None else random.randint(0, 2**31 - 1)
    wf[save]["inputs"]["filename_prefix"] = args.name

    print(f"queuing '{args.name}' on {COMFY} (seed {wf[ksampler]['inputs']['seed']}) ...")
    prompt_id = _api("/prompt", {"prompt": wf})["prompt_id"]

    # Poll /history until this prompt has outputs (ComfyUI also exposes a /ws websocket; polling is fine).
    images = []
    deadline = time.time() + 600
    while True:
        time.sleep(1)
        entry = _api(f"/history/{prompt_id}").get(prompt_id)
        if entry:
            images = [im for out in entry.get("outputs", {}).values() for im in out.get("images", [])]
            if images:
                break
            status = entry.get("status", {})
            if status.get("completed") or status.get("status_str") == "error":
                raise SystemExit(f"ComfyUI finished without an image: {status}")
        if time.time() > deadline:
            raise SystemExit("timed out waiting for ComfyUI (10 min)")

    im = images[-1]
    q = urllib.parse.urlencode(
        {"filename": im["filename"], "subfolder": im.get("subfolder", ""), "type": im.get("type", "output")}
    )
    with urllib.request.urlopen(f"{COMFY}/view?{q}") as r:
        png = r.read()

    out_dir = pathlib.Path(args.out)
    out_dir.mkdir(parents=True, exist_ok=True)
    dst = out_dir / f"{args.name}.png"
    dst.write_bytes(png)
    print(f"saved {dst}")
    print("next: crop so the feet touch the bottom edge, then run `cargo run -p app`")


if __name__ == "__main__":
    main()
