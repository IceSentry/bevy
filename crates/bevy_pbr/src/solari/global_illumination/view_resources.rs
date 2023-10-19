use super::{
    SolariGlobalIlluminationPipelines, SolariGlobalIlluminationSettings, WORLD_CACHE_SIZE,
};
use crate::{bind_group_layout_entries::*, solari::SpatiotemporalBlueNoise};
use bevy_core::FrameCount;
use bevy_core_pipeline::prepass::{
    DepthPrepass, MotionVectorPrepass, NormalPrepass, ViewPrepassTextures, DEPTH_PREPASS_FORMAT,
};
use bevy_ecs::{
    component::Component,
    entity::Entity,
    query::With,
    system::{Commands, Query, Res, ResMut},
};
use bevy_math::UVec2;
use bevy_render::{
    camera::ExtractedCamera,
    render_resource::{
        BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindGroupLayoutDescriptor,
        BindGroupLayoutEntry, BindingResource, BindingType, BufferBindingType, BufferCache,
        BufferDescriptor, BufferUsages, CachedBuffer, Extent3d, ShaderStages, ShaderType,
        StorageTextureAccess, TextureDescriptor, TextureDimension, TextureFormat,
        TextureSampleType, TextureUsages, TextureViewDimension,
    },
    renderer::RenderDevice,
    texture::{CachedTexture, TextureCache},
    view::{ViewUniform, ViewUniforms},
};
use std::num::NonZeroU64;

#[derive(Component)]
pub struct SolariGlobalIlluminationViewResources {
    pub previous_depth_buffer: CachedTexture,
    screen_probes_history: CachedTexture,
    screen_probes: CachedTexture,
    screen_probes_confidence_history: CachedTexture,
    screen_probes_confidence: CachedTexture,
    screen_probes_merge_buffer: CachedTexture,
    screen_probes_spherical_harmonics: CachedBuffer,
    pub diffuse_irradiance_output: CachedTexture,
    world_cache_checksums: CachedBuffer,
    world_cache_life: CachedBuffer,
    world_cache_irradiance: CachedBuffer,
    world_cache_cell_data: CachedBuffer,
    world_cache_active_cells_new_irradiance: CachedBuffer,
    world_cache_a: CachedBuffer,
    world_cache_b: CachedBuffer,
    world_cache_active_cell_indices: CachedBuffer,
    world_cache_active_cells_count: CachedBuffer,
    pub world_cache_active_cells_dispatch: CachedBuffer,
}

pub fn prepare_resources(
    views: Query<
        (Entity, &ExtractedCamera),
        (
            With<SolariGlobalIlluminationSettings>,
            With<DepthPrepass>,
            With<NormalPrepass>,
            With<MotionVectorPrepass>,
        ),
    >,
    mut commands: Commands,
    mut texture_cache: ResMut<TextureCache>,
    mut buffer_cache: ResMut<BufferCache>,
    render_device: Res<RenderDevice>,
    frame_count: Res<FrameCount>,
) {
    let texture = |label, format, size: UVec2| TextureDescriptor {
        label: Some(label),
        size: Extent3d {
            width: size.x,
            height: size.y,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format,
        usage: TextureUsages::STORAGE_BINDING,
        view_formats: &[],
    };
    let buffer = |label, size| BufferDescriptor {
        label: Some(label),
        size,
        usage: BufferUsages::STORAGE,
        mapped_at_creation: false,
    };

    for (entity, camera) in &views {
        let Some(viewport_size) = camera.physical_viewport_size else {
            continue;
        };
        let probe_atlas_size = UVec2::new(
            round_up_to_multiple_of_64(viewport_size.x),
            round_up_to_multiple_of_64(viewport_size.y),
        );
        let cascade_0_probe_count = (round_up_to_multiple_of_8(viewport_size.x) as u64
            * round_up_to_multiple_of_8(viewport_size.y) as u64)
            / 64;

        let previous_depth_buffer = TextureDescriptor {
            label: Some("solari_previous_depth_buffer"),
            size: Extent3d {
                width: viewport_size.x,
                height: viewport_size.y,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: DEPTH_PREPASS_FORMAT,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        };

        let (screen_probes_history, screen_probes) = {
            let mut t1 = texture(
                "solari_global_illumination_screen_probes_1",
                TextureFormat::Rgba16Float,
                probe_atlas_size,
            );
            t1.usage |= TextureUsages::TEXTURE_BINDING;
            t1.size.depth_or_array_layers = 4;

            let t2 = TextureDescriptor {
                label: Some("solari_global_illumination_screen_probes_2"),
                ..t1
            };
            if frame_count.0 % 2 == 0 {
                (t1, t2)
            } else {
                (t2, t1)
            }
        };
        let (screen_probes_confidence_history, screen_probes_confidence) = {
            let mut t1 = texture(
                "solari_global_illumination_screen_probes_confidence_1",
                TextureFormat::R8Uint,
                probe_atlas_size,
            );
            t1.usage |= TextureUsages::TEXTURE_BINDING;
            t1.size.depth_or_array_layers = 4;

            let t2 = TextureDescriptor {
                label: Some("solari_global_illumination_screen_probes_confidence_2"),
                ..t1
            };
            if frame_count.0 % 2 == 0 {
                (t1, t2)
            } else {
                (t2, t1)
            }
        };
        let mut screen_probes_merge_buffer = texture(
            "solari_global_illumination_screen_probes_merge_buffer",
            TextureFormat::Rgba16Float,
            probe_atlas_size,
        );
        screen_probes_merge_buffer.size.depth_or_array_layers = 2;
        let screen_probes_spherical_harmonics = buffer(
            "solari_global_illumination_screen_probes_spherical_harmonics",
            cascade_0_probe_count * 112,
        );
        let mut diffuse_irradiance_output = texture(
            "solari_global_illumination_diffuse_irradiance_output",
            TextureFormat::Rgba16Float,
            viewport_size,
        );
        diffuse_irradiance_output.usage |= TextureUsages::TEXTURE_BINDING;

        let world_cache_checksums = buffer(
            "solari_global_illumination_world_cache_checksums",
            4 * WORLD_CACHE_SIZE,
        );
        let world_cache_life = buffer(
            "solari_global_illumination_world_cache_life",
            4 * WORLD_CACHE_SIZE,
        );
        let world_cache_irradiance = buffer(
            "solari_global_illumination_world_cache_irradiance",
            16 * WORLD_CACHE_SIZE,
        );
        let world_cache_cell_data = buffer(
            "solari_global_illumination_world_cache_cell_data",
            32 * WORLD_CACHE_SIZE,
        );
        let world_cache_active_cells_new_irradiance = buffer(
            "solari_global_illumination_world_cache_active_cells_new_irradiance",
            16 * WORLD_CACHE_SIZE,
        );
        let world_cache_a = buffer(
            "solari_global_illumination_world_cache_a",
            4 * WORLD_CACHE_SIZE,
        );
        let world_cache_b = buffer("solari_global_illumination_world_cache_b", 4 * 1024);
        let world_cache_active_cell_indices = buffer(
            "solari_global_illumination_world_cache_active_cell_indices",
            4 * WORLD_CACHE_SIZE,
        );
        let world_cache_active_cells_count = buffer(
            "solari_global_illumination_world_cache_active_cells_count",
            4,
        );
        let world_cache_active_cells_dispatch = BufferDescriptor {
            label: Some("solari_global_illumination_world_cache_active_cells_dispatch"),
            size: 12,
            usage: BufferUsages::STORAGE | BufferUsages::INDIRECT,
            mapped_at_creation: false,
        };

        commands
            .entity(entity)
            .insert(SolariGlobalIlluminationViewResources {
                previous_depth_buffer: texture_cache.get(&render_device, previous_depth_buffer),
                screen_probes_history: texture_cache.get(&render_device, screen_probes_history),
                screen_probes: texture_cache.get(&render_device, screen_probes),
                screen_probes_confidence_history: texture_cache
                    .get(&render_device, screen_probes_confidence_history),
                screen_probes_confidence: texture_cache
                    .get(&render_device, screen_probes_confidence),
                screen_probes_merge_buffer: texture_cache
                    .get(&render_device, screen_probes_merge_buffer),
                screen_probes_spherical_harmonics: buffer_cache
                    .get(&render_device, screen_probes_spherical_harmonics),
                diffuse_irradiance_output: texture_cache
                    .get(&render_device, diffuse_irradiance_output),
                world_cache_checksums: buffer_cache.get(&render_device, world_cache_checksums),
                world_cache_life: buffer_cache.get(&render_device, world_cache_life),
                world_cache_irradiance: buffer_cache.get(&render_device, world_cache_irradiance),
                world_cache_cell_data: buffer_cache.get(&render_device, world_cache_cell_data),
                world_cache_active_cells_new_irradiance: buffer_cache
                    .get(&render_device, world_cache_active_cells_new_irradiance),
                world_cache_a: buffer_cache.get(&render_device, world_cache_a),
                world_cache_b: buffer_cache.get(&render_device, world_cache_b),
                world_cache_active_cell_indices: buffer_cache
                    .get(&render_device, world_cache_active_cell_indices),
                world_cache_active_cells_count: buffer_cache
                    .get(&render_device, world_cache_active_cells_count),
                world_cache_active_cells_dispatch: buffer_cache
                    .get(&render_device, world_cache_active_cells_dispatch),
            });
    }
}
pub fn create_bind_group_layouts(
    render_device: &RenderDevice,
) -> (BindGroupLayout, BindGroupLayout) {
    let entries = BindGroupLayoutEntries::sequential(
        ShaderStages::COMPUTE,
        (
            // View
            uniform_buffer(true, Some(ViewUniform::min_size())),
            // Spatiotemporal blue noise
            texture_2d_array(TextureSampleType::Float { filterable: false }),
            // Previous depth buffer
            texture_depth_2d(),
            // Depth buffer
            texture_depth_2d(),
            // Normals buffer
            texture_2d(TextureSampleType::Float { filterable: false }),
            // Motion vectors
            texture_2d(TextureSampleType::Float { filterable: false }),
            // Screen probes history
            texture_2d_array(TextureSampleType::Float { filterable: false }),
            // Screen probes
            texture_storage_2d_array(TextureFormat::Rgba16Float, StorageTextureAccess::ReadWrite),
            // Screen probes confidence history
            texture_2d_array(TextureSampleType::Uint),
            // Screen probes confidence
            texture_storage_2d_array(TextureFormat::R8Uint, StorageTextureAccess::WriteOnly),
            // Screen probes merge buffer
            texture_storage_2d_array(TextureFormat::Rgba16Float, StorageTextureAccess::ReadWrite),
            // Screen probe spherical harmonics
            storage_buffer(false, Some(unsafe { NonZeroU64::new_unchecked(112) })),
            // Diffuse irradiance output
            texture_storage_2d(TextureFormat::Rgba16Float, StorageTextureAccess::WriteOnly),
            // World cache checksums
            storage_buffer(false, Some(unsafe { NonZeroU64::new_unchecked(4) })),
            // World cache life
            storage_buffer(false, Some(unsafe { NonZeroU64::new_unchecked(4) })),
            // World cache irradiance
            storage_buffer(false, Some(unsafe { NonZeroU64::new_unchecked(16) })),
            // World cache cell data
            storage_buffer(false, Some(unsafe { NonZeroU64::new_unchecked(32) })),
            // World cache active cells new irradiance
            storage_buffer(false, Some(unsafe { NonZeroU64::new_unchecked(16) })),
            // World cache a
            storage_buffer(false, Some(unsafe { NonZeroU64::new_unchecked(4) })),
            // World cache b
            storage_buffer(false, Some(unsafe { NonZeroU64::new_unchecked(4) })),
            // World cache active cell indices
            storage_buffer(false, Some(unsafe { NonZeroU64::new_unchecked(4) })),
            // World cache active cells count
            storage_buffer(false, Some(unsafe { NonZeroU64::new_unchecked(4) })),
            // World cache active cells dispatch
            storage_buffer(false, Some(unsafe { NonZeroU64::new_unchecked(12) })),
        ),
    );

    (
        render_device.create_bind_group_layout_ext(
            "solari_global_illumination_view_bind_group_layout",
            &entries[0..entries.len() - 1],
        ),
        render_device.create_bind_group_layout_ext(
            "solari_global_illumination_view_with_world_cache_dispatch_bind_group_layout",
            &entries,
        ),
    )
}

#[derive(Component)]
pub struct SolariGlobalIlluminationBindGroups {
    pub view_bind_group: BindGroup,
    pub view_with_world_cache_dispatch_bind_group: BindGroup,
}

pub(crate) fn prepare_bind_groups(
    views: Query<(
        Entity,
        &SolariGlobalIlluminationViewResources,
        &ViewPrepassTextures,
    )>,
    view_uniforms: Res<ViewUniforms>,
    noise: Res<SpatiotemporalBlueNoise>,
    pipelines: Res<SolariGlobalIlluminationPipelines>,
    mut commands: Commands,
    render_device: Res<RenderDevice>,
) {
    let Some(view_uniforms) = view_uniforms.uniforms.binding() else {
        return;
    };

    for (entity, solari_resources, prepass_textures) in &views {
        let mut entry_i = 0;
        let mut entry = |resource| {
            entry_i += 1;
            BindGroupEntry {
                binding: entry_i - 1,
                resource,
            }
        };

        let entries = &[
            entry(view_uniforms.clone()),
            entry(BindingResource::TextureView(&noise.0)),
            entry(t(&solari_resources.previous_depth_buffer)),
            entry(t(prepass_textures.depth.as_ref().unwrap())),
            entry(t(prepass_textures.normal.as_ref().unwrap())),
            entry(t(prepass_textures.motion_vectors.as_ref().unwrap())),
            entry(t(&solari_resources.screen_probes_history)),
            entry(t(&solari_resources.screen_probes)),
            entry(t(&solari_resources.screen_probes_confidence_history)),
            entry(t(&solari_resources.screen_probes_confidence)),
            entry(t(&solari_resources.screen_probes_merge_buffer)),
            entry(b(&solari_resources.screen_probes_spherical_harmonics)),
            entry(t(&solari_resources.diffuse_irradiance_output)),
            entry(b(&solari_resources.world_cache_checksums)),
            entry(b(&solari_resources.world_cache_life)),
            entry(b(&solari_resources.world_cache_irradiance)),
            entry(b(&solari_resources.world_cache_cell_data)),
            entry(b(&solari_resources.world_cache_active_cells_new_irradiance)),
            entry(b(&solari_resources.world_cache_a)),
            entry(b(&solari_resources.world_cache_b)),
            entry(b(&solari_resources.world_cache_active_cell_indices)),
            entry(b(&solari_resources.world_cache_active_cells_count)),
            entry(b(&solari_resources.world_cache_active_cells_dispatch)),
        ];

        let bind_groups = SolariGlobalIlluminationBindGroups {
            view_bind_group: render_device.create_bind_group(&BindGroupDescriptor {
                label: Some("solari_global_illumination_view_bind_group"),
                layout: &pipelines.view_bind_group_layout,
                entries: &entries[0..entries.len() - 1],
            }),
            view_with_world_cache_dispatch_bind_group: render_device.create_bind_group(
                &BindGroupDescriptor {
                    label: Some(
                        "solari_global_illumination_view_with_world_cache_dispatch_bind_group",
                    ),
                    layout: &pipelines.view_with_world_cache_dispatch_bind_group_layout,
                    entries,
                },
            ),
        };
        commands.entity(entity).insert(bind_groups);
    }
}

fn round_up_to_multiple_of_64(x: u32) -> u32 {
    (x + 63) & !63
}

fn round_up_to_multiple_of_8(x: u32) -> u32 {
    (x + 7) & !7
}

fn t(texture: &CachedTexture) -> BindingResource<'_> {
    BindingResource::TextureView(&texture.default_view)
}

fn b(buffer: &CachedBuffer) -> BindingResource<'_> {
    buffer.buffer.as_entire_binding()
}
