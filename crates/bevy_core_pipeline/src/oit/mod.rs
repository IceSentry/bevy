use std::ops::Range;

use bevy_app::prelude::*;
use bevy_derive::Deref;
use bevy_ecs::prelude::*;
use bevy_math::FloatOrd;
use bevy_render::{
    extract_component::{ExtractComponent, ExtractComponentPlugin},
    extract_resource::{ExtractResource, ExtractResourcePlugin},
    render_phase::{
        CachedRenderPipelinePhaseItem, DrawFunctionId, PhaseItem, PhaseItemExtraIndex,
        SortedPhaseItem,
    },
    render_resource::{BindGroup, CachedRenderPipelineId, TextureUsages},
    view::Msaa,
};
use bevy_utils::error_once;

use crate::core_3d::Camera3d;

pub mod node;

pub struct OrderIndependentTransparencyPlugin;
impl Plugin for OrderIndependentTransparencyPlugin {
    fn build(&self, app: &mut bevy_app::App) {
        app.add_plugins((
            ExtractResourcePlugin::<OitLayers>::default(),
            ExtractComponentPlugin::<OitCamera>::default(),
        ))
        .init_resource::<OitLayers>()
        .add_systems(Update, check_msaa)
        .add_systems(Last, configure_depth_texture_usages);

        // let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
        //     return;
        // };
    }
}

#[derive(Component, ExtractComponent, Clone, Copy)]
pub struct OitCamera;

/// Determines how many layers are used for OIT
#[derive(Resource, ExtractResource, Clone, Copy, Debug)]
pub struct OitLayers(pub usize);
impl Default for OitLayers {
    fn default() -> Self {
        Self(8)
    }
}

#[derive(Component, Deref)]
pub struct OitLayersBindGroup(pub BindGroup);

#[derive(Resource, Deref)]
pub struct OitViewBindGroup(pub BindGroup);

#[derive(Component, Deref, Clone, Copy)]
pub struct OitSortPipelineId(pub CachedRenderPipelineId);

/// This will make sure the depth texture can be used as a render attachment
//
// WARN This should only happen for Cameras using OIT but when multiple cameras are present on the same window
// bevy reuses the same depth texture so we need to set this on all cameras.
fn configure_depth_texture_usages(mut new_cameras: Query<&mut Camera3d, Added<Camera3d>>) {
    for mut camera in &mut new_cameras {
        let mut usages = TextureUsages::from(camera.depth_texture_usages);
        usages |= TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING;
        camera.depth_texture_usages = usages.into();
    }
}

fn check_msaa(msaa: Res<Msaa>) {
    if msaa.samples() > 1 {
        error_once!(
            "MSAA should be disabled when using the OitPlugin.\
            It will cause some rendering issues on some platform. Consider using FXAA or TAA instead"
        );
    }
}

pub struct OrderIndependentTransparent3d {
    pub distance: f32,
    pub pipeline: CachedRenderPipelineId,
    pub entity: Entity,
    pub draw_function: DrawFunctionId,
    pub batch_range: Range<u32>,
    pub extra_index: PhaseItemExtraIndex,
}

impl PhaseItem for OrderIndependentTransparent3d {
    #[inline]
    fn entity(&self) -> Entity {
        self.entity
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
        self.extra_index
    }

    #[inline]
    fn batch_range_and_extra_index_mut(&mut self) -> (&mut Range<u32>, &mut PhaseItemExtraIndex) {
        (&mut self.batch_range, &mut self.extra_index)
    }
}

impl CachedRenderPipelinePhaseItem for OrderIndependentTransparent3d {
    fn cached_pipeline(&self) -> CachedRenderPipelineId {
        self.pipeline
    }
}

// TODO eventually, this should be a `BinnedPhaseItem`
impl SortedPhaseItem for OrderIndependentTransparent3d {
    // NOTE: Values increase towards the camera. Back-to-front ordering for transparent means we need an ascending sort.
    type SortKey = FloatOrd;

    #[inline]
    fn sort_key(&self) -> Self::SortKey {
        FloatOrd(self.distance)
    }

    #[inline]
    fn sort(items: &mut [Self]) {
        radsort::sort_by_key(items, |item| item.distance);
    }
}
