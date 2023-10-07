use std::net::SocketAddr;

use bevy::{prelude::*, sprite::MaterialMesh2dBundle};
use bytemuck::{Pod, Zeroable};
use bevy_ggrs::{GgrsAppExtension, GgrsPlugin, GgrsSchedule, Session, AddRollbackCommandExtension, Rollback, PlayerInputs};
use ggrs::{Config, SessionBuilder, PlayerType, PlayerHandle, UdpNonBlockingSocket};
use structopt::StructOpt;

const FPS: usize = 60;
const FRAME_TIME: f32 = 1. / FPS as f32;

const PLAYER_SPEED: f32 = 400.;
const PLAYER_SIZE: f32 = 25.;

const BULLET_SPEED: f32 = 500.;
const BULLET_SIZE: f32 = 10.;

const MAP_SIZE: Vec2 = Vec2{x: 1600., y: 1200.};

const TIME_TO_RELOAD: f32 = 0.5;

const INPUT_UP: u8 = 1 << 0;
const INPUT_DOWN: u8 = 1 << 1;
const INPUT_LEFT: u8 = 1 << 2;
const INPUT_RIGHT: u8 = 1 << 3;

const INPUT_UP2: u8 = 1 << 4;
const INPUT_DOWN2: u8 = 1 << 5;
const INPUT_LEFT2: u8 = 1 << 6;
const INPUT_RIGHT2: u8 = 1 << 7;


pub struct GgrsConfig;
impl Config for GgrsConfig {
    type Input = BoxInput;
    type State = u8;
    type Address = SocketAddr;
}

#[repr(C)]
#[derive(Copy, Clone, PartialEq, Eq, Pod, Zeroable)]
pub struct BoxInput {
    pub val: u8
}

#[derive(StructOpt, Resource)]
struct Opt {
    #[structopt(short, long)]
    local_port: u16,
    #[structopt(short, long)]
    players: Vec<String>,
}


#[derive(Default, Component)]
pub struct Player {
    handle: usize,
    reload_time: f32
}

#[derive(Default, Component)]
pub struct Bullet;

#[derive(Default, Component, Reflect)]
pub struct LifeTime {
    val: f32
}

#[derive(Default, Component, Reflect)]
pub struct Velocity {
    val: Vec3
}



pub fn input(_handle: In<PlayerHandle>, keyboard_input: Res<Input<KeyCode>>) -> BoxInput {
    let mut input: u8 = 0;

    if keyboard_input.pressed(KeyCode::W) {
        input |= INPUT_UP;
    }
    if keyboard_input.pressed(KeyCode::A) {
        input |= INPUT_LEFT;
    }
    if keyboard_input.pressed(KeyCode::S) {
        input |= INPUT_DOWN;
    }
    if keyboard_input.pressed(KeyCode::D) {
        input |= INPUT_RIGHT;
    }
    if keyboard_input.pressed(KeyCode::Up) {
        input |= INPUT_UP2;
    }
    if keyboard_input.pressed(KeyCode::Left) {
        input |= INPUT_LEFT2;
    }
    if keyboard_input.pressed(KeyCode::Down) {
        input |= INPUT_DOWN2;
    }
    if keyboard_input.pressed(KeyCode::Right) {
        input |= INPUT_RIGHT2;
    }

    BoxInput { val: input }
}




fn main() {
    let opt = Opt::from_args();

    let mut sess_build = SessionBuilder::<GgrsConfig>::new()
        .with_num_players(opt.players.len())
        .with_desync_detection_mode(ggrs::DesyncDetection::On { interval: 10 }) // (optional) set how often to exchange state checksums
        .with_max_prediction_window(12) // (optional) set max prediction window
        .with_input_delay(2); // (optional) set input delay for the local player

        info!("ADDING PLAYERS...");
    
    for (i, player_addr) in opt.players.iter().enumerate() {
        // local player
        if player_addr == "localhost" {
            sess_build = sess_build.add_player(PlayerType::Local, i).unwrap();
        } else {
            // remote players
            let remote_addr: SocketAddr = player_addr.parse().unwrap();
            sess_build = sess_build.add_player(PlayerType::Remote(remote_addr), i).unwrap();
        }
    }

    info!("PLAYERS ADDED");
    
    let socket = UdpNonBlockingSocket::bind_to_port(opt.local_port).unwrap();
    let sess = sess_build.start_p2p_session(socket).unwrap();
        
    App::new()
        .add_ggrs_plugin(GgrsPlugin::<GgrsConfig>::new()
            .with_update_frequency(FPS)
            .with_input_system(input)
            .register_rollback_component::<Transform>()
        )
        .add_systems(GgrsSchedule, (
            move_players,
            move_objects.after(move_players),
            age_mortals.after(move_objects)
        )).add_plugins(DefaultPlugins)
        .insert_resource(Session::P2P(sess))
        .add_systems(Startup, setup)
    .run();
}




fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    session: Res<Session<GgrsConfig>>
) {
    let pnum = match &*session {
        Session::SyncTest(s) => s.num_players(),
        Session::P2P(s) => s.num_players(),
        Session::Spectator(s) => s.num_players(),
    };

    for i in 0..pnum {
        println!("{}", (i as f32) / (pnum as f32 + 1.));
        commands.spawn((MaterialMesh2dBundle {
                mesh: meshes.add(shape::Circle::new(PLAYER_SIZE).into()).into(),
                material: materials.add(ColorMaterial::from(Color::Hsla { hue: (i as f32) / (pnum as f32) * 360., saturation: 0.75, lightness: 0.5, alpha: 1. })),
                transform: Transform::from_translation(Vec3 { x: i as f32 * 75., y: 0., z: 0. }),
                ..default()
            },
            Player {handle: i, reload_time: 0.},
        )).add_rollback();
    }

    commands.spawn(MaterialMesh2dBundle {
        mesh: meshes.add(shape::Quad::new(MAP_SIZE).into()).into(),
        material: materials.add(ColorMaterial::from(Color::Rgba { red: 0.5, green: 0.5, blue: 0.69, alpha: 1. })),
        ..default()
    });

    commands.spawn(Camera2dBundle::default());
}




fn move_players(
    mut players: Query<(&mut Transform, &mut Player), With<Rollback>>,

    mut commands: Commands,
    
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    inputs: Res<PlayerInputs<GgrsConfig>>
) {
    for (mut player_t, mut player) in players.iter_mut() {
        let input = inputs[player.handle].0.val;

        let mut delta_pos = Vec3{x: 0., y: 0., z: 0.};

        if input & INPUT_UP    != 0 {delta_pos.y += 1.;}
        if input & INPUT_DOWN  != 0 {delta_pos.y -= 1.;}
        if input & INPUT_RIGHT != 0 {delta_pos.x += 1.;}
        if input & INPUT_LEFT  != 0 {delta_pos.x -= 1.;}

        player_t.translation += delta_pos.normalize_or_zero() * PLAYER_SPEED * FRAME_TIME;

        let mut bullet_speed = Vec3{x: 0., y: 0., z: 0.};

        if input & INPUT_UP2    != 0 {bullet_speed.y += 1.;}
        if input & INPUT_DOWN2  != 0 {bullet_speed.y -= 1.;}
        if input & INPUT_RIGHT2 != 0 {bullet_speed.x += 1.;}
        if input & INPUT_LEFT2  != 0 {bullet_speed.x -= 1.;}

        bullet_speed = bullet_speed.normalize_or_zero();
        
        if bullet_speed.length() != 0. && player.reload_time <= 0. {
            commands.spawn((MaterialMesh2dBundle{
                    mesh: meshes.add(shape::Circle::new(BULLET_SIZE).into()).into(),
                    material: materials.add(ColorMaterial::from(Color::Rgba { red: 0.69, green: 0.69, blue: 0.69, alpha: 1. })),
                    transform: Transform::from_translation(player_t.translation),
                    ..default()
                },
                Bullet,
                Velocity{val: bullet_speed},
                LifeTime{val: 1.}
            )).add_rollback();

            player.reload_time = TIME_TO_RELOAD;
        }

        player.reload_time -= FRAME_TIME;
        if player.reload_time < 0. {player.reload_time = 0.;}
    }
}




fn move_objects(
    mut objects: Query<(&mut Transform, &Velocity), With<Rollback>>
) {
    for (mut object_t, object_v) in objects.iter_mut() {
        object_t.translation += object_v.val * FRAME_TIME * BULLET_SPEED;
    }
}




fn age_mortals(
    mut commands: Commands,

    mut objects: Query<(&mut LifeTime, Entity), With<Rollback>>
) {
    for (mut lifetime, entity) in objects.iter_mut() {
        lifetime.val -= FRAME_TIME;

        if lifetime.val <= 0. {
            commands.entity(entity).despawn();
        }
    }
}