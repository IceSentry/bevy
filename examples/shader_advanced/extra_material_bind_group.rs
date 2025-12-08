//! Demonstrates how to define and use specialized mesh pipeline
//!
//! This example shows how to use the built-in [`SpecializedMeshPipeline`]
//! functionality with a custom [`RenderCommand`] to allow custom mesh rendering with
//! more flexibility than the material api.
//!
//! [`SpecializedMeshPipeline`] let's you customize the entire pipeline used when rendering a mesh.

use bevy::{
    camera::visibility::{self, VisibilityClass},
    core_pipeline::core_3d::{Opaque3d, Opaque3dBatchSetKey, Opaque3dBinKey},
    ecs::{
        change_detection::Tick,
        system::{lifetimeless::SRes, SystemParamItem},
    },
    mesh::MeshVertexBufferLayoutRef,
    pbr::{
        DrawMesh, MeshPipeline, MeshPipelineKey, RenderMeshInstances, SetMeshBindGroup,
        SetMeshViewBindGroup, SetMeshViewBindingArrayBindGroup,
    },
    prelude::*,
    render::{
        batching::gpu_preprocessing::GpuPreprocessingSupport,
        extract_component::{ExtractComponent, ExtractComponentPlugin},
        mesh::{allocator::MeshAllocator, RenderMesh},
        render_asset::RenderAssets,
        render_phase::{
            AddRenderCommand, BinnedRenderPhaseType, DrawFunctions, RenderCommand,
            RenderCommandResult, SetItemPipeline, TrackedRenderPass, ViewBinnedRenderPhases,
        },
        render_resource::{
            BindGroup, BindGroupLayoutDescriptor, BufferUsages, PipelineCache,
            RenderPipelineDescriptor, ShaderStages, SpecializedMeshPipeline,
            SpecializedMeshPipelineError, SpecializedMeshPipelines,
        },
        renderer::{RenderDevice, RenderQueue},
        view::{ExtractedView, RenderVisibleEntities},
        Render, RenderApp, RenderStartup, RenderSystems,
    },
    time::Time,
};
use bevy_render::render_resource::{
    binding_types::uniform_buffer, BindGroupEntries, BindGroupLayoutEntries, BufferVec,
};

const SHADER_ASSET_PATH: &str = "shaders/extra_material_bind_group.wgsl";

/// Resource that stores the prepared color bind group and buffer
#[derive(Resource)]
pub struct ColorBindGroup {
    bind_group: BindGroup,
    buffer: BufferVec<Vec4>,
}

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(CustomRenderedMeshPipelinePlugin)
        .add_systems(Startup, setup)
        .run();
}

/// Spawns the objects in the scene.
fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>) {
    let quad_mesh_handle = meshes.add(Rectangle::new(0.5, 0.5).mesh());

    let grid_size = 100;
    let spacing = 0.75;
    let grid_offset = (grid_size as f32 - 1.0) * spacing / 2.0;

    for x in 0..grid_size {
        for y in 0..grid_size {
            let world_x = x as f32 * spacing - grid_offset;
            let world_y = y as f32 * spacing - grid_offset;

            commands.spawn((
                CustomRenderedEntity,
                Mesh3d(quad_mesh_handle.clone()),
                Transform::from_xyz(world_x, world_y, 0.0),
            ));
        }
    }

    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 0.0, 100.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

// When writing custom rendering code it's generally recommended to use a plugin.
// The main reason for this is that it gives you access to the finish() hook
// which is called after rendering resources are initialized.
struct CustomRenderedMeshPipelinePlugin;
impl Plugin for CustomRenderedMeshPipelinePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(ExtractComponentPlugin::<CustomRenderedEntity>::default());

        // We make sure to add these to the render app, not the main app.
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            // This is needed to tell bevy about your custom pipeline
            .init_resource::<SpecializedMeshPipelines<CustomMeshPipeline>>()
            // We need to use a custom draw command so we need to register it
            .add_render_command::<Opaque3d, DrawSpecializedPipelineCommands>()
            .add_systems(
                RenderStartup,
                (init_custom_mesh_pipeline, prepare_color).chain(),
            )
            .add_systems(
                Render,
                (
                    update_color_buffer.in_set(RenderSystems::Prepare),
                    queue_custom_mesh_pipeline.in_set(RenderSystems::Queue),
                ),
            );
    }
}

/// A marker component that represents an entity that is to be rendered using
/// our specialized pipeline.
///
/// Note the [`ExtractComponent`] trait implementation: this is necessary to
/// tell Bevy that this object should be pulled into the render world. Also note
/// the `on_add` hook, which is needed to tell Bevy's `check_visibility` system
/// that entities with this component need to be examined for visibility.
#[derive(Clone, Component, ExtractComponent)]
#[require(VisibilityClass)]
#[component(on_add = visibility::add_visibility_class::<CustomRenderedEntity>)]
struct CustomRenderedEntity;

/// Custom render command that sets the color bind group
pub struct SetColorBindGroup<const I: usize>;

impl<const I: usize> RenderCommand<Opaque3d> for SetColorBindGroup<I> {
    type Param = SRes<ColorBindGroup>;
    type ViewQuery = ();
    type ItemQuery = ();

    #[inline]
    fn render<'w>(
        _item: &Opaque3d,
        _view: (),
        _entity: Option<()>,
        color_bind_group: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        pass.set_bind_group(I, &color_bind_group.into_inner().bind_group, &[]);
        RenderCommandResult::Success
    }
}

/// The custom draw commands that Bevy executes for each entity we enqueue into
/// the render phase.
type DrawSpecializedPipelineCommands = (
    // Set the pipeline
    SetItemPipeline,
    // Set the view uniform at bind group 0
    SetMeshViewBindGroup<0>,
    // Set an empty material bind group at bind group 1
    SetMeshViewBindingArrayBindGroup<1>,
    // Set the mesh uniform at bind group 2
    SetMeshBindGroup<2>,
    // Set the color tint uniform at bind group 3
    SetColorBindGroup<3>,
    // Draw the mesh
    DrawMesh,
);

// This contains the state needed to specialize a mesh pipeline
#[derive(Resource)]
struct CustomMeshPipeline {
    /// The base mesh pipeline defined by bevy
    ///
    /// This isn't required, but if you want to use a bevy `Mesh` it's easier when you
    /// have access to the base `MeshPipeline` that bevy already defines
    mesh_pipeline: MeshPipeline,
    /// Stores the shader used for this pipeline directly on the pipeline.
    /// This isn't required, it's only done like this for simplicity.
    shader_handle: Handle<Shader>,
    /// The descriptor for the bind group layout for the color uniform
    color_bind_group_layout_descriptor: BindGroupLayoutDescriptor,
}

fn init_custom_mesh_pipeline(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mesh_pipeline: Res<MeshPipeline>,
) {
    // Load the shader
    let shader_handle: Handle<Shader> = asset_server.load(SHADER_ASSET_PATH);

    let color_bind_group_layout_descriptor = BindGroupLayoutDescriptor::new(
        "color_bind_group_layout",
        &BindGroupLayoutEntries::single(ShaderStages::FRAGMENT, uniform_buffer::<Vec4>(false)),
    );

    commands.insert_resource(CustomMeshPipeline {
        mesh_pipeline: mesh_pipeline.clone(),
        shader_handle,
        color_bind_group_layout_descriptor,
    });
}

fn prepare_color(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    custom_pipeline: Res<CustomMeshPipeline>,
    pipeline_cache: Res<PipelineCache>,
) {
    let color = Vec4::new(0.0, 0.0, 0.0, 1.0);

    let mut buf = BufferVec::new(BufferUsages::UNIFORM | BufferUsages::COPY_DST);
    buf.push(color);
    buf.write_buffer(&render_device, &render_queue);

    // Create the bind group with the color buffer
    let bind_group = render_device.create_bind_group(
        "color_bind_group",
        &pipeline_cache.get_bind_group_layout(&custom_pipeline.color_bind_group_layout_descriptor),
        &BindGroupEntries::single(buf.binding().expect("Failed to get color buffer binding")),
    );

    commands.insert_resource(ColorBindGroup {
        bind_group,
        buffer: buf,
    });
}

/// Update the color buffer every frame by rotating through HSL color space
fn update_color_buffer(
    mut color_bind_group: ResMut<ColorBindGroup>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    time: Res<Time>,
) {
    // Rotate hue over time (360 degrees in 10 seconds)
    let hue = (time.elapsed_secs() * 36.0) % 360.0;
    let hsl = Color::hsl(hue, 1.0, 0.5);
    color_bind_group.buffer.clear();
    color_bind_group.buffer.push(hsl.to_linear().to_vec4());
    color_bind_group
        .buffer
        .write_buffer(&render_device, &render_queue);
}

impl SpecializedMeshPipeline for CustomMeshPipeline {
    type Key = MeshPipelineKey;

    fn specialize(
        &self,
        mesh_key: Self::Key,
        layout: &MeshVertexBufferLayoutRef,
    ) -> Result<RenderPipelineDescriptor, SpecializedMeshPipelineError> {
        let mut base_desc = self.mesh_pipeline.specialize(mesh_key, layout)?;

        base_desc
            .layout
            .push(self.color_bind_group_layout_descriptor.clone());

        base_desc.vertex.shader = self.shader_handle.clone();
        if let Some(fragment) = base_desc.fragment.as_mut() {
            fragment.shader = self.shader_handle.clone();
        }

        Ok(base_desc)
    }
}

/// A render-world system that enqueues the entity with custom rendering into
/// the opaque render phases of each view.
fn queue_custom_mesh_pipeline(
    pipeline_cache: Res<PipelineCache>,
    custom_mesh_pipeline: Res<CustomMeshPipeline>,
    (mut opaque_render_phases, opaque_draw_functions): (
        ResMut<ViewBinnedRenderPhases<Opaque3d>>,
        Res<DrawFunctions<Opaque3d>>,
    ),
    mut specialized_mesh_pipelines: ResMut<SpecializedMeshPipelines<CustomMeshPipeline>>,
    views: Query<(&RenderVisibleEntities, &ExtractedView, &Msaa)>,
    (render_meshes, render_mesh_instances): (
        Res<RenderAssets<RenderMesh>>,
        Res<RenderMeshInstances>,
    ),
    mut change_tick: Local<Tick>,
    mesh_allocator: Res<MeshAllocator>,
    gpu_preprocessing_support: Res<GpuPreprocessingSupport>,
) {
    // Get the id for our custom draw function
    let draw_function = opaque_draw_functions
        .read()
        .id::<DrawSpecializedPipelineCommands>();

    // Render phases are per-view, so we need to iterate over all views so that
    // the entity appears in them. (In this example, we have only one view, but
    // it's good practice to loop over all views anyway.)
    for (view_visible_entities, view, msaa) in views.iter() {
        let Some(opaque_phase) = opaque_render_phases.get_mut(&view.retained_view_entity) else {
            continue;
        };

        // Create the key based on the view. In this case we only care about MSAA and HDR
        let view_key = MeshPipelineKey::from_msaa_samples(msaa.samples())
            | MeshPipelineKey::from_hdr(view.hdr);

        // Find all the custom rendered entities that are visible from this
        // view.
        for &(render_entity, visible_entity) in
            view_visible_entities.get::<CustomRenderedEntity>().iter()
        {
            // Get the mesh instance
            let Some(mesh_instance) = render_mesh_instances.render_mesh_queue_data(visible_entity)
            else {
                continue;
            };

            // Get the mesh data
            let Some(mesh) = render_meshes.get(mesh_instance.mesh_asset_id) else {
                continue;
            };

            let (vertex_slab, index_slab) = mesh_allocator.mesh_slabs(&mesh_instance.mesh_asset_id);

            // Specialize the key for the current mesh entity
            // For this example we only specialize based on the mesh topology
            // but you could have more complex keys and that's where you'd need to create those keys
            let mut mesh_key = view_key;
            mesh_key |= MeshPipelineKey::from_primitive_topology(mesh.primitive_topology());

            // Finally, we can specialize the pipeline based on the key
            let pipeline_id = specialized_mesh_pipelines
                .specialize(
                    &pipeline_cache,
                    &custom_mesh_pipeline,
                    mesh_key,
                    &mesh.layout,
                )
                .expect("Failed to specialize mesh pipeline");

            // Bump the change tick so that Bevy is forced to rebuild the bin.
            let next_change_tick = change_tick.get() + 1;
            change_tick.set(next_change_tick);

            // Add the mesh with our specialized pipeline
            opaque_phase.add(
                Opaque3dBatchSetKey {
                    draw_function,
                    pipeline: pipeline_id,
                    material_bind_group_index: None,
                    vertex_slab: vertex_slab.unwrap_or_default(),
                    index_slab,
                    lightmap_slab: None,
                },
                // For this example we can use the mesh asset id as the bin key,
                // but you can use any asset_id as a key
                Opaque3dBinKey {
                    asset_id: mesh_instance.mesh_asset_id.into(),
                },
                (render_entity, visible_entity),
                mesh_instance.current_uniform_index,
                // This example supports batching and multi draw indirect,
                // but if your pipeline doesn't support it you can use
                // `BinnedRenderPhaseType::UnbatchableMesh`
                BinnedRenderPhaseType::mesh(
                    mesh_instance.should_batch(),
                    &gpu_preprocessing_support,
                ),
                *change_tick,
            );
        }
    }
}
