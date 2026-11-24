use godot::prelude::*;
use godot::classes::{
    CharacterBody3D, ICharacterBody3D, CollisionShape3D, Engine, Input,
    InputEvent, InputEventMouseMotion,
};
use godot::classes::input::MouseMode;
use godot::global::Key;

#[derive(GodotClass)]
#[class(base=CharacterBody3D)]
pub struct PlayerController {
    base: Base<CharacterBody3D>,
    #[export]
    speed: f32,
    sprint_speed: f32,
    pub speed_mult: f32,
    jump_velocity: f32,
    mouse_sens: f32,
    gravity: f32,
    pitch: f32,
    pub controls_locked: bool,
    pub noclip: bool,
    head: Option<Gd<Node3D>>,
}

#[godot_api]
impl ICharacterBody3D for PlayerController {
    fn init(base: Base<CharacterBody3D>) -> Self {
        Self {
            base,
            speed: 6.5,
            sprint_speed: 10.5,
            speed_mult: 1.0,
            jump_velocity: 6.2,
            mouse_sens: 0.0025,
            gravity: 16.0,
            pitch: 0.0,
            controls_locked: false,
            noclip: false,
            head: None,
        }
    }

    fn ready(&mut self) {
        if Engine::singleton().is_editor_hint() {
            return;
        }
        self.head = self.base().try_get_node_as::<Node3D>("Head");
    }

    fn input(&mut self, event: Gd<InputEvent>) {
        if Engine::singleton().is_editor_hint() || self.controls_locked {
            return;
        }
        if let Ok(mm) = event.try_cast::<InputEventMouseMotion>() {
            if Input::singleton().get_mouse_mode() == MouseMode::CAPTURED {
                let rel = mm.get_relative();
                let sens = self.mouse_sens;
                self.base_mut().rotate_y(-rel.x * sens);
                self.pitch = (self.pitch - rel.y * sens).clamp(-1.45, 1.45);
                if let Some(head) = self.head.as_mut() {
                    head.set_rotation(Vector3::new(self.pitch, 0.0, 0.0));
                }
            }
        }
    }

    fn physics_process(&mut self, delta: f64) {
        if Engine::singleton().is_editor_hint() {
            return;
        }
        let dt = delta as f32;
        let inp = Input::singleton();

        if self.noclip && !self.controls_locked {
            let b = self
                .head
                .as_ref()
                .map(|h| h.get_global_transform().basis)
                .unwrap_or_else(|| self.base().get_transform().basis);
            let mut mv = Vector3::ZERO;
            if inp.is_key_pressed(Key::W) || inp.is_key_pressed(Key::UP) { mv -= b.col_c(); }
            if inp.is_key_pressed(Key::S) || inp.is_key_pressed(Key::DOWN) { mv += b.col_c(); }
            if inp.is_key_pressed(Key::D) || inp.is_key_pressed(Key::RIGHT) { mv += b.col_a(); }
            if inp.is_key_pressed(Key::A) || inp.is_key_pressed(Key::LEFT) { mv -= b.col_a(); }
            if inp.is_key_pressed(Key::SPACE) { mv.y += 1.0; }
            if inp.is_key_pressed(Key::CTRL) || inp.is_key_pressed(Key::C) { mv.y -= 1.0; }
            if mv.length_squared() > 0.001 {
                mv = mv.normalized();
            }
            let spd = (if inp.is_key_pressed(Key::SHIFT) { 24.0 } else { 12.0 }) * self.speed_mult;
            let pos = self.base().get_position() + mv * (spd * dt);
            self.base_mut().set_position(pos);
            self.base_mut().set_velocity(Vector3::ZERO);
            return;
        }

        let was_on_floor = self.base().is_on_floor();
        let before_xf = self.base().get_global_transform();

        let mut vel = self.base().get_velocity();
        if !was_on_floor {
            vel.y -= self.gravity * dt;
        }
        let mut jumped = false;
        if !self.controls_locked {
            if inp.is_key_pressed(Key::SPACE) && was_on_floor {
                vel.y = self.jump_velocity;
                jumped = true;
            }
            let mut d2 = Vector2::ZERO;
            if inp.is_key_pressed(Key::W) || inp.is_key_pressed(Key::UP) { d2.y -= 1.0; }
            if inp.is_key_pressed(Key::S) || inp.is_key_pressed(Key::DOWN) { d2.y += 1.0; }
            if inp.is_key_pressed(Key::A) || inp.is_key_pressed(Key::LEFT) { d2.x -= 1.0; }
            if inp.is_key_pressed(Key::D) || inp.is_key_pressed(Key::RIGHT) { d2.x += 1.0; }

            let b = self.base().get_transform().basis;
            let mut mv = b.col_a() * d2.x + b.col_c() * d2.y;
            mv.y = 0.0;
            if mv.length_squared() > 0.001 {
                mv = mv.normalized();
            }
            let spd = (if inp.is_key_pressed(Key::SHIFT) { self.sprint_speed } else { self.speed }) * self.speed_mult;
            vel.x = mv.x * spd;
            vel.z = mv.z * spd;
        } else {
            vel.x = 0.0;
            vel.z = 0.0;
        }

        self.base_mut().set_velocity(vel);
        self.base_mut().move_and_slide();

        // Подшаг на препятствия (сейчас max_step = 1.15 м — примерно по пояс)
        let desired_horiz = Vector3::new(vel.x * dt, 0.0, vel.z * dt);
        if was_on_floor && !jumped && desired_horiz.length_squared() > 0.000001 {
            let after_xf = self.base().get_global_transform();
            let actual_horiz = Vector3::new(
                after_xf.origin.x - before_xf.origin.x,
                0.0,
                after_xf.origin.z - before_xf.origin.z,
            );
            if self.base().is_on_wall() || actual_horiz.length_squared() < desired_horiz.length_squared() * 0.95 {
                let max_step = 1.15_f32;
                let mut probe_xf = before_xf;
                probe_xf.origin.y += 0.05;

                let mut stepped = false;
                if !self.base_mut().test_move(probe_xf, Vector3::new(0.0, max_step - 0.05, 0.0)) {
                    let mut raised_xf = before_xf;
                    raised_xf.origin.y += max_step;
                    if !self.base_mut().test_move(raised_xf, desired_horiz) {
                        let mut fwd_xf = raised_xf;
                        fwd_xf.origin += desired_horiz;
                        self.base_mut().set_global_transform(fwd_xf);

                        let down_hit = self
                            .base_mut()
                            .move_and_collide_ex(Vector3::new(0.0, -max_step, 0.0))
                            .test_only(true)
                            .done();

                        if let Some(col) = down_hit {
                            if col.get_normal().y > 0.2 {
                                let target_pos = fwd_xf.origin + col.get_travel();
                                if target_pos.y > before_xf.origin.y + 0.005 {
                                    self.base_mut().set_global_position(target_pos);
                                    self.base_mut().set_velocity(Vector3::new(vel.x, 0.0, vel.z));
                                    stepped = true;
                                }
                            }
                        }
                    }
                }
                if !stepped {
                    self.base_mut().set_global_transform(after_xf);
                }
            }
        }
    }
}

impl PlayerController {
    pub fn set_noclip(&mut self, v: bool) {
        self.noclip = v;
        if let Some(mut cs) = self.base().try_get_node_as::<CollisionShape3D>("CollisionShape3D") {
            cs.set_disabled(v);
        }
    }

    pub fn add_recoil(&mut self, d_pitch: f32, d_yaw: f32) {
        self.base_mut().rotate_y(d_yaw);
        self.pitch = (self.pitch + d_pitch).clamp(-1.45, 1.45);
        if let Some(head) = self.head.as_mut() {
            head.set_rotation(Vector3::new(self.pitch, 0.0, 0.0));
        }
    }
}
