//! An example that shows how to use OIT

use bevy::{
    color::palettes::css::{BLUE, GREEN, RED},
    core_pipeline::oit::{OitCamera, OitLayers, OrderIndependentTransparencyPlugin},
    prelude::*,
    window::PresentMode,
};

fn main() {
    App::new()
        // MSAA needs to be disabled otherwise it may cause issue on some platforms
        .insert_resource(Msaa::Off)
        .insert_resource(OitLayers(8))
        .add_plugins((
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    present_mode: PresentMode::AutoNoVsync,
                    resolution: (1920.0, 1080.0).into(),
                    ..default()
                }),
                ..default()
            }),
            // Add the plugin
            OrderIndependentTransparencyPlugin,
        ))
        .add_systems(Startup, setup)
        .run();
}

#[derive(Component)]
struct KeepMaterial;

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // camera
    commands.spawn((
        Camera3dBundle {
            transform: Transform::from_xyz(0.0, 0.0, 5.0),
            camera: Camera {
                hdr: true,
                ..default()
            },
            ..default()
        },
        OitCamera,
    ));
    commands.spawn(PointLightBundle {
        transform: Transform::from_xyz(0.0, 0.0, 5.0),
        ..default()
    });

    let pos_a = Vec3::new(-0.5, 0.25, 0.0);
    let pos_b = Vec3::new(0.0, -0.25, 0.0);
    let pos_c = Vec3::new(0.5, 0.25, 0.0);

    let sphere_handle = meshes.add(Sphere { radius: 1.0 }.mesh());

    let alpha = 0.5;

    commands.spawn(PbrBundle {
        mesh: sphere_handle.clone(),
        material: materials.add(StandardMaterial {
            base_color: RED.with_alpha(alpha).into(),
            alpha_mode: AlphaMode::Blend,
            ..default()
        }),
        transform: Transform::from_translation(pos_a),
        ..default()
    });
    commands.spawn(MaterialMeshBundle {
        mesh: sphere_handle.clone(),
        material: materials.add(StandardMaterial {
            base_color: GREEN.with_alpha(alpha).into(),
            alpha_mode: AlphaMode::Blend,
            ..default()
        }),
        transform: Transform::from_translation(pos_b),
        ..default()
    });
    commands.spawn(MaterialMeshBundle {
        mesh: sphere_handle.clone(),
        material: materials.add(StandardMaterial {
            base_color: BLUE.with_alpha(alpha).into(),
            alpha_mode: AlphaMode::Blend,
            ..default()
        }),
        transform: Transform::from_translation(pos_c),
        ..default()
    });
}
