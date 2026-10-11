//! Match flow (rounds, score, win conditions) and the bomb.

use bevy::prelude::*;

use crate::body::Body;
use crate::bots::{Brain, Difficulty};
use crate::combat::{self, Combatant, Controlled, Damage, Health, HitZone, Intent, Killed, Team};
use crate::hud::KillFeed;
use crate::map::MapDef;
use crate::physics::GameRng;
use crate::player::{Player, Playing, Spectating};
use crate::sound::{PlaySfx, Sfx};
use crate::weapons::{self, BulletHoles, EffectAssets, GunState, Inventory};

pub const TEAM_SIZE: usize = 5;
pub const ROUNDS_TO_WIN: u32 = 13;
const FREEZE_TIME: f32 = 4.0;
const ROUND_TIME: f32 = 115.0;
const ROUND_OVER_TIME: f32 = 5.0;
pub const BOMB_TIME: f32 = 40.0;
pub const PLANT_TIME: f32 = 3.2;
pub const DEFUSE_TIME: f32 = 10.0;
const PLANT_RANGE: f32 = 1.6;
const EXPLOSION_RADIUS: f32 = 22.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// Start of the round: nobody can move or shoot.
    Freeze,
    Live,
    /// Round decided; short pause before the next one.
    Over,
}

#[derive(Resource)]
pub struct Match {
    pub phase: Phase,
    /// Seconds left in the current phase.
    pub timer: f32,
    pub round: u32,
    pub score_t: u32,
    pub score_ct: u32,
    /// Big message in the middle of the screen, and how long it stays.
    pub banner: Option<(String, f32)>,
    /// The team the player will be on from the next round (press M to change).
    pub player_team_next: Team,
    /// True while the player is planting or defusing (they can't move).
    pub player_busy: bool,
    /// Which bomb site the Terrorist bots attack this round.
    pub t_target: usize,
    match_over: bool,
}

impl Default for Match {
    fn default() -> Self {
        Self {
            // Starts "over" with no time left, so the first frame sets up round 1.
            phase: Phase::Over,
            timer: 0.0,
            round: 0,
            score_t: 0,
            score_ct: 0,
            banner: None,
            // CS_TEAM=CT starts the player on the Counter-Terrorist side.
            player_team_next: if std::env::var("CS_TEAM").is_ok_and(|t| t.eq_ignore_ascii_case("ct")) {
                Team::CT
            } else {
                Team::T
            },
            player_busy: false,
            t_target: 0,
            match_over: false,
        }
    }
}

impl Match {
    /// Starts a new match (score 0-0) from the next frame, keeping the player's team choice.
    pub fn restart(&mut self) {
        *self = Match {
            player_team_next: self.player_team_next,
            ..default()
        };
    }

    fn end_round(&mut self, winner: Team, reason: &str) {
        match winner {
            Team::T => self.score_t += 1,
            Team::CT => self.score_ct += 1,
        }
        self.phase = Phase::Over;
        self.timer = ROUND_OVER_TIME;
        let mut text = format!("{} Win\n{reason}", winner.name());
        if self.score_t >= ROUNDS_TO_WIN || self.score_ct >= ROUNDS_TO_WIN {
            text = format!(
                "{} win the match {} - {}!",
                winner.name(),
                self.score_t.max(self.score_ct),
                self.score_t.min(self.score_ct)
            );
            self.match_over = true;
            self.timer = 8.0;
        }
        info!("round over: {}", text.replace('\n', " - "));
        self.banner = Some((text, self.timer));
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum BombState {
    Carried(Entity),
    Dropped(Vec3),
    Planted { pos: Vec3, site: char },
    Exploded,
    Defused,
}

#[derive(Resource)]
pub struct Bomb {
    pub state: BombState,
    /// Seconds until the planted bomb explodes.
    pub timer: f32,
    /// Plant or defuse progress in seconds, and who is doing it.
    pub progress: f32,
    pub actor: Option<Entity>,
}

impl Default for Bomb {
    fn default() -> Self {
        Self {
            state: BombState::Dropped(Vec3::ZERO),
            timer: 0.0,
            progress: 0.0,
            actor: None,
        }
    }
}

impl Bomb {
    pub fn planted_pos(&self) -> Option<Vec3> {
        match self.state {
            BombState::Planted { pos, .. } => Some(pos),
            _ => None,
        }
    }
}

/// The bomb site containing `pos`, if any.
pub fn site_at(map: &MapDef, pos: Vec3) -> Option<char> {
    let p = pos + Vec3::Y * 0.5;
    map.sites
        .iter()
        .find(|s| p.cmpge(s.area.min).all() && p.cmple(s.area.max).all())
        .map(|s| s.name)
}

const T_NAMES: &[&str] = &["Herobrine", "Creeper", "Noob", "Steve", "Oof"];
const CT_NAMES: &[&str] = &["Builderman", "Alex", "Bacon Hair", "Enderman", "Guest 1337"];

/// Spawns the player and the bots. Positions are set when round 1 starts.
pub fn spawn_teams(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    let mut spawn = |commands: &mut Commands, name: String, team: Team, is_player: bool| {
        let model = combat::spawn_model(commands, &mut meshes, &mut mats, team);
        let mut entity = commands.spawn((
            Combatant {
                name,
                team,
                kills: 0,
                deaths: 0,
                model,
            },
            Health::default(),
            Body::default(),
            Transform::default(),
            Visibility::default(),
            Inventory::for_team(team),
            GunState::default(),
            Intent::default(),
        ));
        if is_player {
            entity.insert(Player);
        } else {
            entity.insert(Brain::default());
        }
        let id = entity.id();
        commands.entity(model).insert(ChildOf(id));
    };
    spawn(&mut commands, "You".to_string(), Team::T, true);
    for name in &T_NAMES[..TEAM_SIZE - 1] {
        spawn(&mut commands, format!("BOT {name}"), Team::T, false);
    }
    for name in &CT_NAMES[..TEAM_SIZE] {
        spawn(&mut commands, format!("BOT {name}"), Team::CT, false);
    }
}

type RoundQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static mut Combatant,
        &'static mut Health,
        &'static mut Body,
        &'static mut Transform,
        &'static mut Inventory,
        &'static mut GunState,
        Has<Player>,
        Option<&'static mut Brain>,
    ),
>;

/// Runs the round timer, checks who won and starts new rounds.
pub fn round_system(
    mut commands: Commands,
    time: Res<Time>,
    map: Res<MapDef>,
    mut game: ResMut<Match>,
    mut bomb: ResMut<Bomb>,
    mut rng: ResMut<GameRng>,
    mut holes: ResMut<BulletHoles>,
    mut feed: ResMut<KillFeed>,
    mut sfx: MessageWriter<PlaySfx>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    difficulty: Res<Difficulty>,
    mut people: RoundQuery,
) {
    let dt = time.delta_secs();
    game.timer -= dt;
    if let Some((_, t)) = &mut game.banner {
        *t -= dt;
        if *t <= 0.0 {
            game.banner = None;
        }
    }

    let alive = |team: Team| people.iter().filter(|p| p.1.team == team && p.2.alive()).count();
    let (t_alive, ct_alive) = (alive(Team::T), alive(Team::CT));
    let phase_before = game.phase;

    match game.phase {
        Phase::Freeze => {
            if game.timer <= 0.0 {
                game.phase = Phase::Live;
                game.timer = ROUND_TIME;
            }
        }
        Phase::Live => match bomb.state {
            BombState::Exploded => game.end_round(Team::T, "The bomb exploded"),
            BombState::Defused => game.end_round(Team::CT, "The bomb has been defused"),
            BombState::Planted { .. } if ct_alive == 0 => game.end_round(Team::T, "All Counter-Terrorists eliminated"),
            BombState::Planted { .. } => {}
            _ if t_alive == 0 => game.end_round(Team::CT, "All Terrorists eliminated"),
            _ if ct_alive == 0 => game.end_round(Team::T, "All Counter-Terrorists eliminated"),
            _ if game.timer <= 0.0 => game.end_round(Team::CT, "Time ran out"),
            _ => {}
        },
        Phase::Over => {
            if game.timer <= 0.0 {
                new_round(
                    &mut commands,
                    &map,
                    &mut game,
                    &mut bomb,
                    &mut rng,
                    &mut meshes,
                    &mut mats,
                    *difficulty,
                    &mut people,
                );
                for hole in holes.0.drain(..) {
                    commands.entity(hole).despawn();
                }
                feed.clear();
            }
        }
    }
    match (phase_before, game.phase) {
        (Phase::Freeze, Phase::Live) => {
            sfx.write(PlaySfx::local(Sfx::RoundStart, 0.5));
        }
        (Phase::Live, Phase::Over) => {
            sfx.write(PlaySfx::local(Sfx::RoundEnd, 0.6));
        }
        _ => {}
    }
}
fn new_round(
    commands: &mut Commands,
    map: &MapDef,
    game: &mut Match,
    bomb: &mut Bomb,
    rng: &mut GameRng,
    meshes: &mut Assets<Mesh>,
    mats: &mut Assets<StandardMaterial>,
    difficulty: Difficulty,
    people: &mut RoundQuery,
) {
    if game.match_over {
        game.score_t = 0;
        game.score_ct = 0;
        game.round = 0;
        game.match_over = false;
    }
    game.round += 1;
    game.phase = Phase::Freeze;
    game.timer = FREEZE_TIME;
    game.t_target = (rng.next_u32() as usize) % map.sites.len();
    game.banner = Some((format!("Round {}", game.round), FREEZE_TIME));

    // Team switch: the player swaps places with a bot from the other team.
    let player_team = people.iter().find(|p| p.7).map(|p| p.1.team);
    if let Some(current) = player_team
        && current != game.player_team_next
    {
        let new_team = game.player_team_next;
        let bot = people.iter().find(|p| !p.7 && p.1.team == new_team).map(|p| p.0);
        for (entity, mut combatant, _, _, _, _, _, is_player, _) in people.iter_mut() {
            if is_player || Some(entity) == bot {
                combatant.team = combatant.team.other();
                commands.entity(combatant.model).despawn();
                let model = combat::spawn_model(commands, meshes, mats, combatant.team);
                commands.entity(model).insert(ChildOf(entity));
                combatant.model = model;
            }
        }
    }

    // Respawn everyone at their team's spawn points.
    let mut counts = [0usize; 2];
    let mut terrorists = Vec::new();
    for (entity, combatant, mut health, mut body, mut transform, mut inv, mut gun, is_player, brain) in
        people.iter_mut()
    {
        let team_index = combatant.team as usize;
        let i = counts[team_index];
        counts[team_index] += 1;
        let (spawns, yaw) = match combatant.team {
            Team::T => (&map.t_spawns, 0.0),
            Team::CT => (&map.ct_spawns, std::f32::consts::PI),
        };
        transform.translation = spawns[i % spawns.len()] + Vec3::Y * 0.05;
        *health = Health::default();
        *body = Body::new(yaw);
        // CS_POSE="x,y,z,yaw_deg,pitch_deg" puts the player somewhere specific (for screenshots).
        if is_player && let Some(v) = debug_pose() {
            transform.translation = Vec3::new(v[0], v[1], v[2]);
            body.yaw = v[3].to_radians();
            body.pitch = v[4].to_radians();
        }
        *inv = Inventory::for_team(combatant.team);
        *gun = GunState::default();
        commands.entity(entity).remove::<Controlled>();
        if let Some(mut brain) = brain {
            brain.new_round(i, combatant.team, map, game.t_target, difficulty, rng);
        }
        if combatant.team == Team::T {
            terrorists.push(entity);
        }
    }

    // A random Terrorist gets the bomb.
    *bomb = Bomb::default();
    if !terrorists.is_empty() {
        bomb.state = BombState::Carried(terrorists[(rng.next_u32() as usize) % terrorists.len()]);
    }
}

fn debug_pose() -> Option<Vec<f32>> {
    let v: Vec<f32> = std::env::var("CS_POSE")
        .ok()?
        .split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();
    (v.len() == 5).then_some(v)
}

/// Picking up, planting, defusing and exploding the bomb.
pub fn bomb_system(
    mut commands: Commands,
    time: Res<Time>,
    map: Res<MapDef>,
    fx: Res<EffectAssets>,
    mut game: ResMut<Match>,
    mut bomb: ResMut<Bomb>,
    mut rng: ResMut<GameRng>,
    mut killed: MessageReader<Killed>,
    mut damage: MessageWriter<Damage>,
    mut sfx: MessageWriter<PlaySfx>,
    mut next_beep: Local<f32>,
    people: Query<(Entity, &Combatant, &Health, &Body, &Transform, &Intent, Has<Player>)>,
) {
    let dt = time.delta_secs();
    for k in killed.read() {
        if bomb.state == BombState::Carried(k.victim) {
            bomb.state = BombState::Dropped(k.position);
        }
    }
    let player = people.iter().find(|p| p.6).map(|p| p.0);
    let mut actor = None;

    match bomb.state {
        BombState::Dropped(pos) => {
            let picker = people.iter().find(|p| {
                p.1.team == Team::T
                    && p.2.alive()
                    && p.4.translation.with_y(0.0).distance(pos.with_y(0.0)) < 1.3
                    && (p.4.translation.y - pos.y).abs() < 1.5
            });
            if let Some(p) = picker {
                bomb.state = BombState::Carried(p.0);
            }
        }
        BombState::Carried(carrier) => {
            if let Ok((_, _, health, body, transform, intent, _)) = people.get(carrier)
                && game.phase == Phase::Live
                && health.alive()
                && intent.use_key
                && body.on_ground
                && let Some(site) = site_at(&map, transform.translation)
            {
                actor = Some(carrier);
                bomb.progress += dt;
                if bomb.progress >= PLANT_TIME {
                    bomb.state = BombState::Planted {
                        pos: transform.translation,
                        site,
                    };
                    bomb.timer = BOMB_TIME;
                    bomb.progress = 0.0;
                    actor = None;
                    info!("bomb planted at {site}");
                    sfx.write(PlaySfx::at(Sfx::Planted, transform.translation, 1.5));
                    *next_beep = 1.0;
                    game.banner = Some((format!("Bomb has been planted at {site}"), 3.0));
                }
            }
        }
        BombState::Planted { pos, .. } => {
            if game.phase == Phase::Live {
                bomb.timer -= dt;
                // Beeps get faster as the bomb is about to blow.
                *next_beep -= dt;
                if *next_beep <= 0.0 && bomb.timer > 0.0 {
                    *next_beep = 0.12 + 0.9 * (bomb.timer / BOMB_TIME);
                    sfx.write(PlaySfx::at(Sfx::Beep, pos, 1.2));
                }
            }
            let defuser = people.iter().find(|p| {
                p.1.team == Team::CT && p.2.alive() && p.5.use_key && p.4.translation.distance(pos) < PLANT_RANGE
            });
            if let Some(d) = defuser {
                actor = Some(d.0);
                bomb.progress = if bomb.actor == Some(d.0) {
                    bomb.progress + dt
                } else {
                    dt
                };
                if bomb.progress >= DEFUSE_TIME && bomb.timer > 0.0 {
                    bomb.state = BombState::Defused;
                    sfx.write(PlaySfx::at(Sfx::Defused, pos, 1.5));
                    actor = None;
                }
            }
            if bomb.timer <= 0.0 && bomb.state != BombState::Defused {
                bomb.state = BombState::Exploded;
                actor = None;
                sfx.write(PlaySfx::at(Sfx::Explosion, pos, 4.0));
                weapons::burst(&mut commands, &fx, fx.fire.clone(), &mut rng, pos, Vec3::Y, 90, 6.0);
                weapons::burst(&mut commands, &fx, fx.dust.clone(), &mut rng, pos, Vec3::Y, 40, 4.0);
                for (entity, _, health, _, transform, _, _) in &people {
                    let d = transform.translation.distance(pos);
                    if health.alive() && d < EXPLOSION_RADIUS {
                        damage.write(Damage {
                            target: entity,
                            attacker: None,
                            amount: 400.0 * (1.0 - d / EXPLOSION_RADIUS).powi(2) + 10.0,
                            armor_pen: 1.0,
                            zone: HitZone::Body,
                            weapon: "C4",
                            from: pos,
                        });
                    }
                }
            }
        }
        BombState::Exploded | BombState::Defused => {}
    }

    if actor.is_none() {
        bomb.progress = 0.0;
    }
    bomb.actor = actor;
    game.player_busy = actor.is_some() && actor == player;
}

#[derive(Component)]
pub struct BombModel;

#[derive(Component)]
pub struct BombLight;

pub fn spawn_bomb_model(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    let body = mats.add(StandardMaterial {
        base_color: Color::srgb(0.25, 0.3, 0.2),
        perceptual_roughness: 0.8,
        ..default()
    });
    let light = mats.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.1, 0.1),
        unlit: true,
        ..default()
    });
    commands.spawn((
        BombModel,
        Mesh3d(meshes.add(Cuboid::new(0.36, 0.14, 0.26))),
        MeshMaterial3d(body),
        Transform::default(),
        Visibility::Hidden,
        children![(
            BombLight,
            Mesh3d(meshes.add(Cuboid::new(0.06, 0.04, 0.06))),
            MeshMaterial3d(light),
            Transform::from_xyz(0.1, 0.08, 0.0),
        )],
    ));
}

/// Shows the bomb on the carrier's back, on the ground, or planted with a blinking light.
pub fn update_bomb_model(
    time: Res<Time>,
    bomb: Res<Bomb>,
    people: Query<(&Transform, &Body), Without<BombModel>>,
    mut model: Single<(&mut Transform, &mut Visibility), With<BombModel>>,
    mut light: Single<&mut Visibility, (With<BombLight>, Without<BombModel>)>,
) {
    let (transform, visibility) = &mut *model;
    let mut blink = false;
    match bomb.state {
        BombState::Carried(e) => {
            if let Ok((t, b)) = people.get(e) {
                let (forward, _) = b.axes();
                transform.translation = t.translation + Vec3::Y * (1.15 - 0.4 * b.crouch) - forward * 0.25;
                transform.rotation = Quat::from_rotation_y(b.yaw) * Quat::from_rotation_x(1.2);
                **visibility = Visibility::Inherited;
            }
        }
        BombState::Dropped(p) => {
            transform.translation = p + Vec3::Y * 0.07;
            transform.rotation = Quat::IDENTITY;
            **visibility = Visibility::Inherited;
        }
        BombState::Planted { pos, .. } => {
            transform.translation = pos + Vec3::Y * 0.07;
            transform.rotation = Quat::IDENTITY;
            **visibility = Visibility::Inherited;
            // Blinks faster as the timer runs out.
            let rate = 1.0 + 8.0 * (1.0 - bomb.timer / BOMB_TIME).clamp(0.0, 1.0);
            blink = (time.elapsed_secs() * rate).fract() < 0.3;
        }
        BombState::Exploded | BombState::Defused => **visibility = Visibility::Hidden,
    }
    **light = if blink {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
}

/// While dead, press E to take control of the teammate bot you are spectating (like in CS).
pub fn take_over_bot(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    playing: Res<Playing>,
    spectating: Res<Spectating>,
    mut bomb: ResMut<Bomb>,
    mut feed: ResMut<KillFeed>,
    mut people: Query<(
        Entity,
        &Combatant,
        &mut Health,
        &mut Body,
        &mut Transform,
        &mut Inventory,
        &mut GunState,
        Has<Player>,
    )>,
) {
    if !playing.0 || !keys.just_pressed(KeyCode::KeyE) {
        return;
    }
    let Some(player) = people.iter().find(|p| p.7).map(|p| p.0) else {
        return;
    };
    let Some(target) = spectating.0 else { return };
    let Ok([mut me, mut bot]) = people.get_many_mut([player, target]) else {
        return;
    };
    if me.2.alive() || !bot.2.alive() || bot.1.team != me.1.team || bot.7 {
        return;
    }

    me.4.translation = bot.4.translation;
    *me.3 = std::mem::take(&mut *bot.3);
    me.2.hp = bot.2.hp;
    me.2.armor = bot.2.armor;
    me.2.since_hit = 99.0;
    me.2.last_hit_from = None;
    std::mem::swap(&mut *me.5, &mut *bot.5);
    std::mem::swap(&mut *me.6, &mut *bot.6);
    // The bot is "gone" for the rest of the round (it doesn't count as a death).
    bot.2.hp = 0.0;
    bot.2.dead_for = 99.0;
    commands.entity(target).insert(Controlled);
    if bomb.state == BombState::Carried(target) {
        bomb.state = BombState::Carried(player);
    }
    feed.push(format!("You took control of {}", bot.1.name));
}
