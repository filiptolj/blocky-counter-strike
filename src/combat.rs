//! Teams, health, hitboxes, bullet tracing, damage and the blocky character models.

use bevy::prelude::*;

use crate::body::Body;
use crate::hud::{HitMarker, KillFeed};
use crate::physics::{Aabb, WorldColliders};
use crate::player::Player;
use crate::sound::{PlaySfx, Sfx};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Team {
    T,
    CT,
}

impl Team {
    pub fn other(self) -> Team {
        match self {
            Team::T => Team::CT,
            Team::CT => Team::T,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Team::T => "Terrorists",
            Team::CT => "Counter-Terrorists",
        }
    }
}

/// Anyone who takes part in the match: the player or a bot.
#[derive(Component)]
pub struct Combatant {
    pub name: String,
    pub team: Team,
    pub kills: u32,
    pub deaths: u32,
    /// The entity holding this character's visible model.
    pub model: Entity,
}

#[derive(Component)]
pub struct Health {
    pub hp: f32,
    pub armor: f32,
    /// Seconds since death (only meaningful when dead).
    pub dead_for: f32,
    /// Where the last bullet came from, and how long ago (bots use this to turn around).
    pub last_hit_from: Option<Vec3>,
    pub since_hit: f32,
}

impl Default for Health {
    fn default() -> Self {
        Self {
            hp: 100.0,
            armor: 100.0,
            dead_for: 0.0,
            last_hit_from: None,
            since_hit: 99.0,
        }
    }
}

impl Health {
    pub fn alive(&self) -> bool {
        self.hp > 0.0
    }
}

/// A bot whose body the (dead) player has taken over for the rest of the round.
#[derive(Component)]
pub struct Controlled;

/// Wants to plant or defuse this frame (held E for the player, decided by the AI for bots).
#[derive(Component, Default)]
pub struct Intent {
    pub use_key: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HitZone {
    Head,
    Body,
    Legs,
}

/// Hitboxes around a character standing at `feet`. Square, so they work for any facing.
pub fn hitboxes(feet: Vec3, crouch: f32) -> [(HitZone, Aabb); 3] {
    [
        (
            HitZone::Head,
            Aabb::from_center(feet + Vec3::Y * (1.63 - 0.5 * crouch), Vec3::new(0.2, 0.19, 0.2)),
        ),
        (
            HitZone::Body,
            Aabb::from_center(feet + Vec3::Y * (1.12 - 0.35 * crouch), Vec3::new(0.38, 0.33, 0.38)),
        ),
        (
            HitZone::Legs,
            Aabb::from_center(
                feet + Vec3::Y * (0.4 - 0.1 * crouch),
                Vec3::new(0.27, 0.4 - 0.1 * crouch, 0.27),
            ),
        ),
    ]
}

/// A snapshot of someone who can be shot.
#[derive(Clone, Copy)]
pub struct ShotTarget {
    pub entity: Entity,
    pub team: Team,
    pub feet: Vec3,
    pub crouch: f32,
    pub yaw: f32,
}

pub struct ShotResult {
    pub end: Vec3,
    pub hit: Option<(Entity, HitZone, f32)>,
    pub wall_normal: Option<Vec3>,
}

/// How far bullets fly.
pub const MAX_RANGE: f32 = 300.0;

/// Traces a bullet through the world. Teammates don't block bullets (no friendly fire).
pub fn trace_shot(
    world: &WorldColliders,
    origin: Vec3,
    dir: Vec3,
    team: Team,
    targets: &[ShotTarget],
    range: f32,
) -> ShotResult {
    let map_hit = world.raycast(origin, dir, range);
    let mut best_t = map_hit.map_or(range, |(t, _)| t);
    let mut hit = None;
    for target in targets.iter().filter(|t| t.team != team) {
        for (zone, hitbox) in hitboxes(target.feet, target.crouch) {
            if let Some((t, _)) = hitbox.ray(origin, dir, best_t) {
                best_t = t;
                hit = Some((target.entity, zone, t));
            }
        }
    }
    let wall_normal = if hit.is_none() { map_hit.map(|(_, n)| n) } else { None };
    ShotResult {
        end: origin + dir * best_t,
        hit,
        wall_normal,
    }
}

/// True if nothing solid is between `a` and `b`.
pub fn line_of_sight(world: &WorldColliders, a: Vec3, b: Vec3) -> bool {
    let d = b - a;
    let len = d.length();
    len < 0.01 || world.raycast(a, d / len, len).is_none()
}

#[derive(Message)]
pub struct Damage {
    pub target: Entity,
    pub attacker: Option<Entity>,
    pub amount: f32,
    pub armor_pen: f32,
    pub zone: HitZone,
    pub weapon: &'static str,
    pub from: Vec3,
}

#[derive(Message)]
pub struct Killed {
    pub victim: Entity,
    pub position: Vec3,
}

pub fn apply_damage(
    mut damage: MessageReader<Damage>,
    mut killed: MessageWriter<Killed>,
    mut people: Query<(&mut Combatant, &mut Health, &Transform)>,
    mut feed: ResMut<KillFeed>,
    mut hit_marker: ResMut<HitMarker>,
    mut sfx: MessageWriter<PlaySfx>,
    player: Query<Entity, With<Player>>,
) {
    let player = player.single().ok();
    for d in damage.read() {
        let Ok((victim, mut health, transform)) = people.get_mut(d.target) else {
            continue;
        };
        if !health.alive() {
            continue;
        }
        // Hit sounds: the helmet "dink" for headshots on armored targets.
        let sound = if d.zone == HitZone::Head && health.armor > 0.0 {
            Sfx::Headshot
        } else {
            Sfx::Hit
        };
        if Some(d.target) == player {
            sfx.write(PlaySfx::local(sound, 0.7));
        } else if d.weapon != "C4" {
            sfx.write(PlaySfx::at(sound, transform.translation + Vec3::Y * 1.2, 1.0));
        }
        let mut amount = d.amount;
        if health.armor > 0.0 && d.zone != HitZone::Legs {
            let absorbed = amount * (1.0 - d.armor_pen);
            amount -= absorbed;
            health.armor = (health.armor - absorbed * 0.5).max(0.0);
        }
        health.hp -= amount;
        health.last_hit_from = Some(d.from);
        health.since_hit = 0.0;
        if d.attacker.is_some() && d.attacker == player {
            hit_marker.timer = 0.15;
        }
        if health.alive() {
            continue;
        }

        health.hp = 0.0;
        health.dead_for = 0.0;
        let victim_name = victim.name.clone();
        let position = transform.translation;
        people.get_mut(d.target).unwrap().0.deaths += 1;
        killed.write(Killed {
            victim: d.target,
            position,
        });

        let killer_name = match d.attacker.and_then(|a| people.get_mut(a).ok()) {
            Some((mut killer, _, _)) => {
                killer.kills += 1;
                killer.name.clone()
            }
            None => "World".to_string(),
        };
        if d.attacker.is_some() && d.attacker == player {
            hit_marker.kill = true;
        }
        let hs = if d.zone == HitZone::Head { " (HS)" } else { "" };
        let line = format!("{killer_name}  [{}]{hs}  {victim_name}", d.weapon);
        info!("kill: {line}");
        feed.push(line);
    }
}

/// Counts time since the last hit and since death.
pub fn tick_health(time: Res<Time>, mut people: Query<&mut Health>) {
    let dt = time.delta_secs();
    for mut health in &mut people {
        health.since_hit += dt;
        if !health.alive() {
            health.dead_for += dt;
        }
    }
}

/// Parts of a character model that animate.
#[derive(Component)]
pub struct Leg(pub f32);

#[derive(Component)]
pub struct ModelRoot;

/// Builds a Roblox-style blocky character for `team` and returns the model entity.
pub fn spawn_model(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mats: &mut Assets<StandardMaterial>,
    team: Team,
) -> Entity {
    let mut mat = |r: f32, g: f32, b: f32| {
        mats.add(StandardMaterial {
            base_color: Color::srgb(r, g, b),
            perceptual_roughness: 0.9,
            ..default()
        })
    };
    let skin = mat(0.96, 0.8, 0.25); // classic yellow Roblox skin
    let black = mat(0.05, 0.05, 0.05);
    let (shirt, pants, hat) = match team {
        Team::T => (mat(0.62, 0.5, 0.32), mat(0.35, 0.27, 0.18), mat(0.15, 0.12, 0.1)),
        Team::CT => (mat(0.15, 0.3, 0.65), mat(0.12, 0.16, 0.3), mat(0.2, 0.28, 0.22)),
    };

    let root = commands
        .spawn((ModelRoot, Transform::default(), Visibility::default()))
        .id();
    let mut part = |commands: &mut Commands,
                    parent: Entity,
                    size: [f32; 3],
                    pos: [f32; 3],
                    material: &Handle<StandardMaterial>| {
        commands
            .spawn((
                Mesh3d(meshes.add(Cuboid::new(size[0], size[1], size[2]))),
                MeshMaterial3d(material.clone()),
                Transform::from_translation(Vec3::from_array(pos)),
                ChildOf(parent),
            ))
            .id()
    };
    part(commands, root, [0.56, 0.65, 0.3], [0.0, 1.125, 0.0], &shirt); // torso
    part(commands, root, [0.22, 0.65, 0.26], [-0.4, 1.125, 0.0], &shirt); // left arm
    part(commands, root, [0.22, 0.65, 0.26], [0.4, 1.125, 0.0], &skin); // right arm
    part(commands, root, [0.36, 0.36, 0.36], [0.0, 1.63, 0.0], &skin); // head
    part(commands, root, [0.4, 0.1, 0.4], [0.0, 1.84, 0.0], &hat); // helmet / cap
    part(commands, root, [0.06, 0.06, 0.02], [-0.08, 1.67, -0.185], &black); // eyes
    part(commands, root, [0.06, 0.06, 0.02], [0.08, 1.67, -0.185], &black);
    part(commands, root, [0.12, 0.03, 0.02], [0.0, 1.56, -0.185], &black); // smile
    part(commands, root, [0.08, 0.08, 0.5], [0.4, 0.95, -0.3], &black); // gun
    if team == Team::T {
        part(commands, root, [0.38, 0.1, 0.38], [0.0, 1.5, 0.0], &hat); // bandana
    }
    for side in [-1.0f32, 1.0] {
        // Legs pivot at the hip so they can swing.
        let hip = commands
            .spawn((
                Leg(side),
                Transform::from_xyz(side * 0.14, 0.8, 0.0),
                Visibility::default(),
                ChildOf(root),
            ))
            .id();
        part(commands, hip, [0.26, 0.8, 0.3], [0.0, -0.4, 0.0], &pants);
    }
    root
}

/// Turns characters to face their aim, swings legs, crouches and makes the dead fall over.
pub fn animate_characters(
    time: Res<Time>,
    mut people: Query<(&Combatant, &Health, &Body, &mut Transform, Has<Player>, Has<Controlled>)>,
    mut models: Query<(&mut Transform, &mut Visibility, &Children), (With<ModelRoot>, Without<Combatant>)>,
    mut legs: Query<(&Leg, &mut Transform), (Without<ModelRoot>, Without<Combatant>)>,
) {
    let t = time.elapsed_secs();
    for (combatant, health, body, mut transform, is_player, controlled) in &mut people {
        transform.rotation = Quat::from_rotation_y(body.yaw);
        let Ok((mut model, mut visibility, children)) = models.get_mut(combatant.model) else {
            continue;
        };

        // You don't see your own body in first person.
        // (Taken-over bots vanish: the player is now in their body.)
        *visibility = if is_player && health.alive() || controlled {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };

        if !health.alive() {
            let fall = (health.dead_for / 0.35).min(1.0);
            model.rotation = Quat::from_rotation_x(fall * std::f32::consts::FRAC_PI_2);
            model.translation = Vec3::new(0.0, 0.15 * fall, 0.0);
            continue;
        }
        model.rotation = Quat::IDENTITY;
        model.translation = Vec3::new(0.0, -0.45 * body.crouch, 0.0);

        let speed = (body.horizontal_speed() / 6.35).min(1.0);
        let swing = (t * 10.0).sin() * 0.7 * speed;
        for child in children.iter() {
            if let Ok((leg, mut leg_tf)) = legs.get_mut(child) {
                leg_tf.rotation = Quat::from_rotation_x(swing * leg.0 + body.crouch * 0.9);
            }
        }
    }
}
