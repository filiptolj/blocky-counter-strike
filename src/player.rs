//! The human player: mouse look, keyboard movement, camera and spectating after death.

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use crate::body::{self, Body, CROUCH_MULT, MoveInput, RUN_SPEED, WALK_MULT};
use crate::combat::{Combatant, Health, Intent};
use crate::game::{Match, Phase};
use crate::physics::WorldColliders;
use crate::weapons::{GunState, Inventory};

/// Marks the entity controlled by the human.
#[derive(Component)]
pub struct Player;

#[derive(Component)]
pub struct PlayerCamera;

/// Mouse sensitivity in radians per pixel. Change with [ and ] in game.
#[derive(Resource)]
pub struct Settings {
    pub sensitivity: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self { sensitivity: 0.0012 }
    }
}

/// True while the mouse is captured and the game receives input.
#[derive(Resource, Default)]
pub struct Playing(pub bool);

/// Who the camera follows while the player is dead.
#[derive(Resource, Default)]
pub struct Spectating(pub Option<Entity>);

pub fn grab_cursor(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    window: Single<(&Window, &mut CursorOptions), With<PrimaryWindow>>,
    mut playing: ResMut<Playing>,
) {
    let (window, mut cursor) = window.into_inner();
    let lock = mouse.just_pressed(MouseButton::Left) && !playing.0;
    let unlock = keys.just_pressed(KeyCode::Escape) || !window.focused;
    if lock {
        cursor.grab_mode = CursorGrabMode::Locked;
        cursor.visible = false;
        playing.0 = true;
    } else if unlock && playing.0 {
        cursor.grab_mode = CursorGrabMode::None;
        cursor.visible = true;
        playing.0 = false;
    }
}

pub fn mouse_look(
    motion: Res<AccumulatedMouseMotion>,
    keys: Res<ButtonInput<KeyCode>>,
    playing: Res<Playing>,
    mut settings: ResMut<Settings>,
    mut body: Single<&mut Body, With<Player>>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
    mut was_playing: Local<bool>,
) {
    if keys.just_pressed(KeyCode::BracketLeft) {
        settings.sensitivity *= 0.85;
    }
    if keys.just_pressed(KeyCode::BracketRight) {
        settings.sensitivity /= 0.85;
    }
    let just_started = playing.0 && !*was_playing;
    *was_playing = playing.0;
    if !playing.0 {
        return;
    }

    // On Linux (and especially WSL) raw mouse motion is unreliable: WSLg reports absolute
    // positions, which sends the view straight to the floor. There we measure how far the
    // cursor moved from the window center instead, and put it back every frame.
    let delta = if cfg!(target_os = "linux") {
        let center = window.size() / 2.0;
        let moved = window.cursor_position().map_or(Vec2::ZERO, |pos| pos - center);
        window.set_cursor_position(Some(center));
        if just_started { Vec2::ZERO } else { moved }
    } else {
        motion.delta
    };
    let d = delta * settings.sensitivity;
    body.yaw -= d.x;
    body.pitch = (body.pitch - d.y).clamp(-1.54, 1.54);
}

pub fn player_input(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    playing: Res<Playing>,
    world: Res<WorldColliders>,
    mut game: ResMut<Match>,
    player: Single<(&Combatant, &Health, &mut Body, &mut Transform, &Inventory, &mut Intent), With<Player>>,
) {
    let (combatant, health, mut body, mut transform, inventory, mut intent) = player.into_inner();
    if playing.0 && keys.just_pressed(KeyCode::KeyM) {
        game.player_team_next = if game.player_team_next == combatant.team {
            combatant.team.other()
        } else {
            combatant.team
        };
    }
    if !health.alive() {
        intent.use_key = false;
        return;
    }

    let pressed = |k: KeyCode| playing.0 && keys.pressed(k);
    intent.use_key = pressed(KeyCode::KeyE);
    let frozen = game.phase == Phase::Freeze;
    // Planting or defusing roots you in place, like in CS.
    let busy = intent.use_key && game.player_busy;

    let mut dir = Vec2::ZERO;
    if pressed(KeyCode::KeyW) {
        dir.y += 1.0;
    }
    if pressed(KeyCode::KeyS) {
        dir.y -= 1.0;
    }
    if pressed(KeyCode::KeyD) {
        dir.x += 1.0;
    }
    if pressed(KeyCode::KeyA) {
        dir.x -= 1.0;
    }
    if frozen || busy {
        dir = Vec2::ZERO;
    }
    let (forward, right) = body.axes();
    let crouch = pressed(KeyCode::ControlLeft) || pressed(KeyCode::KeyC);
    let mut max_speed = RUN_SPEED * inventory.current_def().move_speed;
    if crouch {
        max_speed *= CROUCH_MULT;
    } else if pressed(KeyCode::ShiftLeft) {
        max_speed *= WALK_MULT;
    }
    let input = MoveInput {
        wish: (forward * dir.y + right * dir.x).normalize_or_zero(),
        max_speed,
        jump: playing.0 && keys.just_pressed(KeyCode::Space) && !frozen && !busy,
        crouch,
    };
    let mut pos = transform.translation;
    body::simulate(&world, &mut body, &mut pos, &input, time.delta_secs());
    transform.translation = pos;
}

/// First person while alive; a third-person camera following a teammate while dead.
pub fn update_camera(
    mouse: Res<ButtonInput<MouseButton>>,
    playing: Res<Playing>,
    world: Res<WorldColliders>,
    mut spectating: ResMut<Spectating>,
    player: Single<(&Combatant, &Health, &Body, &Transform, &GunState), (With<Player>, Without<PlayerCamera>)>,
    others: Query<(Entity, &Combatant, &Health, &Body, &Transform), (Without<Player>, Without<PlayerCamera>)>,
    mut camera: Single<&mut Transform, With<PlayerCamera>>,
) {
    let (me, health, body, transform, gun) = player.into_inner();
    if health.alive() {
        spectating.0 = None;
        camera.translation = transform.translation + Vec3::Y * body.eye_height();
        camera.rotation = Quat::from_euler(EulerRot::YXZ, body.yaw + gun.punch.y, body.pitch + gun.punch.x, 0.0);
        return;
    }

    // Pick (or cycle with a click) a living teammate to watch; anyone alive if the team is dead.
    let mut candidates: Vec<Entity> = others
        .iter()
        .filter(|o| o.2.alive() && o.1.team == me.team)
        .map(|o| o.0)
        .collect();
    if candidates.is_empty() {
        candidates = others.iter().filter(|o| o.2.alive()).map(|o| o.0).collect();
    }
    let current = spectating.0.filter(|e| candidates.contains(e));
    let cycle = playing.0 && mouse.just_pressed(MouseButton::Left);
    spectating.0 = if cycle || current.is_none() {
        let next = current
            .and_then(|c| candidates.iter().position(|&e| e == c))
            .map_or(0, |i| i + 1);
        candidates.get(next % candidates.len().max(1)).copied()
    } else {
        current
    };

    let Some(Ok((_, _, _, target_body, target_tf))) = spectating.0.map(|e| others.get(e)) else {
        // Nobody left alive: hover above where we died.
        camera.translation = transform.translation + Vec3::new(0.0, 4.0, 3.0);
        camera.look_at(transform.translation, Vec3::Y);
        return;
    };
    let eye = target_tf.translation + Vec3::Y * 1.6;
    let back = -body::view_dir(target_body.yaw, target_body.pitch.clamp(-0.6, 0.6)) * 3.5 + Vec3::Y * 0.6;
    let len = back.length();
    let dist = world
        .raycast(eye, back / len, len)
        .map_or(len, |(t, _)| (t - 0.3).max(0.2));
    camera.translation = eye + back / len * dist;
    camera.look_at(eye + body::view_dir(target_body.yaw, target_body.pitch) * 5.0, Vec3::Y);
}
