//! Radar (minimap) in the top-left corner: the map, you, your teammates,
//! enemies your team can currently see, and the bomb.

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::body::{Body, view_dir};
use crate::combat::{Combatant, Health, Team, line_of_sight};
use crate::game::{Bomb, BombState};
use crate::map::{CELL, Cell, MapDef};
use crate::nav::NavGrid;
use crate::physics::WorldColliders;
use crate::player::Player;

/// Radar pixels per meter.
const SCALE: f32 = 2.0;
const DOTS: usize = 12;
/// Enemies further than this from every teammate don't show up on the radar.
const SPOT_DISTANCE: f32 = 60.0;

#[derive(Component)]
pub struct RadarDot;

/// The radar image; despawned and rebuilt when the map changes.
#[derive(Component)]
pub struct RadarRoot;

pub fn spawn_radar(commands: &mut Commands, map: &MapDef, nav: &NavGrid, images: &mut Assets<Image>) {
    let g = &map.grid;
    let mut data = Vec::with_capacity((g.width * g.height * 4) as usize);
    for z in 0..g.height {
        for x in 0..g.width {
            let center = g.cell_center(x, z);
            let on_site = map.sites.iter().any(|s| {
                let p = center + Vec3::Y * 0.5;
                p.cmpge(s.area.min).all() && p.cmple(s.area.max).all()
            });
            let rgba: [u8; 4] = match g.get(x, z) {
                Cell::Wall { .. } => [25, 22, 18, 170],
                Cell::Open { .. } if !nav.walkable(x, z) => [95, 72, 45, 230], // crates
                Cell::Open { roof: true, .. } => [92, 84, 72, 230],
                Cell::Open { level, .. } => {
                    let l = level.min(26) * 3;
                    if on_site {
                        [170 + l, 120 + l, 95 + l, 230]
                    } else {
                        [140 + l, 128 + l, 100 + l, 230]
                    }
                }
            };
            data.extend_from_slice(&rgba);
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: g.width as u32,
            height: g.height as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::nearest();

    let size = Vec2::new(g.width as f32, g.height as f32) * CELL * SCALE;
    let radar = commands
        .spawn((
            RadarRoot,
            ImageNode::new(images.add(image)),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(10.0),
                top: Val::Px(10.0),
                width: Val::Px(size.x),
                height: Val::Px(size.y),
                border: UiRect::all(Val::Px(2.0)),
                ..default()
            },
            BorderColor::all(Color::srgba(0.0, 0.0, 0.0, 0.6)),
        ))
        .id();

    for site in &map.sites {
        let c = site.area.center() * SCALE;
        commands.spawn((
            Text::new(site.name.to_string()),
            TextFont::from_font_size(20.0),
            TextColor(Color::srgba(1.0, 0.25, 0.2, 0.9)),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(c.x - 6.0),
                top: Val::Px(c.z - 12.0),
                ..default()
            },
            ChildOf(radar),
        ));
    }
    for _ in 0..DOTS {
        commands.spawn((
            RadarDot,
            Node {
                position_type: PositionType::Absolute,
                border_radius: BorderRadius::MAX,
                ..default()
            },
            BackgroundColor(Color::WHITE),
            Visibility::Hidden,
            ChildOf(radar),
        ));
    }
}
pub fn update_radar(
    time: Res<Time>,
    world: Res<WorldColliders>,
    bomb: Res<Bomb>,
    player: Single<(Entity, &Combatant, &Health, &Body, &Transform), With<Player>>,
    people: Query<(Entity, &Combatant, &Health, &Body, &Transform)>,
    mut dots: Query<(&mut Node, &mut BackgroundColor, &mut Visibility), With<RadarDot>>,
    mut spotted: Local<Vec<Entity>>,
    mut refresh_in: Local<f32>,
) {
    let (me, my, my_health, my_body, my_tf) = player.into_inner();
    let team = my.team;

    // Which enemies can anyone on our team see? (Refreshed a few times per second.)
    *refresh_in -= time.delta_secs();
    if *refresh_in <= 0.0 {
        *refresh_in = 0.2;
        spotted.clear();
        let eyes: Vec<Vec3> = people
            .iter()
            .filter(|p| p.1.team == team && p.2.alive())
            .map(|p| p.4.translation + Vec3::Y * p.3.eye_height())
            .collect();
        for (entity, c, h, _, t) in &people {
            if c.team == team || !h.alive() {
                continue;
            }
            let chest = t.translation + Vec3::Y * 1.2;
            if eyes
                .iter()
                .any(|&eye| eye.distance(chest) < SPOT_DISTANCE && line_of_sight(&world, eye, chest))
            {
                spotted.push(entity);
            }
        }
    }

    // (position, color, size)
    let mut marks: Vec<(Vec3, Color, f32)> = Vec::new();
    let team_color = |t: Team| match t {
        Team::T => Color::srgb(1.0, 0.8, 0.2),
        Team::CT => Color::srgb(0.35, 0.6, 1.0),
    };
    for (entity, c, h, _, t) in &people {
        if entity == me || !h.alive() {
            continue;
        }
        if c.team == team {
            marks.push((t.translation, team_color(c.team), 7.0));
        } else if spotted.contains(&entity) {
            marks.push((t.translation, Color::srgb(1.0, 0.15, 0.15), 7.0));
        }
    }
    let bomb_pos = match bomb.state {
        BombState::Dropped(p) if team == Team::T => Some(p),
        BombState::Planted { pos, .. } => Some(pos),
        _ => None,
    };
    if let Some(p) = bomb_pos {
        marks.push((p, Color::srgb(1.0, 0.45, 0.0), 8.0));
    }
    if my_health.alive() {
        // You: white dot with a small dot showing where you look.
        marks.push((my_tf.translation, Color::WHITE, 9.0));
        marks.push((my_tf.translation + view_dir(my_body.yaw, 0.0) * 4.0, Color::WHITE, 4.0));
    }

    let mut marks = marks.into_iter();
    for (mut node, mut color, mut visibility) in &mut dots {
        let Some((pos, c, size)) = marks.next() else {
            *visibility = Visibility::Hidden;
            continue;
        };
        node.left = Val::Px(pos.x * SCALE - size / 2.0);
        node.top = Val::Px(pos.z * SCALE - size / 2.0);
        node.width = Val::Px(size);
        node.height = Val::Px(size);
        color.0 = c;
        *visibility = Visibility::Inherited;
    }
}
