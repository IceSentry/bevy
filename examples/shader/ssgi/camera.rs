use bevy::{input::mouse::MouseMotion, prelude::*};

#[derive(Component)]
pub struct FlyCam;

pub fn fly_camera(
    time: Res<Time>,
    mut camera_transform: Query<&mut Transform, With<FlyCam>>,
    windows: Res<Windows>,
    mouse_input: Res<Input<MouseButton>>,
    key_input: Res<Input<KeyCode>>,
    mut mouse_motion: EventReader<MouseMotion>,
    mut velocity: Local<Vec3>,
) {
    if !mouse_input.pressed(MouseButton::Right) {
        if !mouse_motion.is_empty() {
            mouse_motion.clear();
        }
        return;
    }

    let dt = time.delta_seconds();

    let mut transform = camera_transform.single_mut();

    // Rotate

    let mut mouse_delta = Vec2::ZERO;
    for mouse_motion in mouse_motion.iter() {
        mouse_delta += mouse_motion.delta;
    }

    if mouse_delta != Vec2::ZERO {
        let window = if let Some(window) = windows.get_primary() {
            Vec2::new(window.width(), window.height())
        } else {
            Vec2::ZERO
        };
        let delta_x = mouse_delta.x / window.x * std::f32::consts::PI * 2.0;
        let delta_y = mouse_delta.y / window.y * std::f32::consts::PI;
        let yaw = Quat::from_rotation_y(-delta_x);
        let pitch = Quat::from_rotation_x(-delta_y);
        transform.rotation = yaw * transform.rotation; // rotate around global y axis
        transform.rotation *= pitch; // rotate around local x axis
    }

    // Translate

    let mut axis_input = Vec3::ZERO;
    if key_input.pressed(KeyCode::W) {
        axis_input.z += 1.0;
    }
    if key_input.pressed(KeyCode::S) {
        axis_input.z -= 1.0;
    }
    if key_input.pressed(KeyCode::D) {
        axis_input.x += 1.0;
    }
    if key_input.pressed(KeyCode::A) {
        axis_input.x -= 1.0;
    }
    if key_input.pressed(KeyCode::E) {
        axis_input.y += 1.0;
    }
    if key_input.pressed(KeyCode::Q) {
        axis_input.y -= 1.0;
    }

    if axis_input != Vec3::ZERO {
        let max_speed = 5.0;
        *velocity = axis_input.normalize() * max_speed;
    } else {
        let friction = 0.5;
        *velocity *= 1.0 - friction;
        if velocity.length_squared() < 1e-6 {
            *velocity = Vec3::ZERO;
        }
    }

    let forward = transform.forward();
    let right = transform.right();
    transform.translation +=
        velocity.x * dt * right + velocity.y * dt * Vec3::Y + velocity.z * dt * forward;
}
