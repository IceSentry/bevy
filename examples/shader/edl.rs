//! A shader and a material that uses it.

use std::f32::consts::{FRAC_PI_2, PI};

use bevy::{
    core_pipeline::{
        fxaa::{Fxaa, Sensitivity},
        prepass::{DepthPrepass, NormalPrepass},
    },
    diagnostic::{Diagnostics, FrameTimeDiagnosticsPlugin},
    math::vec3,
    prelude::*,
    reflect::TypeUuid,
    render::render_resource::{AsBindGroup, ShaderRef},
    window::{PresentMode, WindowResolution},
};

fn main() {
    App::new()
        .insert_resource(Msaa::Sample4)
        .add_plugins(
            DefaultPlugins
                .set(AssetPlugin {
                    watch_for_changes: true,
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        resolution: WindowResolution::new(720.0, 720.0),
                        present_mode: PresentMode::AutoNoVsync,
                        ..default()
                    }),
                    ..default()
                }),
        )
        .add_plugin(MaterialPlugin::<CustomMaterial>::default())
        .add_startup_system(setup)
        .run();
}

/// set up a simple 3D scene
fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<CustomMaterial>>,
    asset_server: Res<AssetServer>,
) {
    commands.spawn((
        Camera3dBundle {
            transform: Transform::from_xyz(0.0, 2.5, -8.75)
                .looking_at(vec3(0.0, 2.5, 0.0), Vec3::Y),
            ..default()
        },
        DepthPrepass,
    ));

    let white = materials.add(CustomMaterial {
        color: Color::WHITE,
    });
    let plane_size = 5.0;
    let plane = meshes.add(shape::Plane { size: plane_size }.into());

    // bottom
    commands.spawn(MaterialMeshBundle {
        mesh: plane.clone(),
        material: white.clone(),
        transform: Transform::from_xyz(0.0, 0.0, 0.0),
        ..default()
    });
    // top
    commands.spawn(MaterialMeshBundle {
        mesh: plane.clone(),
        material: white.clone(),
        transform: Transform::from_xyz(0.0, 5.0, 0.0).with_rotation(Quat::from_rotation_x(PI)),
        ..default()
    });
    // back
    commands.spawn(MaterialMeshBundle {
        mesh: plane.clone(),
        material: white.clone(),
        transform: Transform::from_xyz(0.0, 2.5, 2.5)
            .with_rotation(Quat::from_rotation_x(-FRAC_PI_2)),
        ..default()
    });
    // left
    commands.spawn(MaterialMeshBundle {
        mesh: plane.clone(),
        material: materials.add(CustomMaterial { color: Color::RED }),
        transform: Transform::from_xyz(2.5, 2.5, 0.0)
            .with_rotation(Quat::from_rotation_z(FRAC_PI_2)),
        ..default()
    });
    // right
    commands.spawn(MaterialMeshBundle {
        mesh: plane,
        material: materials.add(CustomMaterial {
            color: Color::GREEN,
        }),
        transform: Transform::from_xyz(-2.5, 2.5, 0.0)
            .with_rotation(Quat::from_rotation_z(-FRAC_PI_2)),
        ..default()
    });

    let box_size = 1.25;
    let half_box_size = box_size / 2.0;

    commands.spawn(MaterialMeshBundle {
        mesh: meshes.add(shape::Box::new(box_size, box_size * 2.0, box_size).into()),
        material: white.clone(),
        transform: Transform::from_xyz(half_box_size, half_box_size * 2.0, half_box_size)
            .with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_6)),
        ..default()
    });

    commands.spawn(MaterialMeshBundle {
        mesh: meshes.add(shape::Cube { size: box_size }.into()),
        material: white.clone(),
        transform: Transform::from_xyz(-half_box_size, half_box_size, -half_box_size)
            .with_rotation(Quat::from_rotation_y(-std::f32::consts::FRAC_PI_6)),
        ..default()
    });
}

// This is the struct that will be passed to your shader
#[derive(AsBindGroup, TypeUuid, Debug, Clone)]
#[uuid = "dbfc2f3d-c26d-5921-881f-b6dff4368eb2"]
pub struct CustomMaterial {
    #[uniform(0)]
    color: Color,
}

/// The Material trait is very configurable, but comes with sensible defaults for all methods.
/// You only need to implement functions for features that need non-default behavior. See the Material api docs for details!
impl Material for CustomMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/edl.wgsl".into()
    }
}
