mod camera;
mod plugin;

use std::f32::consts::{FRAC_PI_2, PI};

use bevy::{
    core_pipeline::prepass::{DepthPrepass, NormalPrepass},
    diagnostic::{Diagnostics, FrameTimeDiagnosticsPlugin},
    math::vec3,
    prelude::*,
};
use camera::FlyCam;
use plugin::SsgiPlugin;

fn main() {
    App::new()
        .insert_resource(Msaa { samples: 1 })
        .add_plugins(
            DefaultPlugins
                .set(AssetPlugin {
                    watch_for_changes: true,
                    ..default()
                })
                .set(WindowPlugin {
                    window: WindowDescriptor {
                        width: 720.0,
                        height: 720.0,
                        ..default()
                    },
                    ..default()
                }),
        )
        .add_plugin(SsgiPlugin)
        .add_plugin(FrameTimeDiagnosticsPlugin)
        .add_startup_system(setup_camera)
        .add_startup_system(spawn_cornell_box)
        .add_startup_system(spawn_boxes)
        .add_system(change_text_system)
        .add_system(camera::fly_camera)
        .run();
}

fn setup_camera(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.spawn((
        Camera3dBundle {
            transform: Transform::from_xyz(0.0, 2.5, -8.75)
                .looking_at(vec3(0.0, 2.5, 0.0), Vec3::Y),
            ..default()
        },
        DepthPrepass,
        NormalPrepass,
        FlyCam,
    ));
    let font = asset_server.load("fonts/FiraMono-Medium.ttf");
    let style = TextStyle {
        font,
        font_size: 16.0,
        color: Color::WHITE,
    };
    commands.spawn((
        TextBundle::from_sections([
            TextSection::from_style(style.clone()),
            TextSection::new(" fps\n", style.clone()),
            TextSection::from_style(style.clone()),
            TextSection::new(" ms", style),
        ])
        .with_style(Style {
            position_type: PositionType::Absolute,
            position: UiRect {
                top: Val::Px(5.0),
                left: Val::Px(5.0),
                ..default()
            },

            ..default()
        }),
        BackgroundColor(Color::BLACK.with_a(0.75)),
    ));
}

fn spawn_cornell_box(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let white = materials.add(Color::WHITE.into());
    let plane_size = 5.0;
    let plane = meshes.add(shape::Plane { size: plane_size }.into());

    // bottom
    commands.spawn(PbrBundle {
        mesh: plane.clone(),
        material: white.clone(),
        transform: Transform::from_xyz(0.0, 0.0, 0.0),
        ..default()
    });
    // top
    commands.spawn(PbrBundle {
        mesh: plane.clone(),
        material: white.clone(),
        transform: Transform::from_xyz(0.0, 5.0, 0.0).with_rotation(Quat::from_rotation_x(PI)),
        ..default()
    });
    // back
    commands.spawn(PbrBundle {
        mesh: plane.clone(),
        material: white,
        transform: Transform::from_xyz(0.0, 2.5, 2.5)
            .with_rotation(Quat::from_rotation_x(-FRAC_PI_2)),
        ..default()
    });
    // left
    commands.spawn(PbrBundle {
        mesh: plane.clone(),
        material: materials.add(Color::RED.into()),
        transform: Transform::from_xyz(2.5, 2.5, 0.0)
            .with_rotation(Quat::from_rotation_z(FRAC_PI_2)),
        ..default()
    });
    // right
    commands.spawn(PbrBundle {
        mesh: plane,
        material: materials.add(Color::GREEN.into()),
        transform: Transform::from_xyz(-2.5, 2.5, 0.0)
            .with_rotation(Quat::from_rotation_z(-FRAC_PI_2)),
        ..default()
    });

    // Light
    commands.spawn(PointLightBundle {
        point_light: PointLight {
            intensity: 1000.0,
            shadows_enabled: true,
            ..default()
        },
        transform: Transform::from_xyz(0.0, 5.0 - 0.005, 0.0)
            .with_rotation(Quat::from_rotation_x(PI)),
        ..default()
    });
}

fn spawn_boxes(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let box_size = 1.25;
    let half_box_size = box_size / 2.0;

    commands.spawn(PbrBundle {
        mesh: meshes.add(shape::Box::new(box_size, box_size * 2.0, box_size).into()),
        material: materials.add(Color::WHITE.into()),
        transform: Transform::from_xyz(half_box_size, half_box_size * 2.0, half_box_size)
            .with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_6)),
        ..default()
    });

    commands.spawn(PbrBundle {
        mesh: meshes.add(shape::Cube { size: box_size }.into()),
        material: materials.add(Color::WHITE.into()),
        transform: Transform::from_xyz(-half_box_size, half_box_size, -half_box_size)
            .with_rotation(Quat::from_rotation_y(-std::f32::consts::FRAC_PI_6)),
        ..default()
    });
}

fn change_text_system(time: Res<Time>, diagnostics: Res<Diagnostics>, mut query: Query<&mut Text>) {
    for mut text in &mut query {
        let mut fps = 0.0;
        if let Some(fps_diagnostic) = diagnostics.get(FrameTimeDiagnosticsPlugin::FPS) {
            if let Some(fps_smoothed) = fps_diagnostic.smoothed() {
                fps = fps_smoothed;
            }
        }

        let mut frame_time = time.delta_seconds_f64();
        if let Some(frame_time_diagnostic) = diagnostics.get(FrameTimeDiagnosticsPlugin::FRAME_TIME)
        {
            if let Some(frame_time_smoothed) = frame_time_diagnostic.smoothed() {
                frame_time = frame_time_smoothed;
            }
        }

        text.sections[0].value = format!("{fps:.1}");
        text.sections[2].value = format!("{frame_time:.3}");
    }
}
