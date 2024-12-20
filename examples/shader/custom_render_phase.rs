//! This example demonstrates how to write a custom phase
//!
//! Render phases in bevy are used whenever you need to draw a groud of neshes in a specific way.
//! For example, bevy's main pass has an opaque phase, a transparent phase for both 2d and 3d.
//! Sometimes, you may want to only draw a subset of meshes before or after the builtin phase. In
//! those situations you need to write your own phase.
//!
//! This examples showcase how writing a custom phase to draw a stencil of a bevy mesh could look
//! like. Some shortcuts have been used for simplicity.
//!
//! This example was made for 3d, but a 2d equivalent would be almost identical.

use std::ops::Range;

use bevy::{
    core_pipeline::core_3d::graph::{Core3d, Node3d},
    ecs::{
        entity::EntityHashSet,
        query::QueryItem,
        system::{lifetimeless::SRes, SystemParamItem},
    },
    math::FloatOrd,
    pbr::{
        material_bind_groups::MaterialBindGroupSlot, DrawMesh, MeshInputUniform, MeshPipeline,
        MeshPipelineKey, MeshPipelineViewLayoutKey, MeshUniform, RenderMeshInstances,
        SetMeshBindGroup, SetMeshViewBindGroup,
    },
    prelude::*,
    render::{
        batching::{
            gpu_preprocessing::{batch_and_prepare_sorted_render_phase, IndirectParametersBuffer},
            GetBatchData, GetFullBatchData,
        },
        camera::ExtractedCamera,
        diagnostic::RecordDiagnostics,
        extract_component::{ExtractComponent, ExtractComponentPlugin},
        mesh::{allocator::MeshAllocator, MeshVertexBufferLayoutRef, RenderMesh},
        render_asset::RenderAssets,
        render_graph::{
            NodeRunError, RenderGraphApp, RenderGraphContext, RenderLabel, ViewNode, ViewNodeRunner,
        },
        render_phase::{
            sort_phase_system, AddRenderCommand, CachedRenderPipelinePhaseItem, DrawFunctionId,
            DrawFunctions, PhaseItem, PhaseItemExtraIndex, SetItemPipeline, SortedPhaseItem,
            TrackedRenderPass, ViewSortedRenderPhases,
        },
        render_resource::{
            CachedRenderPipelineId, ColorTargetState, ColorWrites, CommandEncoderDescriptor, Face,
            FragmentState, FrontFace, MultisampleState, PipelineCache, PolygonMode, PrimitiveState,
            RenderPassDescriptor, RenderPipelineDescriptor, SpecializedMeshPipeline,
            SpecializedMeshPipelineError, SpecializedMeshPipelines, TextureFormat, VertexState,
        },
        renderer::RenderContext,
        sync_world::{MainEntity, RenderEntity},
        view::{ExtractedView, RenderVisibleEntities, ViewTarget},
        Extract, Render, RenderApp, RenderSet,
    },
};
use nonmax::NonMaxU32;

const SHADER_ASSET_PATH: &str = "shaders/custom_stencil.wgsl";

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, MeshStencilPhasePlugin))
        .add_systems(Startup, setup)
        .run();
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // circular base
    commands.spawn((
        Mesh3d(meshes.add(Circle::new(4.0))),
        MeshMaterial3d(materials.add(Color::WHITE)),
        Transform::from_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
    ));
    // cube
    // This cube will be rendered by the main pass, but it will also be rendered by our custom
    // pass. This should result in an unlit red cube
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
        MeshMaterial3d(materials.add(Color::srgb_u8(124, 144, 255))),
        Transform::from_xyz(0.0, 0.5, 0.0),
        // This marker component is used to identify which mesh will be used in our custom pass
        // The circle doesn't have it so it won't be rendered in our pass
        DrawStencil,
    ));
    // light
    commands.spawn((
        PointLight {
            shadows_enabled: true,
            ..default()
        },
        Transform::from_xyz(4.0, 8.0, 4.0),
    ));
    // camera
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(-2.0, 4.5, 9.0).looking_at(Vec3::ZERO, Vec3::Y),
        // disable msaa for simplicity
        Msaa::Off,
    ));
}

#[derive(Component, ExtractComponent, Clone, Copy, Default)]
struct DrawStencil;

struct MeshStencilPhasePlugin;
impl Plugin for MeshStencilPhasePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((ExtractComponentPlugin::<DrawStencil>::default(),));
        // We need to get the render app from the main app
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_resource::<SpecializedMeshPipelines<StencilPipeline>>()
            .init_resource::<DrawFunctions<StencilPhase>>()
            .add_render_command::<StencilPhase, DrawMesh3dStencil>()
            .init_resource::<ViewSortedRenderPhases<StencilPhase>>()
            .add_systems(ExtractSchedule, extract_camera_phases)
            .add_systems(
                Render,
                (
                    sort_phase_system::<StencilPhase>.in_set(RenderSet::PhaseSort),
                    batch_and_prepare_sorted_render_phase::<StencilPhase, StencilPipeline>
                        .in_set(RenderSet::PrepareResources),
                    queue_custom_meshes.in_set(RenderSet::QueueMeshes),
                ),
            );

        render_app
            .add_render_graph_node::<ViewNodeRunner<CustomDrawNode>>(Core3d, CustomDrawPassLabel)
            // Tell the node to run after the main pass
            .add_render_graph_edges(Core3d, (Node3d::MainOpaquePass, CustomDrawPassLabel));
    }

    fn finish(&self, app: &mut App) {
        // We need to get the render app from the main app
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        // The pipeline needs the RenderDevice to be created and it's only available once plugins
        // are intialized
        render_app.init_resource::<StencilPipeline>();
    }
}

#[derive(Resource)]
struct StencilPipeline {
    /// The base mesh pipeline defined by bevy
    ///
    /// Since we want to draw a stencil of an existing bevy mesh we want to reuse the default
    /// pipeline as much as possible
    mesh_pipeline: MeshPipeline,
    /// Stores the shader used for this pipeline directly on the pipeline.
    /// This isn't required, it's only done like this for simplicity.
    shader_handle: Handle<Shader>,
}
impl FromWorld for StencilPipeline {
    fn from_world(world: &mut World) -> Self {
        Self {
            mesh_pipeline: MeshPipeline::from_world(world),
            shader_handle: world.resource::<AssetServer>().load(SHADER_ASSET_PATH),
        }
    }
}
// For more information on how SpecializedMeshPipeline work, please look at the
// specialized_mesh_pipeline example
impl SpecializedMeshPipeline for StencilPipeline {
    type Key = MeshPipelineKey;

    fn specialize(
        &self,
        key: Self::Key,
        layout: &MeshVertexBufferLayoutRef,
    ) -> Result<RenderPipelineDescriptor, SpecializedMeshPipelineError> {
        // We will only use the position of the mesh in our shader so we only need to specify that
        let mut vertex_attributes = Vec::new();
        if layout.0.contains(Mesh::ATTRIBUTE_POSITION) {
            // Make sure this matches the shader location
            vertex_attributes.push(Mesh::ATTRIBUTE_POSITION.at_shader_location(0));
        }
        // This will automatically generate the correct `VertexBufferLayout` based on the vertex attributes
        let vertex_buffer_layout = layout.0.get_layout(&vertex_attributes)?;

        Ok(RenderPipelineDescriptor {
            label: Some("Specialized Mesh Pipeline".into()),
            // We want to reuse the data from bevy so we use the same bind groups as the default
            // mesh pipeline
            layout: vec![
                // Bind group 0 is the view uniform
                self.mesh_pipeline
                    .get_view_layout(MeshPipelineViewLayoutKey::from(key))
                    .clone(),
                // Bind group 1 is the mesh uniform
                self.mesh_pipeline.mesh_layouts.model_only.clone(),
            ],
            push_constant_ranges: vec![],
            vertex: VertexState {
                shader: self.shader_handle.clone(),
                shader_defs: vec![],
                entry_point: "vertex".into(),
                buffers: vec![vertex_buffer_layout],
            },
            fragment: Some(FragmentState {
                shader: self.shader_handle.clone(),
                shader_defs: vec![],
                entry_point: "fragment".into(),
                targets: vec![Some(ColorTargetState {
                    format: TextureFormat::bevy_default(),
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
            }),
            primitive: PrimitiveState {
                topology: key.primitive_topology(),
                front_face: FrontFace::Ccw,
                cull_mode: Some(Face::Back),
                polygon_mode: PolygonMode::Fill,
                ..default()
            },
            depth_stencil: None,
            // It's generally recommended to specialize your pipeline for MSAA,
            // but it's not always possible
            multisample: MultisampleState::default(),
            zero_initialize_workgroup_memory: false,
        })
    }
}

// We will reuse render commands already defined by bevy to draw a 3d mesh
type DrawMesh3dStencil = (
    SetItemPipeline,
    // This will set the view bindings in group 0
    SetMeshViewBindGroup<0>,
    // This will set the mesh bindings in group 1
    SetMeshBindGroup<1>,
    // This will draw the mesh
    DrawMesh,
);

// This is the data required when you define a custom phase in bevy. More specifically this is the
// data required when using a ViewSortedRenderPhase. This would look differently if we wanted a
// batched render phase. Sorted phase are a bit easier to implement, but a batched phased would
// look similar.
//
// If you want to see how a batched phase implementation looks, you should look at the Opaque2d
// phase.
struct StencilPhase {
    pub sort_key: FloatOrd,
    pub entity: (Entity, MainEntity),
    pub pipeline: CachedRenderPipelineId,
    pub draw_function: DrawFunctionId,
    pub batch_range: Range<u32>,
    pub extra_index: PhaseItemExtraIndex,
}

// For more information about writing a phase item, please look at the custom_phase_item example
impl PhaseItem for StencilPhase {
    #[inline]
    fn entity(&self) -> Entity {
        self.entity.0
    }

    #[inline]
    fn main_entity(&self) -> MainEntity {
        self.entity.1
    }

    #[inline]
    fn draw_function(&self) -> DrawFunctionId {
        self.draw_function
    }

    #[inline]
    fn batch_range(&self) -> &Range<u32> {
        &self.batch_range
    }

    #[inline]
    fn batch_range_mut(&mut self) -> &mut Range<u32> {
        &mut self.batch_range
    }

    #[inline]
    fn extra_index(&self) -> PhaseItemExtraIndex {
        self.extra_index.clone()
    }

    #[inline]
    fn batch_range_and_extra_index_mut(&mut self) -> (&mut Range<u32>, &mut PhaseItemExtraIndex) {
        (&mut self.batch_range, &mut self.extra_index)
    }
}

impl SortedPhaseItem for StencilPhase {
    type SortKey = FloatOrd;

    #[inline]
    fn sort_key(&self) -> Self::SortKey {
        self.sort_key
    }

    #[inline]
    fn sort(items: &mut [Self]) {
        // bevy normally uses radsort instead of the std slice::sort_by_key
        // radsort is a stable radix sort that performed better than `slice::sort_by_key` or `slice::sort_unstable_by_key`.
        // Since it is not re-exported by bevy, we just use the std sort for the purpose of the example
        items.sort_by_key(SortedPhaseItem::sort_key);
    }
}

impl CachedRenderPipelinePhaseItem for StencilPhase {
    #[inline]
    fn cached_pipeline(&self) -> CachedRenderPipelineId {
        self.pipeline
    }
}

impl GetBatchData for StencilPipeline {
    type Param = (
        SRes<RenderMeshInstances>,
        SRes<RenderAssets<RenderMesh>>,
        SRes<MeshAllocator>,
    );
    type CompareData = AssetId<Mesh>;
    type BufferData = MeshUniform;

    fn get_batch_data(
        (mesh_instances, _render_assets, mesh_allocator): &SystemParamItem<Self::Param>,
        (_entity, main_entity): (Entity, MainEntity),
    ) -> Option<(Self::BufferData, Option<Self::CompareData>)> {
        let RenderMeshInstances::CpuBuilding(ref mesh_instances) = **mesh_instances else {
            error!(
                "`get_batch_data` should never be called in GPU mesh uniform \
                building mode"
            );
            return None;
        };
        let mesh_instance = mesh_instances.get(&main_entity)?;
        let first_vertex_index =
            match mesh_allocator.mesh_vertex_slice(&mesh_instance.mesh_asset_id) {
                Some(mesh_vertex_slice) => mesh_vertex_slice.range.start,
                None => 0,
            };
        Some((
            MeshUniform::new(
                &mesh_instance.transforms,
                first_vertex_index,
                // TODO don't hardcode this
                MaterialBindGroupSlot(0),
                None,
                None,
                None,
            ),
            None,
        ))
    }
}
impl GetFullBatchData for StencilPipeline {
    type BufferInputData = MeshInputUniform;

    fn get_index_and_compare_data(
        (mesh_instances, _, _): &SystemParamItem<Self::Param>,
        (_entity, main_entity): (Entity, MainEntity),
    ) -> Option<(NonMaxU32, Option<Self::CompareData>)> {
        // This should only be called during GPU building.
        let RenderMeshInstances::GpuBuilding(ref mesh_instances) = **mesh_instances else {
            error!(
                "`get_index_and_compare_data` should never be called in CPU mesh uniform building \
                mode"
            );
            return None;
        };
        let mesh_instance = mesh_instances.get(&main_entity)?;
        Some((
            mesh_instance.current_uniform_index,
            mesh_instance
                .should_batch()
                .then_some(mesh_instance.mesh_asset_id),
        ))
    }

    fn get_binned_batch_data(
        (mesh_instances, _render_assets, mesh_allocator): &SystemParamItem<Self::Param>,
        (_entity, main_entity): (Entity, MainEntity),
    ) -> Option<Self::BufferData> {
        let RenderMeshInstances::CpuBuilding(ref mesh_instances) = **mesh_instances else {
            error!(
                "`get_binned_batch_data` should never be called in GPU mesh uniform building mode"
            );
            return None;
        };
        let mesh_instance = mesh_instances.get(&main_entity)?;
        let first_vertex_index =
            match mesh_allocator.mesh_vertex_slice(&mesh_instance.mesh_asset_id) {
                Some(mesh_vertex_slice) => mesh_vertex_slice.range.start,
                None => 0,
            };

        Some(MeshUniform::new(
            &mesh_instance.transforms,
            first_vertex_index,
            mesh_instance.material_bindings_index.slot,
            None,
            None,
            None,
        ))
    }

    // TODO

    fn get_binned_index(
        (_, _, _): &SystemParamItem<Self::Param>,
        (_entity, _main_entity): (Entity, MainEntity),
    ) -> Option<NonMaxU32> {
        None
    }

    fn get_batch_indirect_parameters_index(
        (_, _, _): &SystemParamItem<Self::Param>,
        _indirect_parameters_buffer: &mut IndirectParametersBuffer,
        _entity: (Entity, MainEntity),
        _instance_index: u32,
    ) -> Option<NonMaxU32> {
        None
    }
}
// When defining a custom phase, we need to extract it from the main world and add it to a resource
// that will be used by the render world. We need to give that resource all views that will use
// that phase
fn extract_camera_phases(
    mut custom_phases: ResMut<ViewSortedRenderPhases<StencilPhase>>,
    cameras: Extract<Query<(RenderEntity, &Camera), With<Camera3d>>>,
    mut live_entities: Local<EntityHashSet>,
) {
    live_entities.clear();
    for (entity, camera) in &cameras {
        if !camera.is_active {
            continue;
        }
        custom_phases.insert_or_clear(entity);
        live_entities.insert(entity);
    }
    // Clear out all dead views.
    custom_phases.retain(|camera_entity, _| live_entities.contains(camera_entity));
}

// This is a very important step when writing a custom phase.
//
// This system determines which mesh will be added to the phase.
#[allow(clippy::too_many_arguments)]
fn queue_custom_meshes(
    custom_draw_functions: Res<DrawFunctions<StencilPhase>>,
    mut pipelines: ResMut<SpecializedMeshPipelines<StencilPipeline>>,
    pipeline_cache: Res<PipelineCache>,
    custom_draw_pipeline: Res<StencilPipeline>,
    render_meshes: Res<RenderAssets<RenderMesh>>,
    render_mesh_instances: Res<RenderMeshInstances>,
    mut custom_render_phases: ResMut<ViewSortedRenderPhases<StencilPhase>>,
    mut views: Query<(Entity, &ExtractedView, &RenderVisibleEntities, &Msaa)>,
    has_marker: Query<(), With<DrawStencil>>,
) {
    for (view_entity, view, visible_entities, msaa) in &mut views {
        let Some(custom_phase) = custom_render_phases.get_mut(&view_entity) else {
            continue;
        };
        let draw_custom = custom_draw_functions.read().id::<DrawMesh3dStencil>();

        // Create the key based on the view.
        // In this case we only care about MSAA and HDR
        let view_key = MeshPipelineKey::from_msaa_samples(msaa.samples())
            | MeshPipelineKey::from_hdr(view.hdr);

        let rangefinder = view.rangefinder3d();
        // Since our phase can work on any 3d mesh we can reuse the default mesh 2d filter
        for (render_entity, visible_entity) in visible_entities.iter::<Mesh3d>() {
            // We only want meshes with the marker component to be queued to our phase.
            if has_marker.get(*render_entity).is_err() {
                continue;
            }
            let Some(mesh_instance) = render_mesh_instances.render_mesh_queue_data(*visible_entity)
            else {
                continue;
            };
            let Some(mesh) = render_meshes.get(mesh_instance.mesh_asset_id) else {
                continue;
            };

            // Specialize the key for the current mesh entity
            // For this example we only specialize based on the mesh topology
            // but you could have more complex keys and that's where you'd need to create those keys
            let mut mesh_key = view_key;
            mesh_key |= MeshPipelineKey::from_primitive_topology(mesh.primitive_topology());

            let pipeline_id = pipelines.specialize(
                &pipeline_cache,
                &custom_draw_pipeline,
                mesh_key,
                &mesh.layout,
            );
            let pipeline_id = match pipeline_id {
                Ok(id) => id,
                Err(err) => {
                    error!("{}", err);
                    continue;
                }
            };
            let distance = rangefinder.distance_translation(&mesh_instance.translation);
            // At this point we have all the data we need to create a phase item and add it to our
            // phase
            custom_phase.add(StencilPhase {
                // Sort the data based on the distance to the view
                sort_key: FloatOrd(distance),
                entity: (*render_entity, *visible_entity),
                pipeline: pipeline_id,
                draw_function: draw_custom,
                // Sorted phase items aren't batched
                batch_range: 0..1,
                extra_index: PhaseItemExtraIndex::None,
            });
        }
    }
}

// Render label used to order our render graph node that will render our phase
#[derive(RenderLabel, Debug, Clone, Hash, PartialEq, Eq)]
struct CustomDrawPassLabel;

#[derive(Default)]
struct CustomDrawNode;
impl ViewNode for CustomDrawNode {
    type ViewQuery = (&'static ExtractedCamera, &'static ViewTarget);

    fn run<'w>(
        &self,
        graph: &mut RenderGraphContext,
        render_context: &mut RenderContext<'w>,
        (camera, target): QueryItem<'w, Self::ViewQuery>,
        world: &'w World,
    ) -> Result<(), NodeRunError> {
        // First, we need to get ou phases resource
        let Some(stencil_phases) = world.get_resource::<ViewSortedRenderPhases<StencilPhase>>()
        else {
            return Ok(());
        };
        // Initiazlie diagnostic recording.
        // not reguired but makes profiling easier
        let diagnostics = render_context.diagnostic_recorder();

        // For the purpose of the example, we will write directly to the view target. A real
        // stencil pass would write to a custom texture and that texture would be used in later
        // passes to render custom effects using it.
        let color_attachments = [Some(target.get_color_attachment())];

        // Get the view entity from the graph
        let view_entity = graph.view_entity();

        // Get the phase for the current view running our node
        let Some(stencil_phase) = stencil_phases.get(&view_entity) else {
            return Ok(());
        };

        // This will generate a task to generate the command buffer in parallel
        render_context.add_command_buffer_generation_task(move |render_device| {
            #[cfg(feature = "trace")]
            let _ = info_span!("custom_stencil_pass").entered();

            // Command encoder setup
            let mut command_encoder =
                render_device.create_command_encoder(&CommandEncoderDescriptor {
                    label: Some("custom stencil pass encoder"),
                });

            // Render pass setup
            let render_pass = command_encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("stencil pass"),
                color_attachments: &color_attachments,
                // We don't bind any depth buffer for this pass
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            let mut render_pass = TrackedRenderPass::new(&render_device, render_pass);
            let pass_span = diagnostics.pass_span(&mut render_pass, "custom_pass");

            if let Some(viewport) = camera.viewport.as_ref() {
                render_pass.set_camera_viewport(viewport);
            }

            // Render the phase
            if !stencil_phase.items.is_empty() {
                if let Err(err) = stencil_phase.render(&mut render_pass, world, view_entity) {
                    error!("Error encountered while rendering the custom phase {err:?}");
                }
            }

            pass_span.end(&mut render_pass);
            drop(render_pass);
            command_encoder.finish()
        });

        Ok(())
    }
}