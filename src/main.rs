// Release builds on Windows open no console window, like a normal game.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! CS Blocky — a Counter-Strike style 5v5 shooter with Minecraft/Roblox style graphics.

// Bevy systems naturally take many parameters with long query types.
#![allow(clippy::too_many_arguments, clippy::type_complexity)]

mod body;
mod bots;
mod combat;
mod dust2;
mod game;
mod hud;
mod map;
mod maps;
mod nav;
mod physics;
mod player;
mod radar;
mod sound;
mod textures;
mod weapons;

use bevy::camera::visibility::RenderLayers;
use bevy::light::CascadeShadowConfigBuilder;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};

use physics::{GameRng, WorldColliders};
use player::PlayerCamera;
use weapons::VIEW_MODEL_LAYER;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "CS Blocky".into(),
                ..default()
            }),
            ..default()
        }))
        .insert_resource(ClearColor(Color::srgb(0.53, 0.75, 0.95)))
        .insert_resource(GlobalAmbientLight {
            color: Color::srgb(1.0, 0.95, 0.85),
            brightness: 2200.0,
            ..default()
        })
        .init_resource::<WorldColliders>()
        .init_resource::<GameRng>()
        .init_resource::<player::Settings>()
        .init_resource::<player::Playing>()
        .init_resource::<player::Spectating>()
        .init_resource::<weapons::BulletHoles>()
        .init_resource::<weapons::Tracers>()
        .init_resource::<hud::KillFeed>()
        .init_resource::<hud::HitMarker>()
        .init_resource::<game::Match>()
        .init_resource::<game::Bomb>()
        .init_resource::<bots::Difficulty>()
        .insert_resource(maps::MapChoice::from_args())
        .add_message::<combat::Damage>()
        .add_message::<combat::Killed>()
        .add_plugins(sound::SoundPlugin)
        .add_systems(
            Startup,
            (
                setup_world,
                weapons::setup_effects,
                hud::setup_hud,
                game::spawn_teams,
                game::spawn_bomb_model,
                maps::load_map.after(setup_world).after(hud::setup_hud),
            ),
        )
        .add_systems(
            Update,
            (
                (
                    maps::change_map,
                    maps::load_map,
                    player::grab_cursor,
                    player::mouse_look,
                    player::player_input,
                    game::take_over_bot,
                    bots::bot_ai,
                    weapons::player_weapons,
                    combat::apply_damage,
                    combat::tick_health,
                    game::bomb_system,
                    game::round_system,
                )
                    .chain(),
                (
                    player::update_camera,
                    weapons::animate_view_model,
                    combat::animate_characters,
                    game::update_bomb_model,
                    weapons::update_particles,
                    weapons::draw_tracers,
                    hud::update_hud_effects,
                    hud::update_name_tags,
                    hud::update_hud,
                    radar::update_radar,
                )
                    .chain(),
            )
                .chain(),
        )
        .add_systems(Update, (debug_screenshot, bots::change_difficulty))
        .run();
}

fn setup_world(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(textures::create_block_materials(&mut images, &mut materials));

    // Sun with shadows. It also lights the view model layer.
    commands.spawn((
        DirectionalLight {
            color: Color::srgb(1.0, 0.96, 0.88),
            illuminance: 11_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::default().looking_to(Vec3::new(0.35, -0.9, -0.25), Vec3::Y),
        CascadeShadowConfigBuilder {
            num_cascades: 3,
            maximum_distance: 90.0,
            first_cascade_far_bound: 12.0,
            ..default()
        }
        .build(),
        RenderLayers::from_layers(&[0, VIEW_MODEL_LAYER]),
    ));

    let camera = commands
        .spawn((
            PlayerCamera,
            Camera3d::default(),
            Projection::from(PerspectiveProjection {
                fov: 74f32.to_radians(),
                near: 0.05,
                ..default()
            }),
            Transform::default(),
            DistanceFog {
                color: Color::srgb(0.78, 0.82, 0.86),
                falloff: FogFalloff::Linear {
                    start: 70.0,
                    end: 200.0,
                },
                ..default()
            },
        ))
        .id();
    // Second camera that only draws the gun on top of everything.
    commands.spawn((
        Camera3d::default(),
        Camera {
            order: 1,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        Projection::from(PerspectiveProjection {
            fov: 62f32.to_radians(),
            near: 0.01,
            ..default()
        }),
        RenderLayers::layer(VIEW_MODEL_LAYER),
        ChildOf(camera),
    ));
    weapons::spawn_view_models(&mut commands, camera, &mut meshes, &mut materials);
}

/// With CS_SCREENSHOT=file.png the game saves a screenshot after CS_FRAMES frames (default 120) and quits.
fn debug_screenshot(mut commands: Commands, mut frame: Local<u32>, mut exit: MessageWriter<AppExit>) {
    let Ok(path) = std::env::var("CS_SCREENSHOT") else {
        return;
    };
    let at = std::env::var("CS_FRAMES")
        .ok()
        .and_then(|f| f.parse().ok())
        .unwrap_or(120);
    *frame += 1;
    if *frame == at {
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
    }
    if *frame == at + 60 {
        exit.write(AppExit::Success);
    }
}
