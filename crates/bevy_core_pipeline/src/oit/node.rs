use bevy_ecs::{prelude::*, query::QueryItem};
use bevy_render::{
    camera::ExtractedCamera,
    diagnostic::RecordDiagnostics,
    render_graph::{NodeRunError, RenderGraphContext, ViewNode},
    render_phase::SortedRenderPhase,
    render_resource::{PipelineCache, RenderPassDescriptor},
    renderer::RenderContext,
    view::{ViewTarget, ViewUniformOffset},
};

use super::{
    OitLayersBindGroup, OitSortPipelineId, OitViewBindGroup, OrderIndependentTransparent3d,
};

#[derive(Default)]
pub struct OitNode;
impl ViewNode for OitNode {
    type ViewQuery = (
        &'static ExtractedCamera,
        &'static SortedRenderPhase<OrderIndependentTransparent3d>,
        &'static ViewTarget,
        &'static OitLayersBindGroup,
        &'static ViewUniformOffset,
        &'static OitSortPipelineId,
    );

    fn run(
        &self,
        graph: &mut RenderGraphContext,
        render_context: &mut RenderContext,
        (
            camera,
            oit_phase,
            view_target,
            oit_layers_bind_group,
            view_uniform,
            oit_sort_pipeline_id,
        ): QueryItem<Self::ViewQuery>,
        world: &World,
    ) -> Result<(), NodeRunError> {
        if oit_phase.items.is_empty() {
            return Ok(());
        }

        let diagnostics = render_context.diagnostic_recorder();

        let color_attachments = [Some(view_target.get_color_attachment())];

        // render
        {
            let label = "oit_render_pass";
            let mut render_pass = render_context.begin_tracked_render_pass(RenderPassDescriptor {
                label: Some(label),
                color_attachments: &color_attachments,
                // we bind the depth in a uniform because on some platforms early-z doesn't
                // work so we need to sample it manually
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            let pass_span = diagnostics.pass_span(&mut render_pass, label);

            if let Some(viewport) = camera.viewport.as_ref() {
                render_pass.set_camera_viewport(viewport);
            }

            oit_phase.render(&mut render_pass, world, graph.view_entity());

            pass_span.end(&mut render_pass);
        }

        // sort oit layers
        {
            let pipeline_cache = world.resource::<PipelineCache>();
            let view_bind_group = world.resource::<OitViewBindGroup>();
            let Some(pipeline) = pipeline_cache.get_render_pipeline(oit_sort_pipeline_id.0) else {
                return Ok(());
            };

            let label = "oit_sort_pass";

            let mut render_pass = render_context.begin_tracked_render_pass(RenderPassDescriptor {
                label: Some(label),
                color_attachments: &color_attachments,
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            let pass_span = diagnostics.pass_span(&mut render_pass, label);

            if let Some(viewport) = camera.viewport.as_ref() {
                render_pass.set_camera_viewport(viewport);
            }

            render_pass.set_render_pipeline(pipeline);
            render_pass.set_bind_group(0, view_bind_group, &[view_uniform.offset]);
            render_pass.set_bind_group(1, oit_layers_bind_group, &[]);
            // Draw a single full screen triangle.
            // This way each fragment sorts it's own oit layer before drawing it.
            render_pass.draw(0..3, 0..1);

            pass_span.end(&mut render_pass);
        }

        Ok(())
    }
}
