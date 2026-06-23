// Cel material — stock PBR lighting, then the lit brightness snapped into a few flat bands.
//
// We deliberately quantise *luminance* and scale the lit RGB by the banded value, so hue and
// saturation survive: the terrain's per-vertex colours still read through while the shading goes
// cartoon-flat. Shadows, fog, and tonemapping all come for free from the standard pipeline — we
// only intercept the lit colour between `apply_pbr_lighting` and the post-processing step.
//
// Structure mirrors Bevy's `extended_material` example so the prepass/deferred paths stay correct;
// only the banding block in the forward branch is ours. The extension binding rides at binding 100
// of the *material* bind group — addressed through `#{MATERIAL_BIND_GROUP}` (group 3 in Bevy 0.18,
// not a hardcoded number) so it lands in the same group the StandardMaterial bindings do.

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::alpha_discard,
}

#ifdef PREPASS_PIPELINE
#import bevy_pbr::{
    prepass_io::{VertexOutput, FragmentOutput},
    pbr_deferred_functions::deferred_output,
}
#else
#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
}
#endif

@group(#{MATERIAL_BIND_GROUP}) @binding(100)
var<uniform> bands: f32;

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    // Rebuild the standard PBR input from the StandardMaterial bindings + the mesh vertex data
    // (this is what carries the vertex colours through).
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);

#ifdef PREPASS_PIPELINE
    // Deferred: lighting runs in a separate fullscreen pass, so we can't band here. Fall back to
    // the standard deferred output (the forward path is what this prototype actually uses).
    let out = deferred_output(in, pbr_input);
#else
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);

    // Toon banding: snap brightness to `bands` discrete levels, keep hue + saturation by scaling
    // the lit RGB by the ratio of the banded luminance to the true luminance.
    let steps = max(bands, 1.0);
    let lum = max(dot(out.color.rgb, vec3<f32>(0.2126, 0.7152, 0.0722)), 1e-5);
    let banded = (floor(lum * steps) + 0.5) / steps;
    out.color = vec4<f32>(out.color.rgb * (banded / lum), out.color.a);

    // Stock post-processing: distance fog, alpha premultiply, and tonemapping on non-HDR cameras.
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
#endif

    return out;
}
