// Block World: a tiny blocky voxel game for Android (built with Bevy).
// Controls: drag on the left side to walk, drag on the right side to look,
// quick tap to place a block, press and hold to break one.

use bevy::input::touch::{TouchInput, TouchPhase};
use bevy::prelude::*;
use std::{collections::HashMap, time::Instant};

const EYE_HEIGHT: f32 = 1.6;
const FEET_Y: f32 = 0.5; // standing on top of the y = 0 ground layer
const WALK_SPEED: f32 = 4.0;
const PLAYER_RADIUS: f32 = 0.3;
const REACH: f32 = 6.0;
const LOOK_SENSITIVITY: f32 = 0.005;
const JOYSTICK_RADIUS: f32 = 80.0; // screen pixels

/// Every block in the world, keyed by grid cell. Blocks are 1x1x1 and centered on whole numbers.
#[derive(Resource, Default)]
struct BlockMap(HashMap<IVec3, Entity>);

#[derive(Resource)]
struct BlockAssets {
    mesh: Handle<Mesh>,
    grass: Handle<StandardMaterial>,
    stone: Handle<StandardMaterial>,
}

#[derive(Resource)]
struct Player {
    pos: Vec3, // feet position
    yaw: f32,
    pitch: f32,
}

#[derive(Resource, Default)]
struct Controls {
    move_id: Option<u64>,
    move_origin: Vec2,
    move_dir: Vec2,
    look_id: Option<u64>,
    look_last: Vec2,
    look_moved: f32,
    look_started: Option<Instant>,
}

#[bevy_main]
pub fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .insert_resource(Player {
            pos: Vec3::new(0.0, FEET_Y, 0.0),
            yaw: 0.0,
            pitch: 0.0,
        })
        .init_resource::<BlockMap>()
        .init_resource::<Controls>()
        .add_systems(Startup, setup)
        .add_systems(Update, (touch_input, walk, sync_camera).chain())
        .run();
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut map: ResMut<BlockMap>,
) {
    let mesh = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let grass = materials.add(StandardMaterial {
        base_color: Color::srgb(0.35, 0.7, 0.3),
        ..default()
    });
    let stone = materials.add(StandardMaterial {
        base_color: Color::srgb(0.5, 0.5, 0.55),
        ..default()
    });

    // Flat 24 x 24 grass floor.
    for x in -12..12 {
        for z in -12..12 {
            let entity = commands
                .spawn((
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(grass.clone()),
                    Transform::from_xyz(x as f32, 0.0, z as f32),
                ))
                .id();
            map.0.insert(IVec3::new(x, 0, z), entity);
        }
    }

    commands.insert_resource(BlockAssets { mesh, grass, stone });
    commands.spawn((Camera3d::default(), Transform::default()));
    commands.spawn((
        DirectionalLight {
            illuminance: 8000.0,
            shadows_enabled: false,
            ..default()
        },
        Transform::from_xyz(4.0, 10.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        Text::new("Left: walk | Right: look | Tap: place | Hold: break"),
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(12.0),
            left: Val::Px(12.0),
            ..default()
        },
    ));
    // Crosshair in the middle of the screen.
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Percent(50.0),
            top: Val::Percent(50.0),
            width: Val::Px(6.0),
            height: Val::Px(6.0),
            margin: UiRect {
                left: Val::Px(-3.0),
                top: Val::Px(-3.0),
                ..default()
            },
            ..default()
        },
        BackgroundColor(Color::WHITE),
    ));
}

fn touch_input(
    mut events: EventReader<TouchInput>,
    windows: Query<&Window>,
    mut controls: ResMut<Controls>,
    mut player: ResMut<Player>,
    mut map: ResMut<BlockMap>,
    assets: Res<BlockAssets>,
    mut commands: Commands,
) {
    // Left 40% of the screen is the joystick; the rest is for looking and tapping.
    let split = windows.get_single().map(|w| w.width() * 0.4).unwrap_or(200.0);

    for ev in events.read() {
        let pos = ev.position;
        match ev.phase {
            TouchPhase::Started => {
                if pos.x < split {
                    if controls.move_id.is_none() {
                        controls.move_id = Some(ev.id);
                        controls.move_origin = pos;
                    }
                } else if controls.look_id.is_none() {
                    controls.look_id = Some(ev.id);
                    controls.look_last = pos;
                    controls.look_moved = 0.0;
                    controls.look_started = Some(Instant::now());
                }
            }
            TouchPhase::Moved => {
                if controls.move_id == Some(ev.id) {
                    let d = (pos - controls.move_origin) / JOYSTICK_RADIUS;
                    controls.move_dir = Vec2::new(d.x, -d.y).clamp_length_max(1.0);
                } else if controls.look_id == Some(ev.id) {
                    let delta = pos - controls.look_last;
                    controls.look_last = pos;
                    controls.look_moved += delta.length();
                    player.yaw -= delta.x * LOOK_SENSITIVITY;
                    player.pitch = (player.pitch - delta.y * LOOK_SENSITIVITY).clamp(-1.5, 1.5);
                }
            }
            TouchPhase::Ended | TouchPhase::Canceled => {
                if controls.move_id == Some(ev.id) {
                    controls.move_id = None;
                    controls.move_dir = Vec2::ZERO;
                } else if controls.look_id == Some(ev.id) {
                    controls.look_id = None;
                    // Only a short, nearly still touch counts as a tap (place) or hold (break).
                    if controls.look_moved < 12.0 {
                        let held = controls
                            .look_started
                            .map_or(0.0, |t| t.elapsed().as_secs_f32());
                        let eye = player.pos + Vec3::Y * EYE_HEIGHT;
                        if let Some((hit, empty)) = raycast(&map, eye, look_dir(&player)) {
                            if held < 0.3 {
                                if !map.0.contains_key(&empty) && !in_player(empty, player.pos) {
                                    let e = commands
                                        .spawn((
                                            Mesh3d(assets.mesh.clone()),
                                            MeshMaterial3d(assets.stone.clone()),
                                            Transform::from_translation(empty.as_vec3()),
                                        ))
                                        .id();
                                    map.0.insert(empty, e);
                                }
                            } else if let Some(e) = map.0.remove(&hit) {
                                commands.entity(e).despawn();
                            }
                        }
                    }
                }
            }
        }
    }
}

fn walk(time: Res<Time>, controls: Res<Controls>, map: Res<BlockMap>, mut player: ResMut<Player>) {
    let (sin, cos) = player.yaw.sin_cos();
    let forward = Vec3::new(-sin, 0.0, -cos);
    let right = Vec3::new(cos, 0.0, -sin);
    let step = (right * controls.move_dir.x + forward * controls.move_dir.y)
        * WALK_SPEED
        * time.delta_secs();

    // Move one axis at a time so the player slides along walls.
    let next_x = player.pos + Vec3::new(step.x, 0.0, 0.0);
    if !blocked(&map, next_x) {
        player.pos = next_x;
    }
    let next_z = player.pos + Vec3::new(0.0, 0.0, step.z);
    if !blocked(&map, next_z) {
        player.pos = next_z;
    }
}

fn sync_camera(player: Res<Player>, mut cameras: Query<&mut Transform, With<Camera3d>>) {
    for mut transform in &mut cameras {
        transform.translation = player.pos + Vec3::Y * EYE_HEIGHT;
        transform.rotation = Quat::from_rotation_y(player.yaw) * Quat::from_rotation_x(player.pitch);
    }
}

/// Grid cell containing a point (cells are centered on whole numbers).
fn cell(p: Vec3) -> IVec3 {
    (p + 0.5).floor().as_ivec3()
}

/// True if a player standing with feet at `feet` would overlap a block.
fn blocked(map: &BlockMap, feet: Vec3) -> bool {
    [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)]
        .iter()
        .any(|&(sx, sz)| {
            let corner = Vec3::new(sx * PLAYER_RADIUS, 0.0, sz * PLAYER_RADIUS);
            let low = feet + corner + Vec3::Y * 0.1;
            let high = feet + corner + Vec3::Y * (EYE_HEIGHT - 0.1);
            map.0.contains_key(&cell(low)) || map.0.contains_key(&cell(high))
        })
}

/// Direction the camera is looking in.
fn look_dir(player: &Player) -> Vec3 {
    Quat::from_rotation_y(player.yaw) * Quat::from_rotation_x(player.pitch) * Vec3::NEG_Z
}

/// Walks from `origin` along `dir`. Returns the first block hit and the empty cell just before it.
fn raycast(map: &BlockMap, origin: Vec3, dir: Vec3) -> Option<(IVec3, IVec3)> {
    let mut last_empty = cell(origin);
    let mut t = 0.0;
    while t < REACH {
        let c = cell(origin + dir * t);
        if map.0.contains_key(&c) {
            return Some((c, last_empty));
        }
        last_empty = c;
        t += 0.05;
    }
    None
}

/// True if cell `c` is where the player's body is, so we never place a block on top of them.
fn in_player(c: IVec3, feet: Vec3) -> bool {
    let p = cell(feet);
    c.x == p.x && c.z == p.z && (c.y == p.y || c.y == p.y + 1)
}
