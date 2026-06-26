//! A post-process outline pass — true inter-object ink lines.
//!
//! The fresnel rim on the toon material draws a per-surface silhouette, but it can't see where one
//! object overlaps another (a tree against the hill behind it). This adds a full-screen pass that
//! reads the depth prepass and inks an edge wherever depth jumps — object silhouettes against what's
//! behind them, and the steps and cliffs of the hex relief. It runs after tonemapping, compositing
//! dark lines over the finished image — the other half of the cel look.
//!
//! Depth alone, not normals: the world's props are densely faceted low-poly, so a normal-based
//! crease detector inks them almost solid (verified by eye). Depth gives clean silhouettes and
//! relief; see `assets/shaders/outline.wgsl`.
//!
//! The shape follows Bevy's own FXAA node: a full-screen [`ViewNode`] that ping-pongs the view
//! target via `post_process_write`, inserted between tonemapping and the end of post-processing.
//! The camera carries `DepthPrepass` (added in `main`) and runs with MSAA off, so the depth texture
//! is single-sampled and binds as a plain `texture_depth_2d` (a multisampled prepass would need a
//! very different, heavier shader). Tuning knobs live as constants in the shader — eyeball a
//! screenshot (`ACHLYDESA_SHOT`) and adjust.

use bevy::core_pipeline::FullscreenShader;
use bevy::core_pipeline::core_3d::graph::{Core3d, Node3d};
use bevy::core_pipeline::prepass::ViewPrepassTextures;
use bevy::ecs::query::QueryItem;
use bevy::image::BevyDefault as _;
use bevy::prelude::*;
use bevy::render::render_graph::{
    NodeRunError, RenderGraphContext, RenderGraphExt, RenderLabel, ViewNode, ViewNodeRunner,
};
use bevy::render::render_resource::binding_types::{sampler, texture_2d, texture_depth_2d};
use bevy::render::render_resource::*;
use bevy::render::renderer::{RenderContext, RenderDevice};
use bevy::render::view::ViewTarget;
use bevy::render::{RenderApp, RenderStartup};

/// Adds the post-process outline. Only the render sub-app is touched; with no camera carrying the
/// prepass textures the node is a no-op, so this stays byte-identical to a build without it for any
/// view that doesn't opt in.
pub struct OutlinePlugin;

impl Plugin for OutlinePlugin {
    fn build(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .add_systems(RenderStartup, init_outline_pipeline)
            .add_render_graph_node::<ViewNodeRunner<OutlineNode>>(Core3d, OutlineLabel)
            .add_render_graph_edges(
                Core3d,
                (
                    Node3d::Tonemapping,
                    OutlineLabel,
                    Node3d::EndMainPassPostProcessing,
                ),
            );
    }
}

#[derive(RenderLabel, Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct OutlineLabel;

#[derive(Default)]
struct OutlineNode;

impl ViewNode for OutlineNode {
    // Run only for a view that has the prepass textures (i.e. our camera); other views skip.
    type ViewQuery = (&'static ViewTarget, &'static ViewPrepassTextures);

    fn run(
        &self,
        _graph: &mut RenderGraphContext,
        render_context: &mut RenderContext,
        (target, prepass): QueryItem<Self::ViewQuery>,
        world: &World,
    ) -> Result<(), NodeRunError> {
        let pipeline_cache = world.resource::<PipelineCache>();
        let outline = world.resource::<OutlinePipeline>();

        // Need the depth prepass texture and a compiled pipeline; otherwise pass the frame through
        // untouched (the ping-pong has already copied source→destination by the next node).
        let Some(depth) = &prepass.depth else {
            return Ok(());
        };
        // Pick the pipeline whose target format matches this view (HDR scene camera vs LDR overworld).
        let pipeline_id = if target.main_texture_format() == ViewTarget::TEXTURE_FORMAT_HDR {
            outline.pipeline_hdr
        } else {
            outline.pipeline_ldr
        };
        let Some(pipeline) = pipeline_cache.get_render_pipeline(pipeline_id) else {
            return Ok(());
        };

        let post_process = target.post_process_write();
        // The source view flips between two textures each frame, so build the bind group fresh
        // rather than caching it.
        let bind_group = render_context.render_device().create_bind_group(
            "outline_bind_group",
            &pipeline_cache.get_bind_group_layout(&outline.layout),
            &BindGroupEntries::sequential((
                post_process.source,
                &outline.sampler,
                &depth.texture.default_view,
            )),
        );

        let mut render_pass =
            render_context
                .command_encoder()
                .begin_render_pass(&RenderPassDescriptor {
                    label: Some("outline_pass"),
                    color_attachments: &[Some(RenderPassColorAttachment {
                        view: post_process.destination,
                        depth_slice: None,
                        resolve_target: None,
                        ops: Operations::default(),
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });

        render_pass.set_pipeline(pipeline);
        render_pass.set_bind_group(0, &bind_group, &[]);
        render_pass.draw(0..3, 0..1);

        Ok(())
    }
}

#[derive(Resource)]
struct OutlinePipeline {
    layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    /// One pipeline per view colour format: the outline runs over both the LDR overworld camera and
    /// the **HDR** (bloom) scene camera, whose post-process targets differ — a pipeline whose target
    /// format mismatches the pass panics, so we keep one of each and pick by the view's format.
    pipeline_ldr: CachedRenderPipelineId,
    pipeline_hdr: CachedRenderPipelineId,
}

fn init_outline_pipeline(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    fullscreen_shader: Res<FullscreenShader>,
    pipeline_cache: Res<PipelineCache>,
    asset_server: Res<AssetServer>,
) {
    let layout = BindGroupLayoutDescriptor::new(
        "outline_bind_group_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                // The tonemapped screen colour (sampled), then the prepass depth (read with
                // textureLoad at integer offsets, so it doesn't need a sampler).
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                texture_depth_2d(),
            ),
        ),
    );

    let sampler = render_device.create_sampler(&SamplerDescriptor {
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        ..default()
    });

    let shader = asset_server.load("shaders/outline.wgsl");
    let mut queue = |format: TextureFormat, label: &'static str| {
        pipeline_cache.queue_render_pipeline(RenderPipelineDescriptor {
            label: Some(label.into()),
            layout: vec![layout.clone()],
            vertex: fullscreen_shader.to_vertex_state(),
            fragment: Some(FragmentState {
                shader: shader.clone(),
                targets: vec![Some(ColorTargetState {
                    format,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            ..default()
        })
    };

    commands.insert_resource(OutlinePipeline {
        pipeline_ldr: queue(TextureFormat::bevy_default(), "outline_pipeline_ldr"),
        pipeline_hdr: queue(ViewTarget::TEXTURE_FORMAT_HDR, "outline_pipeline_hdr"),
        layout,
        sampler,
    });
}
