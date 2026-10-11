//! Axis-aligned boxes, ray tests and a tiny random number generator.
//! Everything in the world is made of boxes, so this is all the "physics" we need.

use bevy::prelude::*;

#[derive(Clone, Copy, Debug)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    pub fn new(min: Vec3, max: Vec3) -> Self {
        Self { min, max }
    }

    pub fn from_center(center: Vec3, half: Vec3) -> Self {
        Self {
            min: center - half,
            max: center + half,
        }
    }

    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    pub fn size(&self) -> Vec3 {
        self.max - self.min
    }

    /// Strict overlap: boxes that only touch do not intersect.
    pub fn intersects(&self, o: &Aabb) -> bool {
        self.min.x < o.max.x
            && self.max.x > o.min.x
            && self.min.y < o.max.y
            && self.max.y > o.min.y
            && self.min.z < o.max.z
            && self.max.z > o.min.z
    }

    /// Slab test. Returns the distance along `dir` and the normal of the face that was hit.
    pub fn ray(&self, origin: Vec3, dir: Vec3, max_t: f32) -> Option<(f32, Vec3)> {
        let mut t_min = 0.0f32;
        let mut t_max = max_t;
        let mut normal = Vec3::ZERO;
        for i in 0..3 {
            let o = origin[i];
            let d = dir[i];
            if d.abs() < 1e-8 {
                if o < self.min[i] || o > self.max[i] {
                    return None;
                }
                continue;
            }
            let (enter, exit) = if d > 0.0 {
                ((self.min[i] - o) / d, (self.max[i] - o) / d)
            } else {
                ((self.max[i] - o) / d, (self.min[i] - o) / d)
            };
            if enter > t_min {
                t_min = enter;
                normal = Vec3::ZERO;
                normal[i] = -d.signum();
            }
            t_max = t_max.min(exit);
            if t_min > t_max {
                return None;
            }
        }
        Some((t_min, normal))
    }
}

/// All solid boxes of the map (walls, floors, roofs, crates).
#[derive(Resource, Default)]
pub struct WorldColliders {
    pub boxes: Vec<Aabb>,
}

impl WorldColliders {
    pub fn overlaps(&self, b: &Aabb) -> bool {
        self.boxes.iter().any(|w| w.intersects(b))
    }

    pub fn raycast(&self, origin: Vec3, dir: Vec3, max_t: f32) -> Option<(f32, Vec3)> {
        let mut best: Option<(f32, Vec3)> = None;
        for b in &self.boxes {
            let limit = best.map_or(max_t, |(t, _)| t);
            if let Some(hit) = b.ray(origin, dir, limit) {
                best = Some(hit);
            }
        }
        best
    }
}

/// Small xorshift generator, so we don't need an external crate for randomness.
#[derive(Resource)]
pub struct GameRng(pub u32);

impl Default for GameRng {
    fn default() -> Self {
        Self(0x9E37_79B9)
    }
}

impl GameRng {
    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    /// Uniform in [0, 1).
    pub fn f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }

    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.f32()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ray_hits_box_front_face() {
        let b = Aabb::new(Vec3::new(-1.0, -1.0, -6.0), Vec3::new(1.0, 1.0, -4.0));
        let (t, n) = b.ray(Vec3::ZERO, Vec3::NEG_Z, 100.0).unwrap();
        assert!((t - 4.0).abs() < 1e-5);
        assert_eq!(n, Vec3::Z);
        assert!(b.ray(Vec3::ZERO, Vec3::Z, 100.0).is_none());
    }
}
