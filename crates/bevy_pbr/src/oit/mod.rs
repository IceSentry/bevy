use bevy_core_pipeline::oit::OitLayersBindGroup;
use bevy_derive::Deref;
use bevy_ecs::{prelude::*, query::ROQueryItem, system::SystemParamItem};
use bevy_render::{render_phase::*, render_resource::*};

use crate::{DrawMesh, SetMaterialBindGroup, SetMeshBindGroup, SetMeshViewBindGroup};

pub struct SetOitLayersBindGroup<const I: usize>;
impl<P: PhaseItem, const I: usize> RenderCommand<P> for SetOitLayersBindGroup<I> {
    type Param = ();
    type ViewQuery = &'static OitLayersBindGroup;
    type ItemQuery = ();

    #[inline]
    fn render<'w>(
        _item: &P,
        bind_group: ROQueryItem<'w, Self::ViewQuery>,
        _mesh_index: Option<ROQueryItem<'w, Self::ItemQuery>>,
        _param: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        pass.set_bind_group(I, bind_group, &[]);
        RenderCommandResult::Success
    }
}

#[derive(Component, Deref)]
pub struct DepthTextureBindGroup(pub BindGroup);

pub struct SetDepthTextureBindGroup<const I: usize>;
impl<P: PhaseItem, const I: usize> RenderCommand<P> for SetDepthTextureBindGroup<I> {
    type Param = ();
    type ViewQuery = &'static DepthTextureBindGroup;
    type ItemQuery = ();

    #[inline]
    fn render<'w>(
        _item: &P,
        bind_group: ROQueryItem<'w, Self::ViewQuery>,
        _mesh_index: Option<ROQueryItem<'w, Self::ItemQuery>>,
        _param: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        pass.set_bind_group(I, bind_group, &[]);
        RenderCommandResult::Success
    }
}

pub type DrawOit<M> = (
    SetItemPipeline,
    SetMeshViewBindGroup<0>,
    SetMaterialBindGroup<M, 1>,
    SetMeshBindGroup<2>,
    SetOitLayersBindGroup<3>,
    SetDepthTextureBindGroup<4>,
    DrawMesh,
);
