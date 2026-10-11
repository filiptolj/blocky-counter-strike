//! Sound effects, synthesized in code (no audio files): gunshots, knife, hits, headshot
//! "dink", footsteps, reloads, bomb beeps and explosion, round start/end.
//!
//! Gameplay code sends [`PlaySfx`] messages. Playback is only compiled on Windows, because
//! Bevy's audio needs ALSA development packages on Linux/WSL; elsewhere messages are ignored.

// Synthesis is only used by the Windows playback backend (and the tests).
#![cfg_attr(not(windows), allow(dead_code))]

use bevy::prelude::*;
use std::collections::HashMap;

use crate::body::{Body, RUN_SPEED};
use crate::combat::Health;
use crate::player::Player;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Sfx {
    Ak,
    M4,
    Glock,
    Usp,
    KnifeSwing,
    KnifeHit,
    Hit,
    Headshot,
    /// Four slightly different footsteps, so walking doesn't sound repetitive.
    Step(u8),
    Reload,
    Empty,
    Beep,
    Planted,
    Defused,
    Explosion,
    RoundStart,
    RoundEnd,
}

/// Request to play a sound. `pos: None` plays it "in your head" (your own gun, UI sounds).
#[derive(Message, Clone, Copy)]
pub struct PlaySfx {
    pub sfx: Sfx,
    pub pos: Option<Vec3>,
    pub volume: f32,
}

impl PlaySfx {
    pub fn at(sfx: Sfx, pos: Vec3, volume: f32) -> Self {
        Self {
            sfx,
            pos: Some(pos),
            volume,
        }
    }

    pub fn local(sfx: Sfx, volume: f32) -> Self {
        Self { sfx, pos: None, volume }
    }
}

pub struct SoundPlugin;

impl Plugin for SoundPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<PlaySfx>().add_systems(Update, footsteps);
        #[cfg(windows)]
        app.add_systems(Startup, backend::setup)
            .add_systems(Update, backend::attach_listener)
            .add_systems(PostUpdate, backend::play);
    }
}

/// Footstep sounds for anyone running (walking with Shift and crouching are silent, like CS).
fn footsteps(
    time: Res<Time>,
    people: Query<(Entity, &Body, &Health, &Transform, Has<Player>)>,
    mut strides: Local<HashMap<Entity, f32>>,
    mut variant: Local<u8>,
    mut sfx: MessageWriter<PlaySfx>,
) {
    let dt = time.delta_secs();
    for (entity, body, health, transform, is_player) in &people {
        let stride = strides.entry(entity).or_default();
        let speed = body.horizontal_speed();
        if !health.alive() || !body.on_ground || speed < RUN_SPEED * 0.6 || body.crouch > 0.5 {
            continue;
        }
        *stride += speed * dt;
        if *stride > 2.0 {
            *stride = 0.0;
            *variant = (*variant + 1) % 4;
            let step = Sfx::Step(*variant);
            sfx.write(if is_player {
                PlaySfx::local(step, 0.12)
            } else {
                PlaySfx::at(step, transform.translation, 0.45)
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Synthesis
// ---------------------------------------------------------------------------

const RATE: u32 = 22_050;

struct Synth {
    seed: u32,
}

impl Synth {
    fn noise(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        (self.seed as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

fn samples(seconds: f32) -> usize {
    (seconds * RATE as f32) as usize
}

fn t_of(i: usize) -> f32 {
    i as f32 / RATE as f32
}

/// A gunshot: a filtered noise blast plus a low "thump".
/// `tone` 0..1 is how bright the noise is, `decay` how fast it dies, `thump` the body frequency.
fn gunshot(seed: u32, length: f32, tone: f32, decay: f32, thump: f32, gain: f32) -> Vec<f32> {
    let mut s = Synth { seed };
    let mut low = 0.0;
    (0..samples(length))
        .map(|i| {
            let t = t_of(i);
            low += (s.noise() - low) * tone;
            let crack = if t < 0.004 { s.noise() } else { 0.0 };
            let body = (t * thump * std::f32::consts::TAU).sin() * (-t * decay * 1.6).exp();
            (low * 1.6 * (-t * decay).exp() + body * 0.8 + crack) * gain
        })
        .collect()
}

fn tone(freq: f32, length: f32, gain: f32) -> Vec<f32> {
    (0..samples(length))
        .map(|i| {
            let t = t_of(i);
            let env = (t / 0.005).min(1.0) * ((length - t) / 0.02).clamp(0.0, 1.0);
            (t * freq * std::f32::consts::TAU).sin() * env * gain
        })
        .collect()
}

fn mix(into: &mut Vec<f32>, other: &[f32], at: f32) {
    let offset = samples(at);
    if into.len() < offset + other.len() {
        into.resize(offset + other.len(), 0.0);
    }
    for (i, v) in other.iter().enumerate() {
        into[offset + i] += v;
    }
}

fn click(seed: u32, gain: f32) -> Vec<f32> {
    let mut s = Synth { seed };
    (0..samples(0.03))
        .map(|i| s.noise() * (-t_of(i) * 180.0).exp() * gain)
        .collect()
}

fn synthesize(sfx: Sfx) -> Vec<f32> {
    use std::f32::consts::TAU;
    match sfx {
        Sfx::Ak => gunshot(11, 0.5, 0.35, 12.0, 70.0, 0.9),
        Sfx::M4 => gunshot(22, 0.4, 0.5, 16.0, 85.0, 0.8),
        Sfx::Glock => gunshot(33, 0.28, 0.55, 24.0, 120.0, 0.7),
        Sfx::Usp => {
            let mut v = gunshot(44, 0.2, 0.12, 30.0, 150.0, 0.45);
            mix(&mut v, &click(45, 0.3), 0.0);
            v
        }
        Sfx::KnifeSwing => {
            let mut s = Synth { seed: 55 };
            let (mut low, mut lower) = (0.0, 0.0);
            let len = 0.22;
            (0..samples(len))
                .map(|i| {
                    let t = t_of(i);
                    let cutoff = 0.05 + 0.4 * (t / len);
                    low += (s.noise() - low) * cutoff;
                    lower += (low - lower) * 0.05;
                    (low - lower) * (std::f32::consts::PI * t / len).sin() * 0.7
                })
                .collect()
        }
        Sfx::KnifeHit | Sfx::Hit => {
            let mut s = Synth { seed: 66 };
            let mut low = 0.0;
            let freq = if sfx == Sfx::KnifeHit { 150.0 } else { 110.0 };
            (0..samples(0.18))
                .map(|i| {
                    let t = t_of(i);
                    low += (s.noise() - low) * 0.2;
                    ((t * freq * TAU).sin() * (-t * 30.0).exp() + low * (-t * 40.0).exp()) * 0.9
                })
                .collect()
        }
        Sfx::Headshot => {
            // The helmet "dink".
            let mut v: Vec<f32> = (0..samples(0.35))
                .map(|i| {
                    let t = t_of(i);
                    ((t * 1900.0 * TAU).sin() + 0.6 * (t * 2750.0 * TAU).sin()) * (-t * 14.0).exp() * 0.45
                })
                .collect();
            mix(&mut v, &click(67, 0.6), 0.0);
            v
        }
        Sfx::Step(variant) => {
            // Boot on gritty sand/stone: a heel crunch, then a softer toe, no tonal "boop".
            let v = variant as f32;
            let mut s = Synth {
                seed: 77 + variant as u32 * 1013,
            };
            let (mut bright, mut dull) = (0.0, 0.0);
            let toe_at = 0.04 + 0.008 * v;
            (0..samples(0.16))
                .map(|i| {
                    let t = t_of(i);
                    let n = s.noise();
                    bright += (n - bright) * (0.32 + 0.04 * v);
                    dull += (bright - dull) * 0.05;
                    let band = bright - dull; // roughly 150 Hz - 1.5 kHz
                    let heel = (t / 0.002).min(1.0) * (-t * 60.0).exp();
                    let toe = if t > toe_at {
                        ((t - toe_at) / 0.002).min(1.0) * (-(t - toe_at) * 75.0).exp() * 0.55
                    } else {
                        0.0
                    };
                    let grit = n * (-t * 120.0).exp() * 0.25;
                    (band * 1.9 + grit) * (heel + toe) + dull * 2.5 * heel
                })
                .collect()
        }
        Sfx::Reload => {
            let mut v = click(88, 0.5);
            mix(&mut v, &click(89, 0.7), 0.35);
            mix(&mut v, &click(90, 0.6), 0.9);
            v
        }
        Sfx::Empty => click(91, 0.4),
        Sfx::Beep => tone(1500.0, 0.09, 0.35),
        Sfx::Planted => {
            let mut v = tone(1200.0, 0.12, 0.35);
            mix(&mut v, &tone(1600.0, 0.2, 0.35), 0.15);
            v
        }
        Sfx::Defused => {
            let mut v = click(92, 0.6);
            mix(&mut v, &tone(900.0, 0.15, 0.3), 0.1);
            mix(&mut v, &tone(600.0, 0.3, 0.3), 0.25);
            v
        }
        Sfx::Explosion => {
            let mut s = Synth { seed: 99 };
            let mut low = 0.0;
            (0..samples(2.8))
                .map(|i| {
                    let t = t_of(i);
                    low += (s.noise() - low) * 0.06;
                    (low * 4.0 + (t * 38.0 * TAU).sin() * 0.6) * (-t * 1.6).exp() * (t / 0.01).min(1.0)
                })
                .collect()
        }
        Sfx::RoundStart => {
            let mut v = tone(660.0, 0.15, 0.25);
            mix(&mut v, &tone(880.0, 0.25, 0.25), 0.15);
            v
        }
        Sfx::RoundEnd => {
            let mut v = tone(523.0, 0.5, 0.18);
            mix(&mut v, &tone(659.0, 0.5, 0.18), 0.0);
            mix(&mut v, &tone(784.0, 0.6, 0.18), 0.12);
            v
        }
    }
}

/// Encodes mono samples as a 16-bit PCM WAV file.
fn wav(samples: &[f32]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * 2).to_le_bytes()); // byte rate
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).to_le_bytes());
    }
    out
}

const ALL: [Sfx; 20] = [
    Sfx::Ak,
    Sfx::M4,
    Sfx::Glock,
    Sfx::Usp,
    Sfx::KnifeSwing,
    Sfx::KnifeHit,
    Sfx::Hit,
    Sfx::Headshot,
    Sfx::Step(0),
    Sfx::Step(1),
    Sfx::Step(2),
    Sfx::Step(3),
    Sfx::Reload,
    Sfx::Empty,
    Sfx::Beep,
    Sfx::Planted,
    Sfx::Defused,
    Sfx::Explosion,
    Sfx::RoundStart,
    Sfx::RoundEnd,
];

#[cfg(windows)]
mod backend {
    use super::*;
    use bevy::audio::{AudioPlayer, AudioSource, PlaybackSettings, SpatialListener, SpatialScale, Volume};

    use crate::physics::GameRng;
    use crate::player::PlayerCamera;

    #[derive(Resource)]
    pub struct SfxHandles(HashMap<Sfx, Handle<AudioSource>>);

    pub fn setup(mut commands: Commands, mut sources: ResMut<Assets<AudioSource>>) {
        let handles = ALL
            .iter()
            .map(|&sfx| {
                (
                    sfx,
                    sources.add(AudioSource {
                        bytes: wav(&synthesize(sfx)).into(),
                    }),
                )
            })
            .collect();
        commands.insert_resource(SfxHandles(handles));
    }

    /// The player's camera is the "ears" for positional sound.
    pub fn attach_listener(
        mut commands: Commands,
        camera: Query<Entity, (With<PlayerCamera>, Without<SpatialListener>)>,
    ) {
        for camera in &camera {
            commands.entity(camera).insert(SpatialListener::new(0.3));
        }
    }

    pub fn play(
        mut commands: Commands,
        mut requests: MessageReader<PlaySfx>,
        handles: Option<Res<SfxHandles>>,
        mut rng: ResMut<GameRng>,
    ) {
        let Some(handles) = handles else { return };
        for request in requests.read() {
            let Some(handle) = handles.0.get(&request.sfx) else {
                continue;
            };
            let mut settings = PlaybackSettings::DESPAWN
                .with_volume(Volume::Linear(request.volume))
                .with_speed(rng.range(0.94, 1.06));
            match request.pos {
                Some(pos) => {
                    // Rodio fades sounds with 1/distance²; scaling down distances keeps far gunfire audible.
                    settings.spatial = true;
                    settings.spatial_scale = Some(SpatialScale::new(0.07));
                    commands.spawn((AudioPlayer(handle.clone()), settings, Transform::from_translation(pos)));
                }
                None => {
                    commands.spawn((AudioPlayer(handle.clone()), settings));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_sound_synthesizes_to_valid_wav() {
        for sfx in ALL {
            let s = synthesize(sfx);
            assert!(!s.is_empty(), "{sfx:?} is empty");
            assert!(s.iter().all(|v| v.is_finite()), "{sfx:?} has NaN");
            let bytes = wav(&s);
            assert_eq!(&bytes[0..4], b"RIFF");
            assert_eq!(bytes.len(), 44 + s.len() * 2);
        }
    }
}
