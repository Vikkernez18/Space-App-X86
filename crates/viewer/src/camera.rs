// AI-assisted
//! Smooth orbital 3D camera controller (rotate, pan, zoom) for terrain inspection.
//!
//! Automatically yields pointer and scroll input whenever the cursor is interacting
//! with the `bevy_egui` user interface.

use bevy::input::mouse::{MouseMotion, MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy_egui::EguiContexts;

/// Component attached to the primary 3D orbital camera.
#[derive(Component, Debug, Clone)]
pub struct OrbitCamera {
    /// World-space target focal point (`Vec3`).
    pub focus: Vec3,
    /// Bounding sphere radius of the currently loaded terrain mesh in meters.
    pub bounding_radius: f32,
    /// Target yaw angle in radians around Bevy's `+Y` up axis.
    pub target_yaw: f32,
    /// Target pitch angle in radians above the horizontal `XZ` plane.
    pub target_pitch: f32,
    /// Target distance from `focus` in meters.
    pub target_radius: f32,
    /// Current smoothed focal point.
    pub current_focus: Vec3,
    /// Current smoothed yaw angle.
    pub current_yaw: f32,
    /// Current smoothed pitch angle.
    pub current_pitch: f32,
    /// Current smoothed distance from `focus`.
    pub current_radius: f32,
    /// Minimum allowed zoom distance in meters.
    pub min_radius: f32,
    /// Maximum allowed zoom distance in meters.
    pub max_radius: f32,
    /// When `true`, smoothly orbits the camera around the terrain while idle (`[Rotate]` / key `A`).
    pub auto_rotate: bool,
    /// Angular speed in radians per second when `auto_rotate` is enabled.
    pub auto_rotate_speed: f32,
}

impl Default for OrbitCamera {
    fn default() -> Self {
        let focus = Vec3::ZERO;
        let bounding_radius = 1415.0_f32;
        let yaw = 0.55_f32;
        let pitch = 0.62_f32;
        let radius = bounding_radius * 1.8;
        Self {
            focus,
            bounding_radius,
            target_yaw: yaw,
            target_pitch: pitch,
            target_radius: radius,
            current_focus: focus,
            current_yaw: yaw,
            current_pitch: pitch,
            current_radius: radius,
            min_radius: (bounding_radius * 0.02).max(1.0),
            max_radius: (bounding_radius * 25.0).max(50_000.0),
            auto_rotate: false,
            auto_rotate_speed: 0.25,
        }
    }
}

impl OrbitCamera {
    /// Resets the camera to frame a terrain of bounding sphere radius `bounding_radius`
    /// centered at `center` (`(0.0, 0.0, 0.0)`) at a proportional distance (`~1.8 * bounding_radius`).
    pub fn frame_terrain(&mut self, center: Vec3, bounding_radius: f32) {
        let safe_radius = bounding_radius.max(10.0);
        self.bounding_radius = safe_radius;
        self.focus = center;
        self.current_focus = center;
        self.target_yaw = 0.55;
        self.target_pitch = 0.62;
        self.min_radius = (safe_radius * 0.02).max(1.0);
        self.max_radius = (safe_radius * 25.0).max(50_000.0);
        self.target_radius = (safe_radius * 1.8).clamp(self.min_radius, self.max_radius);
        self.current_radius = self.target_radius;
    }

    /// Adjusts `target_radius` by a multiplicative zoom `factor` (`< 1.0` zooms in, `> 1.0` zooms out).
    pub fn zoom_by_factor(&mut self, factor: f32) {
        self.target_radius =
            (self.target_radius * factor).clamp(self.min_radius, self.max_radius);
    }

    /// Dynamically updates `Projection::Perspective` `near` and `far` clip planes from
    /// `self.bounding_radius` so large or small terrains never clip when zooming or rotating.
    pub fn apply_perspective_clip_planes(&self, projection: &mut Projection) {
        let br = self.bounding_radius.max(10.0);
        if let Projection::Perspective(ref mut persp) = *projection {
            persp.near = (br * 0.0005).max(0.05);
            persp.far = (br * 60.0).max(100_000.0);
        }
    }

    /// Computes the camera `Transform` from the current smoothed spherical coordinates.
    pub fn compute_transform(&self) -> Transform {
        let cos_p = self.current_pitch.cos();
        let sin_p = self.current_pitch.sin();
        let sin_y = self.current_yaw.sin();
        let cos_y = self.current_yaw.cos();

        let offset = Vec3::new(
            self.current_radius * cos_p * sin_y,
            self.current_radius * sin_p,
            self.current_radius * cos_p * cos_y,
        );
        Transform::from_translation(self.current_focus + offset)
            .looking_at(self.current_focus, Vec3::Y)
    }
}

/// System that processes mouse drag, scroll wheel, and keyboard shortcuts to update [`OrbitCamera`]
/// and synchronize `Projection::Perspective` `near`/`far` clip planes.
pub fn orbit_camera_system(
    time: Res<Time>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut motion_evr: EventReader<MouseMotion>,
    mut scroll_evr: EventReader<MouseWheel>,
    mut egui_contexts: EguiContexts,
    mut query: Query<(&mut OrbitCamera, &mut Transform, &mut Projection)>,
) {
    let ctx = egui_contexts.ctx_mut();
    let pointer_captured = ctx.wants_pointer_input() || ctx.is_pointer_over_area();
    let keyboard_captured = ctx.wants_keyboard_input();

    let mut delta_drag = Vec2::ZERO;
    for ev in motion_evr.read() {
        delta_drag += ev.delta;
    }

    let mut scroll_amount = 0.0_f32;
    for ev in scroll_evr.read() {
        let unit_scale = match ev.unit {
            MouseScrollUnit::Line => 1.0,
            MouseScrollUnit::Pixel => 0.02,
        };
        scroll_amount += ev.y * unit_scale;
    }

    let dt = time.delta_seconds().clamp(0.001, 0.1);
    let smooth_t = (12.0 * dt).clamp(0.05, 1.0);

    for (mut cam, mut transform, mut projection) in &mut query {
        let mut user_dragging = false;
        if !pointer_captured {
            let shift_held =
                keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
            let is_panning = mouse_buttons.pressed(MouseButton::Right)
                || mouse_buttons.pressed(MouseButton::Middle)
                || (mouse_buttons.pressed(MouseButton::Left) && shift_held);
            let is_orbiting = mouse_buttons.pressed(MouseButton::Left) && !shift_held;

            if is_orbiting || is_panning {
                user_dragging = true;
            }

            if is_orbiting && delta_drag.length_squared() > 0.0 {
                cam.target_yaw -= delta_drag.x * 0.0065;
                cam.target_pitch = (cam.target_pitch + delta_drag.y * 0.0065).clamp(0.08, 1.50);
            } else if is_panning && delta_drag.length_squared() > 0.0 {
                let pan_speed = cam.current_radius * 0.0012;
                let right = transform.right().as_vec3();
                let forward_xz = Vec3::new(transform.forward().x, 0.0, transform.forward().z)
                    .try_normalize()
                    .unwrap_or(Vec3::NEG_Z);
                cam.focus -= right * (delta_drag.x * pan_speed);
                cam.focus += forward_xz * (delta_drag.y * pan_speed);
            }

            if scroll_amount.abs() > 0.0 {
                let zoom_factor = (1.0 - scroll_amount * 0.11).clamp(0.75, 1.35);
                cam.target_radius =
                    (cam.target_radius * zoom_factor).clamp(cam.min_radius, cam.max_radius);
            }
        }

        if cam.auto_rotate && !user_dragging {
            cam.target_yaw += cam.auto_rotate_speed * dt;
        }

        if !keyboard_captured {
            if keys.just_pressed(KeyCode::KeyR) {
                let br = cam.bounding_radius;
                cam.frame_terrain(Vec3::ZERO, br);
            }

            let zoom_in_pressed =
                keys.pressed(KeyCode::Equal) || keys.pressed(KeyCode::NumpadAdd);
            let zoom_out_pressed =
                keys.pressed(KeyCode::Minus) || keys.pressed(KeyCode::NumpadSubtract);

            if zoom_in_pressed && !zoom_out_pressed {
                cam.zoom_by_factor((1.0 - 1.65 * dt).clamp(0.75, 0.995));
            } else if zoom_out_pressed && !zoom_in_pressed {
                cam.zoom_by_factor((1.0 + 1.65 * dt).clamp(1.005, 1.35));
            }
        }

        cam.apply_perspective_clip_planes(&mut projection);

        cam.current_yaw += (cam.target_yaw - cam.current_yaw) * smooth_t;
        cam.current_pitch += (cam.target_pitch - cam.current_pitch) * smooth_t;
        cam.current_radius += (cam.target_radius - cam.current_radius) * smooth_t;
        cam.current_focus = cam.current_focus.lerp(cam.focus, smooth_t);

        *transform = cam.compute_transform();
    }
}
