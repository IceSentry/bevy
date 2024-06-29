//! A simple 3D scene with light shining over a cube sitting on a plane.

use bevy::{
    color::palettes::css::{BLUE, GREEN, RED},
    core_pipeline::oit::OrderIndependentTransparencySettings,
    prelude::*,
};

fn main() {
    std::env::set_var("RUST_BACKTRACE", "1");
    App::new()
        .insert_resource(Msaa::Off)
        .add_plugins(DefaultPlugins)
        .add_systems(Startup, setup)
        .add_systems(Update, toggle_oit)
        .run();
}

/// set up a simple 3D scene
fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // camera
    commands
        .spawn(Camera3dBundle {
            transform: Transform::from_xyz(0.0, 0.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
            ..default()
        })
        .insert(OrderIndependentTransparencySettings::default());
    // light
    commands.spawn(PointLightBundle {
        point_light: PointLight {
            shadows_enabled: true,
            ..default()
        },
        transform: Transform::from_xyz(4.0, 8.0, 4.0),
        ..default()
    });

    let pos_a = Vec3::new(-1.0, 0.75, 0.0);
    let pos_b = Vec3::new(0.0, -0.75, 0.0);
    let pos_c = Vec3::new(1.0, 0.75, 0.0);

    let offset = Vec3::new(0.0, 0.0, 0.0);

    let sphere_handle = meshes.add(Sphere::new(2.0).mesh());

    let alpha = 0.5;

    commands.spawn(PbrBundle {
        mesh: sphere_handle.clone(),
        material: materials.add(StandardMaterial {
            base_color: RED.with_alpha(alpha).into(),
            alpha_mode: AlphaMode::Blend,
            ..default()
        }),
        transform: Transform::from_translation(pos_a + offset),
        ..default()
    });
    commands.spawn(MaterialMeshBundle {
        mesh: sphere_handle.clone(),
        material: materials.add(StandardMaterial {
            base_color: GREEN.with_alpha(alpha).into(),
            alpha_mode: AlphaMode::Blend,
            ..default()
        }),
        transform: Transform::from_translation(pos_b + offset),
        ..default()
    });
    commands.spawn(MaterialMeshBundle {
        mesh: sphere_handle,
        material: materials.add(StandardMaterial {
            base_color: BLUE.with_alpha(alpha).into(),
            alpha_mode: AlphaMode::Blend,
            ..default()
        }),
        transform: Transform::from_translation(pos_c + offset),
        ..default()
    });

    commands.spawn(TextBundle::from_section(
        "Oit Enabled",
        TextStyle::default(),
    ));
}

fn toggle_oit(
    mut commands: Commands,
    mut text: Query<&mut Text>,
    keyboard_input: Res<ButtonInput<KeyCode>>,
    q: Query<(Entity, Has<OrderIndependentTransparencySettings>), With<Camera3d>>,
) {
    if keyboard_input.just_pressed(KeyCode::KeyT) {
        let (e, has_oit) = q.single();
        text.single_mut().sections[0].value = if has_oit {
            commands
                .entity(e)
                .remove::<OrderIndependentTransparencySettings>();
            "OIT disabled".to_string()
        } else {
            commands
                .entity(e)
                .insert(OrderIndependentTransparencySettings::default());
            "OIT enabled".to_string()
        };
    }
}
