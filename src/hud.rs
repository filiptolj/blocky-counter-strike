//! On-screen interface: crosshair, health/armor/ammo, score and timer, round banners,
//! bomb status and progress bar, kill feed, scoreboard, teammate name tags and start screen.

use bevy::prelude::*;

use crate::bots::Difficulty;
use crate::combat::{Combatant, Health, Team};
use crate::game::{Bomb, BombState, DEFUSE_TIME, Match, PLANT_TIME, Phase};
use crate::map::MapDef;
use crate::player::{Player, PlayerCamera, Playing, Settings, Spectating};
use crate::weapons::{GunState, Inventory};

#[derive(Resource, Default)]
pub struct KillFeed {
    entries: Vec<(String, f32)>,
}

impl KillFeed {
    pub fn push(&mut self, text: String) {
        self.entries.push((text, 7.0));
        if self.entries.len() > 6 {
            self.entries.remove(0);
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

/// Briefly tints the crosshair when the player's bullet hits someone.
#[derive(Resource, Default)]
pub struct HitMarker {
    pub timer: f32,
    pub kill: bool,
}

#[derive(Component)]
pub struct CrosshairPart;
#[derive(Component)]
pub struct StartScreen;
#[derive(Component)]
pub struct DamageFlash;
#[derive(Component)]
pub struct ProgressBar;
#[derive(Component)]
pub struct ProgressFill;
#[derive(Component)]
pub struct Scoreboard;
#[derive(Component)]
pub struct NameTag;

/// Every text element on the HUD that gets rewritten each frame.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub enum HudText {
    Health,
    Ammo,
    Weapon,
    Feed,
    Info,
    Score,
    Banner,
    Hint,
    Scoreboard,
    MapName,
}

const CROSSHAIR: Color = Color::srgb(0.3, 1.0, 0.3);
const HUD_TEXT: Color = Color::srgb(0.95, 0.9, 0.75);

fn text(value: &str, size: f32, color: Color) -> (Text, TextFont, TextColor) {
    (Text::new(value), TextFont::from_font_size(size), TextColor(color))
}

fn absolute(left: Val, top: Val, right: Val, bottom: Val) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left,
        top,
        right,
        bottom,
        ..default()
    }
}

/// A full-width row used to center things horizontally.
fn centered_row(top: Val, bottom: Val) -> Node {
    Node {
        width: Val::Percent(100.0),
        justify_content: JustifyContent::Center,
        ..absolute(Val::Px(0.0), top, Val::Auto, bottom)
    }
}

pub fn setup_hud(mut commands: Commands) {
    use Val::{Auto, Percent, Px};

    // Red screen flash when taking damage (drawn first, so it's below everything else).
    commands.spawn((
        DamageFlash,
        Node {
            width: Percent(100.0),
            height: Percent(100.0),
            ..absolute(Px(0.0), Px(0.0), Auto, Auto)
        },
        BackgroundColor(Color::srgba(0.8, 0.0, 0.0, 0.0)),
    ));

    // Crosshair: four lines around the screen center, like the CS classic style.
    let center = commands.spawn(absolute(Percent(50.0), Percent(50.0), Auto, Auto)).id();
    let (gap, len, thick) = (4.0, 8.0, 2.0);
    for (left, top, w, h) in [
        (-gap - len, -thick / 2.0, len, thick),
        (gap, -thick / 2.0, len, thick),
        (-thick / 2.0, -gap - len, thick, len),
        (-thick / 2.0, gap, thick, len),
    ] {
        commands.spawn((
            CrosshairPart,
            Node {
                width: Px(w),
                height: Px(h),
                ..absolute(Px(left), Px(top), Auto, Auto)
            },
            BackgroundColor(CROSSHAIR),
            ChildOf(center),
        ));
    }

    let panel = BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.4));
    let padded = |node: Node| Node {
        padding: UiRect::axes(Px(14.0), Px(6.0)),
        ..node
    };

    commands.spawn((
        HudText::Health,
        text("", 30.0, HUD_TEXT),
        padded(absolute(Px(20.0), Auto, Auto, Px(18.0))),
        panel,
    ));
    commands.spawn((
        HudText::Weapon,
        text("", 20.0, HUD_TEXT),
        absolute(Auto, Auto, Px(34.0), Px(72.0)),
    ));
    commands.spawn((
        HudText::Ammo,
        text("", 30.0, HUD_TEXT),
        padded(absolute(Auto, Auto, Px(20.0), Px(18.0))),
        panel,
    ));
    commands.spawn((
        HudText::Feed,
        text("", 17.0, Color::WHITE),
        absolute(Auto, Px(64.0), Px(20.0), Auto),
    ));
    commands.spawn((
        HudText::Info,
        text("", 13.0, Color::srgba(1.0, 1.0, 1.0, 0.6)),
        absolute(Px(22.0), Auto, Auto, Px(66.0)),
    ));

    commands.spawn((
        centered_row(Px(10.0), Auto),
        children![(
            HudText::Score,
            text("", 26.0, Color::WHITE),
            Node {
                padding: UiRect::axes(Px(18.0), Px(4.0)),
                ..default()
            },
            panel,
        )],
    ));
    commands.spawn((
        centered_row(Percent(22.0), Auto),
        children![(
            HudText::Banner,
            text("", 38.0, Color::WHITE),
            TextLayout::justify(Justify::Center),
        )],
    ));
    commands.spawn((
        centered_row(Auto, Px(110.0)),
        children![(
            HudText::Hint,
            text("", 20.0, Color::srgb(1.0, 0.85, 0.4)),
            TextLayout::justify(Justify::Center),
        )],
    ));

    // Plant / defuse progress bar.
    commands.spawn((
        centered_row(Auto, Px(80.0)),
        children![(
            ProgressBar,
            Node {
                width: Px(300.0),
                height: Px(14.0),
                border: UiRect::all(Px(2.0)),
                ..default()
            },
            BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.8)),
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.5)),
            Visibility::Hidden,
            children![(
                ProgressFill,
                Node {
                    width: Percent(0.0),
                    height: Percent(100.0),
                    ..default()
                },
                BackgroundColor(Color::srgb(1.0, 0.75, 0.2))
            )],
        )],
    ));

    // Teammate name tags (positioned every frame).
    for _ in 0..10 {
        commands.spawn((
            NameTag,
            text("", 14.0, Color::srgb(0.5, 1.0, 0.5)),
            absolute(Px(0.0), Px(0.0), Auto, Auto),
            Visibility::Hidden,
        ));
    }

    commands.spawn((
        Scoreboard,
        centered_row(Percent(18.0), Auto),
        Visibility::Hidden,
        children![(
            HudText::Scoreboard,
            text("", 18.0, Color::WHITE),
            Node {
                padding: UiRect::all(Px(18.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.75)),
        )],
    ));

    commands.spawn((
        StartScreen,
        Node {
            width: Percent(100.0),
            height: Percent(100.0),
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            row_gap: Px(14.0),
            ..absolute(Px(0.0), Px(0.0), Auto, Auto)
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.5)),
        children![
            text("CS BLOCKY", 64.0, Color::srgb(1.0, 0.8, 0.3)),
            (HudText::MapName, text("", 26.0, HUD_TEXT)),
            text("CLICK TO PLAY", 32.0, Color::WHITE),
            (
                text(
                    "WASD move   SPACE jump   CTRL crouch   SHIFT walk\n\
                     LMB shoot   R reload   1 / 2 / 3 / Q switch weapon (3 = knife, RMB stab)\n\
                     E (hold) plant the bomb on a site / defuse it\n\
                     TAB scoreboard   M switch team   F2 bot difficulty   F3 next map\n\
                     When dead: E to take control of the teammate you spectate\n\
                     [ and ] mouse sensitivity   ESC release mouse",
                    18.0,
                    HUD_TEXT
                ),
                TextLayout::justify(Justify::Center),
            ),
        ],
    ));
}

fn clock(seconds: f32) -> String {
    let s = seconds.max(0.0).ceil() as u32;
    format!("{}:{:02}", s / 60, s % 60)
}

/// Crosshair, damage flash, plant/defuse bar and which panels are visible.
pub fn update_hud_effects(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    playing: Res<Playing>,
    bomb: Res<Bomb>,
    spectating: Res<Spectating>,
    mut hit: ResMut<HitMarker>,
    player: Single<(Entity, &Health), With<Player>>,
    mut crosshair: Query<&mut BackgroundColor, (With<CrosshairPart>, Without<DamageFlash>, Without<ProgressFill>)>,
    mut flash: Single<&mut BackgroundColor, (With<DamageFlash>, Without<ProgressFill>)>,
    mut fill: Single<&mut Node, With<ProgressFill>>,
    mut visibility: ParamSet<(
        Single<&mut Visibility, With<StartScreen>>,
        Single<&mut Visibility, With<ProgressBar>>,
        Single<&mut Visibility, With<Scoreboard>>,
    )>,
) {
    let (me, health) = player.into_inner();
    let alive = health.alive();
    **visibility.p0() = if playing.0 {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    };
    **visibility.p2() = if keys.pressed(KeyCode::Tab) {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };

    // Crosshair (hidden while dead) and hit marker colors.
    hit.timer -= time.delta_secs();
    let color = if !alive {
        Color::NONE
    } else if hit.timer > 0.0 {
        if hit.kill {
            Color::srgb(1.0, 0.6, 0.1)
        } else {
            Color::srgb(1.0, 0.2, 0.2)
        }
    } else {
        hit.kill = false;
        CROSSHAIR
    };
    for mut part in &mut crosshair {
        part.0 = color;
    }
    let pain = if alive { (0.4 - health.since_hit).max(0.0) } else { 0.0 };
    flash.0 = Color::srgba(0.8, 0.0, 0.0, pain * 0.6);

    // Plant / defuse progress, for the player or whoever they are spectating.
    let progress = match bomb.state {
        BombState::Carried(_) => bomb.progress / PLANT_TIME,
        BombState::Planted { .. } => bomb.progress / DEFUSE_TIME,
        _ => 0.0,
    };
    let show_bar = bomb.actor.is_some() && (bomb.actor == Some(me) || bomb.actor == spectating.0);
    **visibility.p1() = if show_bar {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    fill.width = Val::Percent(progress.clamp(0.0, 1.0) * 100.0);
}

/// Green names above living teammates.
pub fn update_name_tags(
    player: Single<(Entity, &Combatant), With<Player>>,
    people: Query<(Entity, &Combatant, &Health, &Transform)>,
    camera: Single<(&Camera, &GlobalTransform), With<PlayerCamera>>,
    mut tags: Query<(&mut Node, &mut Text, &mut Visibility), With<NameTag>>,
) {
    let (me, combatant) = player.into_inner();
    let (cam, cam_tf) = camera.into_inner();
    let mut mates = people
        .iter()
        .filter(|p| p.0 != me && p.1.team == combatant.team && p.2.alive());
    for (mut node, mut text, mut vis) in &mut tags {
        *vis = Visibility::Hidden;
        let Some((_, mate, _, mate_tf)) = mates.next() else {
            continue;
        };
        let head = mate_tf.translation + Vec3::Y * 2.1;
        if cam_tf.forward().dot(head - cam_tf.translation()) <= 0.0 {
            continue;
        }
        let Ok(screen) = cam.world_to_viewport(cam_tf, head) else {
            continue;
        };
        text.0 = mate.name.trim_start_matches("BOT ").to_string();
        node.left = Val::Px(screen.x - 30.0);
        node.top = Val::Px(screen.y - 10.0);
        *vis = Visibility::Inherited;
    }
}

/// All the text on the HUD.
pub fn update_hud(
    time: Res<Time>,
    settings: Res<Settings>,
    difficulty: Res<Difficulty>,
    map: Res<MapDef>,
    game: Res<Match>,
    bomb: Res<Bomb>,
    spectating: Res<Spectating>,
    mut feed: ResMut<KillFeed>,
    player: Single<(Entity, &Combatant, &Health, &Inventory, &GunState, &Transform), With<Player>>,
    people: Query<(Entity, &Combatant, &Health, &Transform)>,
    mut texts: Query<(&HudText, &mut Text, &mut TextColor)>,
    mut fps: Local<f32>,
) {
    let dt = time.delta_secs();
    let (me, combatant, health, inv, gun, transform) = player.into_inner();
    let alive = health.alive();

    for entry in &mut feed.entries {
        entry.1 -= dt;
    }
    feed.entries.retain(|e| e.1 > 0.0);
    if dt > 0.0 {
        *fps = *fps * 0.95 + (1.0 / dt) * 0.05;
    }

    let count = |team: Team| people.iter().filter(|p| p.1.team == team && p.2.alive()).count();
    let slot = inv.current();

    for (kind, mut text, mut text_color) in &mut texts {
        text.0 = match kind {
            HudText::Health => {
                if alive {
                    format!("+ {}   [] {}", health.hp.ceil() as i32, health.armor.ceil() as i32)
                } else {
                    "DEAD".to_string()
                }
            }
            HudText::Ammo => match () {
                _ if !alive => String::new(),
                _ if slot.def.melee => String::new(),
                _ if gun.reload.is_some() => "RELOADING".to_string(),
                _ => format!("{} / {}", slot.ammo, slot.reserve),
            },
            HudText::Weapon => {
                if alive {
                    slot.def.name.to_string()
                } else {
                    String::new()
                }
            }
            HudText::Feed => feed.entries.iter().map(|e| e.0.as_str()).collect::<Vec<_>>().join("\n"),
            HudText::Info => {
                let p = transform.translation;
                format!(
                    "{:.0} fps   pos {:.1} {:.1} {:.1}   sens {:.2}   bots: {:?} (F2)",
                    *fps,
                    p.x,
                    p.y,
                    p.z,
                    settings.sensitivity * 1000.0,
                    *difficulty
                )
            }
            HudText::Score => {
                let middle = match (game.phase, bomb.state) {
                    (_, BombState::Planted { site, .. }) if game.phase != Phase::Over => {
                        text_color.0 = Color::srgb(1.0, 0.3, 0.2);
                        format!("BOMB {site}  {}", clock(bomb.timer))
                    }
                    (Phase::Freeze, _) => {
                        text_color.0 = Color::WHITE;
                        format!("{}", game.timer.ceil() as i32)
                    }
                    _ => {
                        text_color.0 = Color::WHITE;
                        clock(if game.phase == Phase::Live { game.timer } else { 0.0 })
                    }
                };
                format!(
                    "T {}  ({} alive)    {}    ({} alive)  {} CT",
                    game.score_t,
                    count(Team::T),
                    middle,
                    count(Team::CT),
                    game.score_ct
                )
            }
            HudText::Banner => game.banner.as_ref().map(|b| b.0.clone()).unwrap_or_default(),
            HudText::Hint => {
                let mut hint = String::new();
                if alive && bomb.state == BombState::Carried(me) {
                    hint = "You have the bomb! Go to site A or B and hold E to plant".to_string();
                } else if alive
                    && combatant.team == Team::CT
                    && bomb
                        .planted_pos()
                        .is_some_and(|p| p.distance(transform.translation) < 1.6)
                {
                    hint = "Hold E to defuse".to_string();
                } else if !alive {
                    let watched = spectating.0.and_then(|e| people.get(e).ok());
                    hint = match watched {
                        Some((_, c, _, _)) if c.team == combatant.team && c.name.starts_with("BOT") => {
                            format!("Spectating {}  (click to switch, E to take control)", c.name)
                        }
                        Some((_, c, _, _)) => format!("Spectating {}  (click to switch)", c.name),
                        None => "You are dead".to_string(),
                    };
                }
                if game.player_team_next != combatant.team {
                    hint.push_str(&format!(
                        "\nYou will join the {} next round",
                        game.player_team_next.name()
                    ));
                }
                hint
            }
            HudText::Scoreboard => scoreboard(&people, &game),
            HudText::MapName => format!("{}  -  5 vs 5   (F3: next map)", map.name),
        };
    }
}

fn scoreboard(people: &Query<(Entity, &Combatant, &Health, &Transform)>, game: &Match) -> String {
    let mut s = format!("ROUND {}\n\n", game.round);
    for team in [Team::CT, Team::T] {
        let score = if team == Team::T { game.score_t } else { game.score_ct };
        s.push_str(&format!("{:<22} {:>3}\n", team.name().to_uppercase(), score));
        s.push_str(&format!("  {:<20} {:>5} {:>6}\n", "NAME", "KILLS", "DEATHS"));
        let mut members: Vec<_> = people.iter().filter(|p| p.1.team == team).collect();
        members.sort_by_key(|m| std::cmp::Reverse(m.1.kills));
        for (_, c, h, _) in members {
            let dead = if h.alive() { "" } else { " (dead)" };
            s.push_str(&format!("  {:<20} {:>5} {:>6}{dead}\n", c.name, c.kills, c.deaths));
        }
        s.push('\n');
    }
    s
}
