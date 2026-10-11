//! The map is a top-down grid of 1x1 m cells. Each cell is a wall or an open floor at some
//! height (optionally with a roof, for tunnels and door frames).
//! The grid is merged into as few boxes as possible, which become block meshes (one per
//! material) and collision boxes. Props (crates, doors, cars...) are extra boxes on top,
//! and decor boxes are visual only.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use std::collections::HashMap;

use crate::physics::{Aabb, WorldColliders};
use crate::textures::BlockMaterials;

/// Size of one grid cell in meters.
pub const CELL: f32 = 1.0;
/// Height of one floor level step. Ramps become quarter-meter stairs you can walk up.
pub const LEVEL_H: f32 = 0.25;
/// Free space between floor and roof in tunnels and door frames.
pub const ROOF_CLEARANCE: f32 = 3.5;
/// Height of the sandstone brick band at the bottom of plaster walls.
const BRICK_BAND: f32 = 1.5;
/// World-space size of one texture repeat.
const TEX_TILE: f32 = 2.0;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum FloorKind {
    Sand,
    Tiles,
    Tunnel,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Cell {
    /// Solid wall. `height` is in half meters; 0 means "pick a building height automatically".
    Wall {
        height: u8,
    },
    Open {
        level: u8,
        floor: FloorKind,
        roof: bool,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PropKind {
    Crate,
    Door,
    Car,
    Container,
    Paint,
}

#[derive(Clone, Copy, Debug)]
pub struct Prop {
    pub kind: PropKind,
    pub aabb: Aabb,
}

#[derive(Clone)]
pub struct Grid {
    pub width: i32,
    pub height: i32,
    cells: Vec<Cell>,
}

impl Grid {
    /// Parses a text grid: '#' = wall, 'c' = floor with a box on it, otherwise the floor
    /// level as a digit/letter (see DIGITS; 'c' is skipped). Returns the grid and the box cells.
    pub fn from_ascii(text: &str) -> (Self, Vec<(i32, i32)>) {
        const DIGITS: &str = "0123456789abdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
        let rows: Vec<&str> = text.lines().filter(|l| !l.is_empty()).collect();
        let (width, height) = (rows[0].len() as i32, rows.len() as i32);
        let mut grid = Self {
            width,
            height,
            cells: vec![Cell::Wall { height: 0 }; (width * height) as usize],
        };
        let mut boxes = Vec::new();
        for (z, row) in rows.iter().enumerate() {
            for (x, ch) in row.chars().enumerate() {
                let (x, z) = (x as i32, z as i32);
                if ch == 'c' {
                    boxes.push((x, z));
                } else if let Some(level) = DIGITS.find(ch) {
                    grid.set(
                        x,
                        z,
                        Cell::Open {
                            level: level as u8,
                            floor: FloorKind::Sand,
                            roof: false,
                        },
                    );
                }
            }
        }
        // A box stands on the highest floor next to it.
        for &(x, z) in &boxes {
            let level = (-1..=1)
                .flat_map(|dz| (-1..=1).map(move |dx| (dx, dz)))
                .filter_map(|(dx, dz)| match grid.get(x + dx, z + dz) {
                    Cell::Open { level, .. } => Some(level),
                    _ => None,
                })
                .max()
                .unwrap_or(0);
            grid.set(
                x,
                z,
                Cell::Open {
                    level,
                    floor: FloorKind::Sand,
                    roof: false,
                },
            );
        }
        (grid, boxes)
    }

    pub fn get(&self, x: i32, z: i32) -> Cell {
        if x < 0 || z < 0 || x >= self.width || z >= self.height {
            return Cell::Wall { height: 0 };
        }
        self.cells[(z * self.width + x) as usize]
    }

    fn set(&mut self, x: i32, z: i32, cell: Cell) {
        // The outer border always stays solid so nobody can leave the map.
        if x < 1 || z < 1 || x >= self.width - 1 || z >= self.height - 1 {
            return;
        }
        self.cells[(z * self.width + x) as usize] = cell;
    }

    /// Changes open cells inside an inclusive rectangle.
    fn modify(&mut self, x0: i32, z0: i32, x1: i32, z1: i32, f: impl Fn(&mut FloorKind, &mut bool)) {
        for z in z0.min(z1)..=z0.max(z1) {
            for x in x0.min(x1)..=x0.max(x1) {
                if let Cell::Open {
                    level,
                    mut floor,
                    mut roof,
                } = self.get(x, z)
                {
                    f(&mut floor, &mut roof);
                    self.set(x, z, Cell::Open { level, floor, roof });
                }
            }
        }
    }

    /// Puts a ceiling over the open cells of a rectangle (tunnels, rooms, door frames).
    pub fn roof(&mut self, x0: i32, z0: i32, x1: i32, z1: i32) {
        self.modify(x0, z0, x1, z1, |_, roof| *roof = true);
    }

    pub fn floor_kind(&mut self, x0: i32, z0: i32, x1: i32, z1: i32, kind: FloorKind) {
        self.modify(x0, z0, x1, z1, |floor, _| *floor = kind);
    }

    pub fn floor_height(&self, x: i32, z: i32) -> f32 {
        match self.get(x, z) {
            Cell::Open { level, .. } => level as f32 * LEVEL_H,
            Cell::Wall { .. } => 0.0,
        }
    }

    /// World position of the center of a cell, standing on its floor.
    pub fn cell_center(&self, x: i32, z: i32) -> Vec3 {
        Vec3::new(
            (x as f32 + 0.5) * CELL,
            self.floor_height(x, z),
            (z as f32 + 0.5) * CELL,
        )
    }

    /// Height of the top of walls and roofs here: a few meters above the highest floor nearby,
    /// varying per 8x8 m block for a nicer skyline.
    fn top_height(&self, x: i32, z: i32) -> f32 {
        if let Cell::Wall { height } = self.get(x, z)
            && height > 0
        {
            return height as f32 * LEVEL_H;
        }
        let mut highest: f32 = 0.0;
        for dz in -3..=3 {
            for dx in -3..=3 {
                if let Cell::Open { level, .. } = self.get(x + dx, z + dz) {
                    highest = highest.max(level as f32 * LEVEL_H);
                }
            }
        }
        let h = ((x / 8) as u32).wrapping_mul(73_856_093) ^ ((z / 8) as u32).wrapping_mul(19_349_663);
        highest + ROOF_CLEARANCE + 1.5 + ((h >> 4) % 4) as f32 * 1.0
    }

    /// Simple text picture of the map, handy for checking a layout.
    #[cfg(test)]
    pub fn ascii(&self) -> String {
        let mut s = String::new();
        for z in 0..self.height {
            for x in 0..self.width {
                s.push(match self.get(x, z) {
                    Cell::Wall { .. } => '#',
                    Cell::Open { roof: true, .. } => '=',
                    Cell::Open { level, .. } => char::from_digit(level as u32 / 2, 36).unwrap_or('?'),
                });
            }
            s.push('\n');
        }
        s
    }
}

/// A bomb site plus the bot strategy points that belong to it.
pub struct BombSite {
    pub name: char,
    /// The bomb can be planted anywhere inside this box.
    pub area: Aabb,
    /// Where bot bomb carriers go to plant.
    pub plant_spot: Vec3,
    /// Ways the Terrorists can walk to this site (list of waypoints each).
    pub t_routes: Vec<Vec<Vec3>>,
    /// Where Terrorists wait after planting.
    pub t_holds: Vec<Vec3>,
}

/// Look of a map: which textures its floors and walls use.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Theme {
    Dust,
    Mirage,
    Inferno,
    Ancient,
}

/// A complete map: grid, props, spawn points and bot strategy points.
#[derive(Resource)]
pub struct MapDef {
    pub name: String,
    pub theme: Theme,
    pub grid: Grid,
    /// Solid props.
    pub props: Vec<Prop>,
    /// Visual-only boxes (painted letters).
    pub decor: Vec<Prop>,
    pub t_spawns: Vec<Vec3>,
    pub ct_spawns: Vec<Vec3>,
    pub sites: Vec<BombSite>,
    /// Defensive spots for Counter-Terrorists: (position, point to watch).
    pub ct_holds: Vec<(Vec3, Vec3)>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Mat {
    Sand,
    Bricks,
    Plaster,
    Tiles,
    Tunnel,
    Ceiling,
}

#[derive(Default)]
struct MeshBuf {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
}

impl MeshBuf {
    /// Adds one face of an axis-aligned box. `u` x `v` must equal `normal` (counter-clockwise winding).
    fn face(&mut self, center: Vec3, u: Vec3, v: Vec3, hu: f32, hv: f32, normal: Vec3) {
        let base = self.positions.len() as u32;
        for (su, sv) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            let p = center + u * (su * hu) + v * (sv * hv);
            self.positions.push(p.to_array());
            self.normals.push(normal.to_array());
            self.uvs.push([p.dot(u) / TEX_TILE, -p.dot(v) / TEX_TILE]);
        }
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    fn into_mesh(self) -> Mesh {
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs)
            .with_inserted_indices(Indices::U32(self.indices))
    }
}

/// Which material each face of a box uses. `None` skips the face.
struct BoxFaces {
    top: Option<Mat>,
    bottom: Option<Mat>,
    sides: Option<Mat>,
}

fn add_box(bufs: &mut HashMap<Mat, MeshBuf>, b: &Aabb, faces: &BoxFaces) {
    if b.size().min_element() <= 0.0 {
        return;
    }
    let c = b.center();
    let h = b.size() * 0.5;
    let mut put = |mat: Option<Mat>, center: Vec3, u: Vec3, v: Vec3, hu: f32, hv: f32, n: Vec3| {
        if let Some(m) = mat {
            bufs.entry(m).or_default().face(center, u, v, hu, hv, n);
        }
    };
    put(faces.top, c + Vec3::Y * h.y, Vec3::X, Vec3::NEG_Z, h.x, h.z, Vec3::Y);
    put(faces.bottom, c - Vec3::Y * h.y, Vec3::X, Vec3::Z, h.x, h.z, Vec3::NEG_Y);
    put(faces.sides, c + Vec3::X * h.x, Vec3::NEG_Z, Vec3::Y, h.z, h.y, Vec3::X);
    put(faces.sides, c - Vec3::X * h.x, Vec3::Z, Vec3::Y, h.z, h.y, Vec3::NEG_X);
    put(faces.sides, c + Vec3::Z * h.z, Vec3::X, Vec3::Y, h.x, h.y, Vec3::Z);
    put(
        faces.sides,
        c - Vec3::Z * h.z,
        Vec3::NEG_X,
        Vec3::Y,
        h.x,
        h.y,
        Vec3::NEG_Z,
    );
}

/// Greedily merges identical cells into rectangles: (x0, z0, x1, z1) inclusive, plus the cell.
fn merge_rects(grid: &Grid) -> Vec<(i32, i32, i32, i32, Cell)> {
    let key = |x: i32, z: i32| (grid.get(x, z), grid.top_height(x, z).to_bits());
    let mut used = vec![false; (grid.width * grid.height) as usize];
    let idx = |x: i32, z: i32| (z * grid.width + x) as usize;
    let mut rects = Vec::new();
    for z in 0..grid.height {
        for x in 0..grid.width {
            if used[idx(x, z)] {
                continue;
            }
            let k = key(x, z);
            let mut x1 = x;
            while x1 + 1 < grid.width && !used[idx(x1 + 1, z)] && key(x1 + 1, z) == k {
                x1 += 1;
            }
            let mut z1 = z;
            'grow: while z1 + 1 < grid.height {
                for xx in x..=x1 {
                    if used[idx(xx, z1 + 1)] || key(xx, z1 + 1) != k {
                        break 'grow;
                    }
                }
                z1 += 1;
            }
            for zz in z..=z1 {
                for xx in x..=x1 {
                    used[idx(xx, zz)] = true;
                }
            }
            rects.push((x, z, x1, z1, k.0));
        }
    }
    rects
}

/// Turns the grid into meshes per material plus collision boxes.
fn build_geometry(grid: &Grid) -> (HashMap<Mat, MeshBuf>, Vec<Aabb>) {
    let mut bufs: HashMap<Mat, MeshBuf> = HashMap::new();
    let mut boxes = Vec::new();
    let side_only = |m: Mat| BoxFaces {
        top: None,
        bottom: None,
        sides: Some(m),
    };

    for (x0, z0, x1, z1, cell) in merge_rects(grid) {
        let (wx0, wz0) = (x0 as f32 * CELL, z0 as f32 * CELL);
        let (wx1, wz1) = ((x1 + 1) as f32 * CELL, (z1 + 1) as f32 * CELL);
        let slab = |y0: f32, y1: f32| Aabb::new(Vec3::new(wx0, y0, wz0), Vec3::new(wx1, y1, wz1));
        let top = (x0..=x1)
            .flat_map(|x| (z0..=z1).map(move |z| (x, z)))
            .map(|(x, z)| grid.top_height(x, z))
            .fold(0.0, f32::max);

        // A plaster wall section with a sandstone brick band at the bottom.
        let wall_section = |bufs: &mut HashMap<Mat, MeshBuf>, y0: f32, y1: f32, bottom: Option<Mat>| {
            if y1 - y0 < 3.0 {
                add_box(
                    bufs,
                    &slab(y0, y1),
                    &BoxFaces {
                        top: Some(Mat::Bricks),
                        bottom,
                        sides: Some(Mat::Bricks),
                    },
                );
            } else {
                let band = BRICK_BAND.max(y0).min(y1);
                add_box(bufs, &slab(y0, band), &side_only(Mat::Bricks));
                add_box(
                    bufs,
                    &slab(band, y1),
                    &BoxFaces {
                        top: Some(Mat::Plaster),
                        bottom,
                        sides: Some(Mat::Plaster),
                    },
                );
            }
        };

        match cell {
            Cell::Wall { .. } => {
                wall_section(&mut bufs, -1.0, top, None);
                boxes.push(slab(-1.0, top));
            }
            Cell::Open { level, floor, roof } => {
                let y = level as f32 * LEVEL_H;
                let (top_mat, side_mat) = match floor {
                    FloorKind::Sand => (Mat::Sand, Mat::Bricks),
                    FloorKind::Tiles => (Mat::Tiles, Mat::Bricks),
                    FloorKind::Tunnel => (Mat::Tunnel, Mat::Tunnel),
                };
                let sides = if level > 0 { Some(side_mat) } else { None };
                add_box(
                    &mut bufs,
                    &slab(-1.0, y),
                    &BoxFaces {
                        top: Some(top_mat),
                        bottom: None,
                        sides,
                    },
                );
                boxes.push(slab(-1.0, y));
                if roof {
                    let r0 = y + ROOF_CLEARANCE;
                    let roof_mat = if floor == FloorKind::Tunnel {
                        Mat::Ceiling
                    } else {
                        Mat::Plaster
                    };
                    add_box(
                        &mut bufs,
                        &slab(r0, top),
                        &BoxFaces {
                            top: Some(Mat::Plaster),
                            bottom: Some(roof_mat),
                            sides: Some(Mat::Plaster),
                        },
                    );
                    boxes.push(slab(r0, top));
                }
            }
        }
    }
    (bufs, boxes)
}

#[derive(Component)]
pub struct MapGeometry;

/// Spawns the map meshes, props and decor, and fills [`WorldColliders`].
pub fn spawn_map(
    commands: &mut Commands,
    map: &MapDef,
    meshes: &mut Assets<Mesh>,
    mats: &BlockMaterials,
    colliders: &mut WorldColliders,
) {
    let (bufs, boxes) = build_geometry(&map.grid);
    info!("Map geometry: {} collision boxes", boxes.len());
    colliders.boxes = boxes;

    let theme = mats.theme(map.theme);
    for (mat, buf) in bufs {
        let material = match mat {
            Mat::Sand => theme.floor.clone(),
            Mat::Bricks => theme.band.clone(),
            Mat::Plaster => theme.wall.clone(),
            Mat::Tiles => theme.tiles.clone(),
            Mat::Tunnel => mats.tunnel.clone(),
            Mat::Ceiling => mats.ceiling.clone(),
        };
        commands.spawn((
            MapGeometry,
            Mesh3d(meshes.add(buf.into_mesh())),
            MeshMaterial3d(material),
            Transform::default(),
        ));
    }

    for (prop, solid) in map
        .props
        .iter()
        .map(|p| (p, true))
        .chain(map.decor.iter().map(|p| (p, false)))
    {
        let material = match prop.kind {
            PropKind::Crate => mats.crate_wood.clone(),
            PropKind::Door => mats.door.clone(),
            PropKind::Car => mats.car.clone(),
            PropKind::Container => mats.container.clone(),
            PropKind::Paint => mats.paint.clone(),
        };
        let size = prop.aabb.size();
        commands.spawn((
            MapGeometry,
            Mesh3d(meshes.add(Cuboid::new(size.x, size.y, size.z))),
            MeshMaterial3d(material),
            Transform::from_translation(prop.aabb.center()),
        ));
        if solid {
            colliders.boxes.push(prop.aabb);
        }
    }
}

/// A map generated from a radar image by `tools/radar_to_map.py` (see that script for the format).
pub struct MapFile {
    pub title: String,
    /// Meters per radar pixel; hand-placed things are given in radar pixels.
    pub m_per_px: f32,
    pub sites: Vec<(char, [f32; 4])>,
    pub t_spawns: Vec<(f32, f32)>,
    pub ct_spawns: Vec<(f32, f32)>,
    pub grid: Grid,
    pub boxes: Vec<(i32, i32)>,
}

impl MapFile {
    pub fn parse(text: &str) -> Self {
        let (header, grid) = text.split_once("\ngrid\n").expect("map file has no grid");
        let (grid, boxes) = Grid::from_ascii(grid);
        let mut file = MapFile {
            title: String::new(),
            m_per_px: 0.1,
            sites: vec![],
            t_spawns: vec![],
            ct_spawns: vec![],
            grid,
            boxes,
        };
        for line in header.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            let num = |i: usize| parts.get(i).and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
            match parts.first() {
                Some(&"name") => file.title = parts[1..].join(" "),
                Some(&"m_per_px") => file.m_per_px = num(1),
                Some(&"site") => file
                    .sites
                    .push((parts[1].chars().next().unwrap_or('?'), [num(2), num(3), num(4), num(5)])),
                Some(&"t") => file.t_spawns.push((num(1), num(2))),
                Some(&"ct") => file.ct_spawns.push((num(1), num(2))),
                _ => {}
            }
        }
        file
    }

    /// Grid cell containing a radar pixel (the generated grid has a 1-cell border).
    pub fn cell(&self, px: f32, py: f32) -> (i32, i32) {
        (
            (px * self.m_per_px / CELL) as i32 + 1,
            (py * self.m_per_px / CELL) as i32 + 1,
        )
    }

    /// World position (x, z) in meters of a radar pixel.
    pub fn world(&self, px: f32, py: f32) -> (f32, f32) {
        (px * self.m_per_px + CELL, py * self.m_per_px + CELL)
    }

    /// Standing position at the center of the cell under a radar pixel.
    pub fn at(&self, px: f32, py: f32) -> Vec3 {
        let (cx, cz) = self.cell(px, py);
        self.grid.cell_center(cx, cz)
    }

    /// The bombsite box (in meters) for a radar rectangle, tall enough to cover the floor.
    pub fn site_area(&self, r: [f32; 4]) -> Aabb {
        let (wx0, wz0) = self.world(r[0], r[1]);
        let (wx1, wz1) = self.world(r[2], r[3]);
        let y = self.at((r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0).y;
        Aabb::new(Vec3::new(wx0, y - 3.0, wz0), Vec3::new(wx1, y + 4.0, wz1))
    }

    /// Sets the floor material inside a radar rectangle.
    pub fn floor_kind(&mut self, r: [f32; 4], kind: FloorKind) {
        let (a, b) = (self.cell(r[0], r[1]), self.cell(r[2], r[3]));
        self.grid.floor_kind(a.0, a.1, b.0, b.1, kind);
    }

    /// Puts a roof over a radar rectangle.
    pub fn roof(&mut self, r: [f32; 4]) {
        let (a, b) = (self.cell(r[0], r[1]), self.cell(r[2], r[3]));
        self.grid.roof(a.0, a.1, b.0, b.1);
    }

    /// Turns the radar's box cells into crates (small ones) and cars (big ones).
    pub fn box_props(&self) -> Vec<Prop> {
        let mut remaining: std::collections::HashSet<(i32, i32)> = self.boxes.iter().copied().collect();
        let mut props = Vec::new();
        while let Some(&start) = remaining.iter().min() {
            let mut group = vec![start];
            remaining.remove(&start);
            let mut i = 0;
            while i < group.len() {
                let (x, z) = group[i];
                for n in [(x + 1, z), (x - 1, z), (x, z + 1), (x, z - 1)] {
                    if remaining.remove(&n) {
                        group.push(n);
                    }
                }
                i += 1;
            }
            let (x0, x1) = (
                group.iter().map(|c| c.0).min().unwrap(),
                group.iter().map(|c| c.0).max().unwrap(),
            );
            let (z0, z1) = (
                group.iter().map(|c| c.1).min().unwrap(),
                group.iter().map(|c| c.1).max().unwrap(),
            );
            let floor = group
                .iter()
                .map(|&(x, z)| self.grid.floor_height(x, z))
                .fold(0.0, f32::max);
            let (kind, height) = match group.len() {
                1..=2 => (PropKind::Crate, 1.1),
                3..=11 => (PropKind::Crate, 1.7),
                _ => (PropKind::Car, 1.9),
            };
            let inset = 0.05;
            props.push(Prop {
                kind,
                aabb: Aabb::new(
                    Vec3::new(x0 as f32 * CELL + inset, floor, z0 as f32 * CELL + inset),
                    Vec3::new(
                        (x1 + 1) as f32 * CELL - inset,
                        floor + height,
                        (z1 + 1) as f32 * CELL - inset,
                    ),
                ),
            });
        }
        props
    }
}
