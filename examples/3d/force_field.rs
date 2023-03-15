//! A simple 3D scene with light shining over a cube sitting on a plane.

use bevy::{
    core_pipeline::{bloom::BloomSettings, prepass::DepthPrepass},
    pbr::{NotShadowCaster, NotShadowReceiver},
    prelude::*,
    reflect::TypeUuid,
    render::render_resource::{AsBindGroup, ShaderRef, ShaderType},
};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(AssetPlugin {
            watch_for_changes: true,
            ..default()
        }))
        .add_plugin(MaterialPlugin::<ForceFieldMaterial>::default())
        .add_plugin(MaterialPlugin::<PrepassOutputMaterial> {
            prepass_enabled: false,
            ..default()
        })
        .add_startup_system(setup)
        .add_system(update_show_prepass)
        .run();
}

/// set up a simple 3D scene
fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut force_field_materials: ResMut<Assets<ForceFieldMaterial>>,
    mut depth_materials: ResMut<Assets<PrepassOutputMaterial>>,
) {
    // plane
    commands.spawn(PbrBundle {
        mesh: meshes.add(shape::Plane::from_size(5.0).into()),
        material: materials.add(Color::rgb(0.3, 0.5, 0.3).into()),
        ..default()
    });
    // wall back
    // commands.spawn(PbrBundle {
    //     mesh: meshes.add(shape::Plane::from_size(5.0).into()),
    //     material: materials.add(Color::WHITE.into()),
    //     transform: Transform::from_rotation(Quat::from_axis_angle(
    //         Vec3::X,
    //         std::f32::consts::FRAC_PI_2,
    //     ))
    //     .with_translation(Vec3::new(0.0, 0.0, -0.5)),
    //     ..default()
    // });
    // wall right
    // let wall_size = 4.0;
    // commands.spawn(PbrBundle {
    //     mesh: meshes.add(shape::Plane::from_size(wall_size).into()),
    //     material: materials.add(Color::WHITE.into()),
    //     transform: Transform::from_rotation(Quat::from_axis_angle(
    //         Vec3::Z,
    //         std::f32::consts::FRAC_PI_2,
    //     ))
    //     .with_translation(Vec3::new(1.0, wall_size / 2.0, 0.0)),
    //     ..default()
    // });
    //cube
    commands.spawn(PbrBundle {
        mesh: meshes.add(shape::Cube::new(0.5).into()),
        material: materials.add(Color::WHITE.into()),
        transform: Transform::from_xyz(0.0, 0.25, 0.0),
        ..default()
    });
    // sphere
    commands.spawn((
        MaterialMeshBundle {
            mesh: meshes.add(
                shape::UVSphere {
                    radius: 1.25,
                    sectors: 64,
                    stacks: 64,
                }
                .into(),
                // shape::Icosphere {
                //     radius: 1.5,
                //     subdivisions: 42,
                // }
                // .try_into()
                // .unwrap(),
            ),
            material: force_field_materials.add(ForceFieldMaterial {}),
            transform: Transform::from_xyz(0.0, 0.5, 0.0)
                .with_rotation(Quat::from_axis_angle(Vec3::X, std::f32::consts::FRAC_PI_2)),
            ..default()
        },
        NotShadowReceiver,
        NotShadowCaster,
    ));
    // Quad to show the depth prepass
    commands.spawn((
        MaterialMeshBundle {
            mesh: meshes.add(shape::Quad::new(Vec2::new(20.0, 20.0)).into()),
            material: depth_materials.add(PrepassOutputMaterial {
                settings: ShowPrepassSettings::default(),
            }),
            transform: Transform::from_xyz(-0.75, 1.25, 3.0)
                .looking_at(Vec3::new(2.0, -2.5, -5.0), Vec3::Y),
            ..default()
        },
        NotShadowCaster,
    ));
    // light
    commands.spawn(PointLightBundle {
        point_light: PointLight {
            intensity: 1500.0,
            shadows_enabled: true,
            ..default()
        },
        transform: Transform::from_xyz(4.0, 8.0, 4.0),
        ..default()
    });
    // camera
    commands.spawn((
        Camera3dBundle {
            camera: Camera {
                hdr: true,
                ..default()
            },
            transform: Transform::from_xyz(-2.0, 2.5, 5.0).looking_at(Vec3::ZERO, Vec3::Y),
            ..default()
        },
        DepthPrepass,
        BloomSettings::default(),
    ));
}

// This is the struct that will be passed to your shader
#[derive(AsBindGroup, TypeUuid, Debug, Clone)]
#[uuid = "30be97fb-a62f-4000-a9f9-f85ca7607272"]
pub struct ForceFieldMaterial {}

impl Material for ForceFieldMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/force_field.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Add
    }

    fn specialize(
        _pipeline: &bevy::pbr::MaterialPipeline<Self>,
        descriptor: &mut bevy::render::render_resource::RenderPipelineDescriptor,
        _layout: &bevy::render::mesh::MeshVertexBufferLayout,
        _key: bevy::pbr::MaterialPipelineKey<Self>,
    ) -> Result<(), bevy::render::render_resource::SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}
#[derive(Debug, Clone, Default, ShaderType)]
struct ShowPrepassSettings {
    show_depth: u32,
    show_normals: u32,
    padding_1: u32,
    padding_2: u32,
}

// This shader simply loads the prepass texture and outputs it directly
#[derive(AsBindGroup, TypeUuid, Debug, Clone)]
#[uuid = "0af99895-b96e-4451-bc12-c6b1c1c52750"]
pub struct PrepassOutputMaterial {
    #[uniform(0)]
    settings: ShowPrepassSettings,
}

impl Material for PrepassOutputMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/show_prepass.wgsl".into()
    }

    // This needs to be transparent in order to show the scene behind the mesh
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }
}

/// Every time you press space, it will cycle between transparent, depth and normals view
fn update_show_prepass(
    keycode: Res<Input<KeyCode>>,
    material_handle: Query<&Handle<PrepassOutputMaterial>>,
    mut materials: ResMut<Assets<PrepassOutputMaterial>>,
) {
    if keycode.just_pressed(KeyCode::Space) {
        let handle = material_handle.single();
        let mat = materials.get_mut(handle).unwrap();
        if mat.settings.show_depth == 1 {
            mat.settings.show_depth = 0;
        } else {
            mat.settings.show_depth = 1;
        }
    }
}
