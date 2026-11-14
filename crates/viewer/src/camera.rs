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
}

impl Default for OrbitCamera {
    fn default() -> Self {
        let focus = Vec3::ZERO;
        let yaw = 0.55_f32;
        let pitch = 0.62_f32;
        let radius = 2400.0_f32;
        Self {
            focus,
            target_yaw: yaw,
            target_pitch: pitch,
            target_radius: radius,
            current_focus: focus,
            current_yaw: yaw,
            current_pitch: pitch,
            current_radius: radius,
            min_radius: 80.0,
            max_radius: 50_000.0,
        }
    }
}

impl OrbitCamera {
    /// Resets the camera to frame a terrain of horizontal span `span_meters` centered at `center`.
    pub fn frame_terrain(&mut self, center: Vec3, span_meters: f32) {
        let safe_span = span_meters.max(200.0);
        self.focus = center;
        self.target_yaw = 0.55;
        self.target_pitch = 0.62;
        self.target_radius = (safe_span * 1.25).clamp(self.min_radius, self.max_radius);
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

/// System that processes mouse drag, scroll wheel, and keyboard shortcuts to update [`OrbitCamera`].
pub fn orbit_camera_system(
    time: Res<Time>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut motion_evr: EventReader<MouseMotion>,
    mut scroll_evr: EventReader<MouseWheel>,
    mut egui_contexts: EguiContexts,
    mut query: Query<(&mut OrbitCamera, &mut Transform)>,
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

    for (mut cam, mut transform) in &mut query {
        if !pointer_captured {
            let shift_held =
                keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
            let is_panning = mouse_buttons.pressed(MouseButton::Right)
                || mouse_buttons.pressed(MouseButton::Middle)
                || (mouse_buttons.pressed(MouseButton::Left) && shift_held);
            let is_orbiting = mouse_buttons.pressed(MouseButton::Left) && !shift_held;

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

        if !keyboard_captured && keys.just_pressed(KeyCode::KeyR) {
            cam.frame_terrain(Vec3::ZERO, 2000.0);
        }

        cam.current_yaw += (cam.target_yaw - cam.current_yaw) * smooth_t;
        cam.current_pitch += (cam.target_pitch - cam.current_pitch) * smooth_t;
        cam.current_radius += (cam.target_radius - cam.current_radius) * smooth_t;
        cam.current_focus = cam.current_focus.lerp(cam.focus, smooth_t);

        *transform = cam.compute_transform();
    }
}
