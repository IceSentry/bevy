use bevy_app::{App, Plugin};
use bevy_asset::{AddAsset, Handle};
use bevy_reflect::TypeUuid;
use bevy_render::{
    extract_component::ExtractComponentPlugin,
    render_resource::{AsBindGroup, ShaderRef},
    RenderApp,
};
use std::hash::Hash;
use std::marker::PhantomData;

pub trait PostProcessMaterial:
    AsBindGroup + Send + Sync + Clone + TypeUuid + Sized + 'static
{
    #[allow(unused_variables)]
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Default
    }
}

pub struct PostProcessMaterialPlugin<M: PostProcessMaterial> {
    pub _marker: PhantomData<M>,
}

impl<M: PostProcessMaterial> Default for PostProcessMaterialPlugin<M> {
    fn default() -> Self {
        Self {
            _marker: Default::default(),
        }
    }
}

impl<M: PostProcessMaterial> Plugin for PostProcessMaterialPlugin<M>
where
    M::Data: PartialEq + Eq + Hash + Clone,
{
    fn build(&self, app: &mut App) {
        app.add_asset::<M>()
            .add_plugin(ExtractComponentPlugin::<Handle<M>>::extract_visible());

        if let Ok(render_app) = app.get_sub_app_mut(RenderApp) {
            //
        }
    }

    fn finish(&self, app: &mut App) {
        if let Ok(render_app) = app.get_sub_app_mut(RenderApp) {
            //
        }
    }
}
