//! All playable maps. Dust II is hand-tuned (see `dust2.rs`); the others are generated from
//! their radar images and get an automatic bot strategy.

use bevy::prelude::*;
use std::collections::HashSet;

use crate::dust2::{self, LETTER_A, LETTER_B, paint_letter, wall_face_south};
use crate::game::{Bomb, Match};
use crate::map::{self, BombSite, FloorKind, MapDef, MapFile, MapGeometry, Theme};
use crate::nav::NavGrid;
use crate::physics::WorldColliders;
use crate::radar::{self, RadarRoot};
use crate::textures::BlockMaterials;
use crate::weapons::BulletHoles;

pub struct MapEntry {
    pub name: &'static str,
    pub build: fn() -> MapDef,
}

pub const MAPS: &[MapEntry] = &[
    MapEntry {
        name: "Dust II",
        build: dust2::dust2,
    },
    MapEntry {
        name: "Mirage",
        build: || generic(include_str!("../assets/maps/de_mirage.txt"), Theme::Mirage),
    },
    MapEntry {
        name: "Inferno",
        build: || generic(include_str!("../assets/maps/de_inferno.txt"), Theme::Inferno),
    },
    MapEntry {
        name: "Ancient",
        build: || generic(include_str!("../assets/maps/de_ancient.txt"), Theme::Ancient),
    },
];

/// Index of the map whose name contains `query` (case-insensitive), e.g. from the command line.
pub fn find(query: &str) -> Option<usize> {
    let q = query.to_lowercase().replace("de_", "");
    MAPS.iter()
        .position(|m| m.name.to_lowercase().replace(' ', "").contains(&q.replace(' ', "")))
}

/// Which map is selected, and which one is currently built.
#[derive(Resource)]
pub struct MapChoice {
    pub index: usize,
    loaded: Option<usize>,
}

impl MapChoice {
    /// Starts on the map named on the command line (e.g. `cs-blocky mirage`), or Dust II.
    pub fn from_args() -> Self {
        let index = std::env::args().nth(1).and_then(|a| find(&a)).unwrap_or(0);
        Self { index, loaded: None }
    }
}

/// F3 switches to the next map.
pub fn change_map(keys: Res<ButtonInput<KeyCode>>, mut choice: ResMut<MapChoice>) {
    if keys.just_pressed(KeyCode::F3) {
        choice.index = (choice.index + 1) % MAPS.len();
    }
}

/// (Re)builds the selected map when it changes: geometry, collisions, navigation and radar,
/// then restarts the match on it.
pub fn load_map(
    mut commands: Commands,
    mut choice: ResMut<MapChoice>,
    mats: Res<BlockMaterials>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut colliders: ResMut<WorldColliders>,
    mut game: ResMut<Match>,
    mut bomb: ResMut<Bomb>,
    mut holes: ResMut<BulletHoles>,
    old: Query<Entity, Or<(With<MapGeometry>, With<RadarRoot>)>>,
) {
    if choice.loaded == Some(choice.index) {
        return;
    }
    for entity in old.iter().chain(holes.0.drain(..)) {
        commands.entity(entity).despawn();
    }
    let map = (MAPS[choice.index].build)();
    info!("Loading map {}", map.name);
    map::spawn_map(&mut commands, &map, &mut meshes, &mats, &mut colliders);
    let nav = NavGrid::build(&map);
    radar::spawn_radar(&mut commands, &map, &nav, &mut images);
    commands.insert_resource(nav);
    commands.insert_resource(map);
    game.restart();
    *bomb = Bomb::default();
    choice.loaded = Some(choice.index);
}

/// Builds a map straight from its radar-generated file.
fn generic(text: &str, theme: Theme) -> MapDef {
    let mut f = MapFile::parse(text);
    for &(_, r) in &f.sites.clone() {
        let pad = 40.0;
        f.floor_kind([r[0] - pad, r[1] - pad, r[2] + pad, r[3] + pad], FloorKind::Tiles);
    }
    let props = f.box_props();
    let mut decor = Vec::new();
    for &(name, r) in &f.sites {
        let (x, y, z) = wall_face_south(&f, (r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0);
        paint_letter(
            &mut decor,
            if name == 'A' { LETTER_A } else { LETTER_B },
            x - 0.75,
            y + 1.6,
            z,
        );
    }
    let sites = f
        .sites
        .iter()
        .map(|&(name, r)| BombSite {
            name,
            area: f.site_area(r),
            plant_spot: f.at((r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0),
            t_routes: vec![],
            t_holds: vec![],
        })
        .collect();
    let mut map = MapDef {
        name: f.title.clone(),
        theme,
        t_spawns: f.t_spawns.iter().map(|&(x, y)| f.at(x, y)).collect(),
        ct_spawns: f.ct_spawns.iter().map(|&(x, y)| f.at(x, y)).collect(),
        grid: f.grid,
        props,
        decor,
        sites,
        ct_holds: vec![],
    };
    auto_strategy(&mut map);
    map
}

/// Works out bot routes and positions from the map layout:
/// - two different T routes to each site (the second one avoids the first),
/// - T post-plant spots spread over each site,
/// - CTs on each site watching where those routes come in, plus one watching mid.
fn auto_strategy(map: &mut MapDef) {
    let nav = NavGrid::build(map);
    // Make sure spawns and plant spots stand on walkable cells.
    let snap = |p: Vec3| nav.nearest_walkable(p).map_or(p, |(x, z)| nav.center(x, z));
    for p in map.t_spawns.iter_mut().chain(map.ct_spawns.iter_mut()) {
        *p = snap(*p);
    }
    let t_spawn = map.t_spawns[0];
    let ct_spawn = map.ct_spawns[0];
    let mut ct_holds = Vec::new();

    for site in &mut map.sites {
        site.plant_spot = snap(site.plant_spot);
        let mut used: HashSet<(i32, i32)> = HashSet::new();
        for _ in 0..2 {
            let Some(path) =
                nav.find_path_with(t_spawn, site.plant_spot, |c| if used.contains(&c) { 6.0 } else { 0.0 })
            else {
                break;
            };
            let overlap = path.iter().filter(|p| used.contains(&NavGrid::cell_of(**p))).count();
            if !site.t_routes.is_empty() && overlap * 10 > path.len() * 7 {
                break; // no real alternative route
            }
            for p in &path {
                let (x, z) = NavGrid::cell_of(*p);
                for dz in -3..=3 {
                    for dx in -3..=3 {
                        used.insert((x + dx, z + dz));
                    }
                }
            }
            // CT hold: a few meters inside the site where this route enters, watching the entrance.
            let inside = |p: &Vec3| {
                site.area.min.x <= p.x && p.x <= site.area.max.x && site.area.min.z <= p.z && p.z <= site.area.max.z
            };
            let enter = path.iter().position(inside).unwrap_or(path.len() - 1);
            let entrance = path[enter.saturating_sub(8)];
            let hold = path[(enter + 5).min(path.len() - 1)];
            ct_holds.push((hold, entrance));
            site.t_routes.push(path.iter().step_by(10).copied().collect());
        }
        // T post-plant spots: walkable cells around the site, spread out.
        let a = site.area;
        for (fx, fz) in [(0.2, 0.2), (0.8, 0.2), (0.2, 0.8), (0.8, 0.8)] {
            site.t_holds.push(snap(Vec3::new(
                a.min.x + (a.max.x - a.min.x) * fx,
                site.plant_spot.y,
                a.min.z + (a.max.z - a.min.z) * fz,
            )));
        }
    }

    // One CT watches the middle of the map, on the way from CT spawn to T spawn.
    if let Some(path) = nav.find_path(ct_spawn, t_spawn) {
        ct_holds.insert(
            2.min(ct_holds.len()),
            (path[path.len() * 35 / 100], path[path.len() * 55 / 100]),
        );
    }
    while ct_holds.len() < 5 {
        let site = &map.sites[ct_holds.len() % map.sites.len()];
        ct_holds.push((site.plant_spot, t_spawn));
    }
    map.ct_holds = ct_holds;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every map: spawns walkable, both sites reachable from both spawns, all bot points reachable.
    #[test]
    fn all_maps_are_playable() {
        for entry in MAPS {
            let map = (entry.build)();
            let nav = NavGrid::build(&map);
            assert_eq!(map.sites.len(), 2, "{}: expected 2 bombsites", entry.name);
            for site in &map.sites {
                for spawn in [map.t_spawns[0], map.ct_spawns[0]] {
                    assert!(
                        nav.find_path(spawn, site.plant_spot).is_some(),
                        "{}: site {} unreachable from {spawn}",
                        entry.name,
                        site.name
                    );
                }
                assert!(!site.t_routes.is_empty(), "{}: no T route to {}", entry.name, site.name);
                for &p in site.t_routes.iter().flatten().chain(&site.t_holds) {
                    assert!(
                        nav.find_path(map.t_spawns[0], p).is_some(),
                        "{}: T point {p} unreachable",
                        entry.name
                    );
                }
            }
            assert!(map.ct_holds.len() >= 5, "{}: not enough CT holds", entry.name);
            for &(hold, _) in &map.ct_holds {
                assert!(
                    nav.find_path(map.ct_spawns[0], hold).is_some(),
                    "{}: CT hold {hold} unreachable",
                    entry.name
                );
            }
        }
    }
}
