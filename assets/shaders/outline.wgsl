// Post-process outline. Reads the depth prepass and inks an edge wherever depth jumps — an object's
// silhouette against whatever is behind it, and the steps and cliffs of the hex relief. Runs after
// tonemapping, compositing dark lines over the finished image: the inter-object half of the cel look
// (the toon material's fresnel rim already draws the per-surface silhouettes). The depth texture is
// single-sampled (the camera runs MSAA off) and read with textureLoad at integer texel offsets.
//
// Depth only, on purpose: the world's trees, scrub, and rock are densely faceted low-poly, so a
// normal-based crease detector inks them almost solid. Depth gives clean silhouettes and relief, and
// with an orthographic camera depth is linear — so one fixed threshold reads uniformly across the
// whole view, near and far alike.

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

@group(0) @binding(0) var screen_tex: texture_2d<f32>;
@group(0) @binding(1) var screen_sampler: sampler;
@group(0) @binding(2) var depth_tex: texture_depth_2d;

// Tuning. THICKNESS is the texel offset sampled for neighbours (1 = hairline, 2 = a bolder line).
// DEPTH_THRESHOLD is how big a depth jump must be before a line is drawn — raise it if the world
// gets scribbly, lower it to trace fainter steps. INK is the line colour (near-black, cool-tinted).
const THICKNESS: i32 = 2;
const DEPTH_THRESHOLD: f32 = 0.0015;
const INK: vec3<f32> = vec3<f32>(0.04, 0.05, 0.08);

fn load_depth(coord: vec2<i32>) -> f32 {
    return textureLoad(depth_tex, coord, 0);
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(screen_tex, screen_sampler, in.uv).rgb;

    let coord = vec2<i32>(in.position.xy);
    let t = THICKNESS;
    let dc = load_depth(coord);
    let depth_delta = abs(dc - load_depth(coord + vec2<i32>(-t, 0)))
        + abs(dc - load_depth(coord + vec2<i32>(t, 0)))
        + abs(dc - load_depth(coord + vec2<i32>(0, -t)))
        + abs(dc - load_depth(coord + vec2<i32>(0, t)));
    let edge = smoothstep(DEPTH_THRESHOLD, DEPTH_THRESHOLD * 2.0, depth_delta);

    return vec4<f32>(mix(color, INK, edge), 1.0);
}
