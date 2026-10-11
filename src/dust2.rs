//! A blocky, fan-made recreation of CS2 Dust2.
//!
//! The terrain (walls, floor heights, boxes) in `assets/maps/de_dust2.txt` is generated from the
//! official radar image by `tools/radar_to_map.py`: Valve colors the radar by height, so the
//! floors slope exactly like the real map (T spawn high, mid running down to CT, the pit, ...).
//!
//! Everything placed by hand below uses radar pixel coordinates (0..1024, north up), so it can
//! be checked against the radar picture. Spawns use real in-game coordinates (`setpos`).

use bevy::prelude::*;

use crate::map::{BombSite, CELL, FloorKind, MapDef, MapFile, Prop, PropKind, Theme};
use crate::physics::Aabb;

pub fn dust2() -> MapDef {
    let mut f = MapFile::parse(include_str!("../assets/maps/de_dust2.txt"));

    // Floors: stone tiles on the sites and at CT spawn, sand elsewhere.
    f.floor_kind([620.0, 30.0, 935.0, 275.0], FloorKind::Tiles); // A site
    f.floor_kind([60.0, 10.0, 262.0, 335.0], FloorKind::Tiles); // B site
    f.floor_kind([490.0, 140.0, 680.0, 290.0], FloorKind::Tiles); // CT spawn

    // Roofs: tunnels, the long doors room and door frames.
    let tunnels = [
        [60.0, 405.0, 285.0, 505.0],  // upper tunnels
        [160.0, 505.0, 205.0, 578.0], // tunnel entrance from outside tunnels
        [92.0, 330.0, 133.0, 410.0],  // tunnel exit onto B
        [280.0, 378.0, 442.0, 437.0], // lower tunnels
        [280.0, 437.0, 336.0, 505.0], // tunnel stairs
    ];
    for r in tunnels {
        f.roof(r);
        f.floor_kind(r, FloorKind::Tunnel);
    }
    f.roof([680.0, 553.0, 777.0, 678.0]); // long doors room
    f.roof([440.0, 352.0, 495.0, 380.0]); // mid doors frame
    f.roof([255.0, 225.0, 278.0, 265.0]); // B doors frame

    // --- Props ---------------------------------------------------------------------------
    let mut props = f.box_props();
    // Long's blue container.
    for p in &mut props {
        if p.aabb.min.x < 84.0 && p.aabb.max.x > 84.0 && p.aabb.min.z < 54.0 && p.aabb.max.z > 54.0 {
            p.kind = PropKind::Container;
        }
    }
    // Doors, as drawn on the radar (gaps widened a little so players fit through).
    let mut door = |x0: f32, x1: f32, py: f32, height: f32| {
        let (wx0, wz) = f.world(x0, py);
        let (wx1, _) = f.world(x1, py);
        let y = f.at((x0 + x1) / 2.0, py).y;
        props.push(Prop {
            kind: PropKind::Door,
            aabb: Aabb::new(Vec3::new(wx0, y, wz - 0.08), Vec3::new(wx1, y + height, wz + 0.08)),
        });
    };
    door(444.0, 459.0, 369.0, 3.2); // mid doors (left)
    door(475.0, 490.0, 361.0, 3.2); // mid doors (right)
    door(689.0, 701.0, 559.0, 3.4); // long doors, north pair
    door(716.0, 724.0, 568.0, 3.4);
    door(689.0, 701.0, 661.0, 3.4); // long doors, south pair
    door(716.0, 724.0, 670.0, 3.4);
    {
        // B doors: one door swung open (a vertical line on the radar).
        let (wx, wz0) = f.world(266.0, 236.0);
        let (_, wz1) = f.world(266.0, 254.0);
        let y = f.at(266.0, 245.0).y;
        props.push(Prop {
            kind: PropKind::Door,
            aabb: Aabb::new(Vec3::new(wx - 0.08, y, wz0), Vec3::new(wx + 0.08, y + 3.2, wz1)),
        });
    }

    // Decor: big painted site letters.
    let mut decor = Vec::new();
    let a_wall = wall_face_south(&f, 805.0, 120.0);
    paint_letter(&mut decor, LETTER_A, a_wall.0 - 0.75, a_wall.1 + 1.6, a_wall.2);
    let b_wall = wall_face_south(&f, 115.0, 200.0);
    paint_letter(&mut decor, LETTER_B, b_wall.0 - 0.75, b_wall.1 + 1.6, b_wall.2);

    // --- Spawns (real setpos coordinates, in the map file) and strategy points -------------
    let at = |px: f32, py: f32| f.at(px, py);
    let pts = |list: &[(f32, f32)]| list.iter().map(|&(x, y)| at(x, y)).collect::<Vec<_>>();
    let t_spawns = f.t_spawns.iter().map(|&(x, y)| at(x, y)).collect();
    let ct_spawns = f.ct_spawns.iter().map(|&(x, y)| at(x, y)).collect();
    let site_rect = |name: char| f.sites.iter().find(|s| s.0 == name).map(|s| s.1).unwrap();

    let sites = vec![
        BombSite {
            name: 'A',
            area: f.site_area(site_rect('A')),
            plant_spot: at(812.0, 172.0),
            t_routes: vec![
                // Long: outside long, long doors, long, up to A
                pts(&[
                    (560.0, 930.0),
                    (690.0, 760.0),
                    (712.0, 615.0),
                    (730.0, 520.0),
                    (890.0, 420.0),
                    (890.0, 260.0),
                    (860.0, 170.0),
                ]),
                // Catwalk: top mid, mid, catwalk, short, A
                pts(&[
                    (470.0, 760.0),
                    (470.0, 600.0),
                    (520.0, 470.0),
                    (580.0, 400.0),
                    (640.0, 330.0),
                    (680.0, 230.0),
                    (760.0, 170.0),
                ]),
            ],
            t_holds: pts(&[(880.0, 100.0), (700.0, 140.0), (830.0, 240.0), (760.0, 215.0)]),
        },
        BombSite {
            name: 'B',
            area: f.site_area(site_rect('B')),
            plant_spot: at(205.0, 125.0),
            t_routes: vec![
                // Upper tunnels
                pts(&[
                    (200.0, 890.0),
                    (180.0, 650.0),
                    (182.0, 540.0),
                    (150.0, 455.0),
                    (112.0, 370.0),
                    (150.0, 250.0),
                ]),
                // Mid, lower tunnels, upper tunnels
                pts(&[
                    (470.0, 760.0),
                    (470.0, 560.0),
                    (380.0, 408.0),
                    (305.0, 470.0),
                    (200.0, 455.0),
                    (112.0, 370.0),
                    (150.0, 250.0),
                ]),
            ],
            t_holds: pts(&[(100.0, 130.0), (235.0, 200.0), (150.0, 300.0), (90.0, 250.0)]),
        },
    ];
    let ct_holds = vec![
        (at(880.0, 185.0), at(890.0, 420.0)), // A site watching long
        (at(740.0, 140.0), at(650.0, 300.0)), // A site watching short
        (at(470.0, 250.0), at(470.0, 470.0)), // CT mid watching mid doors
        (at(120.0, 250.0), at(112.0, 380.0)), // B watching the tunnel exit
        (at(235.0, 150.0), at(112.0, 380.0)), // B site from the doors
    ];

    MapDef {
        name: f.title.clone(),
        theme: Theme::Dust,
        grid: f.grid,
        props,
        decor,
        t_spawns,
        ct_spawns,
        sites,
        ct_holds,
    }
}

/// Finds the first wall north of a radar point; returns (x, floor y, wall face z).
pub fn wall_face_south(f: &MapFile, px: f32, py: f32) -> (f32, f32, f32) {
    let (cx, mut cz) = f.cell(px, py);
    let g = &f.grid;
    let floor = g.floor_height(cx, cz);
    while cz > 0 && matches!(g.get(cx, cz - 1), crate::map::Cell::Open { .. }) {
        cz -= 1;
    }
    ((cx as f32 + 0.5) * CELL, floor, cz as f32 * CELL)
}

/// 3x5 pixel letters, top row first.
pub const LETTER_A: [&str; 5] = [".#.", "#.#", "###", "#.#", "#.#"];
pub const LETTER_B: [&str; 5] = ["##.", "#.#", "##.", "#.#", "##."];

/// Paints a big blocky letter on a wall facing south (+Z), starting at (`x`, `y`) bottom left.
pub fn paint_letter(decor: &mut Vec<Prop>, letter: [&str; 5], x: f32, y: f32, wall_z: f32) {
    const PIXEL: f32 = 0.5;
    for (row, line) in letter.iter().enumerate() {
        for (col, ch) in line.chars().enumerate() {
            if ch != '#' {
                continue;
            }
            let px = x + col as f32 * PIXEL;
            let py = y + (4 - row) as f32 * PIXEL;
            decor.push(Prop {
                kind: PropKind::Paint,
                aabb: Aabb::new(
                    Vec3::new(px, py, wall_z),
                    Vec3::new(px + PIXEL, py + PIXEL, wall_z + 0.04),
                ),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn print_dust2() {
        println!("{}", dust2().grid.ascii());
    }

    #[test]
    fn spawns_are_on_open_floor() {
        let map = dust2();
        for p in map.t_spawns.iter().chain(&map.ct_spawns) {
            let cell = map.grid.get((p.x / CELL) as i32, (p.z / CELL) as i32);
            assert!(
                matches!(cell, crate::map::Cell::Open { .. }),
                "spawn {p} is inside a wall"
            );
        }
    }

    #[test]
    fn t_spawn_is_higher_than_ct_spawn() {
        let map = dust2();
        let t = map.t_spawns.iter().map(|p| p.y).sum::<f32>() / 5.0;
        let ct = map.ct_spawns.iter().map(|p| p.y).sum::<f32>() / 5.0;
        assert!(t > ct + 4.0, "T spawn {t} m should be well above CT spawn {ct} m");
    }
}
