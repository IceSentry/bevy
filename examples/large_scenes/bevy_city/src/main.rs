//! A procedurally generated city

use argh::FromArgs;
use assets::{load_assets, CityAssets};
use bevy::{
    anti_alias::taa::TemporalAntiAliasing,
    camera::{visibility::NoCpuCulling, Exposure, Hdr},
    camera_controller::free_camera::{FreeCamera, FreeCameraPlugin},
    color::palettes::css::WHITE,
    feathers::{dark_theme::create_dark_theme, theme::UiTheme, FeathersPlugins},
    light::{
        atmosphere::ScatteringMedium, Atmosphere, AtmosphereEnvironmentMapLight,
        CascadeShadowConfigBuilder,
    },
    pbr::{
        wireframe::{WireframeConfig, WireframePlugin},
        AtmosphereSettings, ContactShadows, ScreenSpaceAmbientOcclusion,
    },
    post_process::bloom::Bloom,
    prelude::*,
    scene::SceneInstanceReady,
    window::{PresentMode, WindowResolution},
    winit::WinitSettings,
};

use crate::settings::Settings;
use crate::{generate_city::spawn_city, settings::setup_settings_ui};

mod assets;
mod generate_city;
mod settings;

#[derive(FromArgs, Resource, Clone)]
/// Config
pub struct Args {
    /// seed
    #[argh(option, default = "42")]
    seed: u64,

    /// size
    #[argh(option, default = "30")]
    size: u32,

    /// adds NoCpuCulling to all meshes
    #[argh(switch)]
    no_cpu_culling: bool,
}

fn main() {
    let args: Args = argh::from_env();

    App::new()
        .add_plugins((
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title: "bevy_city".into(),
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
        .insert_resource(args.clone())
        .insert_resource(ClearColor(Color::BLACK))
        .insert_resource(WinitSettings::continuous())
        .init_resource::<Settings>()
        .insert_resource(UiTheme(create_dark_theme()))
        .insert_resource(WireframeConfig {
            global: false,
            default_color: WHITE.into(),
            ..default()
        })
        // Like in many realistic large scenes, many of the objects don't move
        // We can accelerate transform propagation by optimizing for this case
        .insert_resource(StaticTransformOptimizations::Enabled)
        .insert_resource(GlobalAmbientLight::NONE)
        .add_systems(
            Startup,
            (
                setup,
                setup_settings_ui,
                load_assets,
                (setup_city.after(load_assets), add_no_cpu_culling).chain(),
            ),
        )
        .add_systems(Update, (simulate_cars, update_shadow_cascade))
        .add_observer(add_no_cpu_culling_on_scene_ready)
        .run();
}

fn setup(
    mut commands: Commands,
    mut scattering_mediums: ResMut<Assets<ScatteringMedium>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mut atmosphere = Atmosphere::earth(scattering_mediums.add(ScatteringMedium::default()));
    // atmosphere.ground_albedo = Vec3::new(97.0 / 256.0, 203.0 / 256.0, 139.0 / 256.0);

    commands.spawn((
        Camera3d::default(),
        Hdr,
        Transform::from_xyz(150.0, 100.0, 200.0).looking_at(Vec3::ZERO, Vec3::Y),
        FreeCamera {
            walk_speed: 25.0,
            run_speed: 100.0,
            ..Default::default()
        },
        atmosphere,
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
        // Exposure { ev100: 12.0 },
        // Bloom {
        //     intensity: 0.03,
        //     ..Bloom::NATURAL
        // },
        // AtmosphereEnvironmentMapLight {
        //     intensity: 0.5,
        //     ..default()
        // },
        ScreenSpaceAmbientOcclusion::default(),
    ));

    commands.spawn((
        DirectionalLight {
            shadow_maps_enabled: Settings::default().shadow_maps_enabled,
            contact_shadows_enabled: Settings::default().contact_shadows_enabled,
            illuminance: light_consts::lux::RAW_SUNLIGHT,
            ..default()
        },
        CascadeShadowConfigBuilder {
            num_cascades: 3,
            maximum_distance: 1500.0,
            ..default()
        }
        .build(),
        Transform::from_xyz(1.0, 0.15, 1.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    let ground_material = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 1.0, 1.0),
        perceptual_roughness: 0.0,
        metallic: 0.0,
        ..Default::default()
    });
    commands.spawn((
        Mesh3d(
            meshes.add(
                Plane3d::new(Vec3::Y, Vec2::splat(100_000.0))
                    .mesh()
                    .subdivisions(1),
            ),
        ),
        MeshMaterial3d(ground_material),
        Transform::default(),
    ));
}

fn update_shadow_cascade(
    mut commands: Commands,
    directional_light: Single<Entity, With<DirectionalLight>>,
    camera: Single<&Transform, (With<Camera3d>, Changed<Transform>)>,
) {
    commands.entity(*directional_light).insert(
        CascadeShadowConfigBuilder {
            num_cascades: 3,
            maximum_distance: 2000.0,
            first_cascade_far_bound: camera.translation.y.clamp(30.0, 300.0),
            ..default()
        }
        .build(),
    );
}

fn setup_city(mut commands: Commands, assets: Res<CityAssets>, args: Res<Args>) {
    spawn_city(&mut commands, &assets, args.seed, args.size);
}

#[derive(Component)]
struct Road {
    start: Vec3,
    end: Vec3,
}

#[derive(Component)]
struct Car {
    offset: Vec3,
    distance_traveled: f32,
    dir: f32,
}

fn simulate_cars(
    settings: Res<Settings>,
    roads: Query<(&Road, &Transform, &Children), Without<Car>>,
    mut cars: Query<(&mut Car, &mut Transform), Without<Road>>,
    time: Res<Time>,
) {
    if !settings.simulate_cars {
        return;
    }
    let speed = 1.5;

    for (road, _, children) in &roads {
        for child in children {
            let Ok((mut car, mut car_transform)) = cars.get_mut(*child) else {
                continue;
            };

            car.distance_traveled += speed * time.delta_secs();
            let road_len = (road.end - road.start).length();
            if car.distance_traveled > road_len {
                car.distance_traveled = 0.0;
            }
            let direction = (road.end - road.start).normalize() * car.dir;

            let progress = car.distance_traveled / road_len;
            car_transform.translation = (road.start + car.offset) + direction * road_len * progress;
        }
    }
}

fn add_no_cpu_culling(
    mut commands: Commands,
    meshes: Query<Entity, (With<Mesh3d>, Without<NoCpuCulling>)>,
    args: Res<Args>,
) {
    if args.no_cpu_culling {
        for entity in meshes.iter() {
            commands.entity(entity).insert(NoCpuCulling);
        }
    }
}

fn add_no_cpu_culling_on_scene_ready(
    scene_ready: On<SceneInstanceReady>,
    mut commands: Commands,
    children: Query<&Children>,
    meshes: Query<(), (With<Mesh3d>, Without<NoCpuCulling>)>,
    args: Res<Args>,
) {
    if args.no_cpu_culling {
        for descendant in children.iter_descendants(scene_ready.entity) {
            if meshes.get(descendant).is_ok() {
                commands.entity(descendant).insert(NoCpuCulling);
            }
        }
    }
}
