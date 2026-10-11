# CS Blocky

A Counter-Strike style 5v5 shooter with Minecraft/Roblox style blocky graphics, written in Rust with
[Bevy](https://bevy.org).

Play as a Terrorist or Counter-Terrorist with 4 bot teammates against 5 bots: plant or defuse the
bomb, win rounds, first to 13.

## Features

- **4 maps:** blocky recreations of **Dust II, Mirage, Inferno and Ancient**, generated from the
  official CS2 radar images, so the layouts and elevations match the real maps
- **5v5 rounds:** freeze time, round timer, bomb plant/defuse, score, team switching
- **CS-style gunplay:** AK-47, M4A4, Glock-18, USP-S and knife, with spray patterns, moving
  inaccuracy, armor, headshots and backstabs
- **CS-style movement:** acceleration, air strafing, crouch-jumping, walking
- **Bots** that take different routes, hold sites, spot you, aim, spray, plant and defuse
  (3 difficulty levels)
- Take control of a teammate bot after you die
- Radar, kill feed, scoreboard and teammate name tags
- Everything is generated in code: pixel-art textures, character models and sound effects
  (no image or audio files)

## Play

**Windows:** download `cs-blocky.exe` from the [Releases](../../releases) page and double-click it.
It's a single file with nothing to install.

Start on a specific map from a terminal with `cs-blocky.exe mirage` (or `dust2`, `inferno`,
`ancient`), or press **F3** in game.

## Controls

| Key | Action |
|---|---|
| Click | Capture the mouse and start playing |
| Mouse / Left click | Aim / shoot |
| WASD | Move |
| Space | Jump |
| Ctrl or C | Crouch (crouch in the air to jump higher) |
| Shift | Walk (silent footsteps) |
| R | Reload |
| 1 / 2 / 3 / Q | Rifle / pistol / knife / last weapon |
| Left / right click with knife | Slash / stab (hit from behind for a backstab) |
| E (hold) | Plant the bomb on a site (T) / defuse it (CT) |
| E while dead | Take control of the teammate bot you are spectating |
| Tab | Scoreboard |
| M | Switch team (from the next round) |
| F2 | Bot difficulty: Easy / Normal / Hard (from the next round) |
| F3 | Next map (restarts the match) |
| [ and ] | Mouse sensitivity down / up |
| Esc | Release the mouse |

## Building from source

You need [Rust](https://rustup.rs) (stable). The first build compiles Bevy and takes a while;
later builds are fast.

```sh
cargo run --release
```

- **Windows:** Rust also needs the Visual Studio C++ build tools (the Rust installer offers them).
- **Linux:** sound needs ALSA, so Linux builds don't have sound yet. Under **WSL**, Bevy can't use
  the GPU and the game runs at a few FPS, so build on Windows itself.

The release build on Windows opens no console window. Run `cargo run` (debug build) to see log output.

## How the maps are made

Valve's radar images color the floor by height. `tools/radar_to_map.py` turns each radar into a
1 m grid of walls, boxes and floor heights (in 0.25 m steps), calibrated with the real height of the
T and CT spawns. It also finds the bombsites from their outlines on the radar, and turns stairs into
walkable steps. The results are stored in `assets/maps/`.

Dust II also has hand-placed tunnel roofs, doors and bot routes (`src/dust2.rs`). The other maps
get automatic bot routes and positions (`src/maps.rs`).

To regenerate the maps, download the radar images (`images/radars/<map>_radar_psd.png`) from
[MurkyYT/cs2-map-icons](https://github.com/MurkyYT/cs2-map-icons) into a folder, then run:

```sh
pip install pillow
python3 tools/radar_to_map.py path/to/radars            # all maps
python3 tools/radar_to_map.py path/to/radars de_mirage  # one map
```

The per-map settings at the top of the script hold the height colors and the real spawn positions.

## Project layout

| Path | What it does |
|---|---|
| `src/main.rs` | App setup: window, sun, fog, cameras, system order |
| `src/maps.rs` | Map list, map switching, automatic bot strategy |
| `src/dust2.rs` | Dust II's hand-tuned extras |
| `src/map.rs` | Map grid → block meshes and collision boxes |
| `src/nav.rs` | A* pathfinding for bots |
| `src/game.rs` | Rounds, score, win conditions, the bomb |
| `src/bots.rs` | Bot AI |
| `src/player.rs` | Mouse look, keyboard input, camera, spectating |
| `src/body.rs` | Movement physics shared by player and bots |
| `src/combat.rs` | Teams, health/armor, hitboxes, damage, character models |
| `src/weapons.rs` | Guns, knife, view models, bullet holes, tracers |
| `src/hud.rs` | Crosshair, health/armor/ammo, score, kill feed, scoreboard |
| `src/radar.rs` | Minimap |
| `src/sound.rs` | Synthesized sound effects and positional audio |
| `src/textures.rs` | 16×16 pixel-art textures generated in code |
| `assets/maps/` | Map data generated from the radars |
| `tools/radar_to_map.py` | Radar image → map data converter |

### Development helpers

- `cargo test` checks that every map is fully reachable for bots, among other things.
- `cargo test print_reachability -- --nocapture` prints which parts of Dust II bots can reach.
- Environment variables for testing:
  - `CS_TEAM=CT` starts you on the Counter-Terrorist side.
  - `CS_POSE="x,y,z,yaw,pitch"` puts you at that position every round.
  - `CS_SCREENSHOT=shot.png CS_FRAMES=600` saves a screenshot after 600 frames and quits.

## License

The code is released under the [MIT License](LICENSE).

## Credits

- Radar images and overview data used to generate the maps:
  [MurkyYT/cs2-map-icons](https://github.com/MurkyYT/cs2-map-icons).
- Spawn positions: [NadesDB](https://nadesdb.com).

This is a fan project. It is not affiliated with or endorsed by Valve. Counter-Strike, Dust II,
Mirage, Inferno and Ancient are trademarks of Valve Corporation. No Valve game files are included;
the maps are blocky grids derived from the public radar overviews.
