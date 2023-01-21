use bevy::{
    core_pipeline::{
        core_3d, fullscreen_vertex_shader::fullscreen_shader_vertex_state,
        prepass::ViewPrepassTextures,
    },
    prelude::*,
    render::{
        render_graph::{Node, NodeRunError, RenderGraph, RenderGraphContext, SlotInfo, SlotType},
        render_resource::{
            BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindGroupLayoutDescriptor,
            BindGroupLayoutEntry, BindingResource, BindingType, BufferBindingType,
            CachedRenderPipelineId, ColorTargetState, ColorWrites, FragmentState, MultisampleState,
            Operations, PipelineCache, PrimitiveState, RenderPassColorAttachment,
            RenderPassDescriptor, RenderPipelineDescriptor, Sampler, SamplerBindingType,
            SamplerDescriptor, ShaderStages, ShaderType, TextureFormat, TextureSampleType,
            TextureViewDimension,
        },
        renderer::{RenderContext, RenderDevice},
        texture::BevyDefault,
        view::{ExtractedView, ViewTarget, ViewUniform, ViewUniforms},
        RenderApp,
    },
};

pub struct SsgiPlugin;
impl Plugin for SsgiPlugin {
    fn build(&self, app: &mut App) {
        let Ok(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };

        render_app.init_resource::<SsgiPipeline>();

        let node = SsgiNode::new(&mut render_app.world);

        let mut graph = render_app.world.resource_mut::<RenderGraph>();
        let core_3d_graph = graph.get_sub_graph_mut(core_3d::graph::NAME).unwrap();
        core_3d_graph.add_node(SsgiNode::NAME, node);

        core_3d_graph.add_slot_edge(
            core_3d_graph.input_node().id,
            core_3d::graph::input::VIEW_ENTITY,
            SsgiNode::NAME,
            SsgiNode::IN_VIEW,
        );

        core_3d_graph.add_node_edge(core_3d::graph::node::MAIN_PASS, SsgiNode::NAME);
        core_3d_graph.add_node_edge(SsgiNode::NAME, core_3d::graph::node::TONEMAPPING);
    }
}

struct SsgiNode {
    query: QueryState<(&'static ViewTarget, &'static ViewPrepassTextures), With<ExtractedView>>,
}

impl SsgiNode {
    pub const IN_VIEW: &str = "view";
    pub const NAME: &str = "ssgi";

    fn new(world: &mut World) -> Self {
        Self {
            query: QueryState::new(world),
        }
    }
}

impl Node for SsgiNode {
    fn input(&self) -> Vec<SlotInfo> {
        vec![SlotInfo::new(SsgiNode::IN_VIEW, SlotType::Entity)]
    }

    fn update(&mut self, world: &mut World) {
        self.query.update_archetypes(world);
    }

    fn run(
        &self,
        graph_context: &mut RenderGraphContext,
        render_context: &mut RenderContext,
        world: &World,
    ) -> Result<(), NodeRunError> {
        let view_entity = graph_context.get_input_entity(SsgiNode::IN_VIEW)?;

        let Ok((view_target, prepass_textures)) = self.query.get_manual(world, view_entity) else {
            return Ok(());
        };

        let post_process_pipeline = world.resource::<SsgiPipeline>();
        let pipeline_cache = world.resource::<PipelineCache>();

        let Some(pipeline) = pipeline_cache.get_render_pipeline(post_process_pipeline.pipeline_id) else {
            return Ok(());
        };

        let post_process = view_target.post_process_write();
        let source = post_process.source;
        let destination = post_process.destination;

        let view_uniforms = world.resource::<ViewUniforms>();

        let Some(view_binding) = view_uniforms.uniforms.binding() else {
            return Ok(());
        };

        let bind_group_descriptor = BindGroupDescriptor {
            label: Some("ssgi_bind_group"),
            layout: &post_process_pipeline.layout,
            entries: &[
                // screen texture
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(source),
                },
                // sampler
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::Sampler(&post_process_pipeline.sampler),
                },
                // depth
                BindGroupEntry {
                    binding: 2,
                    resource: BindingResource::TextureView(
                        &prepass_textures.depth.as_ref().unwrap().default_view,
                    ),
                },
                // normal
                BindGroupEntry {
                    binding: 3,
                    resource: BindingResource::TextureView(
                        &prepass_textures.normal.as_ref().unwrap().default_view,
                    ),
                },
                BindGroupEntry {
                    binding: 4,
                    resource: view_binding.clone(),
                },
            ],
        };

        let bind_group = render_context
            .render_device
            .create_bind_group(&bind_group_descriptor);

        let descriptor = RenderPassDescriptor {
            label: Some("ssgi_pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: destination,
                resolve_target: None,
                ops: Operations::default(),
            })],
            depth_stencil_attachment: None,
        };

        let mut render_pass = render_context
            .command_encoder
            .begin_render_pass(&descriptor);

        render_pass.set_pipeline(pipeline);
        render_pass.set_bind_group(0, &bind_group, &[]);
        render_pass.draw(0..3, 0..1);

        Ok(())
    }
}

#[derive(Resource)]
struct SsgiPipeline {
    layout: BindGroupLayout,
    sampler: Sampler,
    pipeline_id: CachedRenderPipelineId,
}

impl FromWorld for SsgiPipeline {
    fn from_world(world: &mut World) -> Self {
        let render_device = world.resource::<RenderDevice>();

        let layout = render_device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("ssgi_bind_group_layout"),
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
                // Depth
                BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        multisampled: false,
                        sample_type: TextureSampleType::Depth,
                        view_dimension: TextureViewDimension::D2,
                    },
                    count: None,
                },
                // Normals
                BindGroupLayoutEntry {
                    binding: 3,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        multisampled: false,
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                    },
                    count: None,
                },
                // View
                BindGroupLayoutEntry {
                    binding: 4,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let sampler = render_device.create_sampler(&SamplerDescriptor::default());

        let shader = world.resource::<AssetServer>().load("shaders/ssgi.wgsl");

        let pipeline_id =
            world
                .resource_mut::<PipelineCache>()
                .queue_render_pipeline(RenderPipelineDescriptor {
                    label: Some("ssgi_pipeline".into()),
                    layout: Some(vec![layout.clone()]),
                    // This will setup a fullscreen triangle for the vertex state
                    vertex: fullscreen_shader_vertex_state(),
                    fragment: Some(FragmentState {
                        shader,
                        shader_defs: vec![],
                        entry_point: "fragment".into(),
                        targets: vec![Some(ColorTargetState {
                            format: TextureFormat::bevy_default(),
                            blend: None,
                            write_mask: ColorWrites::ALL,
                        })],
                    }),
                    primitive: PrimitiveState::default(),
                    depth_stencil: None,
                    multisample: MultisampleState::default(),
                });

        Self {
            layout,
            sampler,
            pipeline_id,
        }
    }
}
