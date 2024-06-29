use bevy_app::{Last, Plugin, Startup};
use bevy_asset::{load_internal_asset, Handle};
use bevy_derive::Deref;
use bevy_ecs::{
    prelude::*,
    query::ROQueryItem,
    system::{lifetimeless::SRes, SystemParamItem},
};
use bevy_math::UVec2;
use bevy_render::{
    camera::ExtractedCamera,
    extract_component::{ExtractComponent, ExtractComponentPlugin},
    render_graph::{RenderGraphApp, ViewNodeRunner},
    render_phase::{PhaseItem, RenderCommand, RenderCommandResult, TrackedRenderPass},
    render_resource::{
        binding_types::storage_buffer_sized, BindGroup, BindGroupLayout, BindGroupLayoutEntries,
        BufferUsages, BufferVec, Shader, ShaderStages, TextureUsages,
    },
    renderer::{RenderDevice, RenderQueue},
    view::Msaa,
    Render, RenderApp, RenderSet,
};
use bevy_utils::tracing::warn;
use node::{OitNode, OitPass};
use resolve::OitResolvePlugin;

use crate::core_3d::{
    graph::{Core3d, Node3d},
    Camera3d,
};

pub mod node;
pub mod resolve;

pub const OIT_DRAW_SHADER_HANDLE: Handle<Shader> = Handle::weak_from_u128(4042527984320512);

#[derive(Component, Clone, Copy, ExtractComponent)]
pub struct OrderIndependentTransparencySettings {
    layer_count: u8,
}

impl Default for OrderIndependentTransparencySettings {
    fn default() -> Self {
        Self { layer_count: 8 }
    }
}

pub struct OrderIndependentTransparencyPlugin;
impl Plugin for OrderIndependentTransparencyPlugin {
    fn build(&self, app: &mut bevy_app::App) {
        load_internal_asset!(
            app,
            OIT_DRAW_SHADER_HANDLE,
            "oit_draw.wgsl",
            Shader::from_wgsl
        );

        app.add_plugins((
            ExtractComponentPlugin::<OrderIndependentTransparencySettings>::default(),
            OitResolvePlugin,
        ))
        .add_systems(Startup, check_msaa)
        .add_systems(Last, configure_depth_texture_usages);

        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };

        render_app.add_systems(
            Render,
            prepare_oit_buffers.in_set(RenderSet::PrepareResources),
        );

        render_app
            .add_render_graph_node::<ViewNodeRunner<OitNode>>(Core3d, OitPass)
            .add_render_graph_edges(Core3d, (Node3d::MainTransparentPass, OitPass));
    }

    fn finish(&self, app: &mut bevy_app::App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };

        render_app
            .init_resource::<OitLayersBindGroupLayout>()
            .init_resource::<OitBuffers>();
    }
}

#[derive(Resource)]
pub struct OitBuffers {
    pub layers: BufferVec<UVec2>,
    pub layer_ids: BufferVec<i32>,
}

impl FromWorld for OitBuffers {
    fn from_world(world: &mut World) -> Self {
        let render_device = world.resource::<RenderDevice>();
        let render_queue = world.resource::<RenderQueue>();

        // initialize buffers with something so there's a valid binding

        let mut layers = BufferVec::new(BufferUsages::COPY_DST | BufferUsages::STORAGE);
        layers.reserve(0, render_device);
        layers.write_buffer(render_device, render_queue);

        let mut layer_ids = BufferVec::new(BufferUsages::COPY_DST | BufferUsages::STORAGE);
        layer_ids.reserve(0, render_device);
        layer_ids.write_buffer(render_device, render_queue);

        Self { layers, layer_ids }
    }
}

// WARN This should only happen for cameras with the [`OrderIndependentTransparencySettings`]
// but when multiple cameras are present on the same window
// bevy reuses the same depth texture so we need to set this on all cameras.
// TODO do the same for 2d cameras once they have a depth texture
fn configure_depth_texture_usages(mut new_cameras: Query<&mut Camera3d, Added<Camera3d>>) {
    for mut camera in &mut new_cameras {
        let mut usages = TextureUsages::from(camera.depth_texture_usages);
        usages |= TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING;
        camera.depth_texture_usages = usages.into();
    }
}

// TODO check if any cameras has the OIT component
fn check_msaa(msaa: Res<Msaa>) {
    if msaa.samples() > 1 {
        warn!(
            "MSAA should be disabled when using the OrderIndependentTransparencyPlugin. \
            It will cause some rendering issues on some platform. Consider using another AA method."
        );
    }
}

#[derive(Resource)]
pub struct OitLayersBindGroupLayout(pub BindGroupLayout);
impl FromWorld for OitLayersBindGroupLayout {
    fn from_world(world: &mut World) -> Self {
        let render_device = world.resource::<RenderDevice>();
        let layout = render_device.create_bind_group_layout(
            "oit_layers_bind_group_layout",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (
                    // layers
                    storage_buffer_sized(false, None),
                    // layer ids
                    storage_buffer_sized(false, None),
                ),
            ),
        );
        Self(layout)
    }
}

#[derive(Resource, Deref)]
pub struct OitLayersBindGroup(pub BindGroup);

pub struct SetOitLayersBindGroup<const I: usize>;
impl<P: PhaseItem, const I: usize> RenderCommand<P> for SetOitLayersBindGroup<I> {
    type Param = SRes<OitLayersBindGroup>;
    type ViewQuery = ();
    type ItemQuery = ();

    #[inline]
    fn render<'w>(
        _item: &P,
        _view: ROQueryItem<'w, Self::ViewQuery>,
        _entity: Option<ROQueryItem<'w, Self::ItemQuery>>,
        bind_group: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        pass.set_bind_group(I, bind_group.into_inner(), &[]);
        RenderCommandResult::Success
    }
}

/// This creates the required buffers for each camera
#[allow(clippy::type_complexity)]
pub fn prepare_oit_buffers(
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    cameras: Query<
        (&ExtractedCamera, &OrderIndependentTransparencySettings),
        (
            Changed<ExtractedCamera>,
            Changed<OrderIndependentTransparencySettings>,
        ),
    >,
    mut buffers: ResMut<OitBuffers>,
) {
    let mut max_layer_ids_size = usize::MIN;
    let mut max_layers_size = usize::MIN;
    for (camera, settings) in &cameras {
        let Some(size) = camera.physical_target_size else {
            continue;
        };

        let layer_count = settings.layer_count as usize;
        let size = (size.x * size.y) as usize;
        max_layer_ids_size = max_layer_ids_size.max(size);
        max_layers_size = max_layers_size.max(size * layer_count);
    }

    // TODO this is extremely slow when resizing
    // consider debouncing

    if buffers.layers.capacity() < max_layers_size {
        println!("layers {} < {}", buffers.layers.capacity(), max_layers_size);
        let remaining = max_layers_size - buffers.layers.capacity();
        for _ in 0..remaining {
            buffers.layers.push(UVec2::ZERO);
        }
        buffers.layers.write_buffer(&device, &queue);
    }

    if buffers.layer_ids.capacity() < max_layer_ids_size {
        println!(
            "ids {} < {}",
            buffers.layer_ids.capacity(),
            max_layer_ids_size
        );
        let remaining = max_layer_ids_size - buffers.layer_ids.capacity();
        for _ in 0..remaining {
            buffers.layer_ids.push(0);
        }
        buffers.layer_ids.write_buffer(&device, &queue);
    }
}
