//! A procedurally generated city

use argh::FromArgs;
use assets::{load_assets, CityAssets};
use bevy::{
    anti_alias::taa::TemporalAntiAliasing,
    camera::{visibility::NoCpuCulling, Exposure, Hdr},
    camera_controller::free_camera::{FreeCamera, FreeCameraPlugin},
    color::palettes::css::WHITE,
    feathers::{dark_theme::create_dark_theme, theme::UiTheme, FeathersPlugins},
    light::{atmosphere::ScatteringMedium, Atmosphere, AtmosphereEnvironmentMapLight},
    pbr::{
        wireframe::{WireframeConfig, WireframePlugin},
        AtmosphereSettings, ContactShadows,
    },
    post_process::bloom::Bloom,
    prelude::*,
    scene::SceneInstanceReady,
    window::{PresentMode, WindowResolution},
    winit::WinitSettings,
};

use crate::settings::{Settings, SettingsPanel};
use crate::{generate_city::{spawn_city, spawn_city_blocks, CitySpawnQueue}, settings::setup_settings_ui};

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

    /// number of city blocks to spawn per frame
    #[argh(option, default = "10")]
    pub blocks_per_frame: u32,
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
        .add_systems(
            Startup,
            (setup, setup_settings_ui, load_assets, setup_loading_screen),
        )
        .add_systems(
            Update,
            (simulate_cars, setup_city_when_loaded, spawn_city_blocks, add_no_cpu_culling),
        )
        .add_observer(add_no_cpu_culling_on_scene_ready)
        .run();
}

fn setup(mut commands: Commands, mut scattering_mediums: ResMut<Assets<ScatteringMedium>>) {
    commands.spawn((
        Camera3d::default(),
        Hdr,
        Transform::from_xyz(15.0, 10.0, 20.0).looking_at(Vec3::ZERO, Vec3::Y),
        FreeCamera::default(),
        Atmosphere::earth(scattering_mediums.add(ScatteringMedium::default())),
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
            shadow_maps_enabled: Settings::default().shadow_maps_enabled,
            contact_shadows_enabled: Settings::default().contact_shadows_enabled,
            illuminance: light_consts::lux::RAW_SUNLIGHT,
            ..default()
        },
        Transform::from_xyz(1.0, 0.15, 1.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

fn setup_loading_screen(mut commands: Commands) {
    commands.spawn((
        LoadingScreen,
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(Color::BLACK),
        children![(
            LoadingText,
            Text::new("Loading..."),
            TextFont {
                font_size: FontSize::Px(24.0),
                ..default()
            },
        )],
    ));
}

fn setup_city_when_loaded(
    mut commands: Commands,
    assets: Res<CityAssets>,
    asset_server: Res<AssetServer>,
    args: Res<Args>,
    mut city_spawned: Local<bool>,
    mut all_assets_loaded: Local<bool>,
    mut last_counts: Local<(usize, usize)>,
    loading_screen: Option<Single<Entity, With<LoadingScreen>>>,
    mut loading_text: Option<Single<&mut Text, With<LoadingText>>>,
    spawn_queue: Option<Res<CitySpawnQueue>>,
    settings_panel: Option<Single<Entity, With<SettingsPanel>>>,
) {
    if *city_spawned {
        match spawn_queue {
            None => {
                if let Some(entity) = loading_screen {
                    commands.entity(*entity).despawn();
                }
                if let Some(entity) = settings_panel {
                    commands.entity(*entity).insert(Visibility::Visible);
                }
            }
            Some(queue) => {
                if let Some(ref mut text) = loading_text {
                    text.0 = format!("Spawning city: {} blocks remaining", queue.blocks_remaining());
                }
            }
        }
        return;
    }
    // All assets were loaded last frame — spawn city now that the text was rendered.
    if *all_assets_loaded {
        spawn_city(&mut commands, &assets, args.seed, args.size, args.blocks_per_frame);
        if let Some(entity) = loading_screen {
            commands.entity(*entity).remove::<BackgroundColor>();
        }
        *city_spawned = true;
        return;
    }
    let progress = assets.load_progress(&asset_server);
    if (progress.loaded, progress.total) != *last_counts {
        let pending_str = progress
            .pending
            .iter()
            .map(|p| assets::strip_base_url(p.clone()))
            .collect::<Vec<_>>()
            .join("\n");
        if let Some(ref mut text) = loading_text {
            text.0 = format!(
                "Loading assets: {}/{}\n\n{}",
                progress.loaded, progress.total, pending_str
            );
        }
        *last_counts = (progress.loaded, progress.total);
    }
    if progress.loaded == progress.total {
        *all_assets_loaded = true;
    }
}

#[derive(Component)]
struct LoadingScreen;

#[derive(Component)]
struct LoadingText;

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
