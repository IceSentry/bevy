//! Not a minecraft clone

use bevy::{
    anti_alias::taa::TemporalAntiAliasing,
    camera::{Exposure, Hdr},
    camera_controller::free_camera::{FreeCamera, FreeCameraPlugin},
    feathers::{dark_theme::create_dark_theme, theme::UiTheme, FeathersPlugins},
    light::{atmosphere::ScatteringMedium, Atmosphere, AtmosphereEnvironmentMapLight},
    math::DVec3,
    pbr::{wireframe::WireframePlugin, AtmosphereSettings, ContactShadows},
    post_process::bloom::Bloom,
    prelude::*,
    window::{PresentMode, WindowResolution},
    winit::WinitSettings,
};
use noise::{NoiseFn, OpenSimplex};

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title: "bevycraft".into(),
                    resolution: WindowResolution::new(1920, 1080).with_scale_factor_override(1.0),
                    present_mode: PresentMode::AutoNoVsync,
                    ..default()
                }),
                ..default()
            }),
            FreeCameraPlugin,
            FeathersPlugins,
            WireframePlugin::default(),
        ))
        .insert_resource(ClearColor(Color::BLACK))
        .insert_resource(WinitSettings::continuous())
        .insert_resource(UiTheme(create_dark_theme()))
        .add_systems(Startup, (setup, spawn_chunks).chain())
        .run();
}

fn setup(
    mut commands: Commands,
    mut scattering_mediums: ResMut<Assets<ScatteringMedium>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Camera3d::default(),
        Hdr,
        Transform::from_xyz(64.0, 64.0, 64.0).looking_at(Vec3::ZERO, Vec3::Y),
        FreeCamera::default(),
        Atmosphere::earthlike(scattering_mediums.add(ScatteringMedium::default())),
        AtmosphereSettings::default(),
        // The directional light illuminance used in this scene is
        // quite bright, so raising the exposure compensation helps
        // bring the scene to a nicer brightness range.
        Exposure { ev100: 13.0 },
        // Bloom gives the sun a much more natural look.
        Bloom::NATURAL,
        // Enables the atmosphere to drive reflections and ambient lighting (IBL) for this view
        AtmosphereEnvironmentMapLight::default(),
        Msaa::Off,
        TemporalAntiAliasing::default(),
        ContactShadows::default(),
    ));

    commands.spawn((
        DirectionalLight {
            shadow_maps_enabled: true,
            contact_shadows_enabled: true,
            illuminance: light_consts::lux::RAW_SUNLIGHT,
            ..default()
        },
        Transform::from_xyz(1.0, 0.15, 1.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    let cube = meshes.add(Cuboid::new(1.0, 1.0, 1.0).mesh());
    let material = materials.add(StandardMaterial::from_color(Color::srgb(0.0, 1.0, 0.0)));

    commands.insert_resource(ChunkSpawner {
        mesh_handle: cube.clone(),
        material_handle: material.clone(),
    });
}

fn spawn_chunks(mut commands: Commands, chunk_spawner: Res<ChunkSpawner>) {
    let noise = OpenSimplex::new(42);

    let half_size = 12;
    let mut count = 0;
    for x in -half_size..half_size {
        for z in -half_size..half_size {
            count += 1;
            spawn_chunk(
                &mut commands,
                &chunk_spawner,
                Vec2::new(x as f32, z as f32) * 32.0,
                noise,
            );
        }
    }
    println!("Spawned {} chunks and {} cubes", count, count * (32 * 32));
}

#[derive(Resource)]
struct ChunkSpawner {
    mesh_handle: Handle<Mesh>,
    material_handle: Handle<StandardMaterial>,
}

#[derive(Component)]
struct Chunk;

fn spawn_chunk<N: NoiseFn<f64, 2>>(
    commands: &mut Commands,
    chunk_spawner: &ChunkSpawner,
    offset: Vec2,
    noise: N,
) {
    let chunk_size = 32;
    let noise_scale = 0.025;
    let mut cubes = Vec::with_capacity(chunk_size * chunk_size);
    for x in 0..chunk_size as u32 {
        for z in 0..chunk_size as u32 {
            let noise_pos = ((UVec2::new(x, z).as_vec2() + offset) * noise_scale).as_dvec2();
            let value = noise.get(noise_pos.to_array()) * 0.5 + 0.5;
            let y = (value * chunk_size as f64).round();
            let pos = Vec3::new(x as f32, y as f32, z as f32); //+ Vec3::new(offset.x, 0.0, offset.y);
            cubes.push((
                Mesh3d(chunk_spawner.mesh_handle.clone()),
                MeshMaterial3d(chunk_spawner.material_handle.clone()),
                Transform::from_translation(pos),
            ));
        }
    }
    commands
        // TODO use the chunk transform for the offset
        .spawn((
            Chunk,
            Transform::from_xyz(offset.x, 0.0, offset.y),
            Visibility::default(),
        ))
        .with_children(|commands| {
            for cube in cubes {
                commands.spawn(cube);
            }
            // not sure why this is not supported for child spawners
            // commands.spawn_batch(cubes);
        });
}
