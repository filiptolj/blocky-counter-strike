//! Counter-Strike style movement and box collision, shared by the player and the bots.

use bevy::prelude::*;

use crate::physics::{Aabb, WorldColliders};

// Speeds roughly match CS (250 units/s ≈ 6.35 m/s).
pub const RUN_SPEED: f32 = 6.35;
pub const WALK_MULT: f32 = 0.52;
pub const CROUCH_MULT: f32 = 0.34;
const GROUND_ACCEL: f32 = 5.5;
const AIR_ACCEL: f32 = 12.0;
const AIR_SPEED_CAP: f32 = 0.8;
const FRICTION: f32 = 5.2;
const STOP_SPEED: f32 = 2.5;
const GRAVITY: f32 = 20.0;
const JUMP_SPEED: f32 = 6.6;
const STEP_HEIGHT: f32 = 0.55;

pub const RADIUS: f32 = 0.4;
const STAND_HEIGHT: f32 = 1.8;
const CROUCH_HEIGHT: f32 = 1.3;
const STAND_EYE: f32 = 1.65;
const CROUCH_EYE: f32 = 1.15;

/// Physical state of anyone who walks around: the player or a bot.
#[derive(Component, Default, Clone)]
pub struct Body {
    pub velocity: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
    /// 0 = standing, 1 = fully crouched.
    pub crouch: f32,
    /// Smooths the camera when stepping up/down stairs.
    pub step_smooth: f32,
}

/// What the body wants to do this frame.
#[derive(Default)]
pub struct MoveInput {
    /// Desired horizontal direction (normalized or zero).
    pub wish: Vec3,
    pub max_speed: f32,
    pub jump: bool,
    pub crouch: bool,
}

impl Body {
    pub fn new(yaw: f32) -> Self {
        Self { yaw, ..default() }
    }

    pub fn height(&self) -> f32 {
        STAND_HEIGHT + (CROUCH_HEIGHT - STAND_HEIGHT) * self.crouch
    }

    pub fn eye_height(&self) -> f32 {
        STAND_EYE + (CROUCH_EYE - STAND_EYE) * self.crouch + self.step_smooth
    }

    pub fn horizontal_speed(&self) -> f32 {
        Vec2::new(self.velocity.x, self.velocity.z).length()
    }

    /// Horizontal forward and right vectors for the current yaw.
    pub fn axes(&self) -> (Vec3, Vec3) {
        (
            Vec3::new(-self.yaw.sin(), 0.0, -self.yaw.cos()),
            Vec3::new(self.yaw.cos(), 0.0, -self.yaw.sin()),
        )
    }
}

/// Direction for a yaw/pitch pair (yaw 0 looks towards -Z).
pub fn view_dir(yaw: f32, pitch: f32) -> Vec3 {
    Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0) * Vec3::NEG_Z
}

/// Yaw and pitch that look along `dir`.
pub fn yaw_pitch(dir: Vec3) -> (f32, f32) {
    let yaw = f32::atan2(-dir.x, -dir.z);
    let pitch = f32::atan2(dir.y, Vec2::new(dir.x, dir.z).length());
    (yaw, pitch)
}

fn body_box(feet: Vec3, height: f32) -> Aabb {
    Aabb::new(
        feet - Vec3::new(RADIUS, 0.0, RADIUS),
        feet + Vec3::new(RADIUS, height, RADIUS),
    )
}

/// Moves along one axis and stops at the first obstacle. Returns true if something was hit.
fn move_axis(world: &WorldColliders, pos: &mut Vec3, axis: usize, delta: f32, height: f32) -> bool {
    if delta == 0.0 {
        return false;
    }
    let mut p = *pos;
    p[axis] += delta;
    let b = body_box(p, height);
    let mut hit = false;
    for w in &world.boxes {
        if !w.intersects(&b) {
            continue;
        }
        hit = true;
        if axis == 1 {
            p.y = if delta < 0.0 {
                p.y.max(w.max.y)
            } else {
                p.y.min(w.min.y - height)
            };
        } else if delta > 0.0 {
            p[axis] = p[axis].min(w.min[axis] - RADIUS - 0.001);
        } else {
            p[axis] = p[axis].max(w.max[axis] + RADIUS + 0.001);
        }
    }
    *pos = p;
    hit
}

/// Quake/Source style acceleration: speeds up towards `wish_speed` by at most `accel * dt`.
fn accelerate(vel: &mut Vec3, wish_dir: Vec3, wish_speed: f32, accel: f32, dt: f32) {
    let current = vel.dot(wish_dir);
    let add = wish_speed - current;
    if add <= 0.0 {
        return;
    }
    *vel += wish_dir * (accel * dt).min(add);
}

/// Advances the body by one frame: crouching, friction, acceleration, gravity and collision.
pub fn simulate(world: &WorldColliders, body: &mut Body, pos: &mut Vec3, input: &MoveInput, dt: f32) {
    let dt = dt.min(0.05);

    // --- Crouch (in the air, crouching pulls the legs up: crouch-jumps reach higher) ---
    let old_height = body.height();
    let target = if input.crouch { 1.0 } else { 0.0 };
    let mut new_crouch = body.crouch + (target - body.crouch).clamp(-8.0 * dt, 8.0 * dt);
    let new_height = STAND_HEIGHT + (CROUCH_HEIGHT - STAND_HEIGHT) * new_crouch;
    let mut new_pos = *pos;
    if !body.on_ground {
        new_pos.y += old_height - new_height;
    }
    if new_height > old_height && world.overlaps(&body_box(new_pos, new_height)) {
        new_crouch = body.crouch; // no room to stand up
    } else {
        *pos = new_pos;
    }
    body.crouch = new_crouch;
    let height = body.height();

    // --- Velocity ---
    let mut vel = body.velocity;
    if body.on_ground {
        let speed = Vec2::new(vel.x, vel.z).length();
        if speed > 0.0 {
            let drop = speed.max(STOP_SPEED) * FRICTION * dt;
            let scale = (speed - drop).max(0.0) / speed;
            vel.x *= scale;
            vel.z *= scale;
        }
        accelerate(
            &mut vel,
            input.wish,
            input.max_speed,
            GROUND_ACCEL * input.max_speed,
            dt,
        );
        if input.jump {
            vel.y = JUMP_SPEED;
            body.on_ground = false;
        }
    } else {
        accelerate(
            &mut vel,
            input.wish,
            input.max_speed.min(AIR_SPEED_CAP),
            AIR_ACCEL * input.max_speed,
            dt,
        );
    }
    vel.y -= GRAVITY * dt;

    // --- Horizontal move with auto step-up for stairs ---
    let start = *pos;
    let (dx, dz) = (vel.x * dt, vel.z * dt);
    let mut flat = start;
    let hit_x = move_axis(world, &mut flat, 0, dx, height);
    let hit_z = move_axis(world, &mut flat, 2, dz, height);
    let mut p = flat;
    let (mut block_x, mut block_z) = (hit_x, hit_z);

    if (hit_x || hit_z) && body.on_ground {
        let mut stepped = start;
        move_axis(world, &mut stepped, 1, STEP_HEIGHT, height);
        let sx = move_axis(world, &mut stepped, 0, dx, height);
        let sz = move_axis(world, &mut stepped, 2, dz, height);
        let lift = stepped.y - start.y;
        move_axis(world, &mut stepped, 1, -lift, height);
        let flat_dist = (flat - start).with_y(0.0).length_squared();
        let step_dist = (stepped - start).with_y(0.0).length_squared();
        if step_dist > flat_dist + 1e-6 {
            body.step_smooth -= stepped.y - start.y;
            p = stepped;
            block_x = sx;
            block_z = sz;
        }
    }
    if block_x {
        vel.x = 0.0;
    }
    if block_z {
        vel.z = 0.0;
    }

    // --- Vertical move ---
    let was_on_ground = body.on_ground;
    let hit_y = move_axis(world, &mut p, 1, vel.y * dt, height);
    body.on_ground = hit_y && vel.y < 0.0;
    if hit_y {
        vel.y = 0.0;
    }
    // Stick to the ground when walking down stairs instead of falling off each step.
    if was_on_ground && !body.on_ground && vel.y <= 0.0 {
        let mut probe = p;
        if move_axis(world, &mut probe, 1, -STEP_HEIGHT, height) {
            body.step_smooth += p.y - probe.y;
            p = probe;
            body.on_ground = true;
            vel.y = 0.0;
        }
    }

    body.step_smooth *= (-18.0 * dt).exp();
    body.velocity = vel;

    // Safety net: if something fell out of the world, put it back on top.
    if p.y < -10.0 {
        p.y = 10.0;
        body.velocity = Vec3::ZERO;
    }
    *pos = p;
}
