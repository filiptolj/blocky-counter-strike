//! Bot navigation: A* pathfinding over the map's 2x2 m cells.
//! A bot can walk to a neighbouring cell if it is at most 0.5 m higher,
//! and can drop down any height.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use bevy::prelude::*;

use crate::map::{CELL, Cell, LEVEL_H, MapDef};

#[derive(Resource)]
pub struct NavGrid {
    width: i32,
    height: i32,
    /// Floor level per cell, `None` if the cell can't be walked on.
    cells: Vec<Option<u8>>,
}

#[derive(Copy, Clone, PartialEq)]
struct Node {
    cost: f32,
    index: usize,
}

impl Eq for Node {}

impl Ord for Node {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reversed so BinaryHeap pops the cheapest node first.
        other.cost.partial_cmp(&self.cost).unwrap_or(Ordering::Equal)
    }
}

impl PartialOrd for Node {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl NavGrid {
    pub fn build(map: &MapDef) -> Self {
        let g = &map.grid;
        let mut cells = Vec::with_capacity((g.width * g.height) as usize);
        for z in 0..g.height {
            for x in 0..g.width {
                cells.push(match g.get(x, z) {
                    Cell::Open { level, .. } => Some(level),
                    Cell::Wall { .. } => None,
                });
            }
        }
        // Props block every cell whose middle part they touch (at body height).
        let margin = 0.45 * CELL;
        for prop in &map.props {
            let (x0, z0) = Self::cell_of(prop.aabb.min);
            let (x1, z1) = Self::cell_of(prop.aabb.max);
            for z in z0.max(0)..=z1.min(g.height - 1) {
                for x in x0.max(0)..=x1.min(g.width - 1) {
                    let c = g.cell_center(x, z);
                    let touches = prop.aabb.min.x < c.x + margin
                        && prop.aabb.max.x > c.x - margin
                        && prop.aabb.min.z < c.z + margin
                        && prop.aabb.max.z > c.z - margin;
                    if touches && prop.aabb.max.y > c.y + 0.6 && prop.aabb.min.y < c.y + 1.8 {
                        cells[(z * g.width + x) as usize] = None;
                    }
                }
            }
        }
        Self {
            width: g.width,
            height: g.height,
            cells,
        }
    }

    fn level(&self, x: i32, z: i32) -> Option<u8> {
        if x < 0 || z < 0 || x >= self.width || z >= self.height {
            return None;
        }
        self.cells[(z * self.width + x) as usize]
    }

    pub fn cell_of(pos: Vec3) -> (i32, i32) {
        ((pos.x / CELL).floor() as i32, (pos.z / CELL).floor() as i32)
    }

    pub fn center(&self, x: i32, z: i32) -> Vec3 {
        let y = self.level(x, z).unwrap_or(0) as f32 * LEVEL_H;
        Vec3::new((x as f32 + 0.5) * CELL, y, (z as f32 + 0.5) * CELL)
    }

    pub fn walkable(&self, x: i32, z: i32) -> bool {
        self.level(x, z).is_some()
    }

    /// The closest walkable cell to `pos` (searching outward a few rings).
    pub fn nearest_walkable(&self, pos: Vec3) -> Option<(i32, i32)> {
        let (cx, cz) = Self::cell_of(pos);
        for r in 0..8i32 {
            let mut best: Option<((i32, i32), f32)> = None;
            for dz in -r..=r {
                for dx in -r..=r {
                    if dx.abs().max(dz.abs()) != r || !self.walkable(cx + dx, cz + dz) {
                        continue;
                    }
                    let d = self.center(cx + dx, cz + dz).distance_squared(pos);
                    if best.is_none_or(|b| d < b.1) {
                        best = Some(((cx + dx, cz + dz), d));
                    }
                }
            }
            if let Some((cell, _)) = best {
                return Some(cell);
            }
        }
        None
    }

    fn can_step(&self, from: (i32, i32), to: (i32, i32)) -> bool {
        match (self.level(from.0, from.1), self.level(to.0, to.1)) {
            // Up at most one stair step (0.5 m); dropping down is always fine.
            (Some(a), Some(b)) => (b as f32 - a as f32) * LEVEL_H <= 0.5,
            _ => false,
        }
    }

    /// Path of world points (cell centers) from `from` to `to`, or `None` if unreachable.
    pub fn find_path(&self, from: Vec3, to: Vec3) -> Option<Vec<Vec3>> {
        self.find_path_with(from, to, |_| 0.0)
    }

    /// Like [`find_path`](Self::find_path), with an extra cost for entering some cells
    /// (used to find a second, different route).
    pub fn find_path_with(&self, from: Vec3, to: Vec3, extra_cost: impl Fn((i32, i32)) -> f32) -> Option<Vec<Vec3>> {
        let start = self.nearest_walkable(from)?;
        let goal = self.nearest_walkable(to)?;
        let idx = |c: (i32, i32)| (c.1 * self.width + c.0) as usize;
        let n = (self.width * self.height) as usize;
        let mut best = vec![f32::INFINITY; n];
        let mut came_from = vec![usize::MAX; n];
        let mut open = BinaryHeap::new();
        let h = |c: (i32, i32)| (((c.0 - goal.0).pow(2) + (c.1 - goal.1).pow(2)) as f32).sqrt();

        best[idx(start)] = 0.0;
        open.push(Node {
            cost: h(start),
            index: idx(start),
        });
        while let Some(Node { index, .. }) = open.pop() {
            let cur = ((index as i32) % self.width, (index as i32) / self.width);
            if cur == goal {
                break;
            }
            for dz in -1..=1 {
                for dx in -1..=1 {
                    if dx == 0 && dz == 0 {
                        continue;
                    }
                    let next = (cur.0 + dx, cur.1 + dz);
                    if !self.can_step(cur, next) {
                        continue;
                    }
                    // No cutting corners on diagonals.
                    if dx != 0
                        && dz != 0
                        && !(self.can_step(cur, (cur.0 + dx, cur.1)) && self.can_step(cur, (cur.0, cur.1 + dz)))
                    {
                        continue;
                    }
                    let cost = best[index] + if dx != 0 && dz != 0 { 1.414 } else { 1.0 } + extra_cost(next);
                    let ni = idx(next);
                    if cost < best[ni] {
                        best[ni] = cost;
                        came_from[ni] = index;
                        open.push(Node {
                            cost: cost + h(next),
                            index: ni,
                        });
                    }
                }
            }
        }
        if best[idx(goal)].is_infinite() {
            return None;
        }

        let mut path = Vec::new();
        let mut i = idx(goal);
        while i != usize::MAX && i != idx(start) {
            let c = ((i as i32) % self.width, (i as i32) / self.width);
            path.push(self.center(c.0, c.1));
            i = came_from[i];
        }
        path.reverse();
        Some(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `cargo test print_reachability -- --nocapture`: '.' = reachable from T spawn,
    /// letters = walkable but unreachable (level/2 in base 36), '#' = blocked.
    #[test]
    fn print_reachability() {
        let map = crate::dust2::dust2();
        let nav = NavGrid::build(&map);
        let start = nav.nearest_walkable(map.t_spawns[0]).unwrap();
        let mut seen = vec![false; (nav.width * nav.height) as usize];
        let mut stack = vec![start];
        seen[(start.1 * nav.width + start.0) as usize] = true;
        while let Some(c) = stack.pop() {
            for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let n = (c.0 + dx, c.1 + dz);
                if nav.can_step(c, n) && !seen[(n.1 * nav.width + n.0) as usize] {
                    seen[(n.1 * nav.width + n.0) as usize] = true;
                    stack.push(n);
                }
            }
        }
        for z in 0..nav.height {
            let row: String = (0..nav.width)
                .map(|x| match nav.level(x, z) {
                    None => '#',
                    Some(_) if seen[(z * nav.width + x) as usize] => '.',
                    Some(l) => char::from_digit(l as u32 / 2, 36).unwrap(),
                })
                .collect();
            println!("{z:3} {row}");
        }
    }

    #[test]
    fn t_spawn_reaches_both_sites_and_ct_spawn() {
        let map = crate::dust2::dust2();
        let nav = NavGrid::build(&map);
        let t = map.t_spawns[0];
        for site in &map.sites {
            assert!(
                nav.find_path(t, site.plant_spot).is_some(),
                "no path to site {}",
                site.name
            );
        }
        assert!(nav.find_path(t, map.ct_spawns[0]).is_some());
    }

    #[test]
    fn routes_and_holds_are_reachable() {
        let map = crate::dust2::dust2();
        let nav = NavGrid::build(&map);
        for site in &map.sites {
            for route in &site.t_routes {
                let mut from = map.t_spawns[0];
                for &p in route.iter().chain([&site.plant_spot]) {
                    assert!(
                        nav.find_path(from, p).is_some(),
                        "site {}: no path {from} -> {p}",
                        site.name
                    );
                    from = p;
                }
            }
            for &hold in &site.t_holds {
                assert!(
                    nav.find_path(site.plant_spot, hold).is_some(),
                    "no path to T hold {hold}"
                );
            }
        }
        for &(hold, _) in &map.ct_holds {
            assert!(
                nav.find_path(map.ct_spawns[0], hold).is_some(),
                "no path to CT hold {hold}"
            );
            for site in &map.sites {
                assert!(
                    nav.find_path(hold, site.plant_spot).is_some(),
                    "CT can't reach site {}",
                    site.name
                );
            }
        }
    }

    #[test]
    fn all_strategy_points_are_walkable() {
        let map = crate::dust2::dust2();
        let nav = NavGrid::build(&map);
        let check = |p: Vec3, what: &str| {
            let (x, z) = NavGrid::cell_of(p);
            assert!(nav.walkable(x, z), "{what} at cell ({x},{z}) is not walkable");
        };
        for site in &map.sites {
            check(site.plant_spot, "plant spot");
            site.t_routes.iter().flatten().for_each(|&p| check(p, "T route point"));
            site.t_holds.iter().for_each(|&p| check(p, "T hold"));
        }
        map.ct_holds.iter().for_each(|h| check(h.0, "CT hold"));
    }
}
