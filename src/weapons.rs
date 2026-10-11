//! Guns: definitions, firing with CS-style spray patterns, reloading, switching,
//! the first-person view models and impact effects. Used by both player and bots.

use std::collections::VecDeque;
use std::f32::consts::{PI, TAU};

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;

use crate::body::{Body, RUN_SPEED, view_dir};
use crate::combat::{Combatant, Damage, Health, MAX_RANGE, ShotResult, ShotTarget, Team, trace_shot};
use crate::game::{Match, Phase};
use crate::physics::{GameRng, WorldColliders};
use crate::player::{Player, Playing};
use crate::sound::{PlaySfx, Sfx};

/// Render layer used by the first-person gun, drawn by a separate camera so it never clips into walls.
pub const VIEW_MODEL_LAYER: usize = 1;

pub struct WeaponDef {
    pub name: &'static str,
    /// Index of the first-person model.
    pub model: usize,
    pub damage: f32,
    /// Fraction of damage that goes through armor.
    pub armor_pen: f32,
    /// Seconds between shots.
    pub interval: f32,
    pub mag: u32,
    pub reserve: u32,
    pub reload_time: f32,
    pub draw_time: f32,
    pub auto: bool,
    /// Multiplier on run speed while holding this gun.
    pub move_speed: f32,
    /// Inaccuracy (radians) when standing still / extra at full running speed.
    pub spread_stand: f32,
    pub spread_move: f32,
    /// Damage multiplier per 10 m of distance.
    pub range_mod: f32,
    /// Spray pattern: where the n-th bullet of a spray goes, in degrees (right, up).
    pub pattern: &'static [(f32, f32)],
    pub melee: bool,
    pub sound: Sfx,
}

pub static AK47: WeaponDef = WeaponDef {
    name: "AK-47",
    model: 0,
    damage: 36.0,
    armor_pen: 0.775,
    interval: 0.1,
    mag: 30,
    reserve: 90,
    reload_time: 2.45,
    draw_time: 0.9,
    auto: true,
    move_speed: 0.86,
    spread_stand: 0.0025,
    spread_move: 0.09,
    range_mod: 0.98,
    pattern: &[
        (0.0, 0.0),
        (0.0, 0.6),
        (0.1, 1.4),
        (-0.1, 2.4),
        (0.2, 3.4),
        (0.4, 4.4),
        (0.6, 5.2),
        (0.4, 5.9),
        (-0.3, 6.3),
        (-1.0, 6.5),
        (-1.6, 6.7),
        (-1.9, 6.8),
        (-1.5, 7.0),
        (-0.6, 7.1),
        (0.4, 7.1),
        (1.3, 7.2),
        (1.9, 7.2),
        (2.3, 7.3),
        (2.0, 7.4),
        (1.2, 7.4),
        (0.2, 7.5),
        (-0.8, 7.5),
        (-1.5, 7.5),
        (-1.9, 7.6),
        (-1.6, 7.6),
        (-0.8, 7.6),
        (0.2, 7.6),
        (1.0, 7.7),
        (1.5, 7.7),
        (1.6, 7.7),
    ],
    melee: false,
    sound: Sfx::Ak,
};

pub static M4A4: WeaponDef = WeaponDef {
    name: "M4A4",
    model: 1,
    damage: 33.0,
    armor_pen: 0.7,
    interval: 0.09,
    mag: 30,
    reserve: 90,
    reload_time: 3.1,
    draw_time: 1.0,
    auto: true,
    move_speed: 0.9,
    spread_stand: 0.002,
    spread_move: 0.08,
    range_mod: 0.97,
    pattern: &[
        (0.0, 0.0),
        (0.0, 0.5),
        (0.1, 1.2),
        (0.0, 2.0),
        (-0.1, 2.9),
        (0.1, 3.7),
        (0.4, 4.4),
        (0.6, 4.9),
        (0.4, 5.3),
        (-0.1, 5.6),
        (-0.7, 5.8),
        (-1.1, 5.9),
        (-1.2, 6.0),
        (-0.8, 6.1),
        (-0.1, 6.1),
        (0.6, 6.2),
        (1.1, 6.2),
        (1.3, 6.3),
        (1.0, 6.3),
        (0.4, 6.4),
        (-0.3, 6.4),
        (-0.9, 6.4),
        (-1.2, 6.5),
        (-0.9, 6.5),
        (-0.3, 6.5),
        (0.4, 6.5),
        (0.9, 6.6),
        (1.1, 6.6),
        (0.8, 6.6),
        (0.3, 6.6),
    ],
    melee: false,
    sound: Sfx::M4,
};

pub static GLOCK: WeaponDef = WeaponDef {
    name: "Glock-18",
    model: 2,
    damage: 30.0,
    armor_pen: 0.47,
    interval: 0.15,
    mag: 20,
    reserve: 120,
    reload_time: 2.2,
    draw_time: 0.5,
    auto: false,
    move_speed: 0.96,
    spread_stand: 0.004,
    spread_move: 0.035,
    range_mod: 0.85,
    pattern: &[
        (0.0, 0.0),
        (0.1, 1.0),
        (-0.2, 1.8),
        (0.3, 2.5),
        (-0.2, 3.0),
        (0.2, 3.4),
        (0.0, 3.7),
        (-0.3, 3.9),
        (0.3, 4.1),
        (0.0, 4.2),
    ],
    melee: false,
    sound: Sfx::Glock,
};

pub static USP_S: WeaponDef = WeaponDef {
    name: "USP-S",
    model: 3,
    damage: 35.0,
    armor_pen: 0.505,
    interval: 0.17,
    mag: 12,
    reserve: 24,
    reload_time: 2.2,
    draw_time: 0.5,
    auto: false,
    move_speed: 0.96,
    spread_stand: 0.003,
    spread_move: 0.035,
    range_mod: 0.97,
    pattern: &[
        (0.0, 0.0),
        (0.1, 1.2),
        (-0.2, 2.2),
        (0.3, 3.0),
        (-0.2, 3.6),
        (0.2, 4.0),
        (0.0, 4.3),
        (-0.3, 4.5),
        (0.3, 4.7),
        (0.0, 4.8),
        (-0.2, 4.9),
        (0.2, 5.0),
    ],
    melee: false,
    sound: Sfx::Usp,
};

/// Knife: left click slashes, right click stabs. Hitting someone from behind is a backstab.
pub static KNIFE: WeaponDef = WeaponDef {
    name: "Knife",
    model: 4,
    damage: 40.0,
    armor_pen: 0.85,
    interval: 0.4,
    mag: 0,
    reserve: 0,
    reload_time: 0.0,
    draw_time: 0.4,
    auto: true,
    move_speed: 1.0,
    spread_stand: 0.0,
    spread_move: 0.0,
    range_mod: 1.0,
    pattern: &[(0.0, 0.0)],
    melee: true,
    sound: Sfx::KnifeSwing,
};
const KNIFE_RANGE: f32 = 1.9;
const KNIFE_STAB_DAMAGE: f32 = 65.0;
const KNIFE_STAB_INTERVAL: f32 = 1.0;

pub struct WeaponSlot {
    pub def: &'static WeaponDef,
    pub ammo: u32,
    pub reserve: u32,
}

impl WeaponSlot {
    fn new(def: &'static WeaponDef) -> Self {
        Self {
            def,
            ammo: def.mag,
            reserve: def.reserve,
        }
    }
}

#[derive(Component)]
pub struct Inventory {
    pub slots: Vec<WeaponSlot>,
    pub current: usize,
    pub previous: usize,
}

impl Inventory {
    pub fn for_team(team: Team) -> Self {
        let (primary, pistol) = match team {
            Team::T => (&AK47, &GLOCK),
            Team::CT => (&M4A4, &USP_S),
        };
        Self {
            slots: vec![
                WeaponSlot::new(primary),
                WeaponSlot::new(pistol),
                WeaponSlot::new(&KNIFE),
            ],
            current: 0,
            previous: 1,
        }
    }

    pub fn current(&self) -> &WeaponSlot {
        &self.slots[self.current]
    }

    pub fn current_def(&self) -> &'static WeaponDef {
        self.slots[self.current].def
    }

    pub fn switch_to(&mut self, slot: usize, gun: &mut GunState) {
        if slot == self.current || slot >= self.slots.len() {
            return;
        }
        self.previous = self.current;
        self.current = slot;
        gun.draw = self.current_def().draw_time;
        gun.reload = None;
        gun.shots_fired = 0;
        gun.punch = Vec2::ZERO;
    }

    /// Starts reloading if it makes sense. Returns true if a reload started.
    pub fn start_reload(&self, gun: &mut GunState) -> bool {
        let slot = self.current();
        if !slot.def.melee && slot.ammo < slot.def.mag && slot.reserve > 0 && gun.reload.is_none() && gun.draw <= 0.0 {
            gun.reload = Some(slot.def.reload_time);
            return true;
        }
        false
    }
}

/// Per-person shooting state.
#[derive(Component, Default)]
pub struct GunState {
    pub cooldown: f32,
    /// Seconds of reload left, if reloading.
    pub reload: Option<f32>,
    /// Seconds left until the freshly drawn gun can shoot.
    pub draw: f32,
    pub shots_fired: u32,
    pub since_shot: f32,
    /// Recoil offset of the view (pitch, yaw) in radians.
    pub punch: Vec2,
    /// View model kick animation, 1 right after a shot.
    pub kick: f32,
    pub flash: f32,
    /// The last knife attack was a stab (right click).
    pub heavy: bool,
}

/// Advances timers, finishes reloads and recovers from recoil.
pub fn tick_gun(inv: &mut Inventory, gun: &mut GunState, dt: f32) {
    let def = inv.current_def();
    gun.cooldown -= dt;
    gun.draw -= dt;
    gun.since_shot += dt;
    gun.flash -= dt;
    gun.kick *= (-14.0 * dt).exp();
    if let Some(left) = gun.reload {
        if left - dt <= 0.0 {
            let current = inv.current;
            let slot = &mut inv.slots[current];
            let take = (def.mag - slot.ammo).min(slot.reserve);
            slot.ammo += take;
            slot.reserve -= take;
            gun.reload = None;
        } else {
            gun.reload = Some(left - dt);
        }
    }
    if gun.since_shot > def.interval * 1.5 {
        gun.punch *= (-7.0 * dt).exp();
    }
    if gun.since_shot > 0.4 {
        gun.shots_fired = 0;
    }
}

/// Fires the current gun if possible. Returns the bullet direction.
/// `recoil_control` is how much of the spray pattern the shooter cancels out (bots pull down a bit).
pub fn try_fire(
    inv: &mut Inventory,
    gun: &mut GunState,
    body: &Body,
    rng: &mut GameRng,
    recoil_control: f32,
    extra_spread: f32,
) -> Option<Vec3> {
    let def = inv.current_def();
    if gun.cooldown > 0.0 || gun.reload.is_some() || gun.draw > 0.0 {
        return None;
    }
    if inv.current().ammo == 0 {
        inv.start_reload(gun);
        gun.cooldown = 0.25;
        return None;
    }
    let current = inv.current;
    inv.slots[current].ammo -= 1;
    gun.cooldown = def.interval;
    gun.kick = 1.0;
    gun.flash = 0.04;

    let idx = (gun.shots_fired as usize).min(def.pattern.len() - 1);
    let (right, up) = def.pattern[idx];
    let recoil = Vec2::new(up.to_radians(), -right.to_radians()) * (1.0 - recoil_control); // (pitch, yaw)
    gun.punch = recoil * 0.5;
    gun.shots_fired += 1;
    gun.since_shot = 0.0;

    let moving = (body.horizontal_speed() / RUN_SPEED).min(1.0);
    let mut inaccuracy = def.spread_stand + def.spread_move * moving * moving + extra_spread;
    if !body.on_ground {
        inaccuracy += 0.12;
    }
    if body.crouch > 0.5 {
        inaccuracy *= 0.75;
    }
    let angle = rng.range(0.0, TAU);
    let radius = inaccuracy * rng.f32().sqrt();
    Some(view_dir(
        body.yaw + recoil.y + angle.cos() * radius,
        body.pitch + recoil.x + angle.sin() * radius,
    ))
}

/// Damage after distance falloff and hit zone multiplier.
pub fn shot_damage(def: &WeaponDef, result: &ShotResult) -> Option<(Entity, Damage)> {
    let (target, zone, dist) = result.hit?;
    let multiplier = match zone {
        crate::combat::HitZone::Head => 4.0,
        crate::combat::HitZone::Body => 1.0,
        crate::combat::HitZone::Legs => 0.75,
    };
    Some((
        target,
        Damage {
            target,
            attacker: None,
            amount: def.damage * multiplier * def.range_mod.powf(dist / 10.0),
            armor_pen: def.armor_pen,
            zone,
            weapon: def.name,
            from: Vec3::ZERO,
        },
    ))
}

// ---------------------------------------------------------------------------
// Effects
// ---------------------------------------------------------------------------

#[derive(Component)]
pub struct Particle {
    pub velocity: Vec3,
    pub life: f32,
}

#[derive(Resource)]
pub struct EffectAssets {
    pub cube: Handle<Mesh>,
    pub hole: Handle<StandardMaterial>,
    pub dust: Handle<StandardMaterial>,
    pub blood: Handle<StandardMaterial>,
    pub fire: Handle<StandardMaterial>,
}

#[derive(Resource, Default)]
pub struct BulletHoles(pub VecDeque<Entity>);

#[derive(Resource, Default)]
pub struct Tracers(Vec<(Vec3, Vec3, f32)>);

const MAX_BULLET_HOLES: usize = 120;

pub fn setup_effects(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    let flat = |c: Color| StandardMaterial {
        base_color: c,
        perceptual_roughness: 1.0,
        ..default()
    };
    commands.insert_resource(EffectAssets {
        cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        hole: mats.add(flat(Color::srgb(0.08, 0.07, 0.06))),
        dust: mats.add(flat(Color::srgb(0.78, 0.68, 0.5))),
        blood: mats.add(flat(Color::srgb(0.6, 0.05, 0.05))),
        fire: mats.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.55, 0.1),
            unlit: true,
            ..default()
        }),
    });
}

/// Tracer, bullet hole and impact particles for one shot.
pub fn shot_effects(
    commands: &mut Commands,
    fx: &EffectAssets,
    holes: &mut BulletHoles,
    tracers: &mut Tracers,
    rng: &mut GameRng,
    muzzle: Vec3,
    dir: Vec3,
    result: &ShotResult,
) {
    tracers.0.push((muzzle, result.end, 0.06));
    if result.hit.is_some() {
        burst(commands, fx, fx.blood.clone(), rng, result.end, -dir, 8, 1.0);
    } else if let Some(normal) = result.wall_normal {
        let scale = Vec3::splat(0.09) - normal.abs() * 0.08; // thin along the surface normal
        let hole = commands
            .spawn((
                Mesh3d(fx.cube.clone()),
                MeshMaterial3d(fx.hole.clone()),
                Transform::from_translation(result.end + normal * 0.006).with_scale(scale),
            ))
            .id();
        holes.0.push_back(hole);
        if holes.0.len() > MAX_BULLET_HOLES
            && let Some(old) = holes.0.pop_front()
        {
            commands.entity(old).despawn();
        }
        burst(commands, fx, fx.dust.clone(), rng, result.end, normal, 5, 1.0);
    }
}
pub fn burst(
    commands: &mut Commands,
    fx: &EffectAssets,
    material: Handle<StandardMaterial>,
    rng: &mut GameRng,
    point: Vec3,
    normal: Vec3,
    count: usize,
    power: f32,
) {
    for _ in 0..count {
        let random = Vec3::new(rng.range(-1.0, 1.0), rng.range(0.0, 1.5), rng.range(-1.0, 1.0));
        commands.spawn((
            Particle {
                velocity: (normal * 2.5 + random * 1.8) * power,
                life: rng.range(0.3, 0.6) * power.sqrt(),
            },
            Mesh3d(fx.cube.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(point + normal * 0.05)
                .with_scale(Vec3::splat(rng.range(0.04, 0.08) * power.sqrt())),
        ));
    }
}

pub fn update_particles(
    mut commands: Commands,
    time: Res<Time>,
    mut particles: Query<(Entity, &mut Particle, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (entity, mut p, mut transform) in &mut particles {
        p.life -= dt;
        if p.life <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }
        p.velocity.y -= 12.0 * dt;
        transform.translation += p.velocity * dt;
    }
}

pub fn draw_tracers(time: Res<Time>, mut tracers: ResMut<Tracers>, mut gizmos: Gizmos) {
    let dt = time.delta_secs();
    for (from, to, life) in tracers.0.iter_mut() {
        gizmos.line(*from, *to, Color::srgba(1.0, 0.9, 0.5, 0.8));
        *life -= dt;
    }
    tracers.0.retain(|t| t.2 > 0.0);
}

// ---------------------------------------------------------------------------
// Player shooting
// ---------------------------------------------------------------------------

/// Ignore the mouse button until it is released (e.g. the click that captured the mouse).
#[derive(Default)]
pub struct NeedRelease(bool);
pub fn player_weapons(
    mut commands: Commands,
    time: Res<Time>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    playing: Res<Playing>,
    game: Res<Match>,
    world: Res<WorldColliders>,
    fx: Res<EffectAssets>,
    mut rng: ResMut<GameRng>,
    mut holes: ResMut<BulletHoles>,
    mut tracers: ResMut<Tracers>,
    mut damage: MessageWriter<Damage>,
    mut sfx: MessageWriter<PlaySfx>,
    mut need_release: Local<NeedRelease>,
    player: Single<
        (
            Entity,
            &Combatant,
            &Health,
            &Body,
            &Transform,
            &mut Inventory,
            &mut GunState,
        ),
        With<Player>,
    >,
    others: Query<(Entity, &Combatant, &Health, &Body, &Transform), Without<Player>>,
) {
    let (me, combatant, health, body, transform, mut inv, mut gun) = player.into_inner();
    tick_gun(&mut inv, &mut gun, time.delta_secs());
    if !health.alive() {
        return;
    }

    if playing.0 {
        if keys.just_pressed(KeyCode::Digit1) {
            inv.switch_to(0, &mut gun);
        } else if keys.just_pressed(KeyCode::Digit2) {
            inv.switch_to(1, &mut gun);
        } else if keys.just_pressed(KeyCode::Digit3) {
            inv.switch_to(2, &mut gun);
        } else if keys.just_pressed(KeyCode::KeyQ) {
            let previous = inv.previous;
            inv.switch_to(previous, &mut gun);
        }
        if keys.just_pressed(KeyCode::KeyR) && inv.start_reload(&mut gun) {
            sfx.write(PlaySfx::local(Sfx::Reload, 0.5));
        }
    }

    if !playing.0 {
        need_release.0 = true;
    } else if need_release.0 && !mouse.any_pressed([MouseButton::Left, MouseButton::Right]) {
        need_release.0 = false;
    }
    let def = inv.current_def();
    let can_shoot = playing.0 && !need_release.0 && game.phase != Phase::Freeze;
    let eye = transform.translation + Vec3::Y * body.eye_height();
    let targets: Vec<ShotTarget> = others
        .iter()
        .filter(|(_, _, h, _, _)| h.alive())
        .map(|(entity, c, _, b, t)| ShotTarget {
            entity,
            team: c.team,
            feet: t.translation,
            crouch: b.crouch,
            yaw: b.yaw,
        })
        .collect();

    if def.melee {
        let light = mouse.pressed(MouseButton::Left);
        let heavy = mouse.pressed(MouseButton::Right);
        if can_shoot && (light || heavy) && gun.cooldown <= 0.0 && gun.draw <= 0.0 {
            knife_attack(
                &mut commands,
                &fx,
                &world,
                &mut rng,
                &mut gun,
                &mut damage,
                &mut sfx,
                me,
                combatant.team,
                body,
                eye,
                heavy,
                &targets,
            );
        }
        return;
    }

    let pressed = if def.auto {
        mouse.pressed(MouseButton::Left)
    } else {
        mouse.just_pressed(MouseButton::Left)
    };
    if !can_shoot || !pressed {
        return;
    }
    if inv.current().ammo == 0 && gun.cooldown <= 0.0 && gun.reload.is_none() {
        sfx.write(PlaySfx::local(Sfx::Empty, 0.5));
        if inv.current().reserve > 0 {
            sfx.write(PlaySfx::local(Sfx::Reload, 0.5));
        }
    }
    let Some(dir) = try_fire(&mut inv, &mut gun, body, &mut rng, 0.0, 0.0) else {
        return;
    };
    sfx.write(PlaySfx::local(def.sound, 0.55));

    let result = trace_shot(&world, eye, dir, combatant.team, &targets, MAX_RANGE);
    let (forward, right) = body.axes();
    let muzzle = eye + forward * 0.6 + right * 0.15 - Vec3::Y * 0.15;
    shot_effects(
        &mut commands,
        &fx,
        &mut holes,
        &mut tracers,
        &mut rng,
        muzzle,
        dir,
        &result,
    );
    if let Some((_, mut dmg)) = shot_damage(def, &result) {
        dmg.attacker = Some(me);
        dmg.from = eye;
        damage.write(dmg);
    }
}

/// One knife slash (or stab with `heavy`). Backstabs do a lot more damage.
fn knife_attack(
    commands: &mut Commands,
    fx: &EffectAssets,
    world: &WorldColliders,
    rng: &mut GameRng,
    gun: &mut GunState,
    damage: &mut MessageWriter<Damage>,
    sfx: &mut MessageWriter<PlaySfx>,
    me: Entity,
    team: Team,
    body: &Body,
    eye: Vec3,
    heavy: bool,
    targets: &[ShotTarget],
) {
    gun.cooldown = if heavy { KNIFE_STAB_INTERVAL } else { KNIFE.interval };
    gun.kick = 1.0;
    gun.heavy = heavy;
    let dir = view_dir(body.yaw, body.pitch);
    let result = trace_shot(world, eye, dir, team, targets, KNIFE_RANGE);
    if let Some((target, zone, _)) = result.hit {
        let victim = targets.iter().find(|t| t.entity == target);
        // Backstab: the victim faces away from us.
        let backstab = victim.is_some_and(|v| view_dir(v.yaw, 0.0).dot(dir.with_y(0.0).normalize_or_zero()) > 0.4);
        let base = if heavy { KNIFE_STAB_DAMAGE } else { KNIFE.damage };
        let amount = if backstab {
            base * if heavy { 2.8 } else { 2.25 }
        } else {
            base
        };
        damage.write(Damage {
            target,
            attacker: Some(me),
            amount,
            armor_pen: KNIFE.armor_pen,
            zone: if zone == crate::combat::HitZone::Head {
                crate::combat::HitZone::Body
            } else {
                zone
            },
            weapon: KNIFE.name,
            from: eye,
        });
        burst(commands, fx, fx.blood.clone(), rng, result.end, -dir, 6, 1.0);
        sfx.write(PlaySfx::local(Sfx::KnifeHit, 0.8));
    } else if let Some(normal) = result.wall_normal {
        burst(commands, fx, fx.dust.clone(), rng, result.end, normal, 4, 0.7);
        sfx.write(PlaySfx::local(Sfx::KnifeHit, 0.5));
    } else {
        sfx.write(PlaySfx::local(Sfx::KnifeSwing, 0.6));
    }
}

// ---------------------------------------------------------------------------
// First-person view models
// ---------------------------------------------------------------------------

#[derive(Component)]
pub struct GunModel(pub usize);

#[derive(Component)]
pub struct MuzzleFlash(pub usize);

const VIEW_OFFSETS: [Vec3; 5] = [
    Vec3::new(0.2, -0.2, -0.3),
    Vec3::new(0.2, -0.2, -0.3),
    Vec3::new(0.17, -0.17, -0.32),
    Vec3::new(0.17, -0.17, -0.32),
    Vec3::new(0.2, -0.2, -0.3),
];

/// Builds the voxel guns and hands as children of the camera.
pub fn spawn_view_models(
    commands: &mut Commands,
    camera: Entity,
    meshes: &mut Assets<Mesh>,
    mats: &mut Assets<StandardMaterial>,
) {
    let layer = RenderLayers::layer(VIEW_MODEL_LAYER);
    let mut mat = |r: f32, g: f32, b: f32| {
        mats.add(StandardMaterial {
            base_color: Color::srgb(r, g, b),
            perceptual_roughness: 0.8,
            ..default()
        })
    };
    let metal = mat(0.13, 0.13, 0.14);
    let grey = mat(0.32, 0.33, 0.34);
    let wood = mat(0.5, 0.28, 0.12);
    let glove = mat(0.18, 0.17, 0.15);
    let sleeve = mat(0.45, 0.42, 0.35);
    let steel = mat(0.75, 0.77, 0.8);
    let flash = mats.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.85, 0.3),
        unlit: true,
        ..default()
    });

    // (size, position, x-rotation, material)
    type Part = ([f32; 3], [f32; 3], f32, Handle<StandardMaterial>);
    let ak: Vec<Part> = vec![
        ([0.05, 0.07, 0.30], [0.0, 0.0, -0.10], 0.0, metal.clone()), // receiver
        ([0.022, 0.022, 0.26], [0.0, 0.012, -0.36], 0.0, metal.clone()), // barrel
        ([0.056, 0.056, 0.14], [0.0, -0.005, -0.27], 0.0, wood.clone()), // handguard
        ([0.03, 0.02, 0.16], [0.0, 0.035, -0.27], 0.0, metal.clone()), // gas tube
        ([0.046, 0.075, 0.22], [0.0, -0.03, 0.13], 0.0, wood.clone()), // stock
        ([0.036, 0.085, 0.045], [0.0, -0.07, 0.01], -0.3, wood.clone()), // grip
        ([0.036, 0.09, 0.06], [0.0, -0.075, -0.12], 0.2, metal.clone()), // magazine (upper)
        ([0.036, 0.08, 0.06], [0.0, -0.14, -0.10], 0.5, metal.clone()), // magazine (curved lower)
        ([0.01, 0.035, 0.012], [0.0, 0.04, -0.47], 0.0, metal.clone()), // front sight
        ([0.075, 0.055, 0.1], [0.0, -0.045, -0.27], 0.0, glove.clone()), // left hand
        ([0.065, 0.07, 0.07], [0.005, -0.075, 0.02], 0.0, glove.clone()), // right hand
        ([0.09, 0.09, 0.32], [0.05, -0.13, 0.19], 0.4, sleeve.clone()), // right arm
    ];
    let m4: Vec<Part> = vec![
        ([0.05, 0.075, 0.30], [0.0, 0.0, -0.10], 0.0, grey.clone()), // receiver
        ([0.03, 0.03, 0.08], [0.0, 0.055, -0.08], 0.0, metal.clone()), // carry handle / sight
        ([0.022, 0.022, 0.24], [0.0, 0.012, -0.37], 0.0, metal.clone()), // barrel
        ([0.06, 0.06, 0.16], [0.0, 0.0, -0.28], 0.0, metal.clone()), // handguard
        ([0.046, 0.08, 0.2], [0.0, -0.02, 0.13], 0.0, metal.clone()), // stock
        ([0.036, 0.085, 0.045], [0.0, -0.07, 0.01], -0.3, metal.clone()), // grip
        ([0.034, 0.12, 0.055], [0.0, -0.09, -0.12], 0.1, grey.clone()), // straight magazine
        ([0.075, 0.055, 0.1], [0.0, -0.045, -0.28], 0.0, glove.clone()), // left hand
        ([0.065, 0.07, 0.07], [0.005, -0.075, 0.02], 0.0, glove.clone()), // right hand
        ([0.09, 0.09, 0.32], [0.05, -0.13, 0.19], 0.4, sleeve.clone()), // right arm
    ];
    let glock: Vec<Part> = vec![
        ([0.035, 0.045, 0.19], [0.0, 0.0, -0.06], 0.0, metal.clone()), // slide
        ([0.03, 0.025, 0.17], [0.0, -0.03, -0.06], 0.0, grey.clone()), // frame
        ([0.032, 0.09, 0.05], [0.0, -0.08, 0.02], -0.25, grey.clone()), // grip
        ([0.065, 0.08, 0.07], [0.0, -0.08, 0.025], -0.25, glove.clone()), // hand
        ([0.09, 0.09, 0.32], [0.03, -0.15, 0.18], 0.4, sleeve.clone()), // arm
    ];
    let usp: Vec<Part> = vec![
        ([0.035, 0.045, 0.19], [0.0, 0.0, -0.06], 0.0, metal.clone()), // slide
        ([0.03, 0.025, 0.17], [0.0, -0.03, -0.06], 0.0, metal.clone()), // frame
        ([0.032, 0.09, 0.05], [0.0, -0.08, 0.02], -0.25, metal.clone()), // grip
        ([0.032, 0.032, 0.17], [0.0, 0.0, -0.24], 0.0, metal.clone()), // silencer
        ([0.065, 0.08, 0.07], [0.0, -0.08, 0.025], -0.25, glove.clone()), // hand
        ([0.09, 0.09, 0.32], [0.03, -0.15, 0.18], 0.4, sleeve.clone()), // arm
    ];
    let knife: Vec<Part> = vec![
        ([0.012, 0.045, 0.2], [0.0, 0.02, -0.17], 0.0, steel.clone()), // blade
        ([0.01, 0.02, 0.06], [0.0, 0.045, -0.24], -0.35, steel.clone()), // tip
        ([0.05, 0.02, 0.02], [0.0, 0.0, -0.065], 0.0, metal.clone()),  // guard
        ([0.03, 0.035, 0.11], [0.0, -0.005, 0.0], 0.0, metal.clone()), // handle
        ([0.065, 0.075, 0.08], [0.0, -0.01, 0.0], 0.0, glove.clone()), // hand
        ([0.09, 0.09, 0.32], [0.03, -0.08, 0.18], 0.4, sleeve.clone()), // arm
    ];
    let guns = [
        (ak, Vec3::new(0.0, 0.012, -0.52)),
        (m4, Vec3::new(0.0, 0.012, -0.52)),
        (glock, Vec3::new(0.0, 0.0, -0.18)),
        (usp, Vec3::new(0.0, 0.0, -0.35)),
        (knife, Vec3::new(0.0, 0.0, 0.0)),
    ];

    for (index, (parts, muzzle)) in guns.into_iter().enumerate() {
        let root = commands
            .spawn((
                GunModel(index),
                Transform::from_translation(VIEW_OFFSETS[index]),
                Visibility::Hidden,
                ChildOf(camera),
            ))
            .id();
        for (size, pos, rot, material) in parts {
            commands.spawn((
                Mesh3d(meshes.add(Cuboid::new(size[0], size[1], size[2]))),
                MeshMaterial3d(material),
                Transform::from_translation(Vec3::from_array(pos)).with_rotation(Quat::from_rotation_x(rot)),
                layer.clone(),
                ChildOf(root),
            ));
        }
        commands.spawn((
            MuzzleFlash(index),
            Mesh3d(meshes.add(Cuboid::new(0.07, 0.07, 0.07))),
            MeshMaterial3d(flash.clone()),
            Transform::from_translation(muzzle).with_rotation(Quat::from_rotation_z(PI / 4.0)),
            Visibility::Hidden,
            layer.clone(),
            ChildOf(root),
        ));
    }
}

/// Shows the held gun and animates kick, walking bob, reload and draw.
pub fn animate_view_model(
    time: Res<Time>,
    player: Single<(&Body, &Health, &Inventory, &GunState), With<Player>>,
    mut guns: Query<(&GunModel, &mut Transform, &mut Visibility)>,
    mut flashes: Query<(&MuzzleFlash, &mut Visibility), Without<GunModel>>,
) {
    let (body, health, inv, gun) = player.into_inner();
    let def = inv.current_def();
    let t = time.elapsed_secs();
    let bob = if body.on_ground {
        (body.horizontal_speed() / RUN_SPEED).min(1.0)
    } else {
        0.0
    };

    for (model, mut transform, mut visibility) in &mut guns {
        let active = model.0 == def.model && health.alive();
        *visibility = if active {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if !active {
            continue;
        }
        let mut offset = Vec3::new(
            (t * 8.0).sin() * 0.012 * bob,
            -(t * 8.0).cos().abs() * 0.01 * bob,
            gun.kick * 0.05,
        );
        let mut pitch = gun.kick * 0.07;
        if def.melee {
            // Slash sweeps sideways, stab thrusts forward.
            transform.translation = VIEW_OFFSETS[model.0]
                + offset.with_z(0.0)
                + if gun.heavy {
                    Vec3::new(-0.08, 0.05, -0.18) * gun.kick
                } else {
                    Vec3::new(-0.15 * gun.kick, 0.0, 0.0)
                };
            transform.rotation = if gun.heavy {
                Quat::from_rotation_x(-0.3 * gun.kick)
            } else {
                Quat::from_euler(EulerRot::YXZ, 1.1 * gun.kick, -0.2 * gun.kick, 0.9 * gun.kick)
            };
            if gun.draw > 0.0 {
                transform.translation.y -= 0.3 * (gun.draw / def.draw_time).clamp(0.0, 1.0);
            }
            continue;
        }
        if let Some(left) = gun.reload {
            let dip = (PI * (1.0 - left / def.reload_time)).sin();
            offset.y -= dip * 0.12;
            pitch -= dip * 0.6;
        }
        if gun.draw > 0.0 {
            offset.y -= 0.3 * (gun.draw / def.draw_time).clamp(0.0, 1.0);
        }
        transform.translation = VIEW_OFFSETS[model.0] + offset;
        transform.rotation = Quat::from_rotation_x(pitch);
    }
    for (flash, mut visibility) in &mut flashes {
        let show = flash.0 == def.model && gun.flash > 0.0 && health.alive() && !def.melee;
        *visibility = if show {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}
