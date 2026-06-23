//! The overworld's cel material.
//!
//! A toon look is just the standard PBR surface with a fragment that snaps the lit brightness into
//! a few flat bands (see `assets/shaders/toon_band.wgsl`). We get there with Bevy's
//! [`ExtendedMaterial`]: the base [`StandardMaterial`] keeps doing all the real work — vertex
//! colours, the single key light, shadows, the cool distance fog — and the extension only quantises
//! the result. That means the terrain, props, fauna, and the figures can all share one material
//! *type* and the whole view reads as a single cartoon surface, while the selection rings (plain
//! `StandardMaterial`) ride the stock pipeline alongside it untouched.

use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;

/// The world material: [`StandardMaterial`] extended with the [`ToonBand`] cel pass.
pub type ToonMaterial = ExtendedMaterial<StandardMaterial, ToonBand>;

/// The cel extension. One knob — how many flat shades the lit colour collapses to.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct ToonBand {
    /// Number of brightness bands the lit colour snaps to. Fewer reads chunkier/more cartoon;
    /// ~4 keeps terrain relief legible while still going flat. Bound past the StandardMaterial
    /// slots at binding 100.
    #[uniform(100)]
    pub bands: f32,
}

impl Default for ToonBand {
    fn default() -> Self {
        Self { bands: 4.0 }
    }
}

impl MaterialExtension for ToonBand {
    fn fragment_shader() -> ShaderRef {
        "shaders/toon_band.wgsl".into()
    }
}

/// Wrap a `StandardMaterial` in the default cel pass — the one-liner every world material spawns
/// through, so the band count lives in exactly one place.
pub fn toon(base: StandardMaterial) -> ToonMaterial {
    ExtendedMaterial {
        base,
        extension: ToonBand::default(),
    }
}
