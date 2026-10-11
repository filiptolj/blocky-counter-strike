//! Bot AI: walks routes with A* pathfinding, spots enemies, aims, shoots in bursts,
//! and plays the objective (Terrorists push a site and plant, CTs hold and retake/defuse).

use std::f32::consts::{PI, TAU};

use bevy::prelude::*;

use crate::body::{self, Body, MoveInput, RUN_SPEED, view_dir, yaw_pitch};
use crate::combat::{Combatant, Damage, Health, Intent, MAX_RANGE, ShotTarget, Team, line_of_sight, trace_shot};
use crate::game::{Bomb, BombState, Match, Phase, site_at};
use crate::map::MapDef;
use crate::nav::NavGrid;
use crate::physics::{GameRng, WorldColliders};
use crate::sound::{PlaySfx, Sfx};
use crate::weapons::{self, BulletHoles, EffectAssets, GunState, Inventory, Tracers, try_fire};

const THINK_INTERVAL: f32 = 0.12;
/// Bots don't see further than this (the fog makes far players hard to see for humans too).
const VIEW_DISTANCE: f32 = 60.0;
/// How long a bot keeps aiming at the spot where it last saw an enemy.
const MEMORY: f32 = 2.5;

/// Bot difficulty. Press F2 in game to change it (applies from the next round).
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Difficulty {
    Easy,
    #[default]
    Normal,
    Hard,
}

impl Difficulty {
    pub fn next(self) -> Self {
        match self {
            Difficulty::Easy => Difficulty::Normal,
            Difficulty::Normal => Difficulty::Hard,
            Difficulty::Hard => Difficulty::Easy,
        }
    }

    /// Range the bots' skill is picked from.
    fn skill_range(self) -> (f32, f32) {
        match self {
            Difficulty::Easy => (0.0, 0.25),
            Difficulty::Normal => (0.25, 0.55),
            Difficulty::Hard => (0.6, 1.0),
        }
    }
}

pub fn change_difficulty(keys: Res<ButtonInput<KeyCode>>, mut difficulty: ResMut<Difficulty>) {
    if keys.just_pressed(KeyCode::F2) {
        *difficulty = difficulty.next();
    }
}

#[derive(Component, Default)]
pub struct Brain {
    /// 0..1, affects reaction time, aim speed and accuracy.
    skill: f32,
    /// Position within the team, used to spread bots over routes and spots.
    index: usize,
    // Strategy for this round.
    route: Vec<Vec3>,
    route_index: usize,
    site: usize,
    site_spot: Vec3,
    hold: Option<(Vec3, Vec3)>,
    // Navigation.
    goal: Option<Vec3>,
    path: Vec<Vec3>,
    path_index: usize,
    repath_in: f32,
    stuck_for: f32,
    // Combat.
    think_in: f32,
    enemy: Option<Entity>,
    enemy_point: Vec3,
    lost_for: f32,
    reaction: f32,
    aim_error: Vec2,
    aim_head: bool,
    burst_left: u32,
    burst_pause: f32,
    strafe: f32,
    strafe_in: f32,
}

impl Brain {
    /// Picks this bot's role for the round. `index` is its position within its team.
    pub fn new_round(
        &mut self,
        index: usize,
        team: Team,
        map: &MapDef,
        t_target: usize,
        difficulty: Difficulty,
        rng: &mut GameRng,
    ) {
        let (lo, hi) = difficulty.skill_range();
        *self = Brain {
            skill: rng.range(lo, hi),
            index,
            burst_left: 3,
            ..default()
        };
        match team {
            Team::T => {
                let site = &map.sites[t_target];
                self.site = t_target;
                self.route = site.t_routes[index % site.t_routes.len()].clone();
                self.site_spot = site.t_holds[index % site.t_holds.len()];
            }
            Team::CT => {
                self.hold = Some(map.ct_holds[index % map.ct_holds.len()]);
            }
        }
    }
}

/// What the objective logic wants the bot to do.
#[derive(Default)]
struct Plan {
    goal: Option<Vec3>,
    /// Stop when this close to the goal.
    stop_distance: f32,
    /// Where to look once there.
    watch: Option<Vec3>,
    use_key: bool,
}

#[derive(Clone, Copy)]
struct Snapshot {
    entity: Entity,
    team: Team,
    alive: bool,
    is_bot: bool,
    feet: Vec3,
    crouch: f32,
    yaw: f32,
}

fn nearest_bot(people: &[Snapshot], team: Team, pos: Vec3) -> Option<Entity> {
    people
        .iter()
        .filter(|p| p.alive && p.is_bot && p.team == team)
        .min_by(|a, b| a.feet.distance_squared(pos).total_cmp(&b.feet.distance_squared(pos)))
        .map(|p| p.entity)
}

fn plan(brain: &mut Brain, me: Entity, team: Team, pos: Vec3, bomb: &Bomb, map: &MapDef, people: &[Snapshot]) -> Plan {
    let site = &map.sites[brain.site];
    // Follow the route first, then the destination.
    let mut along_route = |dest: Vec3| -> Vec3 {
        while brain.route_index < brain.route.len() && pos.distance(brain.route[brain.route_index]) < 2.5 {
            brain.route_index += 1;
        }
        brain.route.get(brain.route_index).copied().unwrap_or(dest)
    };
    match (team, bomb.state) {
        (Team::T, BombState::Dropped(p)) if nearest_bot(people, Team::T, p) == Some(me) => Plan {
            goal: Some(p),
            stop_distance: 0.2,
            ..default()
        },
        (Team::T, BombState::Carried(c)) if c == me => {
            let goal = along_route(site.plant_spot);
            let on_site = site_at(map, pos) == Some(site.name);
            let plant = on_site && pos.distance(site.plant_spot) < 1.5;
            Plan {
                goal: Some(goal),
                stop_distance: 0.4,
                use_key: plant,
                watch: None,
            }
        }
        (
            Team::T,
            BombState::Planted {
                pos: bomb_pos,
                site: name,
            },
        ) => {
            let planted = map.sites.iter().find(|s| s.name == name).unwrap_or(site);
            let spot = planted.t_holds[brain.index % planted.t_holds.len()];
            Plan {
                goal: Some(spot),
                stop_distance: 1.0,
                watch: Some(bomb_pos + (map.ct_spawns[0] - bomb_pos).normalize() * 15.0),
                use_key: false,
            }
        }
        (Team::T, _) => {
            let goal = along_route(brain.site_spot);
            Plan {
                goal: Some(goal),
                stop_distance: 1.0,
                watch: Some(map.ct_spawns[0]),
                use_key: false,
            }
        }
        (Team::CT, BombState::Planted { pos: bomb_pos, .. }) => {
            if nearest_bot(people, Team::CT, bomb_pos) == Some(me) {
                let close = pos.distance(bomb_pos) < 1.2;
                Plan {
                    goal: Some(bomb_pos),
                    stop_distance: 0.5,
                    use_key: close,
                    watch: None,
                }
            } else {
                Plan {
                    goal: Some(bomb_pos),
                    stop_distance: 5.0,
                    watch: Some(bomb_pos),
                    use_key: false,
                }
            }
        }
        (Team::CT, _) => match brain.hold {
            Some((spot, watch)) => Plan {
                goal: Some(spot),
                stop_distance: 0.6,
                watch: Some(watch),
                use_key: false,
            },
            None => Plan::default(),
        },
    }
}

fn wrap_angle(a: f32) -> f32 {
    (a + PI).rem_euclid(TAU) - PI
}

fn turn_towards(current: f32, target: f32, max_step: f32) -> f32 {
    current + wrap_angle(target - current).clamp(-max_step, max_step)
}

/// Where to shoot at someone: head or chest.
fn aim_point(target: &Snapshot, head: bool) -> Vec3 {
    let y = if head {
        1.63 - 0.5 * target.crouch
    } else {
        1.15 - 0.35 * target.crouch
    };
    target.feet + Vec3::Y * y
}
pub fn bot_ai(
    mut commands: Commands,
    time: Res<Time>,
    world: Res<WorldColliders>,
    nav: Res<NavGrid>,
    map: Res<MapDef>,
    game: Res<Match>,
    bomb: Res<Bomb>,
    fx: Res<EffectAssets>,
    mut rng: ResMut<GameRng>,
    mut holes: ResMut<BulletHoles>,
    mut tracers: ResMut<Tracers>,
    mut damage: MessageWriter<Damage>,
    mut sfx: MessageWriter<PlaySfx>,
    mut people: Query<(
        Entity,
        &Combatant,
        &Health,
        &mut Body,
        &mut Transform,
        &mut Inventory,
        &mut GunState,
        &mut Intent,
        Option<&mut Brain>,
    )>,
) {
    let dt = time.delta_secs();
    let snapshot: Vec<Snapshot> = people
        .iter()
        .map(|(entity, c, h, b, t, _, _, _, brain)| Snapshot {
            entity,
            team: c.team,
            alive: h.alive(),
            is_bot: brain.is_some(),
            feet: t.translation,
            crouch: b.crouch,
            yaw: b.yaw,
        })
        .collect();
    let targets: Vec<ShotTarget> = snapshot
        .iter()
        .filter(|s| s.alive)
        .map(|s| ShotTarget {
            entity: s.entity,
            team: s.team,
            feet: s.feet,
            crouch: s.crouch,
            yaw: s.yaw,
        })
        .collect();
    let frozen = game.phase == Phase::Freeze;

    for (me, combatant, health, mut body, mut transform, mut inv, mut gun, mut intent, brain) in people.iter_mut() {
        let Some(mut brain) = brain else { continue };
        weapons::tick_gun(&mut inv, &mut gun, dt);
        intent.use_key = false;
        if !health.alive() {
            continue;
        }
        let pos = transform.translation;
        let eye = pos + Vec3::Y * body.eye_height();
        let team = combatant.team;

        // --- Perception: look for the closest visible enemy a few times per second ---
        brain.think_in -= dt;
        if brain.think_in <= 0.0 {
            brain.think_in = THINK_INTERVAL;
            let facing = view_dir(body.yaw, 0.0);
            let recently_hit = health.since_hit < 1.5;
            let mut best: Option<(&Snapshot, f32)> = None;
            for s in snapshot.iter().filter(|s| s.alive && s.team != team) {
                let to = aim_point(s, false) - eye;
                let dist = to.length();
                if dist > VIEW_DISTANCE || best.is_some_and(|b| dist >= b.1) {
                    continue;
                }
                let in_view = to.normalize().with_y(0.0).dot(facing) > 0.2 || dist < 4.0 || recently_hit;
                let visible = in_view
                    && (line_of_sight(&world, eye, aim_point(s, true))
                        || line_of_sight(&world, eye, aim_point(s, false)));
                if visible {
                    best = Some((s, dist));
                }
            }
            if let Some((target, _)) = best {
                if brain.enemy != Some(target.entity) {
                    let skill = brain.skill;
                    brain.enemy = Some(target.entity);
                    brain.reaction = 0.95 - 0.6 * skill + rng.range(0.0, 0.25);
                    let spread = 0.28 - 0.2 * skill;
                    brain.aim_error = Vec2::new(rng.range(-spread, spread), rng.range(-spread, spread) * 0.6);
                    brain.aim_head = rng.f32() < 0.05 + 0.3 * skill;
                }
                brain.enemy_point = aim_point(target, brain.aim_head);
                brain.lost_for = 0.0;
            } else if brain.enemy.is_some() {
                brain.lost_for += THINK_INTERVAL;
                if brain.lost_for > MEMORY {
                    brain.enemy = None;
                }
            }
        }
        let engaged = brain.enemy.is_some() && brain.lost_for < 0.3;
        let remembering = brain.enemy.is_some() && !engaged;

        // --- Objective ---
        let plan = plan(&mut brain, me, team, pos, &bomb, &map, &snapshot);
        let busy = plan.use_key && !engaged;
        intent.use_key = busy && !frozen;

        // --- Movement ---
        let mut wish = Vec3::ZERO;
        let mut jump = false;
        let mut arrived = false;
        brain.repath_in -= dt;
        if engaged {
            // Strafe a little while reacting, then stand still to shoot accurately.
            brain.strafe_in -= dt;
            if brain.strafe_in <= 0.0 {
                brain.strafe_in = rng.range(0.3, 0.7);
                brain.strafe = if rng.f32() < 0.5 { -1.0 } else { 1.0 };
            }
            if brain.reaction > 0.0 {
                wish = body.axes().1 * brain.strafe;
            }
        } else if let Some(goal) = plan.goal {
            if brain.goal.is_none_or(|g| g.distance(goal) > 1.0) || brain.repath_in <= 0.0 {
                brain.path = nav.find_path(pos, goal).unwrap_or_default();
                brain.path_index = 0;
                brain.goal = Some(goal);
                brain.repath_in = 3.0;
            }
            while brain.path_index < brain.path.len()
                && pos.with_y(0.0).distance(brain.path[brain.path_index].with_y(0.0)) < 0.6
            {
                brain.path_index += 1;
            }
            let next = brain.path.get(brain.path_index).copied().unwrap_or(goal);
            arrived = pos.with_y(0.0).distance(goal.with_y(0.0)) < plan.stop_distance;
            if !arrived && !busy && !remembering {
                wish = (next - pos).with_y(0.0).normalize_or_zero();
            }
        }
        // Unstick: jump and recompute the path if we're not getting anywhere.
        if wish != Vec3::ZERO && body.horizontal_speed() < 0.6 {
            brain.stuck_for += dt;
            if brain.stuck_for > 0.8 {
                jump = true;
                brain.repath_in = 0.0;
                brain.stuck_for = 0.0;
            }
        } else {
            brain.stuck_for = 0.0;
        }
        if frozen {
            wish = Vec3::ZERO;
            jump = false;
        }

        // --- Looking around ---
        let look_target = if brain.enemy.is_some() {
            Some(brain.enemy_point)
        } else if health.since_hit < 1.5 {
            health.last_hit_from
        } else if arrived {
            plan.watch.map(|w| w + Vec3::Y * 1.4)
        } else if wish != Vec3::ZERO {
            Some(eye + wish * 10.0)
        } else {
            None
        };
        if let Some(target) = look_target {
            let (mut yaw, mut pitch) = yaw_pitch(target - eye);
            let mut turn_speed = 5.0;
            if engaged {
                yaw += brain.aim_error.x;
                pitch += brain.aim_error.y;
                let settle = 0.8 + 2.4 * brain.skill;
                brain.aim_error *= (-settle * dt).exp();
                turn_speed = 3.5 + 8.0 * brain.skill;
            }
            body.yaw = turn_towards(body.yaw, yaw, turn_speed * dt);
            body.pitch = body.pitch + (pitch - body.pitch).clamp(-turn_speed * dt, turn_speed * dt);
        }

        // --- Shooting ---
        brain.reaction -= dt;
        brain.burst_pause -= dt;
        if engaged && brain.reaction <= 0.0 && brain.burst_pause <= 0.0 && !frozen {
            let to_enemy = brain.enemy_point - eye;
            let dist = to_enemy.length();
            let off = view_dir(body.yaw, body.pitch).angle_between(to_enemy / dist);
            let tolerance = (0.45 / dist).atan().max(0.02) * 1.5;
            if off < tolerance
                && let Some(dir) = try_fire(
                    &mut inv,
                    &mut gun,
                    &body,
                    &mut rng,
                    0.15 + 0.55 * brain.skill,
                    0.03 * (1.0 - brain.skill) + dist * 0.0006,
                )
            {
                sfx.write(PlaySfx::at(inv.current_def().sound, eye, 1.0));
                let result = trace_shot(&world, eye, dir, team, &targets, MAX_RANGE);
                let (forward, right) = body.axes();
                let muzzle = eye + forward * 0.6 + right * 0.3 - Vec3::Y * 0.5;
                weapons::shot_effects(
                    &mut commands,
                    &fx,
                    &mut holes,
                    &mut tracers,
                    &mut rng,
                    muzzle,
                    dir,
                    &result,
                );
                if let Some((_, mut dmg)) = weapons::shot_damage(inv.current_def(), &result) {
                    dmg.attacker = Some(me);
                    dmg.from = eye;
                    damage.write(dmg);
                }
                brain.burst_left = brain.burst_left.saturating_sub(1);
                if brain.burst_left == 0 {
                    brain.burst_pause = rng.range(0.3, 0.7) - 0.2 * brain.skill;
                    brain.burst_left = if dist > 25.0 {
                        rng.range(1.0, 3.0) as u32
                    } else {
                        rng.range(3.0, 7.0) as u32
                    };
                }
            }
        } else if !engaged
            && gun.reload.is_none()
            && inv.current().ammo < inv.current_def().mag / 3
            && inv.start_reload(&mut gun)
        {
            sfx.write(PlaySfx::at(Sfx::Reload, eye, 0.6));
        }

        // --- Physics ---
        let input = MoveInput {
            wish,
            max_speed: RUN_SPEED * inv.current_def().move_speed,
            jump: jump && body.on_ground,
            crouch: false,
        };
        let mut new_pos = pos;
        body::simulate(&world, &mut body, &mut new_pos, &input, dt);
        transform.translation = new_pos;
    }
}
