//! Procedurally generated 16x16 pixel-art textures (Minecraft style).
//! No image files needed: every texture is painted pixel by pixel here.

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::map::Theme;
use crate::physics::GameRng;

const SIZE: u32 = 16;

/// Materials used by the map and props.
#[derive(Resource)]
pub struct BlockMaterials {
    pub tunnel: Handle<StandardMaterial>,
    pub ceiling: Handle<StandardMaterial>,
    pub crate_wood: Handle<StandardMaterial>,
    pub door: Handle<StandardMaterial>,
    pub car: Handle<StandardMaterial>,
    pub container: Handle<StandardMaterial>,
    pub paint: Handle<StandardMaterial>,
    pub themes: Vec<(Theme, ThemeMaterials)>,
}

/// The textures that give each map its look.
pub struct ThemeMaterials {
    pub floor: Handle<StandardMaterial>,
    pub tiles: Handle<StandardMaterial>,
    pub wall: Handle<StandardMaterial>,
    /// The band at the bottom of walls, and the sides of raised floors.
    pub band: Handle<StandardMaterial>,
}

impl BlockMaterials {
    pub fn theme(&self, theme: Theme) -> &ThemeMaterials {
        &self.themes.iter().find(|t| t.0 == theme).unwrap_or(&self.themes[0]).1
    }
}

pub fn create_block_materials(images: &mut Assets<Image>, materials: &mut Assets<StandardMaterial>) -> BlockMaterials {
    let mut rng = GameRng(12345);
    let mut mat = |img: Image| {
        materials.add(StandardMaterial {
            base_color_texture: Some(images.add(img)),
            perceptual_roughness: 0.95,
            reflectance: 0.1,
            ..default()
        })
    };

    let sand_h = mat(paint(&mut rng, sand));
    let bricks_h = mat(paint(&mut rng, sandstone_bricks));
    let tiles_h = mat(paint(&mut rng, stone_tiles));
    let plaster_h = mat(paint(&mut rng, plaster));
    let themes = vec![
        (
            Theme::Dust,
            ThemeMaterials {
                floor: sand_h,
                tiles: tiles_h.clone(),
                wall: plaster_h,
                band: bricks_h.clone(),
            },
        ),
        (
            Theme::Mirage,
            ThemeMaterials {
                floor: mat(paint(&mut rng, limestone_floor)),
                tiles: tiles_h.clone(),
                wall: mat(paint(&mut rng, limestone)),
                band: bricks_h.clone(),
            },
        ),
        (
            Theme::Inferno,
            ThemeMaterials {
                floor: mat(paint(&mut rng, cobble_floor)),
                tiles: mat(paint(&mut rng, terracotta_tiles)),
                wall: mat(paint(&mut rng, warm_plaster)),
                band: mat(paint(&mut rng, red_bricks)),
            },
        ),
        (
            Theme::Ancient,
            ThemeMaterials {
                floor: mat(paint(&mut rng, mossy_ground)),
                tiles: mat(paint(&mut rng, mossy_tiles)),
                wall: mat(paint(&mut rng, ancient_stone)),
                band: mat(paint(&mut rng, mossy_bricks)),
            },
        ),
    ];
    BlockMaterials {
        themes,
        tunnel: mat(paint(&mut rng, cobble)),
        ceiling: mat(paint(&mut rng, wood_planks)),
        crate_wood: mat(paint(&mut rng, crate_wood)),
        door: mat(paint(&mut rng, blue_door)),
        car: mat(paint(&mut rng, car_paint)),
        container: mat(paint(&mut rng, blue_metal)),
        paint: materials.add(StandardMaterial {
            base_color: Color::srgb(0.75, 0.12, 0.08),
            perceptual_roughness: 1.0,
            ..default()
        }),
    }
}

/// Paints a 16x16 texture with a per-pixel function and wraps it in a pixelated, repeating Image.
fn paint(rng: &mut GameRng, f: fn(u32, u32, &mut GameRng) -> [f32; 3]) -> Image {
    let mut data = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let [r, g, b] = f(x, y, rng);
            data.extend_from_slice(&[
                r.clamp(0.0, 255.0) as u8,
                g.clamp(0.0, 255.0) as u8,
                b.clamp(0.0, 255.0) as u8,
                255,
            ]);
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Nearest,
        min_filter: ImageFilterMode::Nearest,
        mipmap_filter: ImageFilterMode::Nearest,
        ..default()
    });
    image
}

fn shade(c: [f32; 3], amount: f32) -> [f32; 3] {
    [c[0] + amount, c[1] + amount, c[2] + amount]
}

fn hash(a: u32, b: u32) -> u32 {
    let mut h = a.wrapping_mul(374_761_393) ^ b.wrapping_mul(668_265_263);
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    h ^ (h >> 16)
}

fn sand(_x: u32, _y: u32, rng: &mut GameRng) -> [f32; 3] {
    let base = [219.0, 191.0, 140.0];
    let speck = if rng.f32() < 0.08 { -25.0 } else { 0.0 };
    shade(base, rng.range(-10.0, 10.0) + speck)
}

fn sandstone_bricks(x: u32, y: u32, rng: &mut GameRng) -> [f32; 3] {
    let row = y / 4;
    let shifted = x + (row % 2) * 4;
    if y % 4 == 3 || shifted % 8 == 7 {
        return shade([168.0, 134.0, 92.0], rng.range(-6.0, 6.0));
    }
    let brick_tint = (hash(shifted / 8, row) % 20) as f32 - 10.0;
    shade([208.0, 172.0, 120.0], brick_tint + rng.range(-7.0, 7.0))
}

fn stone_tiles(x: u32, y: u32, rng: &mut GameRng) -> [f32; 3] {
    if x.is_multiple_of(8) || y.is_multiple_of(8) {
        return shade([160.0, 146.0, 122.0], rng.range(-5.0, 5.0));
    }
    let tint = (hash(x / 8, y / 8) % 16) as f32 - 8.0;
    shade([205.0, 190.0, 162.0], tint + rng.range(-6.0, 6.0))
}

fn cobble(x: u32, y: u32, rng: &mut GameRng) -> [f32; 3] {
    let stone = hash(x / 4 + (y / 4) % 2, y / 4) % 30;
    let edge = if x.is_multiple_of(4) || y.is_multiple_of(4) {
        -22.0
    } else {
        0.0
    };
    shade([132.0, 120.0, 104.0], stone as f32 - 15.0 + edge + rng.range(-5.0, 5.0))
}

fn wood_planks(x: u32, y: u32, rng: &mut GameRng) -> [f32; 3] {
    if y % 4 == 3 {
        return shade([70.0, 48.0, 28.0], rng.range(-4.0, 4.0));
    }
    let tint = (hash(x / 8 + y / 4, y / 4) % 14) as f32 - 7.0;
    shade([118.0, 84.0, 50.0], tint + rng.range(-6.0, 6.0))
}

fn crate_wood(x: u32, y: u32, rng: &mut GameRng) -> [f32; 3] {
    let frame = x < 2 || y < 2 || x > 13 || y > 13;
    let brace = (x as i32 - y as i32).abs() <= 1;
    if frame || brace {
        return shade([122.0, 84.0, 44.0], rng.range(-6.0, 6.0));
    }
    let seam = if x % 4 == 1 { -14.0 } else { 0.0 };
    shade([178.0, 130.0, 72.0], seam + rng.range(-8.0, 8.0))
}

fn blue_door(x: u32, y: u32, rng: &mut GameRng) -> [f32; 3] {
    if x.is_multiple_of(4) {
        return shade([30.0, 62.0, 110.0], rng.range(-4.0, 4.0));
    }
    if (y == 7 || y == 8) && (x == 13 || x == 14) {
        return [60.0, 60.0, 60.0];
    }
    let worn = if rng.f32() < 0.06 { 40.0 } else { 0.0 };
    shade([52.0, 98.0, 160.0], worn + rng.range(-8.0, 8.0))
}

fn car_paint(x: u32, y: u32, rng: &mut GameRng) -> [f32; 3] {
    if y < 6 && (2..14).contains(&x) {
        return shade([70.0, 95.0, 110.0], rng.range(-5.0, 5.0)); // window
    }
    shade([150.0, 40.0, 35.0], rng.range(-10.0, 10.0))
}

/// Dust2's cream plaster walls, with a few cracks and stains.
fn plaster(x: u32, y: u32, rng: &mut GameRng) -> [f32; 3] {
    let stain = if hash(x / 4, y / 4).is_multiple_of(7) {
        -10.0
    } else {
        0.0
    };
    let crack = if hash(x, y).is_multiple_of(53) { -35.0 } else { 0.0 };
    shade([226.0, 205.0, 160.0], stain + crack + rng.range(-6.0, 6.0))
}

/// Corrugated blue metal (the "blue" container on long).
fn blue_metal(x: u32, _y: u32, rng: &mut GameRng) -> [f32; 3] {
    let ridge = if x.is_multiple_of(4) {
        25.0
    } else if x % 4 == 2 {
        -15.0
    } else {
        0.0
    };
    let rust = if rng.f32() < 0.04 {
        [-40.0, -10.0, 30.0]
    } else {
        [0.0; 3]
    };
    [
        40.0 + ridge * 0.5 - rust[0],
        85.0 + ridge - rust[1],
        140.0 + ridge - rust[2],
    ]
}

/// Mirage: pale limestone blocks.
fn limestone(x: u32, y: u32, rng: &mut GameRng) -> [f32; 3] {
    let row = y / 8;
    let shifted = x + (row % 2) * 6;
    if y % 8 == 7 || shifted % 12 == 11 {
        return shade([190.0, 178.0, 155.0], rng.range(-5.0, 5.0));
    }
    let tint = (hash(shifted / 12, row) % 14) as f32 - 7.0;
    shade([232.0, 222.0, 198.0], tint + rng.range(-5.0, 5.0))
}

fn limestone_floor(x: u32, y: u32, rng: &mut GameRng) -> [f32; 3] {
    if x.is_multiple_of(8) || y.is_multiple_of(8) {
        return shade([170.0, 158.0, 135.0], rng.range(-4.0, 4.0));
    }
    let tint = (hash(x / 8, y / 8) % 16) as f32 - 8.0;
    shade([214.0, 200.0, 170.0], tint + rng.range(-6.0, 6.0))
}

/// Inferno: warm orange/peach plaster, red bricks, cobblestone streets.
fn warm_plaster(x: u32, y: u32, rng: &mut GameRng) -> [f32; 3] {
    let stain = if hash(x / 4, y / 4).is_multiple_of(6) {
        -14.0
    } else {
        0.0
    };
    let v = stain + rng.range(-7.0, 7.0);
    [228.0 + v, 170.0 + v, 120.0 + v]
}

fn red_bricks(x: u32, y: u32, rng: &mut GameRng) -> [f32; 3] {
    let row = y / 4;
    let shifted = x + (row % 2) * 4;
    if y % 4 == 3 || shifted % 8 == 7 {
        return shade([150.0, 135.0, 120.0], rng.range(-5.0, 5.0));
    }
    let tint = (hash(shifted / 8, row) % 24) as f32 - 12.0;
    let v = tint + rng.range(-6.0, 6.0);
    [160.0 + v, 72.0 + v * 0.6, 50.0 + v * 0.5]
}

fn cobble_floor(x: u32, y: u32, rng: &mut GameRng) -> [f32; 3] {
    let stone = hash(x / 4 + (y / 4) % 2, y / 4) % 26;
    let edge = if x.is_multiple_of(4) || y.is_multiple_of(4) {
        -28.0
    } else {
        0.0
    };
    shade([150.0, 140.0, 128.0], stone as f32 - 13.0 + edge + rng.range(-5.0, 5.0))
}

fn terracotta_tiles(x: u32, y: u32, rng: &mut GameRng) -> [f32; 3] {
    if x.is_multiple_of(8) || y.is_multiple_of(8) {
        return shade([120.0, 80.0, 60.0], rng.range(-4.0, 4.0));
    }
    let v = (hash(x / 8, y / 8) % 16) as f32 - 8.0 + rng.range(-6.0, 6.0);
    [190.0 + v, 110.0 + v * 0.7, 75.0 + v * 0.5]
}

/// Ancient: grey-green temple stone covered in moss.
fn ancient_stone(x: u32, y: u32, rng: &mut GameRng) -> [f32; 3] {
    let row = y / 8;
    let shifted = x + (row % 2) * 8;
    if y % 8 == 7 || shifted % 16 == 15 {
        return shade([70.0, 74.0, 62.0], rng.range(-4.0, 4.0));
    }
    let moss = hash(x / 3, y / 3).is_multiple_of(5);
    let v = (hash(shifted / 16, row) % 16) as f32 - 8.0 + rng.range(-6.0, 6.0);
    if moss {
        [80.0 + v, 110.0 + v, 55.0 + v]
    } else {
        [140.0 + v, 142.0 + v, 125.0 + v]
    }
}

fn mossy_bricks(x: u32, y: u32, rng: &mut GameRng) -> [f32; 3] {
    let [r, g, b] = ancient_stone(x, y, rng);
    [r * 0.85, g * 0.95, b * 0.8]
}

fn mossy_ground(x: u32, y: u32, rng: &mut GameRng) -> [f32; 3] {
    let grass = hash(x / 2, y / 2).is_multiple_of(3);
    let v = rng.range(-10.0, 10.0);
    if grass {
        [75.0 + v, 105.0 + v, 45.0 + v]
    } else {
        [120.0 + v, 110.0 + v, 85.0 + v]
    }
}

fn mossy_tiles(x: u32, y: u32, rng: &mut GameRng) -> [f32; 3] {
    if x.is_multiple_of(8) || y.is_multiple_of(8) {
        return shade([75.0, 90.0, 60.0], rng.range(-4.0, 4.0));
    }
    let v = (hash(x / 8, y / 8) % 16) as f32 - 8.0 + rng.range(-6.0, 6.0);
    [150.0 + v, 148.0 + v, 128.0 + v]
}
