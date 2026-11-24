use godot::prelude::*;
use godot::classes::{
    Animation, AnimationPlayer, AudioStream, AudioStreamPlayer, BoxMesh,
    Button, Camera3D, CanvasLayer, ColorRect, Control, CylinderMesh, Engine,
    FileAccess, Font, HttpRequest, Input, InputEvent, InputEventKey, Json, Label, LineEdit,
    Mesh, MeshInstance3D, OmniLight3D, PackedScene, PhysicsRayQueryParameters3D, QuadMesh,
    ResourceLoader, Shader, ShaderMaterial, Skeleton3D, SphereMesh, StandardMaterial3D, Texture2D,
    TextureRect, WorldEnvironment,
};
use godot::classes::animation::LoopMode;
use godot::classes::base_material_3d::{Feature, ShadingMode, TextureParam, Transparency};
use godot::classes::control::{LayoutPreset, MouseFilter};
use godot::classes::file_access::ModeFlags;
use godot::classes::http_client::Method;
use godot::classes::input::MouseMode;
use godot::classes::light_3d::Param as LightParam;
use godot::classes::texture_rect::ExpandMode;
use godot::global::{Key, MouseButton};

use crate::player_controller::PlayerController;

const FB_URL: &str = "https://kabaneze-default-rtdb.firebaseio.com";

struct BlobProj {
    mesh: Gd<MeshInstance3D>,
    vel: Vector3,
    life: f32,
    is_chunk: bool,
    dmg: i32,
}

struct SmokePuff {
    mesh: Gd<MeshInstance3D>,
    mat: Gd<StandardMaterial3D>,
    life: f32,
    max_life: f32,
    start_scale: f32,
}

struct BloodPuddle {
    mesh: Gd<MeshInstance3D>,
    mat: Gd<StandardMaterial3D>,
    age: f32,
    target_r: f32,
    on_asphalt: bool,
}

struct GoreChunk {
    node: Gd<Node3D>,
    vel: Vector3,
    ang_vel: Vector3,
    life: f32,
    landed: bool,
}

struct RocketProj {
    mesh: Gd<MeshInstance3D>,
    vel: Vector3,
    life: f32,
}

struct CanisterUnit {
    node: Gd<Node3D>,
    active: bool,
    respawn_timer: f32,
}

struct MinionUnit {
    node: Option<Gd<Node3D>>,
    hp: i32,
    respawn_timer: f32,
    shoot_timer: f32,
    phase: f32,
}

struct GameNpc {
    root: Gd<Node3D>,
    anim: Option<Gd<AnimationPlayer>>,
    dir: Vector3,
    hp: i32,
    role: i32,
    timer: f32,
    shoot_cd: f32,
    alert_timer: f32,
    panic_t: f32,
    walk_anim: String,
    death_vel: Vector3,
    has_death_vel: bool,
}

struct ErrandItem {
    text: String,
    done: bool,
}

struct KillEntry {
    text: String,
    life: f32,
}

struct LeaderRow {
    name: String,
    elo: i32,
    kills: i32,
}

fn dist_xz(a: Vector3, b: Vector3) -> f32 {
    Vector2::new(a.x, a.z).distance_to(Vector2::new(b.x, b.z))
}

fn ray_hit_sphere(ro: Vector3, rd: Vector3, c: Vector3, r: f32) -> Option<f32> {
    let oc = ro - c;
    let b = oc.dot(rd);
    if b > 0.0 {
        return None;
    }
    let cc = oc.dot(oc) - r * r;
    let h = b * b - cc;
    if h < 0.0 {
        return None;
    }
    let mut t = -b - h.sqrt();
    if t < 0.0 {
        t = 0.01;
    }
    Some(t)
}

fn find_node_substr(root: &Gd<Node>, sub: &str) -> Option<Gd<Node3D>> {
    let name = root.get_name().to_string().to_lowercase();
    if name.contains(&sub.to_lowercase()) {
        if let Ok(n3) = root.clone().try_cast::<Node3D>() {
            return Some(n3);
        }
    }
    for i in 0..root.get_child_count() {
        if let Some(ch) = root.get_child(i) {
            if let Some(f) = find_node_substr(&ch, sub) {
                return Some(f);
            }
        }
    }
    None
}

fn find_anim_player(root: &Gd<Node>) -> Option<Gd<AnimationPlayer>> {
    if let Ok(ap) = root.clone().try_cast::<AnimationPlayer>() {
        return Some(ap);
    }
    for i in 0..root.get_child_count() {
        if let Some(ch) = root.get_child(i) {
            if let Some(f) = find_anim_player(&ch) {
                return Some(f);
            }
        }
    }
    None
}

fn find_first_mesh(root: &Gd<Node>) -> Option<Gd<MeshInstance3D>> {
    if let Ok(m) = root.clone().try_cast::<MeshInstance3D>() {
        return Some(m);
    }
    for i in 0..root.get_child_count() {
        if let Some(ch) = root.get_child(i) {
            if let Some(f) = find_first_mesh(&ch) {
                return Some(f);
            }
        }
    }
    None
}

fn collect_aabb(cur: &Gd<Node>, root: &Gd<Node>, out: &mut Option<Aabb>) {
    if let Ok(m) = cur.clone().try_cast::<MeshInstance3D>() {
        let mut xf = Transform3D::IDENTITY;
        let mut p = Some(cur.clone());
        while let Some(n) = p {
            if n == *root { break; }
            if let Ok(n3) = n.clone().try_cast::<Node3D>() {
                xf = n3.get_transform() * xf;
            }
            p = n.get_parent();
        }
        let b = xf * m.get_aabb();
        *out = Some(match *out {
            Some(prev) => prev.merge(b),
            None => b,
        });
    }
    for i in 0..cur.get_child_count() {
        if let Some(ch) = cur.get_child(i) {
            collect_aabb(&ch, root, out);
        }
    }
}

fn apply_material_recursive(cur: &Gd<Node>, mat: &Gd<StandardMaterial3D>) {
    if let Ok(mut m) = cur.clone().try_cast::<MeshInstance3D>() {
        m.set_material_override(mat);
    }
    for i in 0..cur.get_child_count() {
        if let Some(ch) = cur.get_child(i) {
            apply_material_recursive(&ch, mat);
        }
    }
}

#[derive(GodotClass)]
#[class(base=Node)]
pub struct GameManager {
    base: Base<Node>,
    rng_state: u32,
    player: Option<Gd<PlayerController>>,
    cam: Option<Gd<Camera3D>>,
    pig_node: Option<Gd<Node3D>>,
    weapon_mount: Option<Gd<Node3D>>,
    shotgun_node: Option<Gd<Node3D>>,
    ak_node: Option<Gd<Node3D>>,
    awp_node: Option<Gd<Node3D>>,
    awp_anim: Option<Gd<AnimationPlayer>>,
    awp_fire_anim: String,
    rpg_node: Option<Gd<Node3D>>,
    quest_beacon: Option<Gd<MeshInstance3D>>,
    muzzle_light: Option<Gd<OmniLight3D>>,
    muzzle_flash: Option<Gd<Node3D>>,
    flash_timer: f32,

    shop_pos: Vector3,
    gunshop_pos: Vector3,
    gas_drop_pos: Vector3,
    ahmed_pos: Vector3,
    quest_item_pos: Vector3,

    elo: i32,
    pig_kills: i32,
    last_elo_delta: i32,
    player_nick: String,
    day_idx: usize,
    quest_stage: i32,
    has_quest_item: bool,
    round_finished: bool,
    weapon_type: i32,
    has_ak: bool,
    has_awp: bool,
    has_rpg: bool,
    has_dmg_buff: bool,
    has_speed_buff: bool,
    carrying_canister: bool,
    buy_menu_open: bool,
    wanted_level: i32,
    wanted_decay: f32,

    pig_max_hp: i32,
    pig_hp: i32,
    player_hp: i32,
    pig_yaw: f32,
    pig_lift_y: f32,
    spray_timer: f32,
    panic_timer: f32,
    shoot_cd: f32,
    recoil: f32,
    spread_kick: f32,
    hit_marker: f32,
    hurt_flash: f32,
    crit_hit: bool,
    sim_time: f32,

    in_menu: bool,
    console_open: bool,
    god_mode: bool,
    always_crit_ak: bool,
    console_input: String,

    ahmed_msg_t: f32,
    shop_msg_t: f32,
    day_banner_t: f32,
    warn_msg_t: f32,
    cheat_msg_t: f32,
    ahmed_msg: String,
    shop_msg: String,
    warn_msg: String,
    cheat_msg: String,

    blobs: Vec<BlobProj>,
    rockets: Vec<RocketProj>,
    smokes: Vec<SmokePuff>,
    puddles: Vec<BloodPuddle>,
    gore_chunks: Vec<GoreChunk>,
    canisters: Vec<CanisterUnit>,
    minions: Vec<MinionUnit>,
    npcs: Vec<GameNpc>,
    errands: Vec<ErrandItem>,
    kill_feed: Vec<KillEntry>,

    music_player: Option<Gd<AudioStreamPlayer>>,
    snd_hurt: Option<Gd<AudioStreamPlayer>>,
    snd_win: Option<Gd<AudioStreamPlayer>>,
    snd_shotgun: Option<Gd<AudioStreamPlayer>>,
    snd_ak: Option<Gd<AudioStreamPlayer>>,
    snd_awp: Option<Gd<AudioStreamPlayer>>,
    snd_rpg: Option<Gd<AudioStreamPlayer>>,
    snd_boom: Option<Gd<AudioStreamPlayer>>,
    http_put: Option<Gd<HttpRequest>>,
    http_get: Option<Gd<HttpRequest>>,

    menu_panel: Option<Gd<Control>>,
    hud_panel: Option<Gd<Control>>,
    nick_edit: Option<Gd<LineEdit>>,
    lb_label: Option<Gd<Label>>,
    menu_elo_label: Option<Gd<Label>>,
    hud_elo_label: Option<Gd<Label>>,
    hud_day_label: Option<Gd<Label>>,
    hud_errands_label: Option<Gd<Label>>,
    hud_wanted_label: Option<Gd<Label>>,
    wanted_stars: Vec<Gd<TextureRect>>,
    hud_hp_label: Option<Gd<Label>>,
    hud_hp_bar: Option<Gd<ColorRect>>,
    boss_hp_bg: Option<Gd<ColorRect>>,
    boss_hp_bar: Option<Gd<ColorRect>>,
    boss_hp_label: Option<Gd<Label>>,
    hud_weapon_label: Option<Gd<Label>>,
    hud_prompt_label: Option<Gd<Label>>,
    hud_center_msg: Option<Gd<Label>>,
    hud_killfeed_label: Option<Gd<Label>>,
    hud_end_label: Option<Gd<Label>>,
    hud_console_label: Option<Gd<Label>>,
    buy_bg: Option<Gd<ColorRect>>,
    buy_label: Option<Gd<Label>>,
    scope_overlay: Option<Gd<TextureRect>>,
    scope_left: Option<Gd<ColorRect>>,
    scope_right: Option<Gd<ColorRect>>,
    scope_dot: Option<Gd<ColorRect>>,
    hurt_overlay: Option<Gd<ColorRect>>,
    ch_l: Option<Gd<ColorRect>>,
    ch_r: Option<Gd<ColorRect>>,
    ch_u: Option<Gd<ColorRect>>,
    ch_d: Option<Gd<ColorRect>>,
    ui_font: Option<Gd<Font>>,
    anim_run_res: Option<Gd<Animation>>,
    anim_die_res: Option<Gd<Animation>>,

    e_was: bool,
    f_was: bool,
    g_was: bool,
    h_was: bool,
}

#[godot_api]
impl INode for GameManager {
    fn init(base: Base<Node>) -> Self {
        Self {
            base,
            rng_state: 1337,
            player: None,
            cam: None,
            pig_node: None,
            weapon_mount: None,
            shotgun_node: None,
            ak_node: None,
            awp_node: None,
            awp_anim: None,
            awp_fire_anim: String::new(),
            rpg_node: None,
            quest_beacon: None,
            muzzle_light: None,
            muzzle_flash: None,
            flash_timer: 0.0,
            shop_pos: Vector3::new(-8.5, 0.0, 36.5),
            gunshop_pos: Vector3::new(10.0, 0.0, 9.0),
            gas_drop_pos: Vector3::new(18.0, 0.0, 12.0),
            ahmed_pos: Vector3::new(0.0, 0.0, 24.0),
            quest_item_pos: Vector3::new(-6.0, 0.0, 16.0),
            elo: 1000,
            pig_kills: 0,
            last_elo_delta: 0,
            player_nick: "Player".to_string(),
            day_idx: 0,
            quest_stage: 0,
            has_quest_item: false,
            round_finished: false,
            weapon_type: 1,
            has_ak: false,
            has_awp: false,
            has_rpg: false,
            has_dmg_buff: false,
            has_speed_buff: false,
            carrying_canister: false,
            buy_menu_open: false,
            wanted_level: 0,
            wanted_decay: 0.0,
            pig_max_hp: 500,
            pig_hp: 500,
            player_hp: 100,
            pig_yaw: 0.0,
            pig_lift_y: 1.35,
            spray_timer: 0.0,
            panic_timer: 0.0,
            shoot_cd: 0.0,
            recoil: 0.0,
            spread_kick: 0.0,
            hit_marker: 0.0,
            hurt_flash: 0.0,
            crit_hit: false,
            sim_time: 0.0,
            in_menu: true,
            console_open: false,
            god_mode: false,
            always_crit_ak: false,
            console_input: String::new(),
            ahmed_msg_t: 0.0,
            shop_msg_t: 0.0,
            day_banner_t: 3.5,
            warn_msg_t: 0.0,
            cheat_msg_t: 0.0,
            ahmed_msg: String::new(),
            shop_msg: String::new(),
            warn_msg: String::new(),
            cheat_msg: String::new(),
            blobs: Vec::new(),
            rockets: Vec::new(),
            smokes: Vec::new(),
            puddles: Vec::new(),
            gore_chunks: Vec::new(),
            canisters: Vec::new(),
            minions: Vec::new(),
            npcs: Vec::new(),
            errands: Vec::new(),
            kill_feed: Vec::new(),
            music_player: None,
            snd_hurt: None,
            snd_win: None,
            snd_shotgun: None,
            snd_ak: None,
            snd_awp: None,
            snd_rpg: None,
            snd_boom: None,
            http_put: None,
            http_get: None,
            menu_panel: None,
            hud_panel: None,
            nick_edit: None,
            lb_label: None,
            menu_elo_label: None,
            hud_elo_label: None,
            hud_day_label: None,
            hud_errands_label: None,
            hud_wanted_label: None,
            wanted_stars: Vec::new(),
            hud_hp_label: None,
            hud_hp_bar: None,
            boss_hp_bg: None,
            boss_hp_bar: None,
            boss_hp_label: None,
            hud_weapon_label: None,
            hud_prompt_label: None,
            hud_center_msg: None,
            hud_killfeed_label: None,
            hud_end_label: None,
            hud_console_label: None,
            buy_bg: None,
            buy_label: None,
            scope_overlay: None,
            scope_left: None,
            scope_right: None,
            scope_dot: None,
            hurt_overlay: None,
            ch_l: None,
            ch_r: None,
            ch_u: None,
            ch_d: None,
            ui_font: None,
            anim_run_res: None,
            anim_die_res: None,
            e_was: false,
            f_was: false,
            g_was: false,
            h_was: false,
        }
    }

    fn ready(&mut self) {
        if Engine::singleton().is_editor_hint() {
            return;
        }
        let main_node = self.base().get_tree().unwrap().get_current_scene().unwrap();
        if let Some(p) = find_node_substr(&main_node, "Player").and_then(|n| n.try_cast::<PlayerController>().ok()) {
            let mut p_mut = p;
            self.cam = p_mut.try_get_node_as::<Camera3D>("Head/Camera3D");
            p_mut.bind_mut().controls_locked = true;
            self.player = Some(p_mut);
        }
        if let Some(mut pig) = find_node_substr(&main_node, "Kabanyeze") {
            if let Some(pm) = find_first_mesh(&pig.clone().upcast()) {
                let bb = pm.get_aabb();
                if bb.size.y > 0.01 {
                    self.pig_lift_y = -bb.position.y * pig.get_scale().y;
                }
            }
            let pp = pig.get_position();
            pig.set_position(Vector3::new(pp.x, self.pig_lift_y, pp.z));
            self.pig_node = Some(pig);
        }
        if let Some(sz) = find_node_substr(&main_node, "ShopZone") {
            self.shop_pos = sz.get_global_position();
        }
        if let Some(gs) = find_node_substr(&main_node, "gas") {
            self.gas_drop_pos = gs.get_global_position();
        }
        if let Some(gz) = find_node_substr(&main_node, "GunShopZone") {
            self.gunshop_pos = gz.get_global_position();
        }

        let mut rl = ResourceLoader::singleton();

        // Площадка разгрузки канистр
        let mut dzm = BoxMesh::new_gd();
        dzm.set_size(Vector3::new(4.5, 0.08, 4.5));
        let mut dzmat = StandardMaterial3D::new_gd();
        dzmat.set_albedo(Color::from_rgb(0.95, 0.75, 0.1));
        dzm.set_material(&dzmat);
        let mut gdm = MeshInstance3D::new_alloc();
        gdm.set_mesh(&dzm);
        gdm.set_position(Vector3::new(self.gas_drop_pos.x, 0.06, self.gas_drop_pos.z));
        self.base_mut().add_child(&gdm);

        // 8 канистр (3 на улице + 5 у склада)
        let can_sc = if rl.exists("res://assets/resources/gas/canister.glb") {
            rl.load("res://assets/resources/gas/canister.glb").and_then(|r| r.try_cast::<PackedScene>().ok())
        } else {
            None
        };
        let can_spots = [
            Vector3::new(8.0, 0.0, 26.0), Vector3::new(-8.0, 0.0, 26.0), Vector3::new(12.0, 0.0, 32.0),
            Vector3::new(-18.55, 0.0, 41.6), Vector3::new(-18.55, 0.0, 42.3), Vector3::new(-18.55, 0.0, 43.0),
            Vector3::new(-18.72, 0.34, 41.95), Vector3::new(-18.72, 0.34, 42.65),
        ];
        for (i, &sp) in can_spots.iter().enumerate() {
            let mut cn = if let Some(sc) = can_sc.as_ref() {
                sc.instantiate().and_then(|n| n.try_cast::<Node3D>().ok()).unwrap_or_else(Node3D::new_alloc)
            } else {
                let mut n = Node3D::new_alloc();
                let mut bm = BoxMesh::new_gd();
                bm.set_size(Vector3::new(0.55, 0.72, 0.32));
                let mut cm = StandardMaterial3D::new_gd();
                cm.set_albedo(Color::from_rgb(0.85, 0.12, 0.12));
                bm.set_material(&cm);
                let mut mi = MeshInstance3D::new_alloc();
                mi.set_mesh(&bm);
                mi.set_position(Vector3::new(0.0, 0.36, 0.0));
                n.add_child(&mi);
                n
            };
            let mut pivot = Node3D::new_alloc();
            pivot.set_position(sp);
            pivot.add_child(&cn);
            cn.set_scale(Vector3::new(0.4, 0.4, 0.4));
            let lean = if i >= 6 { 0.24 } else if i >= 3 { 0.10 } else { 0.0 };
            cn.set_rotation(Vector3::new(3.14159, 0.0, lean));
            self.base_mut().add_child(&pivot);
            let mut cbb = None;
            collect_aabb(&cn.clone().upcast(), &pivot.clone().upcast(), &mut cbb);
            if let Some(bb) = cbb {
                cn.set_position(Vector3::new(0.0, -bb.position.y, 0.0));
            }
            self.canisters.push(CanisterUnit { node: pivot, active: true, respawn_timer: 0.0 });
        }

        // Миньоны
        for i in 0..2 {
            let mname = if i == 0 { "Minion0" } else { "Minion1" };
            let mn = find_node_substr(&main_node, mname);
            self.minions.push(MinionUnit {
                node: mn,
                hp: 80,
                respawn_timer: 0.0,
                shoot_timer: 1.5 + i as f32 * 0.8,
                phase: i as f32 * 3.14,
            });
        }

        // Оружие в руках
        if let Some(mut cam) = self.cam.clone() {
            let mut wm = Node3D::new_alloc();
            wm.set_position(Vector3::new(0.28, -0.24, -0.52));
            cam.add_child(&wm);

            if let Some(we) = main_node.find_child_ex("WorldEnvironment").recursive(true).owned(false).done().and_then(|n| n.try_cast::<WorldEnvironment>().ok()) {
                if let Some(mut env) = we.get_environment() {
                    env.set_glow_enabled(true);
                    env.set_glow_intensity(0.5);
                    env.set_glow_bloom(0.0);
                    env.set_glow_hdr_bleed_threshold(1.35);
                }
            }

            let mut ml = OmniLight3D::new_alloc();
            ml.set_position(Vector3::new(0.0, 0.05, -0.95));
            ml.set_color(Color::from_rgb(1.0, 0.72, 0.30));
            ml.set_param(LightParam::ENERGY, 9.0);
            ml.set_param(LightParam::RANGE, 18.0);
            ml.set_visible(false);
            wm.add_child(&ml);
            self.muzzle_light = Some(ml);

            let mut fshader = Shader::new_gd();
            fshader.set_code("shader_type spatial;\nrender_mode blend_add, unshaded, cull_disabled, depth_draw_never;\nvoid fragment() {\n  vec2 uv = UV; float along = 1.0 - uv.y; float w = abs(uv.x - 0.5) * 2.0;\n  float shape = smoothstep(0.95, 0.05, w / (0.18 + along * 0.85)) * smoothstep(0.0, 0.12, uv.y) * smoothstep(1.0, 0.2, uv.y);\n  ALBEDO = mix(vec3(1.0,0.35,0.05), vec3(1.0,0.95,0.78), shape) * 6.0;\n  ALPHA = shape;\n}");
            let mut fmat = ShaderMaterial::new_gd();
            fmat.set_shader(&fshader);
            let mut mf = Node3D::new_alloc();
            mf.set_position(Vector3::new(0.0, 0.04, -0.78));
            mf.set_visible(false);
            wm.add_child(&mf);
            let mut qm = QuadMesh::new_gd();
            qm.set_size(Vector2::new(0.42, 0.68));
            for k in 0..3 {
                let mut pl = MeshInstance3D::new_alloc();
                pl.set_mesh(&qm);
                pl.set_material_override(&fmat);
                pl.set_rotation(Vector3::new(-1.5708, 0.0, k as f32 * 1.0472));
                pl.set_position(Vector3::new(0.0, 0.0, -0.28));
                mf.add_child(&pl);
            }
            self.muzzle_flash = Some(mf);

            // Shotgun
            if rl.exists("res://assets/resources/shotgun/Shotgun.obj") {
                if let Some(sm) = rl.load("res://assets/resources/shotgun/Shotgun.obj").and_then(|r| r.try_cast::<Mesh>().ok()) {
                    let mut mi = MeshInstance3D::new_alloc();
                    mi.set_mesh(&sm);
                    mi.set_scale(Vector3::new(0.025, 0.025, 0.025));
                    mi.set_rotation(Vector3::new(0.0, 4.71239, 0.0));
                    if rl.exists("res://assets/resources/shotgun/Sg_Diffuse.png") {
                        if let Some(sgt) = rl.load("res://assets/resources/shotgun/Sg_Diffuse.png").and_then(|r| r.try_cast::<Texture2D>().ok()) {
                            let mut sgm = StandardMaterial3D::new_gd();
                            sgm.set_texture(TextureParam::ALBEDO, &sgt);
                            mi.set_material_override(&sgm);
                        }
                    }
                    wm.add_child(&mi);
                    self.shotgun_node = Some(mi.upcast());
                }
            }

            // AK-47
            if rl.exists("res://assets/resources/ak-47/ak-47.glb") {
                if let Some(sc) = rl.load("res://assets/resources/ak-47/ak-47.glb").and_then(|r| r.try_cast::<PackedScene>().ok()) {
                    if let Some(mut raw) = sc.instantiate().and_then(|n| n.try_cast::<Node3D>().ok()) {
                        let mut piv = Node3D::new_alloc();
                        wm.add_child(&piv);
                        piv.add_child(&raw);
                        let mut bb = None;
                        collect_aabb(&raw.clone().upcast(), &piv.clone().upcast(), &mut bb);
                        if let Some(b) = bb {
                            let len = b.size.x.max(b.size.y).max(b.size.z).max(0.001);
                            let sc_f = 0.85 / len;
                            let ry = if b.size.z > b.size.x { 0.0 } else { 1.5708 };
                            raw.set_position(-(b.position + b.size * 0.5));
                            piv.set_scale(Vector3::new(sc_f, sc_f, sc_f));
                            piv.set_rotation(Vector3::new(0.0, ry, 0.0));
                        }
                        self.ak_node = Some(piv);
                    }
                }
            }

            // AWP
            let awp_path = if rl.exists("res://assets/resources/awp/awp.glb") {
                Some("res://assets/resources/awp/awp.glb")
            } else if rl.exists("res://assets/resources/ak-47/awp.glb") {
                Some("res://assets/resources/ak-47/awp.glb")
            } else {
                None
            };
            if let Some(ap_str) = awp_path {
                if let Some(sc) = rl.load(ap_str).and_then(|r| r.try_cast::<PackedScene>().ok()) {
                    if let Some(mut raw) = sc.instantiate().and_then(|n| n.try_cast::<Node3D>().ok()) {
                        let mut piv = Node3D::new_alloc();
                        wm.add_child(&piv);
                        piv.add_child(&raw);
                        let mut bb = None;
                        collect_aabb(&raw.clone().upcast(), &piv.clone().upcast(), &mut bb);
                        if let Some(b) = bb {
                            let len = b.size.x.max(b.size.y).max(b.size.z).max(0.001);
                            let sc_f = 1.425 / len;
                            let ry = if b.size.z > b.size.x { 3.14159 } else { -1.5708 };
                            raw.set_position(-(b.position + b.size * 0.5));
                            piv.set_scale(Vector3::new(sc_f, sc_f, sc_f));
                            piv.set_rotation(Vector3::new(0.0, ry, 0.0));
                        }
                        if rl.exists("res://assets/resources/awp/awp dragon lore.bmp") {
                            if let Some(dt) = rl.load("res://assets/resources/awp/awp dragon lore.bmp").and_then(|r| r.try_cast::<Texture2D>().ok()) {
                                let mut mat = StandardMaterial3D::new_gd();
                                mat.set_texture(TextureParam::ALBEDO, &dt);
                                apply_material_recursive(&raw.clone().upcast(), &mat);
                            }
                        }
                        if let Some(mut ap) = find_anim_player(&raw.clone().upcast()) {
                            let list = ap.get_animation_list();
                            for i in 0..list.len() {
                                let name = list[i].to_string();
                                if name == "RESET" { continue; }
                                if let Some(mut a) = ap.get_animation(&name) {
                                    if a.get_length() > 0.1 {
                                        a.set_loop_mode(LoopMode::NONE);
                                        self.awp_fire_anim = name;
                                        break;
                                    }
                                }
                            }
                            ap.stop();
                            self.awp_anim = Some(ap);
                        }
                        piv.set_position(Vector3::new(-0.08, 0.08, 0.0));
                        self.awp_node = Some(piv);
                    }
                }
            }

            // RPG
            if rl.exists("res://assets/resources/rpg/rpg.glb") {
                if let Some(sc) = rl.load("res://assets/resources/rpg/rpg.glb").and_then(|r| r.try_cast::<PackedScene>().ok()) {
                    if let Some(mut raw) = sc.instantiate().and_then(|n| n.try_cast::<Node3D>().ok()) {
                        let mut piv = Node3D::new_alloc();
                        wm.add_child(&piv);
                        piv.add_child(&raw);
                        let mut bb = None;
                        collect_aabb(&raw.clone().upcast(), &piv.clone().upcast(), &mut bb);
                        if let Some(b) = bb {
                            let len = b.size.x.max(b.size.y).max(b.size.z).max(0.001);
                            let sc_f = 1.15 / len;
                            let ry = if b.size.z > b.size.x { 0.0 } else { 1.5708 };
                            raw.set_position(-(b.position + b.size * 0.5));
                            piv.set_scale(Vector3::new(sc_f, sc_f, sc_f));
                            piv.set_rotation(Vector3::new(0.0, ry, 0.0));
                        }
                        self.rpg_node = Some(piv);
                    }
                }
            }
            self.weapon_mount = Some(wm);
        }

        // Маяк задания
        let mut qm = BoxMesh::new_gd();
        qm.set_size(Vector3::new(0.9, 0.9, 0.9));
        let mut qmat = StandardMaterial3D::new_gd();
        qmat.set_albedo(Color::from_rgb(0.2, 0.7, 1.0));
        qm.set_material(&qmat);
        let mut qb = MeshInstance3D::new_alloc();
        qb.set_mesh(&qm);
        self.base_mut().add_child(&qb);
        self.quest_beacon = Some(qb);

        // Звуки
        let mut audio_nodes: Vec<(Gd<AudioStreamPlayer>, bool)> = Vec::new();
        let mut mk_audio = |path: &str, play_now: bool, vol_db: f32| -> Option<Gd<AudioStreamPlayer>> {
            if !rl.exists(path) { return None; }
            let st = rl.load(path).and_then(|r| r.try_cast::<AudioStream>().ok())?;
            let mut ap = AudioStreamPlayer::new_alloc();
            ap.set_stream(&st);
            ap.set_volume_db(vol_db);
            audio_nodes.push((ap.clone(), play_now));
            Some(ap)
        };
        self.music_player = mk_audio("res://assets/sounds/kabaneze.mp3", true, -8.0);
        self.snd_hurt = mk_audio("res://assets/sounds/pig_nedovolen.mp3", false, 6.0);
        self.snd_win = mk_audio("res://assets/sounds/pig_win.mp3", false, 6.0);
        self.snd_shotgun = mk_audio("res://assets/sounds/weapon/shotgun_shot.mp3", false, 0.0)
            .or_else(|| mk_audio("res://assets/sounds/shotgun_shot.mp3", false, 0.0));
        self.snd_ak = mk_audio("res://assets/sounds/weapon/ak47_shot.mp3", false, 0.0)
            .or_else(|| mk_audio("res://assets/sounds/ak47_shot.mp3", false, 0.0));
        self.snd_awp = mk_audio("res://assets/sounds/weapon/awp_shot.mp3", false, 6.0)
            .or_else(|| mk_audio("res://assets/sounds/awp_shot.mp3", false, 6.0));
        self.snd_rpg = mk_audio("res://assets/sounds/weapon/rpg_shot.mp3", false, 0.0)
            .or_else(|| mk_audio("res://assets/sounds/rpg_shot.mp3", false, 0.0));
        self.snd_boom = mk_audio("res://assets/sounds/weapon/explosion.mp3", false, 0.0)
            .or_else(|| mk_audio("res://assets/sounds/explosion.mp3", false, 0.0));
        for (mut ap, play_now) in audio_nodes {
            self.base_mut().add_child(&ap);
            if play_now { ap.play(); }
        }

        // Сеть
        let put = HttpRequest::new_alloc();
        self.base_mut().add_child(&put);
        self.http_put = Some(put);

        let mut get = HttpRequest::new_alloc();
        self.base_mut().add_child(&get);
        let cb = self.base().callable("on_http_get_completed");
        get.connect("request_completed", &cb);
        self.http_get = Some(get);

        // Анимации бега/смерти NPC
        let mut grab_anim = |path: &str, loop_on: bool| -> Option<Gd<Animation>> {
            if !rl.exists(path) { return None; }
            let sc = rl.load(path).and_then(|r| r.try_cast::<PackedScene>().ok())?;
            let tmp = sc.instantiate()?;
            let mut out = None;
            if let Some(ap) = find_anim_player(&tmp) {
                let list = ap.get_animation_list();
                let mut best: Option<Gd<Animation>> = None;
                let mut best_len = 0.05_f32;
                for i in 0..list.len() {
                    let name = list[i].to_string();
                    if name == "RESET" { continue; }
                    if let Some(a) = ap.get_animation(&name) {
                        let len = a.get_length();
                        if len > best_len {
                            best_len = len;
                            best = Some(a);
                        }
                    }
                }
                if let Some(a) = best {
                    if let Ok(mut dup) = a.duplicate().unwrap().try_cast::<Animation>() {
                        dup.set_loop_mode(if loop_on { LoopMode::LINEAR } else { LoopMode::NONE });
                        out = Some(dup);
                    }
                }
            }
            let mut tmp = tmp;
            tmp.queue_free();
            out
        };
        self.anim_run_res = grab_anim("res://assets/resources/npc/npc_run.fbx", true);
        self.anim_die_res = grab_anim("res://assets/resources/npc/npc_die.fbx", false);

        self.load_progress();
        self.weapon_type = if self.pig_kills >= 5 { 2 } else { 1 };
        self.build_ui();
        self.setup_day();
        self.sync_leaderboard(false);
    }

    fn input(&mut self, event: Gd<InputEvent>) {
        if Engine::singleton().is_editor_hint() || self.in_menu {
            return;
        }
        let Ok(ek) = event.try_cast::<InputEventKey>() else { return; };
        if !ek.is_pressed() || ek.is_echo() {
            return;
        }
        let kc = ek.get_keycode();
        let pkc = ek.get_physical_keycode();

        if kc == Key::QUOTELEFT || pkc == Key::QUOTELEFT {
            self.console_open = !self.console_open;
            self.console_input.clear();
            if let Some(p) = self.player.as_mut() {
                p.bind_mut().controls_locked = self.console_open;
            }
            return;
        }

        if self.console_open {
            if pkc == Key::BACKSPACE {
                self.console_input.pop();
            } else if pkc == Key::ENTER || pkc == Key::KP_ENTER {
                match self.console_input.as_str() {
                    "QAZ" => {
                        self.god_mode = !self.god_mode;
                        self.cheat_msg = if self.god_mode { "ЧИТ: БЕССМЕРТИЕ ВКЛ" } else { "ЧИТ: БЕССМЕРТИЕ ВЫКЛ" }.into();
                        self.cheat_msg_t = 2.5;
                    }
                    "WSX" => {
                        self.always_crit_ak = !self.always_crit_ak;
                        if self.always_crit_ak { self.weapon_type = 2; }
                        self.cheat_msg = if self.always_crit_ak { "ЧИТ: КРИТ-КАЛАШ ВКЛ" } else { "ЧИТ: КРИТ-КАЛАШ ВЫКЛ" }.into();
                        self.cheat_msg_t = 2.5;
                    }
                    "EDC" => {
                        let nc = self.player.as_ref().map(|p| !p.bind().noclip).unwrap_or(false);
                        if let Some(p) = self.player.as_mut() { p.bind_mut().set_noclip(nc); }
                        self.cheat_msg = if nc { "ЧИТ: NOCLIP ВКЛ" } else { "ЧИТ: NOCLIP ВЫКЛ" }.into();
                        self.cheat_msg_t = 2.5;
                    }
                    "RFV" => {
                        self.has_awp = true;
                        self.weapon_type = 3;
                        self.cheat_msg = "ЧИТ: ПОЛУЧЕНА AWP".into();
                        self.cheat_msg_t = 2.5;
                    }
                    "TGB" => {
                        self.has_rpg = true;
                        self.weapon_type = 4;
                        self.cheat_msg = "ЧИТ: ПОЛУЧЕНА БАЗУКА (РПГ)".into();
                        self.cheat_msg_t = 2.5;
                    }
                    _ => {}
                }
                self.console_open = false;
                self.console_input.clear();
                if let Some(p) = self.player.as_mut() {
                    p.bind_mut().controls_locked = false;
                }
            } else if pkc.ord() >= Key::A.ord() && pkc.ord() <= Key::Z.ord() && self.console_input.len() < 16 {
                let ch = (b'A' + (pkc.ord() - Key::A.ord()) as u8) as char;
                self.console_input.push(ch);
            }
            return;
        }

        if kc == Key::ESCAPE {
            self.in_menu = true;
            if let Some(m) = self.menu_panel.as_mut() { m.set_visible(true); }
            if let Some(h) = self.hud_panel.as_mut() { h.set_visible(false); }
            if let Some(p) = self.player.as_mut() { p.bind_mut().controls_locked = true; }
            Input::singleton().set_mouse_mode(MouseMode::VISIBLE);
            if let Some(mp) = self.music_player.as_mut() {
                if !mp.is_playing() { mp.play(); }
            }
            return;
        }

        if pkc == Key::B {
            let near = self.player.as_ref().map(|p| dist_xz(p.get_global_position(), self.gunshop_pos) < 3.65).unwrap_or(false);
            if near {
                self.buy_menu_open = !self.buy_menu_open;
            } else {
                self.buy_menu_open = false;
                self.warn_msg = "ОРУЖЕЙКА на востоке города! Подойди к магазину оружия.".into();
                self.warn_msg_t = 2.5;
            }
            return;
        }

        if self.buy_menu_open {
            if kc == Key::KEY_1 { self.weapon_type = 1; self.buy_menu_open = false; }
            if kc == Key::KEY_2 {
                if self.has_ak || self.pig_kills >= 5 || self.always_crit_ak {
                    self.weapon_type = 2; self.buy_menu_open = false;
                } else if self.elo >= 1500 {
                    self.elo -= 1500; self.has_ak = true; self.weapon_type = 2; self.save_progress();
                    self.buy_menu_open = false; self.shop_msg = "Куплен АК-47!".into(); self.shop_msg_t = 2.5;
                } else {
                    self.shop_msg = "Нужно 1500 ELO!".into(); self.shop_msg_t = 2.0;
                }
            }
            if kc == Key::KEY_3 {
                if self.has_awp {
                    self.weapon_type = 3; self.buy_menu_open = false;
                } else if self.elo >= 25000 {
                    self.elo -= 25000; self.has_awp = true; self.weapon_type = 3; self.save_progress();
                    self.buy_menu_open = false; self.shop_msg = "Куплена AWP!".into(); self.shop_msg_t = 2.5;
                } else {
                    self.shop_msg = "Нужно 25000 ELO!".into(); self.shop_msg_t = 2.0;
                }
            }
            if kc == Key::KEY_4 {
                if self.has_rpg {
                    self.weapon_type = 4; self.buy_menu_open = false;
                } else if self.elo >= 40000 {
                    self.elo -= 40000; self.has_rpg = true; self.weapon_type = 4; self.save_progress();
                    self.buy_menu_open = false; self.shop_msg = "Куплен РПГ!".into(); self.shop_msg_t = 2.5;
                } else {
                    self.shop_msg = "Нужно 40000 ELO!".into(); self.shop_msg_t = 2.0;
                }
            }
            return;
        }

        if kc == Key::KEY_1 { self.weapon_type = 1; }
        if kc == Key::KEY_2 && (self.has_ak || self.pig_kills >= 5 || self.always_crit_ak) { self.weapon_type = 2; }
        if kc == Key::KEY_3 && self.has_awp { self.weapon_type = 3; }
        if kc == Key::KEY_4 && self.has_rpg { self.weapon_type = 4; }
        if kc == Key::R && (self.pig_hp <= 0 || self.player_hp <= 0) {
            self.reset_game();
        }
    }

    fn process(&mut self, delta: f64) {
        if Engine::singleton().is_editor_hint() || self.in_menu || self.player.is_none() {
            return;
        }
        let dt = delta as f32;
        self.sim_time += dt;
        self.ahmed_msg_t = (self.ahmed_msg_t - dt).max(0.0);
        self.shop_msg_t = (self.shop_msg_t - dt).max(0.0);
        self.day_banner_t = (self.day_banner_t - dt).max(0.0);
        self.warn_msg_t = (self.warn_msg_t - dt).max(0.0);
        self.cheat_msg_t = (self.cheat_msg_t - dt).max(0.0);
        self.shoot_cd = (self.shoot_cd - dt).max(0.0);
        self.recoil = (self.recoil - dt * if self.weapon_type == 2 { 10.0 } else { 6.0 }).max(0.0);
        self.spread_kick = (self.spread_kick - dt * 3.5).max(0.0);
        self.hit_marker = (self.hit_marker - dt).max(0.0);
        self.hurt_flash = (self.hurt_flash - dt).max(0.0);

        let inp = Input::singleton();
        let scoped = self.weapon_type == 3 && !self.console_open && !self.buy_menu_open && inp.is_mouse_button_pressed(MouseButton::RIGHT);

        if let Some(n) = self.shotgun_node.as_mut() { n.set_visible(self.weapon_type == 1); }
        if let Some(n) = self.ak_node.as_mut() { n.set_visible(self.weapon_type == 2); }
        if let Some(n) = self.awp_node.as_mut() { n.set_visible(self.weapon_type == 3 && !scoped); }
        if let Some(n) = self.rpg_node.as_mut() { n.set_visible(self.weapon_type == 4); }
        if let Some(n) = self.scope_overlay.as_mut() { n.set_visible(scoped); }
        if let Some(n) = self.scope_left.as_mut() { n.set_visible(scoped); }
        if let Some(n) = self.scope_right.as_mut() { n.set_visible(scoped); }
        if let Some(n) = self.scope_dot.as_mut() { n.set_visible(scoped); }
        if let Some(cam) = self.cam.as_mut() {
            let tfov = if scoped { 18.0 } else { 75.0 };
            let cur = cam.get_fov();
            cam.set_fov(cur + (tfov - cur) * (dt * 12.0).min(1.0));
        }

        // Канистры
        for c in &mut self.canisters {
            if !c.active {
                c.respawn_timer -= dt;
                if c.respawn_timer <= 0.0 {
                    c.active = true;
                    c.node.set_visible(true);
                }
            }
        }

        // Дым и ракеты
        let mut i = self.smokes.len();
        while i > 0 {
            i -= 1;
            self.smokes[i].life -= dt;
            if self.smokes[i].life <= 0.0 {
                self.smokes[i].mesh.queue_free();
                self.smokes.remove(i);
                continue;
            }
            let p = 1.0 - (self.smokes[i].life / self.smokes[i].max_life);
            let sc = self.smokes[i].start_scale * (1.0 + p * 2.2);
            self.smokes[i].mesh.set_scale(Vector3::new(sc, sc, sc));
            let pos = self.smokes[i].mesh.get_position() + Vector3::new(0.0, 0.35 * dt, 0.0);
            self.smokes[i].mesh.set_position(pos);
            self.smokes[i].mat.set_albedo(Color::from_rgba(0.84, 0.84, 0.86, 0.55 * (1.0 - p)));
        }

        let mut explosions: Vec<Vector3> = Vec::new();
        let mut i = self.rockets.len();
        while i > 0 {
            i -= 1;
            let prev_p = self.rockets[i].mesh.get_position();
            let np = prev_p + self.rockets[i].vel * dt;
            self.rockets[i].mesh.set_position(np);
            self.rockets[i].life -= dt;
            self.add_smoke_puff(prev_p, 0.45, 2.0);
            self.add_smoke_puff((prev_p + np) * 0.5, 0.40, 2.0);
            let hit_pig = self.pig_hp > 0 && self.pig_node.as_ref().map(|p| np.distance_to(p.get_global_position() + Vector3::new(0.0, 1.3, 0.0)) < 2.0).unwrap_or(false);
            if hit_pig || np.y < 0.2 || self.rockets[i].life <= 0.0 {
                explosions.push(np);
                self.rockets[i].mesh.queue_free();
                self.rockets.remove(i);
            }
        }
        for ep in explosions {
            self.explode_at(ep, false);
        }

        // Вспышка и отдача
        self.flash_timer = (self.flash_timer - dt).max(0.0);
        if let Some(ml) = self.muzzle_light.as_mut() { ml.set_visible(self.flash_timer > 0.0); }
        let flash_sc = if self.flash_timer > 0.0 { 0.85 + self.rng_range(55) as f32 / 100.0 } else { 1.0 };
        let flash_rz = if self.flash_timer > 0.0 { self.rng_range(628) as f32 / 100.0 } else { 0.0 };
        if let Some(mf) = self.muzzle_flash.as_mut() {
            mf.set_visible(self.flash_timer > 0.0);
            if self.flash_timer > 0.0 {
                mf.set_scale(Vector3::new(flash_sc, flash_sc, flash_sc * 1.25));
                mf.set_rotation(Vector3::new(0.0, 0.0, flash_rz));
            }
        }
        if let Some(wm) = self.weapon_mount.as_mut() {
            wm.set_position(Vector3::new(0.28, -0.24 + self.recoil * 0.03, -0.52 + self.recoil * 0.07));
            wm.set_rotation(Vector3::new(self.recoil * 0.12, 0.0, 0.0));
        }
        if let Some(qb) = self.quest_beacon.as_mut() {
            qb.set_visible(self.quest_stage == 1 && !self.has_quest_item);
            qb.rotate_y(dt * 2.0);
        }

        let ppos = self.player.as_ref().unwrap().get_global_position();
        let player_rid = self.player.as_ref().unwrap().get_rid();
        let ai_space = self.player.as_ref().unwrap().get_world_3d().and_then(|mut w| w.get_direct_space_state());

        let e_now = inp.is_key_pressed(Key::E);
        let f_now = inp.is_key_pressed(Key::F);
        let g_now = inp.is_key_pressed(Key::G);
        let h_now = inp.is_key_pressed(Key::H);

        if e_now && !self.e_was {
            if dist_xz(ppos, self.ahmed_pos) < 5.0 {
                if self.quest_stage == 0 {
                    self.quest_stage = 1;
                    if !self.errands.is_empty() { self.errands[0].done = true; }
                    self.ahmed_msg = "Дядя Ахмед: Брат, выполни дело дня и завали Кабаньезе!".into();
                    self.ahmed_msg_t = 4.5;
                } else if self.quest_stage == 1 {
                    self.ahmed_msg = if self.has_quest_item { "Дядя Ахмед: Вали Кабаньезе на арене!" } else { "Дядя Ахмед: Сперва забери голубой маяк дела дня!" }.into();
                    self.ahmed_msg_t = 3.5;
                } else if self.quest_stage == 2 {
                    self.quest_stage = 3;
                    self.elo += 300;
                    self.save_progress();
                    self.ahmed_msg = "Дядя Ахмед: Красава! +300 ELO! Жми [R] для нового дня.".into();
                    self.ahmed_msg_t = 4.5;
                }
            } else if !self.carrying_canister {
                for c in &mut self.canisters {
                    if c.active && ppos.distance_to(c.node.get_global_position()) < 3.5 {
                        c.active = false;
                        c.respawn_timer = 4.0;
                        c.node.set_visible(false);
                        self.carrying_canister = true;
                        self.shop_msg = "Взята канистра! Неси на жёлтую зону заправки!".into();
                        self.shop_msg_t = 3.0;
                        break;
                    }
                }
                if !self.carrying_canister && self.quest_stage == 1 && !self.has_quest_item && dist_xz(ppos, self.quest_item_pos) < 4.5 {
                    self.has_quest_item = true;
                    if self.errands.len() > 1 { self.errands[1].done = true; }
                    self.elo += 150;
                    self.save_progress();
                    self.shop_msg = "Дело дня выполнено (+150 ELO)! Пора к Кабаньезе!".into();
                    self.shop_msg_t = 3.0;
                }
            }
        }
        if self.carrying_canister && dist_xz(ppos, self.gas_drop_pos) < 5.5 {
            self.carrying_canister = false;
            self.elo += 120;
            self.save_progress();
            self.shop_msg = "Канистра доставлена на заправку! +120 ELO!".into();
            self.shop_msg_t = 2.5;
        }
        if dist_xz(ppos, self.shop_pos) < 2.5 {
            if f_now && !self.f_was {
                if self.elo >= 250 && self.player_hp < 150 {
                    self.elo -= 250;
                    self.player_hp = (self.player_hp + 50).min(150);
                    self.save_progress();
                    self.shop_msg = "Куплена шаурма: +50 HP!".into();
                } else {
                    self.shop_msg = "Мало ELO или HP максимум!".into();
                }
                self.shop_msg_t = 2.5;
            }
            if g_now && !self.g_was {
                if !self.has_dmg_buff && self.elo >= 500 {
                    self.elo -= 500; self.has_dmg_buff = true; self.save_progress();
                    self.shop_msg = "Бронебойные пули x2 урон!".into();
                } else {
                    self.shop_msg = "Уже куплено или нужно 500 ELO!".into();
                }
                self.shop_msg_t = 2.5;
            }
            if h_now && !self.h_was {
                if !self.has_speed_buff && self.elo >= 400 {
                    self.elo -= 400; self.has_speed_buff = true;
                    if let Some(p) = self.player.as_mut() { p.bind_mut().speed_mult = 1.35; }
                    self.save_progress();
                    self.shop_msg = "Скороходы +35% скорости!".into();
                } else {
                    self.shop_msg = "Уже куплено или нужно 400 ELO!".into();
                }
                self.shop_msg_t = 2.5;
            }
        }
        self.e_was = e_now;
        self.f_was = f_now;
        self.g_was = g_now;
        self.h_was = h_now;

        if !self.console_open && self.player_hp > 0 && inp.is_mouse_button_pressed(MouseButton::LEFT) && self.shoot_cd <= 0.0 {
            self.handle_shoot();
        }

        // Босс Кабаньезе
        let pig_spd = if self.day_idx == 1 || self.day_idx == 4 { 1.45 } else { 1.0 };
        if self.pig_hp > 0 {
            self.pig_yaw += 0.95 * pig_spd * dt;
            let px = (self.sim_time * 0.65 * pig_spd).sin() * 3.8;
            let pz = -24.0 + (self.sim_time * 0.45 * pig_spd).cos() * 3.2;
            let py = if self.panic_timer > 0.0 {
                self.panic_timer -= dt;
                (self.sim_time * 11.0).sin().abs() * 1.45
            } else { 0.0 };
            if let Some(pig) = self.pig_node.as_mut() {
                pig.set_position(Vector3::new(px, py + self.pig_lift_y, pz));
                pig.set_rotation(Vector3::new(0.0, self.pig_yaw, 0.0));
            }
            if self.player_hp > 0 && self.panic_timer > 0.0 {
                self.spray_timer -= dt;
                if self.spray_timer <= 0.0 {
                    self.spray_timer = 0.055;
                    let pig_fwd = Vector3::new(self.pig_yaw.sin(), 0.0, self.pig_yaw.cos());
                    let anus = Vector3::new(px, py + 1.2, pz) - pig_fwd * 1.55;
                    let base_dir = (ppos + Vector3::new(0.0, 1.4, 0.0) - anus).normalized();
                    for _ in 0..2 {
                        let sdir = Vector3::new(
                            -pig_fwd.x * 0.45 + base_dir.x * 0.7 + (self.rng_range(100) - 50) as f32 / 130.0,
                            0.14,
                            -pig_fwd.z * 0.45 + base_dir.z * 0.7 + (self.rng_range(100) - 50) as f32 / 130.0,
                        ).normalized();
                        let spd = 11.0 + self.rng_range(5) as f32;
                        self.spawn_blob(anus, sdir * spd, 2.6, false, Color::from_rgb(0.38, 0.22, 0.08), 0);
                    }
                }
            }
        }

        // Миньоны
        if self.pig_hp > 0 && self.player_hp > 0 {
            let mut m_shots = Vec::new();
            for m in &mut self.minions {
                let Some(node) = m.node.as_mut() else { continue; };
                if m.hp <= 0 {
                    m.respawn_timer -= dt;
                    if m.respawn_timer <= 0.0 {
                        m.hp = 80;
                        node.set_visible(true);
                    }
                    continue;
                }
                let mp = Vector3::new(
                    (self.sim_time * 1.15 + m.phase).sin() * 4.5,
                    1.1,
                    -24.0 + (self.sim_time * 0.95 + m.phase).cos() * 4.2,
                );
                node.set_position(mp);
                if ppos.z < 2.0 {
                    m.shoot_timer -= dt;
                    if m.shoot_timer <= 0.0 {
                        m.shoot_timer = 1.8;
                        let d = (ppos + Vector3::new(0.0, 1.3, 0.0) - mp).normalized();
                        m_shots.push((mp, d * 20.0));
                    }
                }
            }
            for (mp, vel) in m_shots {
                self.spawn_blob(mp, vel, 3.5, false, Color::from_rgb(0.5, 0.9, 0.15), 12);
            }
        }

        // Розыск
        if self.wanted_level > 0 {
            let cops = self.npcs.iter().filter(|n| n.hp > 0 && n.role == 2).count() as i32;
            if cops < self.wanted_level * 2 && self.rng_range(120) == 0 {
                self.spawn_npc(Vector3::new(25.0, 0.0, 34.0), 2);
            }
            let in_barn = (ppos.x - 14.5).abs() < 2.7 && (ppos.z + 10.0).abs() < 3.0;
            self.wanted_decay -= dt * if in_barn { 2.5 } else { 1.0 };
            if self.wanted_decay <= 0.0 {
                self.wanted_level -= 1;
                self.wanted_decay = 12.0;
            }
        }

        // Лужи крови и оторванные куски
        for pd in &mut self.puddles {
            pd.age += dt;
            let grow = (pd.age / 0.65).min(1.0);
            let sc = 0.25 + (pd.target_r - 0.25) * grow;
            pd.mesh.set_scale(Vector3::new(sc, 1.0, sc));
            if pd.age > 0.65 {
                let d_t = ((pd.age - 0.65) / 4.0).min(1.0);
                if pd.on_asphalt {
                    pd.mat.set_albedo(Color::from_rgba(0.62 - 0.50 * d_t, 0.02 - 0.01 * d_t, 0.01, 0.92 + 0.05 * d_t));
                } else {
                    pd.mat.set_albedo(Color::from_rgba(0.58 - 0.44 * d_t, 0.02 + 0.01 * d_t, 0.02, 0.85 - 0.45 * d_t));
                }
            }
        }

        let mut new_puddles = Vec::new();
        let mut i = self.gore_chunks.len();
        while i > 0 {
            i -= 1;
            self.gore_chunks[i].life -= dt;
            if self.gore_chunks[i].life <= 0.0 {
                self.gore_chunks[i].node.queue_free();
                self.gore_chunks.remove(i);
                continue;
            }
            if !self.gore_chunks[i].landed {
                self.gore_chunks[i].vel.y -= 16.0 * dt;
                let mut gp = self.gore_chunks[i].node.get_position() + self.gore_chunks[i].vel * dt;
                let rot = self.gore_chunks[i].node.get_rotation() + self.gore_chunks[i].ang_vel * dt;
                self.gore_chunks[i].node.set_rotation(rot);
                if gp.y <= 0.18 {
                    gp.y = 0.18;
                    if self.gore_chunks[i].vel.y.abs() > 3.0 {
                        self.gore_chunks[i].vel.y = -self.gore_chunks[i].vel.y * 0.35;
                        self.gore_chunks[i].vel.x *= 0.55;
                        self.gore_chunks[i].vel.z *= 0.55;
                        new_puddles.push((gp, 1.1));
                    } else {
                        self.gore_chunks[i].vel = Vector3::ZERO;
                        self.gore_chunks[i].ang_vel = Vector3::ZERO;
                        self.gore_chunks[i].landed = true;
                        new_puddles.push((gp, 1.3));
                    }
                }
                self.gore_chunks[i].node.set_position(gp);
            }
        }
        for (p, r) in new_puddles {
            self.spawn_blood_puddle(p, r);
        }

        // Обновление NPC (обзор, слух, обход стен, стрельба)
        let mut cop_shots = Vec::new();
        let wanted = self.wanted_level;
        let player_hp = self.player_hp;
        let mut rng_val = self.rng_state;
        let mut next_rand = |n: i32| -> i32 {
            rng_val = rng_val.wrapping_mul(1664525).wrapping_add(1013904223);
            ((rng_val >> 16) as i32).abs() % n
        };

        for n in &mut self.npcs {
            if n.hp <= 0 {
                if n.has_death_vel && n.root.is_visible() {
                    n.death_vel.y -= 18.0 * dt;
                    let mut p = n.root.get_position() + n.death_vel * dt;
                    if p.y <= 0.0 {
                        p.y = 0.0;
                        n.death_vel.y = 0.0;
                        n.death_vel.x *= (1.0 - 6.0 * dt).max(0.0);
                        n.death_vel.z *= (1.0 - 6.0 * dt).max(0.0);
                        if Vector2::new(n.death_vel.x, n.death_vel.z).length() < 0.2 {
                            n.has_death_vel = false;
                        }
                    }
                    n.root.set_position(p);
                }
                continue;
            }

            let np = n.root.get_position();
            let mut to_p = ppos - np;
            to_p.y = 0.0;
            let d = to_p.length();

            let mut sees_player = false;
            if player_hp > 0 && d < 42.0 {
                let to_dir = if d > 0.01 { to_p / d } else { n.dir };
                if n.dir.dot(to_dir) > 0.25 || d < 3.0 {
                    if let Some(mut sp) = ai_space.clone() {
                        if let Some(mut q) = PhysicsRayQueryParameters3D::create(np + Vector3::new(0.0, 1.55, 0.0), ppos + Vector3::new(0.0, 1.40, 0.0)) {
                            let mut excl = Array::new();
                            excl.push(player_rid);
                            q.set_exclude(&excl);
                            if sp.intersect_ray(&q).is_empty() {
                                sees_player = true;
                            }
                        }
                    } else {
                        sees_player = true;
                    }
                }
            }

            let try_move = |root: &mut Gd<Node3D>, step: Vector3| -> bool {
                if step.length_squared() < 1e-6 { return false; }
                let cur = root.get_position();
                let nxt = cur + step;
                if nxt.x < -58.0 || nxt.x > 58.0 || nxt.z < -44.0 || nxt.z > 58.0 { return false; }
                if let Some(mut sp) = ai_space.clone() {
                    let dir_n = step.normalized();
                    if let Some(mut wq) = PhysicsRayQueryParameters3D::create(cur + Vector3::new(0.0, 1.0, 0.0), nxt + dir_n * 0.55 + Vector3::new(0.0, 1.0, 0.0)) {
                        let mut excl = Array::new();
                        excl.push(player_rid);
                        wq.set_exclude(&excl);
                        if !sp.intersect_ray(&wq).is_empty() {
                            return false;
                        }
                    }
                }
                root.set_position(nxt);
                true
            };

            if n.role == 0 {
                n.panic_t = (n.panic_t - dt).max(0.0);
                let panicking = n.panic_t > 0.0 || wanted > 0;
                n.timer -= dt;
                if n.timer <= 0.0 {
                    n.timer = if panicking { 1.2 + next_rand(2) as f32 } else { 2.5 + next_rand(3) as f32 };
                    n.dir = Vector3::new((next_rand(200) - 100) as f32, 0.0, (next_rand(200) - 100) as f32).normalized();
                }
                if let Some(ap) = n.anim.as_mut() {
                    if panicking {
                        if ap.has_animation("Running") {
                            ap.set_speed_scale(1.2);
                            if ap.get_current_animation() != "Running".into() || !ap.is_playing() {
                                ap.play_ex().name("Running").done();
                            }
                        } else if !n.walk_anim.is_empty() {
                            ap.set_speed_scale(2.2);
                            if ap.get_current_animation() != n.walk_anim.clone().into() || !ap.is_playing() {
                                ap.play_ex().name(&n.walk_anim).done();
                            }
                        }
                    } else if !n.walk_anim.is_empty() {
                        ap.set_speed_scale(1.0);
                        if ap.get_current_animation() != n.walk_anim.clone().into() || !ap.is_playing() {
                            ap.play_ex().name(&n.walk_anim).done();
                        }
                    }
                }
                let spd = if panicking { 4.2 } else { 1.7 } * dt;
                if !try_move(&mut n.root, n.dir * spd) {
                    n.dir = Vector3::new(-n.dir.z, 0.0, n.dir.x).normalized();
                    if !try_move(&mut n.root, n.dir * spd) { n.dir = -n.dir; }
                }
            } else if n.role == 2 && player_hp > 0 {
                if sees_player { n.alert_timer = 4.0; } else { n.alert_timer = (n.alert_timer - dt).max(0.0); }
                if let Some(ap) = n.anim.as_mut() {
                    if !ap.is_playing() && ap.has_animation("Running") {
                        ap.play_ex().name("Running").done();
                    }
                }
                if n.alert_timer > 0.0 {
                    if d > 5.5 {
                        let want = to_p.normalized();
                        if !try_move(&mut n.root, want * (4.8 * dt)) {
                            let side = Vector3::new(-want.z, 0.0, want.x);
                            if !try_move(&mut n.root, side * (4.8 * dt)) {
                                try_move(&mut n.root, -side * (4.8 * dt));
                            }
                        }
                        n.dir = want;
                    } else {
                        let strafe = Vector3::new(-to_p.z, 0.0, to_p.x);
                        if strafe.length_squared() > 0.001 {
                            let st = strafe.normalized();
                            try_move(&mut n.root, st * (3.2 * dt));
                            n.dir = st;
                        }
                    }
                    n.shoot_cd -= dt;
                    if sees_player && n.shoot_cd <= 0.0 && d < 28.0 {
                        n.shoot_cd = 1.4;
                        let org = np + Vector3::new(0.0, 1.4, 0.0);
                        cop_shots.push((org, (ppos + Vector3::new(0.0, 1.3, 0.0) - org).normalized() * 24.0));
                    }
                } else {
                    n.timer -= dt;
                    if n.timer <= 0.0 {
                        n.timer = 2.5 + next_rand(3) as f32;
                        n.dir = Vector3::new((next_rand(200) - 100) as f32, 0.0, (next_rand(200) - 100) as f32).normalized();
                    }
                    if !try_move(&mut n.root, n.dir * (4.2 * dt)) {
                        n.dir = Vector3::new(-n.dir.z, 0.0, n.dir.x).normalized();
                        if !try_move(&mut n.root, n.dir * (4.2 * dt)) { n.dir = -n.dir; }
                    }
                }
            }
            n.root.set_rotation(Vector3::new(0.0, n.dir.x.atan2(n.dir.z), 0.0));
        }
        self.rng_state = rng_val;
        for (org, vel) in cop_shots {
            self.spawn_blob(org, vel, 2.5, false, Color::from_rgb(0.3, 0.7, 1.0), 8);
        }

        // Снаряды (дрист и пули)
        let mut i = self.blobs.len();
        while i > 0 {
            i -= 1;
            self.blobs[i].vel.y -= 7.5 * dt;
            let prev_bp = self.blobs[i].mesh.get_position();
            let bp = prev_bp + self.blobs[i].vel * dt;
            self.blobs[i].mesh.set_position(bp);
            self.blobs[i].life -= dt;

            let mut hit_wall = false;
            if !self.blobs[i].is_chunk {
                if let Some(mut sp) = ai_space.clone() {
                    if let Some(mut bq) = PhysicsRayQueryParameters3D::create(prev_bp, bp) {
                        let mut excl = Array::new();
                        excl.push(player_rid);
                        bq.set_exclude(&excl);
                        if !sp.intersect_ray(&bq).is_empty() {
                            hit_wall = true;
                        }
                    }
                }
            }
            if hit_wall || bp.y < 0.05 || self.blobs[i].life <= 0.0 {
                self.blobs[i].mesh.queue_free();
                self.blobs.remove(i);
                continue;
            }
            if !self.blobs[i].is_chunk && self.player_hp > 0 && bp.distance_to(ppos + Vector3::new(0.0, 1.3, 0.0)) < 1.05 {
                let pdmg = if self.god_mode { 0 } else if self.blobs[i].dmg > 0 { self.blobs[i].dmg } else if self.day_idx >= 3 { 14 } else { 8 };
                self.player_hp = (self.player_hp - pdmg).max(0);
                self.hurt_flash = 0.25;
                self.blobs[i].mesh.queue_free();
                self.blobs.remove(i);
                if self.player_hp == 0 && !self.round_finished {
                    self.round_finished = true;
                    if let Some(sw) = self.snd_win.as_mut() { sw.play(); }
                    self.last_elo_delta = -350;
                    self.elo = (self.elo + self.last_elo_delta).max(0);
                    self.save_progress();
                    self.sync_leaderboard(true);
                }
            }
        }

        let mut i = self.kill_feed.len();
        while i > 0 {
            i -= 1;
            self.kill_feed[i].life -= dt;
            if self.kill_feed[i].life <= 0.0 {
                self.kill_feed.remove(i);
            }
        }

        self.update_hud();
    }
}

#[godot_api]
impl GameManager {
    #[func]
    fn on_play_pressed(&mut self) {
        if let Some(ne) = self.nick_edit.as_ref() {
            let t = ne.get_text().to_string().trim().to_string();
            self.player_nick = if t.is_empty() { "Player".into() } else { t };
        }
        self.save_progress();
        self.in_menu = false;
        if let Some(m) = self.menu_panel.as_mut() { m.set_visible(false); }
        if let Some(h) = self.hud_panel.as_mut() { h.set_visible(true); }
        if let Some(p) = self.player.as_mut() { p.bind_mut().controls_locked = false; }
        Input::singleton().set_mouse_mode(MouseMode::CAPTURED);
        self.day_banner_t = 3.5;
        if let Some(mp) = self.music_player.as_mut() { mp.stop(); }
    }

    #[func]
    fn on_sync_pressed(&mut self) {
        if let Some(ne) = self.nick_edit.as_ref() {
            let t = ne.get_text().to_string().trim().to_string();
            self.player_nick = if t.is_empty() { "Player".into() } else { t };
        }
        self.save_progress();
        self.sync_leaderboard(true);
    }

    #[func]
    fn on_exit_pressed(&mut self) {
        self.base().get_tree().unwrap().quit();
    }

    #[func]
    fn on_http_get_completed(&mut self, _res: i32, code: i32, _hdrs: PackedStringArray, body: PackedByteArray) {
        if code != 200 { return; }
        let txt = body.get_string_from_utf8();
        let v = Json::parse_string(&txt);
        let Ok(dict) = v.try_to::<Dictionary>() else { return; };
        let mut rows = Vec::new();
        for (_, val) in dict.iter_shared() {
            if let Ok(row) = val.try_to::<Dictionary>() {
                let name = row.get("name").map(|x| x.to_string()).unwrap_or_else(|| "Player".into());
                let elo = row.get("elo").and_then(|x| x.try_to::<f64>().ok().or_else(|| x.try_to::<i64>().ok().map(|v| v as f64))).unwrap_or(0.0) as i32;
                let kills = row.get("kills").and_then(|x| x.try_to::<f64>().ok().or_else(|| x.try_to::<i64>().ok().map(|v| v as f64))).unwrap_or(0.0) as i32;
                rows.push(LeaderRow { name, elo, kills });
            }
        }
        rows.sort_by(|a, b| b.elo.cmp(&a.elo));
        let mut out = String::from("ОНЛАЙН ТОП ELO:\n");
        for (i, r) in rows.iter().take(7).enumerate() {
            out.push_str(&format!("{}. {} — {} ELO ({})\n", i + 1, r.name, r.elo, r.kills));
        }
        if let Some(lbl) = self.lb_label.as_mut() {
            lbl.set_text(&out);
        }
    }
}

impl GameManager {
    fn rng_range(&mut self, n: i32) -> i32 {
        self.rng_state = self.rng_state.wrapping_mul(1664525).wrapping_add(1013904223);
        ((self.rng_state >> 16) as i32).abs() % n
    }

    fn load_progress(&mut self) {
        if FileAccess::file_exists("user://elo.dat") {
            if let Some(mut f) = FileAccess::open("user://elo.dat", ModeFlags::READ) {
                self.elo = f.get_32() as i32;
                self.pig_kills = f.get_32() as i32;
                let n = f.get_pascal_string().to_string();
                if !n.is_empty() { self.player_nick = n; }
                if !f.eof_reached() {
                    self.has_ak = f.get_8() != 0;
                    self.has_awp = f.get_8() != 0;
                    self.has_rpg = f.get_8() != 0;
                }
            }
        }
    }

    fn save_progress(&self) {
        if let Some(mut f) = FileAccess::open("user://elo.dat", ModeFlags::WRITE) {
            f.store_32(self.elo as u32);
            f.store_32(self.pig_kills as u32);
            f.store_pascal_string(&self.player_nick);
            f.store_8(if self.has_ak { 1 } else { 0 });
            f.store_8(if self.has_awp { 1 } else { 0 });
            f.store_8(if self.has_rpg { 1 } else { 0 });
        }
    }

    fn sync_leaderboard(&mut self, upload: bool) {
        if upload && !self.player_nick.is_empty() {
            if let Some(put) = self.http_put.as_mut() {
                let url = format!("{}/leaderboard/{}.json", FB_URL, self.player_nick);
                let body = format!("{{\"name\":\"{}\",\"elo\":{},\"kills\":{}}}", self.player_nick, self.elo, self.pig_kills);
                put.request_ex(&url).method(Method::PUT).request_data(&body).done();
            }
        }
        if let Some(get) = self.http_get.as_mut() {
            let url = format!("{}/leaderboard.json", FB_URL);
            get.request(&url);
        }
    }

    fn build_ui(&mut self) {
        let mut rl = ResourceLoader::singleton();
        if rl.exists("res://assets/fonts/font.ttf") {
            self.ui_font = rl.load("res://assets/fonts/font.ttf").and_then(|r| r.try_cast::<Font>().ok());
        }
        let mut ul = CanvasLayer::new_alloc();
        self.base_mut().add_child(&ul);

        let style_lbl = |l: &mut Gd<Label>, sz: i32, col: Color, font: &Option<Gd<Font>>| {
            if let Some(f) = font { l.add_theme_font_override("font", f); }
            l.add_theme_font_size_override("font_size", sz);
            l.add_theme_color_override("font_color", col);
        };

        // Меню
        let mut mp = Control::new_alloc();
        mp.set_anchors_preset(LayoutPreset::FULL_RECT);
        ul.add_child(&mp);

        if rl.exists("res://assets/images/menu_bg.jpg") {
            if let Some(bt) = rl.load("res://assets/images/menu_bg.jpg").and_then(|r| r.try_cast::<Texture2D>().ok()) {
                let mut bg = TextureRect::new_alloc();
                bg.set_texture(&bt);
                bg.set_anchors_preset(LayoutPreset::FULL_RECT);
                bg.set_expand_mode(ExpandMode::IGNORE_SIZE);
                mp.add_child(&bg);
            }
        }
        let mut dim = ColorRect::new_alloc();
        dim.set_anchors_preset(LayoutPreset::FULL_RECT);
        dim.set_color(Color::from_rgba(0.0, 0.0, 0.0, 0.45));
        mp.add_child(&dim);

        let mut box_l = ColorRect::new_alloc();
        box_l.set_position(Vector2::new(60.0, 140.0));
        box_l.set_size(Vector2::new(540.0, 350.0));
        box_l.set_color(Color::from_rgba(0.06, 0.05, 0.04, 0.85));
        mp.add_child(&box_l);

        let mut title = Label::new_alloc();
        title.set_text("КАБАНЬЕЗЕ 3D: RUST EDITION");
        title.set_position(Vector2::new(85.0, 165.0));
        style_lbl(&mut title, 30, Color::from_rgb(1.0, 0.85, 0.45), &self.ui_font);
        mp.add_child(&title);

        let mut melo = Label::new_alloc();
        melo.set_position(Vector2::new(85.0, 210.0));
        style_lbl(&mut melo, 20, Color::from_rgb(0.4, 0.8, 1.0), &self.ui_font);
        mp.add_child(&melo);
        self.menu_elo_label = Some(melo);

        let mut ne = LineEdit::new_alloc();
        ne.set_position(Vector2::new(85.0, 255.0));
        ne.set_size(Vector2::new(260.0, 38.0));
        ne.set_text(&self.player_nick);
        mp.add_child(&ne);
        self.nick_edit = Some(ne);

        let mut bp = Button::new_alloc();
        bp.set_text("ИГРАТЬ");
        bp.set_position(Vector2::new(85.0, 310.0));
        bp.set_size(Vector2::new(260.0, 56.0));
        bp.connect("pressed", &self.base().callable("on_play_pressed"));
        mp.add_child(&bp);

        let mut be = Button::new_alloc();
        be.set_text("ВЫХОД");
        be.set_position(Vector2::new(85.0, 380.0));
        be.set_size(Vector2::new(260.0, 50.0));
        be.connect("pressed", &self.base().callable("on_exit_pressed"));
        mp.add_child(&be);

        let mut box_r = ColorRect::new_alloc();
        box_r.set_position(Vector2::new(650.0, 140.0));
        box_r.set_size(Vector2::new(470.0, 350.0));
        box_r.set_color(Color::from_rgba(0.06, 0.06, 0.09, 0.85));
        mp.add_child(&box_r);

        let mut lbl = Label::new_alloc();
        lbl.set_position(Vector2::new(675.0, 160.0));
        lbl.set_text("ОНЛАЙН ТОП ELO:\nЗагрузка...");
        style_lbl(&mut lbl, 18, Color::from_rgb(1.0, 0.85, 0.2), &self.ui_font);
        mp.add_child(&lbl);
        self.lb_label = Some(lbl);

        let mut bs = Button::new_alloc();
        bs.set_text("ОБНОВИТЬ ТОП");
        bs.set_position(Vector2::new(675.0, 425.0));
        bs.set_size(Vector2::new(220.0, 42.0));
        bs.connect("pressed", &self.base().callable("on_sync_pressed"));
        mp.add_child(&bs);
        self.menu_panel = Some(mp);

        // HUD
        let mut hp = Control::new_alloc();
        hp.set_anchors_preset(LayoutPreset::FULL_RECT);
        hp.set_visible(false);
        ul.add_child(&hp);

        let mut so = TextureRect::new_alloc();
        so.set_expand_mode(ExpandMode::IGNORE_SIZE);
        so.set_mouse_filter(MouseFilter::IGNORE);
        if rl.exists("res://assets/images/aim.png") {
            if let Some(at) = rl.load("res://assets/images/aim.png").and_then(|r| r.try_cast::<Texture2D>().ok()) {
                so.set_texture(&at);
            }
        }
        so.set_visible(false);
        hp.add_child(&so);
        self.scope_overlay = Some(so);

        let mut sl = ColorRect::new_alloc();
        sl.set_color(Color::BLACK); sl.set_mouse_filter(MouseFilter::IGNORE); sl.set_visible(false);
        hp.add_child(&sl); self.scope_left = Some(sl);

        let mut sr = ColorRect::new_alloc();
        sr.set_color(Color::BLACK); sr.set_mouse_filter(MouseFilter::IGNORE); sr.set_visible(false);
        hp.add_child(&sr); self.scope_right = Some(sr);

        let mut sd = ColorRect::new_alloc();
        sd.set_color(Color::from_rgba(1.0, 0.1, 0.1, 0.9)); sd.set_size(Vector2::new(4.0, 4.0));
        sd.set_mouse_filter(MouseFilter::IGNORE); sd.set_visible(false);
        hp.add_child(&sd); self.scope_dot = Some(sd);

        let mut ho = ColorRect::new_alloc();
        ho.set_anchors_preset(LayoutPreset::FULL_RECT);
        ho.set_color(Color::from_rgba(0.5, 0.25, 0.05, 0.0));
        ho.set_mouse_filter(MouseFilter::IGNORE);
        hp.add_child(&ho); self.hurt_overlay = Some(ho);

        let mk_lbl = |hp: &mut Control, pos: Vector2, sz: i32, col: Color| -> Gd<Label> {
            let mut l = Label::new_alloc();
            l.set_position(pos);
            style_lbl(&mut l, sz, col, &self.ui_font);
            hp.add_child(&l);
            l
        };

        self.hud_elo_label = Some(mk_lbl(&mut hp, Vector2::new(16.0, 14.0), 22, Color::from_rgb(0.4, 0.8, 1.0)));
        self.hud_day_label = Some(mk_lbl(&mut hp, Vector2::new(16.0, 46.0), 18, Color::from_rgb(1.0, 0.85, 0.2)));

        let mut ebg = ColorRect::new_alloc();
        ebg.set_position(Vector2::new(16.0, 76.0)); ebg.set_size(Vector2::new(390.0, 120.0));
        ebg.set_color(Color::from_rgba(0.06, 0.07, 0.09, 0.72));
        hp.add_child(&ebg);
        self.hud_errands_label = Some(mk_lbl(&mut hp, Vector2::new(24.0, 82.0), 15, Color::WHITE));

        let star_tex = if rl.exists("res://assets/images/star.png") {
            rl.load("res://assets/images/star.png").and_then(|r| r.try_cast::<Texture2D>().ok())
        } else { None };
        for s in 0..3 {
            let mut st = TextureRect::new_alloc();
            st.set_position(Vector2::new(126.0 + s as f32 * 34.0, 198.0));
            st.set_size(Vector2::new(30.0, 30.0));
            st.set_expand_mode(ExpandMode::IGNORE_SIZE);
            if let Some(t) = star_tex.as_ref() { st.set_texture(t); }
            st.set_visible(false);
            hp.add_child(&st);
            self.wanted_stars.push(st);
        }
        self.hud_wanted_label = Some(mk_lbl(&mut hp, Vector2::new(16.0, 200.0), 22, Color::from_rgb(1.0, 0.2, 0.2)));

        let mut bbg = ColorRect::new_alloc();
        bbg.set_position(Vector2::new(340.0, 18.0)); bbg.set_size(Vector2::new(600.0, 26.0));
        bbg.set_color(Color::from_rgba(0.1, 0.1, 0.1, 0.8)); hp.add_child(&bbg);
        self.boss_hp_bg = Some(bbg);
        let mut bbar = ColorRect::new_alloc();
        bbar.set_position(Vector2::new(342.0, 20.0)); bbar.set_size(Vector2::new(596.0, 22.0));
        bbar.set_color(Color::from_rgb(0.9, 0.15, 0.15)); hp.add_child(&bbar);
        self.boss_hp_bar = Some(bbar);
        self.boss_hp_label = Some(mk_lbl(&mut hp, Vector2::new(490.0, 19.0), 18, Color::WHITE));

        let mut hbg = ColorRect::new_alloc();
        hbg.set_position(Vector2::new(16.0, 650.0)); hbg.set_size(Vector2::new(220.0, 26.0));
        hbg.set_color(Color::from_rgba(0.1, 0.1, 0.1, 0.8)); hp.add_child(&hbg);
        let mut hbar = ColorRect::new_alloc();
        hbar.set_position(Vector2::new(18.0, 652.0)); hbar.set_size(Vector2::new(216.0, 22.0));
        hbar.set_color(Color::from_rgb(0.2, 0.85, 0.2)); hp.add_child(&hbar);
        self.hud_hp_bar = Some(hbar);
        self.hud_hp_label = Some(mk_lbl(&mut hp, Vector2::new(24.0, 652.0), 18, Color::WHITE));

        self.hud_weapon_label = Some(mk_lbl(&mut hp, Vector2::new(250.0, 652.0), 18, Color::from_rgb(1.0, 0.9, 0.3)));
        let mut hint = mk_lbl(&mut hp, Vector2::new(16.0, 686.0), 15, Color::from_rgb(0.9, 0.9, 0.9));
        hint.set_text("ЛКМ: Стрелять | E: Ахмед/Дело | F/G/H: Магазин | 1/2/3/4: Оружие | R: Новый день | ESC: Меню");

        self.hud_prompt_label = Some(mk_lbl(&mut hp, Vector2::new(440.0, 410.0), 20, Color::from_rgb(1.0, 0.9, 0.3)));
        self.hud_center_msg = Some(mk_lbl(&mut hp, Vector2::new(280.0, 560.0), 22, Color::WHITE));
        self.hud_killfeed_label = Some(mk_lbl(&mut hp, Vector2::new(980.0, 20.0), 16, Color::from_rgb(1.0, 0.8, 0.3)));
        self.hud_end_label = Some(mk_lbl(&mut hp, Vector2::new(340.0, 280.0), 28, Color::from_rgb(1.0, 0.9, 0.2)));

        let mut b_bg = ColorRect::new_alloc();
        b_bg.set_position(Vector2::new(380.0, 170.0)); b_bg.set_size(Vector2::new(520.0, 270.0));
        b_bg.set_color(Color::from_rgba(0.05, 0.06, 0.09, 0.92)); b_bg.set_visible(false);
        hp.add_child(&b_bg); self.buy_bg = Some(b_bg);

        let mut bl = mk_lbl(&mut hp, Vector2::new(405.0, 190.0), 19, Color::from_rgb(1.0, 0.92, 0.5));
        bl.set_visible(false); self.buy_label = Some(bl);

        self.hud_console_label = Some(mk_lbl(&mut hp, Vector2::new(420.0, 220.0), 22, Color::from_rgb(0.3, 1.0, 0.3)));

        let mut mk_ch = || -> Gd<ColorRect> {
            let mut r = ColorRect::new_alloc(); r.set_color(Color::WHITE); hp.add_child(&r); r
        };
        self.ch_l = Some(mk_ch()); self.ch_r = Some(mk_ch()); self.ch_u = Some(mk_ch()); self.ch_d = Some(mk_ch());
        self.hud_panel = Some(hp);
    }

    fn spawn_npc(&mut self, pos: Vector3, role: i32) {
        let mut rl = ResourceLoader::singleton();
        let npc_path = if rl.exists("res://assets/resources/npc/npc.fbx") {
            Some("res://assets/resources/npc/npc.fbx")
        } else if rl.exists("res://assets/resources/npc/npc.glb") {
            Some("res://assets/resources/npc/npc.glb")
        } else { None };

        if let Some(np) = npc_path {
            if let Some(sc) = rl.load(np).and_then(|r| r.try_cast::<PackedScene>().ok()) {
                if let Some(mut model) = sc.instantiate().and_then(|n| n.try_cast::<Node3D>().ok()) {
                    let mut root = Node3D::new_alloc();
                    root.set_position(pos);
                    root.add_child(&model);
                    let mut bb = None;
                    collect_aabb(&model.clone().upcast(), &root.clone().upcast(), &mut bb);
                    if let Some(b) = bb {
                        if b.size.y > 0.001 {
                            let s = 1.82 / b.size.y;
                            model.set_scale(Vector3::new(s, s, s));
                            model.set_position(Vector3::new(0.0, -b.position.y * s, 0.0));
                        }
                    }
                    self.base_mut().add_child(&root);
                    if role == 2 && rl.exists("res://assets/resources/npc/cop_dif.png") {
                        if let Some(ctex) = rl.load("res://assets/resources/npc/cop_dif.png").and_then(|r| r.try_cast::<Texture2D>().ok()) {
                            let mut cmat = StandardMaterial3D::new_gd();
                            cmat.set_texture(TextureParam::ALBEDO, &ctex);
                            apply_material_recursive(&model.clone().upcast(), &cmat);
                        }
                    }
                    let mut chosen = String::new();
                    let ap_opt = find_anim_player(&model.clone().upcast());
                    if let Some(mut ap) = ap_opt.clone() {
                        let list = ap.get_animation_list();
                        for i in 0..list.len() {
                            let an = list[i].to_string();
                            if an == "RESET" { continue; }
                            if an.to_lowercase().contains("walk") || an.to_lowercase().contains("mixamo") {
                                chosen = an;
                                break;
                            }
                        }
                        if chosen.is_empty() {
                            for i in 0..list.len() {
                                let an = list[i].to_string();
                                if an != "RESET" { chosen = an; break; }
                            }
                        }
                        if !chosen.is_empty() {
                            if let Some(mut ar) = ap.get_animation(&chosen) {
                                ar.set_loop_mode(LoopMode::LINEAR);
                            }
                            let mut sk_prefix = String::new();
                            if let Some(ar) = ap.get_animation(&chosen) {
                                if ar.get_track_count() > 0 {
                                    let tp = ar.track_get_path(0).to_string();
                                    if let Some(col_idx) = tp.find(':') {
                                        sk_prefix = tp[..col_idx].to_string();
                                    }
                                }
                            }
                            let remap_anim = |src: &Gd<Animation>, prefix: &str| -> Gd<Animation> {
                                let mut d = src.duplicate().unwrap().try_cast::<Animation>().unwrap();
                                if !prefix.is_empty() {
                                    for t in 0..d.get_track_count() {
                                        let tp = d.track_get_path(t).to_string();
                                        if let Some(col_idx) = tp.find(':') {
                                            let np = format!("{}{}", prefix, &tp[col_idx..]);
                                            d.track_set_path(t, &np);
                                        }
                                    }
                                }
                                d
                            };
                            if let Some(mut lib) = ap.get_animation_library("") {
                                if let Some(run_a) = self.anim_run_res.as_ref() {
                                    if !lib.has_animation("Running") {
                                        let fixed = remap_anim(run_a, &sk_prefix);
                                        lib.add_animation("Running", &fixed);
                                    }
                                }
                                if let Some(die_a) = self.anim_die_res.as_ref() {
                                    if !lib.has_animation("Dying") {
                                        let fixed = remap_anim(die_a, &sk_prefix);
                                        lib.add_animation("Dying", &fixed);
                                    }
                                }
                            }
                            if role == 2 && ap.has_animation("Running") {
                                ap.play_ex().name("Running").done();
                            } else {
                                ap.play_ex().name(&chosen).done();
                            }
                        }
                    }
                    self.npcs.push(GameNpc {
                        root,
                        anim: ap_opt,
                        dir: Vector3::new(1.0, 0.0, 0.3).normalized(),
                        hp: if role == 2 { 90 } else { 60 },
                        role,
                        timer: 1.5,
                        shoot_cd: 1.0,
                        alert_timer: 0.0,
                        panic_t: 0.0,
                        walk_anim: chosen,
                        death_vel: Vector3::ZERO,
                        has_death_vel: false,
                    });
                }
            }
        }
    }

    fn setup_day(&mut self) {
        let hps = [500, 650, 600, 700, 900];
        self.pig_max_hp = hps[self.day_idx];
        self.pig_hp = self.pig_max_hp;
        self.has_quest_item = false;
        self.wanted_level = 0;
        self.wanted_decay = 0.0;
        let qps = [
            Vector3::new(-6.0, 0.0, 16.0), Vector3::new(0.0, 0.0, -5.0),
            Vector3::new(-14.5, 0.0, -36.0), Vector3::new(14.5, 0.0, -6.0), Vector3::new(0.0, 0.0, -14.0),
        ];
        self.quest_item_pos = qps[self.day_idx];
        if let Some(qb) = self.quest_beacon.as_mut() {
            qb.set_position(self.quest_item_pos + Vector3::new(0.0, 0.8, 0.0));
            qb.set_visible(true);
        }
        let t2 = [
            "Забрать корм у трассы [E]", "Разогнать веганов у перевала [E]",
            "Забрать чек под Деревом [E]", "Починить генератор у Сарая [E]", "Активировать детонатор [E]",
        ];
        self.errands = vec![
            ErrandItem { text: "Взять задание у Ахмеда [E]".into(), done: self.quest_stage > 0 },
            ErrandItem { text: t2[self.day_idx].into(), done: false },
            ErrandItem { text: "Завалить Кабаньезе на арене".into(), done: false },
        ];
        for n in &mut self.npcs { n.root.queue_free(); }
        self.npcs.clear();
        let spots = [
            Vector3::new(-14.0, 0.0, 22.0), Vector3::new(14.0, 0.0, 18.0), Vector3::new(-25.0, 0.0, 14.0),
            Vector3::new(24.0, 0.0, 24.0), Vector3::new(-12.0, 0.0, 38.0), Vector3::new(16.0, 0.0, 38.0),
        ];
        for sp in spots { self.spawn_npc(sp, 0); }
        if self.day_idx == 1 {
            for i in -1..=1 { self.spawn_npc(Vector3::new(i as f32 * 2.5, 0.0, -4.0), 1); }
        }
    }

    fn reset_game(&mut self) {
        self.day_idx = (self.day_idx + 1) % 5;
        self.day_banner_t = 3.5;
        if let Some(p) = self.player.as_mut() {
            p.set_position(Vector3::new(0.0, 1.2, 31.0));
            p.bind_mut().speed_mult = 1.0;
        }
        if let Some(pig) = self.pig_node.as_mut() {
            pig.set_visible(true);
            pig.set_position(Vector3::new(0.0, 0.0, -24.0));
        }
        self.player_hp = 100;
        self.panic_timer = 0.0;
        self.has_dmg_buff = false;
        self.has_speed_buff = false;
        self.round_finished = false;
        self.last_elo_delta = 0;
        if self.quest_stage == 2 || self.quest_stage == 3 { self.quest_stage = 1; }
        for b in &mut self.blobs { b.mesh.queue_free(); }
        self.blobs.clear();
        for m in &mut self.minions {
            m.hp = 80;
            if let Some(n) = m.node.as_mut() { n.set_visible(true); }
        }
        self.setup_day();
    }

    fn spawn_blob(&mut self, pos: Vector3, vel: Vector3, life: f32, is_chunk: bool, col: Color, dmg: i32) {
        let mut sm = SphereMesh::new_gd();
        sm.set_radius(if is_chunk { 0.22 } else { 0.26 });
        sm.set_height(if is_chunk { 0.44 } else { 0.52 });
        let mut m = StandardMaterial3D::new_gd();
        m.set_albedo(col);
        sm.set_material(&m);
        let mut mi = MeshInstance3D::new_alloc();
        mi.set_mesh(&sm);
        mi.set_position(pos);
        self.base_mut().add_child(&mi);
        self.blobs.push(BlobProj { mesh: mi, vel, life, is_chunk, dmg });
    }

    fn add_smoke_puff(&mut self, pos: Vector3, start_sc: f32, life: f32) {
        let mut sm = SphereMesh::new_gd();
        sm.set_radius(0.5); sm.set_height(1.0);
        sm.set_radial_segments(8); sm.set_rings(5);
        let mut mat = StandardMaterial3D::new_gd();
        mat.set_transparency(Transparency::ALPHA);
        mat.set_albedo(Color::from_rgba(0.84, 0.84, 0.86, 0.55));
        mat.set_shading_mode(ShadingMode::UNSHADED);
        sm.set_material(&mat);
        let mut mi = MeshInstance3D::new_alloc();
        mi.set_mesh(&sm);
        mi.set_position(pos);
        mi.set_scale(Vector3::new(start_sc, start_sc, start_sc));
        self.base_mut().add_child(&mi);
        self.smokes.push(SmokePuff { mesh: mi, mat, life, max_life: life, start_scale: start_sc });
    }

    fn spawn_blood_puddle(&mut self, pos: Vector3, r: f32) {
        let on_asphalt = (pos.x.abs() < 4.1 && pos.z > 0.0) || (pos.z > 18.0 && pos.z < 26.0 && pos.x.abs() < 48.0);
        let mut cm = CylinderMesh::new_gd();
        cm.set_top_radius(1.0); cm.set_bottom_radius(1.0);
        cm.set_height(0.015); cm.set_radial_segments(14);
        let mut mat = StandardMaterial3D::new_gd();
        mat.set_transparency(Transparency::ALPHA);
        mat.set_albedo(Color::from_rgba(0.62, 0.02, 0.02, if on_asphalt { 0.92 } else { 0.85 }));
        if on_asphalt { mat.set_roughness(0.15); }
        cm.set_material(&mat);
        let mut mi = MeshInstance3D::new_alloc();
        mi.set_mesh(&cm);
        mi.set_position(Vector3::new(pos.x, if on_asphalt { 0.056 } else { 0.015 }, pos.z));
        mi.set_scale(Vector3::new(0.25, 1.0, 0.25));
        self.base_mut().add_child(&mi);
        self.puddles.push(BloodPuddle { mesh: mi, mat, age: 0.0, target_r: r, on_asphalt });
    }

    fn tear_npc_at(&mut self, idx: usize, impulse: Vector3) {
        if !self.npcs[idx].root.is_visible() { return; }
        let np = self.npcs[idx].root.get_global_position();
        let role = self.npcs[idx].role;
        self.npcs[idx].root.set_visible(false);
        if let Some(ap) = self.npcs[idx].anim.as_mut() { ap.stop(); }
        self.spawn_blood_puddle(np, 1.65);
        for _ in 0..18 {
            let v = Vector3::new((self.rng_range(100) - 50) as f32 / 7.5, 3.5 + self.rng_range(6) as f32, (self.rng_range(100) - 50) as f32 / 7.5);
            self.spawn_blob(np + Vector3::new(0.0, 1.0, 0.0), v, 1.4, true, Color::from_rgb(0.75, 0.02, 0.02), 0);
        }
        let mk_part = |parent: &mut Gd<Node3D>, sz: Vector3, col: Color, off: Vector3| {
            let mut bm = BoxMesh::new_gd(); bm.set_size(sz);
            let mut m = StandardMaterial3D::new_gd(); m.set_albedo(col); bm.set_material(&m);
            let mut mi = MeshInstance3D::new_alloc(); mi.set_mesh(&bm); mi.set_position(off);
            parent.add_child(&mi);
        };
        let mut top = Node3D::new_alloc();
        top.set_position(np + Vector3::new(0.0, 1.25, 0.0));
        let sc = if role == 2 { Color::from_rgb(0.05, 0.08, 0.30) } else { Color::from_rgb(0.82, 0.84, 0.88) };
        mk_part(&mut top, Vector3::new(0.48, 0.62, 0.26), sc, Vector3::ZERO);
        mk_part(&mut top, Vector3::new(0.42, 0.12, 0.24), Color::from_rgb(0.65, 0.02, 0.02), Vector3::new(0.0, -0.32, 0.0));
        mk_part(&mut top, Vector3::new(0.30, 0.32, 0.30), Color::from_rgb(0.90, 0.74, 0.60), Vector3::new(0.0, 0.48, 0.0));
        self.base_mut().add_child(&top);
        self.gore_chunks.push(GoreChunk { node: top, vel: impulse * 0.45 + Vector3::new(0.0, 6.5, 0.0), ang_vel: Vector3::new(4.5, 3.0, -5.0), life: 22.0, landed: false });

        let mut bot = Node3D::new_alloc();
        bot.set_position(np + Vector3::new(0.0, 0.50, 0.0));
        mk_part(&mut bot, Vector3::new(0.19, 0.72, 0.21), Color::from_rgb(0.16, 0.22, 0.34), Vector3::new(-0.14, 0.0, 0.0));
        mk_part(&mut bot, Vector3::new(0.19, 0.72, 0.21), Color::from_rgb(0.16, 0.22, 0.34), Vector3::new(0.14, 0.0, 0.0));
        mk_part(&mut bot, Vector3::new(0.40, 0.12, 0.22), Color::from_rgb(0.65, 0.02, 0.02), Vector3::new(0.0, 0.35, 0.0));
        self.base_mut().add_child(&bot);
        let side = Vector3::new(-impulse.z * 0.35, 4.5, impulse.x * 0.35);
        self.gore_chunks.push(GoreChunk { node: bot, vel: impulse * 0.25 + side, ang_vel: Vector3::new(-5.0, 2.5, 4.0), life: 22.0, landed: false });
    }

    fn explode_at(&mut self, pos: Vector3, from_canister: bool) {
        if let Some(sb) = self.snd_boom.as_mut() { sb.play(); }
        for k in 0..28 {
            let c = if k % 2 == 0 { Color::from_rgb(1.0, 0.45, 0.05) } else { Color::from_rgb(0.95, 0.15, 0.05) };
            let v = Vector3::new((self.rng_range(100) - 50) as f32 / 4.5, 4.0 + self.rng_range(7) as f32, (self.rng_range(100) - 50) as f32 / 4.5);
            self.spawn_blob(pos + Vector3::new(0.0, 0.6, 0.0), v, 1.5, true, c, 0);
        }
        if let Some(p) = self.player.as_ref() {
            if !self.god_mode && p.get_global_position().distance_to(pos) < 7.5 {
                self.player_hp = (self.player_hp - 75).max(0);
                self.hurt_flash = 0.45;
                if self.player_hp == 0 && !self.round_finished {
                    self.round_finished = true;
                    if let Some(sw) = self.snd_win.as_mut() { sw.play(); }
                    self.last_elo_delta = -350;
                    self.elo = (self.elo + self.last_elo_delta).max(0);
                    self.save_progress();
                    self.sync_leaderboard(true);
                }
            }
        }
        for i in 0..self.npcs.len() {
            if self.npcs[i].hp > 0 {
                let np = self.npcs[i].root.get_global_position();
                let dist = np.distance_to(pos);
                if dist < 12.0 {
                    self.npcs[i].hp = 0;
                    let role = self.npcs[i].role;
                    self.kill_feed.push(KillEntry { text: if role == 2 { "ВЗРЫВ -> Участковый" } else { "ВЗРЫВ -> Житель" }.into(), life: 4.0 });
                    if role == 2 { self.elo += 50; }
                    let mut dir = np - pos; dir.y = 0.0;
                    if dir.length_squared() < 0.01 { dir = Vector3::RIGHT; }
                    let force = 26.0 * (1.0 - dist / 14.0);
                    self.tear_npc_at(i, dir.normalized() * force + Vector3::new(0.0, 8.0, 0.0));
                }
            }
        }
        for m in &mut self.minions {
            if m.hp > 0 && (from_canister || m.node.as_ref().map(|n| n.get_global_position().distance_to(pos) < 12.0).unwrap_or(false)) {
                m.hp = 0; m.respawn_timer = 15.0;
                if let Some(n) = m.node.as_mut() { n.set_visible(false); }
            }
        }
        if from_canister {
            self.kill_feed.push(KillEntry { text: "КАНИСТРА -> Все миньоны убиты!".into(), life: 5.0 });
        }
        if self.pig_hp > 0 {
            if let Some(pig) = self.pig_node.as_ref() {
                if pig.get_global_position().distance_to(pos) < 10.0 {
                    self.pig_hp = (self.pig_hp - 180).max(0);
                    self.panic_timer = if self.day_idx == 2 || self.day_idx == 4 { 3.2 } else { 2.2 };
                    if let Some(sh) = self.snd_hurt.as_mut() { if !sh.is_playing() { sh.play(); } }
                    self.kill_feed.push(KillEntry { text: "ВЗРЫВ -> Кабаньезе (-180)".into(), life: 4.0 });
                }
            }
        }
        let mut chain = Vec::new();
        for c in &mut self.canisters {
            if c.active && c.node.get_global_position().distance_to(pos) < 3.2 {
                c.active = false; c.respawn_timer = 12.0; c.node.set_visible(false);
                chain.push(c.node.get_global_position());
            }
        }
        for cp in chain { self.explode_at(cp, true); }
    }

    fn handle_shoot(&mut self) {
        let Some(cam) = self.cam.clone() else { return; };
        let Some(player) = self.player.clone() else { return; };

        self.shoot_cd = match self.weapon_type { 2 => 0.09, 3 => 1.25, 4 => 1.1, _ => 0.28 };
        self.recoil = 1.0;
        self.flash_timer = if self.weapon_type == 4 { 0.0 } else { 0.06 };

        let ro = cam.get_global_position();
        let rd = -cam.get_global_transform().basis.col_c().normalized();

        for n in &mut self.npcs {
            if n.hp > 0 && n.role == 0 && n.root.get_global_position().distance_to(ro) < 75.0 {
                n.panic_t = 60.0;
            }
        }

        match self.weapon_type {
            1 => if let Some(s) = self.snd_shotgun.as_mut() { s.play(); },
            2 => if let Some(s) = self.snd_ak.as_mut() { s.play(); },
            3 => {
                if let Some(s) = self.snd_awp.as_mut() { s.play(); }
                if let Some(ap) = self.awp_anim.as_mut() {
                    if !self.awp_fire_anim.is_empty() {
                        ap.stop();
                        ap.play_ex().name(&self.awp_fire_anim).done();
                    }
                }
            }
            4 => if let Some(s) = self.snd_rpg.as_mut() { s.play(); },
            _ => {}
        }

        if self.weapon_type == 4 {
            let mut rm = SphereMesh::new_gd();
            rm.set_radius(0.16); rm.set_height(0.45);
            let mut mat = StandardMaterial3D::new_gd();
            mat.set_albedo(Color::from_rgb(0.25, 0.30, 0.20));
            mat.set_feature(Feature::EMISSION, true);
            mat.set_emission(Color::from_rgb(1.0, 0.55, 0.12));
            mat.set_emission_energy_multiplier(3.0);
            rm.set_material(&mat);
            let mut mi = MeshInstance3D::new_alloc();
            mi.set_mesh(&rm);
            mi.set_position(ro + rd * 1.0);
            self.base_mut().add_child(&mi);
            self.rockets.push(RocketProj { mesh: mi, vel: rd * 36.0, life: 3.0 });
            for k in 0..5 {
                let j = Vector3::new((self.rng_range(100) - 50) as f32 / 80.0, (self.rng_range(100) - 50) as f32 / 80.0, (self.rng_range(100) - 50) as f32 / 80.0);
                self.add_smoke_puff(ro + rd * (0.9 + k as f32 * 0.45) + j, 0.75 + k as f32 * 0.15, 2.0);
            }
            let mut p_mut = player;
            p_mut.bind_mut().add_recoil(0.06, 0.0);
            return;
        }

        let dp = if self.weapon_type == 2 { 0.014 } else { 0.038 };
        let dy = (self.rng_range(100) - 50) as f32 * if self.weapon_type == 2 { 0.00022 } else { 0.00035 };
        {
            let mut p_mut = player.clone();
            p_mut.bind_mut().add_recoil(dp, dy);
        }
        self.spread_kick = (self.spread_kick + if self.weapon_type == 2 { 0.24 } else { 0.55 }).min(1.0);

        let mut t_wall = 9999.0_f32;
        if let Some(mut sp) = cam.get_world_3d().and_then(|mut w| w.get_direct_space_state()) {
            if let Some(mut q) = PhysicsRayQueryParameters3D::create(ro, ro + rd * 1500.0) {
                let mut excl = Array::new();
                excl.push(player.get_rid());
                q.set_exclude(&excl);
                let res = sp.intersect_ray(&q);
                if let Some(pos_v) = res.get("position") {
                    if let Ok(hp) = pos_v.try_to::<Vector3>() {
                        t_wall = ro.distance_to(hp);
                    }
                }
            }
        }

        let dmg_mult = if self.has_dmg_buff { 2 } else { 1 };

        // Канистры
        let mut hit_can = None;
        for (i, c) in self.canisters.iter().enumerate() {
            if c.active {
                if let Some(tc) = ray_hit_sphere(ro, rd, c.node.get_global_position() + Vector3::new(0.0, 0.4, 0.0), 0.75) {
                    if tc < t_wall { hit_can = Some(i); break; }
                }
            }
        }
        if let Some(ci) = hit_can {
            self.canisters[ci].active = false;
            self.canisters[ci].respawn_timer = 10.0;
            self.canisters[ci].node.set_visible(false);
            self.hit_marker = 0.25; self.crit_hit = true;
            let p = self.canisters[ci].node.get_global_position();
            self.explode_at(p, true);
            return;
        }

        // NPC
        for i in 0..self.npcs.len() {
            if self.npcs[i].hp <= 0 { continue; }
            let np_pos = self.npcs[i].root.get_global_position();
            let h1 = ray_hit_sphere(ro, rd, np_pos + Vector3::new(0.0, 0.85, 0.0), 0.52);
            let h2 = ray_hit_sphere(ro, rd, np_pos + Vector3::new(0.0, 1.45, 0.0), 0.45);
            let tn = match (h1, h2) {
                (Some(a), Some(b)) => a.min(b),
                (Some(a), None) | (None, Some(a)) => a,
                _ => continue,
            };
            if tn < t_wall {
                let npc_dmg = match self.weapon_type { 3 => 150, 2 => 28, _ => 60 };
                self.npcs[i].hp -= npc_dmg * dmg_mult;
                self.hit_marker = 0.18; self.crit_hit = true;
                if self.npcs[i].role != 2 {
                    self.wanted_level = (self.wanted_level + 1).min(3);
                    self.wanted_decay = 14.0;
                }
                if self.npcs[i].hp <= 0 {
                    let role = self.npcs[i].role;
                    self.kill_feed.push(KillEntry { text: if role == 2 { "Вы -> Участковый" } else { "Вы -> Житель" }.into(), life: 4.0 });
                    if role == 2 { self.elo += 50; }
                    if self.weapon_type == 3 || (self.weapon_type == 1 && tn < 5.5) {
                        self.tear_npc_at(i, rd * if self.weapon_type == 3 { 16.0 } else { 10.0 } + Vector3::new(0.0, 5.0, 0.0));
                    } else {
                        if let Some(ap) = self.npcs[i].anim.as_mut() {
                            if ap.has_animation("Dying") { ap.set_speed_scale(1.1); ap.play_ex().name("Dying").done(); }
                            else { ap.stop(); }
                        }
                        self.npcs[i].death_vel = (rd * 10.0 + Vector3::new(0.0, 3.0, 0.0)) * 0.45;
                        self.npcs[i].has_death_vel = true;
                        self.spawn_blood_puddle(np_pos, 1.15);
                    }
                }
                return;
            }
        }

        let ppos = player.get_global_position();
        if (ppos.z <= -2.0 || self.weapon_type == 3) && self.quest_stage > 0 {
            for m in &mut self.minions {
                if m.hp > 0 {
                    if let Some(node) = m.node.as_mut() {
                        if let Some(tm) = ray_hit_sphere(ro, rd, node.get_global_position(), 1.0) {
                            if tm < t_wall {
                                m.hp -= if self.weapon_type == 2 { 20 } else { 45 } * dmg_mult;
                                self.hit_marker = 0.18; self.crit_hit = true;
                                if m.hp <= 0 {
                                    m.hp = 0; m.respawn_timer = 15.0; node.set_visible(false);
                                    self.kill_feed.push(KillEntry { text: "Вы -> Миньон".into(), life: 4.0 });
                                }
                            }
                        }
                    }
                }
            }
        }

        if self.pig_hp > 0 {
            if let Some(pig) = self.pig_node.clone() {
                let pig_p = pig.get_global_position();
                let pig_fwd = Vector3::new(self.pig_yaw.sin(), 0.0, self.pig_yaw.cos());
                let pig_body = pig_p + Vector3::new(0.0, 1.4, 0.0);
                let pig_anus = pig_p - pig_fwd * 1.55 + Vector3::new(0.0, 1.2, 0.0);
                let ha = ray_hit_sphere(ro, rd, pig_anus, 0.7).filter(|&t| t < t_wall);
                let hb = ray_hit_sphere(ro, rd, pig_body, 1.6).filter(|&t| t < t_wall);
                if (ha.is_some() || hb.is_some()) && ppos.z > -2.0 && self.weapon_type != 3 {
                    self.warn_msg = "ЗАЙДИ НА СЕВЕРНУЮ АРЕНУ ЧЕРЕЗ ПЕРЕВАЛ!".into(); self.warn_msg_t = 2.0;
                } else if (ha.is_some() || hb.is_some()) && self.quest_stage == 0 {
                    self.warn_msg = "СНАЧАЛА ПОГОВОРИ С ДЯДЕЙ АХМЕДОМ [E]!".into(); self.warn_msg_t = 2.0;
                } else if ha.is_some() || hb.is_some() {
                    let is_crit = matches!((ha, hb), (Some(a), Some(b)) if a <= b + 0.35) || matches!((ha, hb), (Some(_), None)) || (self.always_crit_ak && self.weapon_type == 2 && hb.is_some());
                    if let Some(sh) = self.snd_hurt.as_mut() { if !sh.is_playing() { sh.play(); } }
                    let dmg = if is_crit {
                        match self.weapon_type { 3 => 110, 2 => 18, _ => 35 }
                    } else {
                        match self.weapon_type { 3 => 40, 2 => 4, _ => 5 }
                    } * dmg_mult;
                    self.pig_hp = (self.pig_hp - dmg).max(0);
                    self.panic_timer = if self.day_idx == 2 || self.day_idx == 4 { 3.2 } else { 2.2 };
                    self.hit_marker = if is_crit { 0.22 } else { 0.15 };
                    self.crit_hit = is_crit;

                    if self.pig_hp == 0 && !self.round_finished {
                        self.round_finished = true;
                        self.pig_kills += 1;
                        if self.errands.len() > 2 { self.errands[2].done = true; }
                        self.kill_feed.push(KillEntry { text: "Вы -> КАБАНЬЕЗЕ!".into(), life: 6.0 });
                        self.wanted_level = 0;
                        let rew = [1250, 1400, 1550, 1750, 2500];
                        self.last_elo_delta = rew[self.day_idx];
                        self.elo += self.last_elo_delta;
                        if self.quest_stage == 1 { self.quest_stage = 2; }
                        if self.pig_kills == 5 { self.weapon_type = 2; }
                        let mut p = pig; p.set_visible(false);
                        self.save_progress();
                        self.sync_leaderboard(true);
                    }
                }
            }
        }
    }

    fn update_hud(&mut self) {
        if let Some(l) = self.menu_elo_label.as_mut() { l.set_text(&format!("ВАШ ELO: {} | УБИЙСТВ: {}", self.elo, self.pig_kills)); }
        if let Some(l) = self.hud_elo_label.as_mut() { l.set_text(&format!("{} ELO | УБИЙСТВ: {}", self.elo, self.pig_kills)); }
        let days = ["ПОНЕДЕЛЬНИК — Обычный замес", "ВТОРНИК — Шустрый хряк", "СРЕДА — Долгая паника", "ЧЕТВЕРГ — Токсичный дрист", "ПЯТНИЦА — АПОКАЛИПСИС"];
        if let Some(l) = self.hud_day_label.as_mut() { l.set_text(days[self.day_idx]); }

        let mut etxt = String::from("СПИСОК ДЕЛ:\n");
        for e in &self.errands {
            etxt.push_str(if e.done { "[X] " } else { "[ ] " });
            etxt.push_str(&e.text);
            etxt.push('\n');
        }
        if let Some(l) = self.hud_errands_label.as_mut() { l.set_text(&etxt); }
        if let Some(l) = self.hud_wanted_label.as_mut() { l.set_text(if self.wanted_level > 0 { "РОЗЫСК:" } else { "" }); }
        for (s, st) in self.wanted_stars.iter_mut().enumerate() {
            st.set_visible((s as i32) < self.wanted_level);
        }

        let ppos = self.player.as_ref().unwrap().get_global_position();
        let show_boss = ppos.z < 2.0 && self.pig_hp > 0;
        if let Some(b) = self.boss_hp_bg.as_mut() { b.set_visible(show_boss); }
        if let Some(b) = self.boss_hp_bar.as_mut() {
            b.set_visible(show_boss);
            if show_boss { b.set_size(Vector2::new(596.0 * self.pig_hp as f32 / self.pig_max_hp.max(1) as f32, 22.0)); }
        }
        if let Some(l) = self.boss_hp_label.as_mut() {
            l.set_visible(show_boss);
            if show_boss { l.set_text(&format!("БОСС КАБАНЬЕЗЕ: {} / {}", self.pig_hp, self.pig_max_hp)); }
        }

        if let Some(b) = self.hud_hp_bar.as_mut() { b.set_size(Vector2::new(216.0 * self.player_hp.min(100) as f32 / 100.0, 22.0)); }
        if let Some(l) = self.hud_hp_label.as_mut() { l.set_text(&format!("HP: {}", self.player_hp)); }
        if let Some(l) = self.hud_weapon_label.as_mut() { l.set_text("[1] Дробовик | [2] АК-47 | [3] AWP | [4] РПГ"); }

        let mut pr = String::new();
        if dist_xz(ppos, self.ahmed_pos) < 5.0 { pr = "[E] Поговорить с Дядей Ахмедом".into(); }
        else if self.quest_stage == 1 && !self.has_quest_item && dist_xz(ppos, self.quest_item_pos) < 4.5 { pr = "[E] Выполнить дело дня".into(); }
        else if dist_xz(ppos, self.gunshop_pos) < 3.65 { pr = "ОРУЖЕЙНЫЙ МАГАЗИН: Жми [B] чтобы купить стволы".into(); }
        else if dist_xz(ppos, self.shop_pos) < 2.5 { pr = "МАГАЗИН: [F] Шаурма 250 | [G] Пули x2 500 | [H] Скороходы 400".into(); }
        if self.buy_menu_open && dist_xz(ppos, self.gunshop_pos) >= 3.65 { self.buy_menu_open = false; }
        if self.carrying_canister { pr = "КАНИСТРА В РУКАХ — неси на жёлтую площадку заправки!".into(); }
        else if pr.is_empty() {
            for c in &self.canisters {
                if c.active && ppos.distance_to(c.node.get_global_position()) < 3.5 {
                    pr = "[E] Взять канистру с бензином (+120 ELO)".into();
                    break;
                }
            }
        }
        if let Some(l) = self.hud_prompt_label.as_mut() { l.set_text(&pr); }
        if let (Some(bg), Some(bl)) = (self.buy_bg.as_mut(), self.buy_label.as_mut()) {
            bg.set_visible(self.buy_menu_open);
            bl.set_visible(self.buy_menu_open);
            if self.buy_menu_open {
                let mut bt = String::from("ОРУЖЕЙНЫЙ МАГАЗИН [B - закрыть]:\n\n[1] Дробовик (ЕСТЬ)\n");
                bt.push_str(if self.has_ak || self.pig_kills >= 5 || self.always_crit_ak { "[2] АК-47 (ЕСТЬ)\n" } else { "[2] АК-47 — 1500 ELO\n" });
                bt.push_str(if self.has_awp { "[3] Снайперка AWP (ЕСТЬ)\n" } else { "[3] Снайперка AWP — 25000 ELO\n" });
                bt.push_str(if self.has_rpg { "[4] Гранатомёт РПГ (ЕСТЬ)\n" } else { "[4] Гранатомёт РПГ — 40000 ELO\n" });
                bl.set_text(&bt);
            }
        }

        let cm = if self.ahmed_msg_t > 0.0 { self.ahmed_msg.as_str() }
            else if self.shop_msg_t > 0.0 { self.shop_msg.as_str() }
            else if self.warn_msg_t > 0.0 { self.warn_msg.as_str() }
            else if self.cheat_msg_t > 0.0 { self.cheat_msg.as_str() }
            else if self.day_banner_t > 0.0 { days[self.day_idx] }
            else { "" };
        if let Some(l) = self.hud_center_msg.as_mut() { l.set_text(cm); }

        let kf: String = self.kill_feed.iter().map(|k| format!("{}\n", k.text)).collect();
        if let Some(l) = self.hud_killfeed_label.as_mut() { l.set_text(&kf); }

        if let Some(l) = self.hud_end_label.as_mut() {
            l.set_text(if self.pig_hp <= 0 { "ПОБЕДА! КАБАНЬЕЗЕ РАЗОРВАЛО! (R - следующий день)" }
                       else if self.player_hp <= 0 { "ВАС ЗАДРИСТАЛИ! (R - следующий день)" }
                       else { "" });
        }
        if let Some(l) = self.hud_console_label.as_mut() {
            let txt = if self.console_open { format!("КОНСОЛЬ: {}_", self.console_input) } else { String::new() };
            l.set_text(&txt);
        }
        if let Some(ho) = self.hurt_overlay.as_mut() {
            ho.set_color(Color::from_rgba(0.5, 0.25, 0.05, if self.hurt_flash > 0.0 { 0.35 } else { 0.0 }));
        }

        let vs = self.base().get_viewport().unwrap().get_visible_rect().size;
        let sq = vs.x.min(vs.y);
        let side = (vs.x - sq) * 0.5;
        if let Some(so) = self.scope_overlay.as_mut() { so.set_position(Vector2::new(side, 0.0)); so.set_size(Vector2::new(sq, sq)); }
        if let Some(sl) = self.scope_left.as_mut() { sl.set_position(Vector2::ZERO); sl.set_size(Vector2::new(side + 2.0, vs.y)); }
        if let Some(sr) = self.scope_right.as_mut() { sr.set_position(Vector2::new(side + sq - 2.0, 0.0)); sr.set_size(Vector2::new(side + 4.0, vs.y)); }
        if let Some(sd) = self.scope_dot.as_mut() { sd.set_position(Vector2::new(vs.x * 0.5 - 2.0, vs.y * 0.5 - 2.0)); }

        let cx = vs.x * 0.5; let cy = vs.y * 0.5;
        let gap = 5.0 + self.spread_kick * 16.0;
        let cc = if self.hit_marker > 0.0 {
            if self.crit_hit { Color::from_rgb(1.0, 0.2, 0.2) } else { Color::from_rgb(1.0, 0.9, 0.1) }
        } else { Color::WHITE };
        let show_ch = !(self.weapon_type == 3 && Input::singleton().is_mouse_button_pressed(MouseButton::RIGHT));
        if let Some(r) = self.ch_l.as_mut() { r.set_visible(show_ch); r.set_position(Vector2::new(cx - gap - 8.0, cy - 1.0)); r.set_size(Vector2::new(8.0, 2.0)); r.set_color(cc); }
        if let Some(r) = self.ch_r.as_mut() { r.set_visible(show_ch); r.set_position(Vector2::new(cx + gap, cy - 1.0)); r.set_size(Vector2::new(8.0, 2.0)); r.set_color(cc); }
        if let Some(r) = self.ch_u.as_mut() { r.set_visible(show_ch); r.set_position(Vector2::new(cx - 1.0, cy - gap - 8.0)); r.set_size(Vector2::new(2.0, 8.0)); r.set_color(cc); }
        if let Some(r) = self.ch_d.as_mut() { r.set_visible(show_ch); r.set_position(Vector2::new(cx - 1.0, cy + gap)); r.set_size(Vector2::new(2.0, 8.0)); r.set_color(cc); }
    }
}
