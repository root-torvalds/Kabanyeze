use godot::prelude::*;
use godot::classes::{
    BoxMesh, BoxShape3D, CollisionShape3D, CylinderMesh, Engine, Image, ImageTexture,
    MeshInstance3D, MultiMesh, MultiMeshInstance3D, PackedScene, PlaneMesh, ResourceLoader,
    Sprite3D, StandardMaterial3D, StaticBody3D, Texture2D,
};
use godot::classes::base_material_3d::{BillboardMode, CullMode, TextureFilter};
use godot::classes::image::Format;
use godot::classes::multi_mesh::TransformFormat;

use crate::game_manager::GameManager;

struct Rng(u32);
impl Rng {
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        self.0 >> 16
    }
    fn range(&mut self, n: i32) -> i32 {
        (self.next() as i32).abs() % n
    }
}

fn make_grass_tex(rng: &mut Rng) -> Gd<ImageTexture> {
    let s = 256;
    let mut img = Image::create_empty(s, s, false, Format::RGBA8).unwrap();
    for y in 0..s {
        for x in 0..s {
            let n = rng.range(28) - 14;
            let mud = ((x / 7 + y / 5) % 19 == 0) || (rng.range(22) == 0);
            let col = if mud {
                Color::from_rgba8((38 + rng.range(18)) as u8, (32 + rng.range(14)) as u8, 14, 255)
            } else {
                Color::from_rgba8(
                    (22 + n).clamp(8, 45) as u8,
                    (58 + n * 2).clamp(28, 92) as u8,
                    (18 + n).clamp(6, 35) as u8,
                    255,
                )
            };
            img.set_pixel(x, y, col);
        }
    }
    ImageTexture::create_from_image(&img).unwrap()
}

fn make_rock_tex(rng: &mut Rng) -> Gd<ImageTexture> {
    let s = 64;
    let mut img = Image::create_empty(s, s, false, Format::RGBA8).unwrap();
    for y in 0..s {
        for x in 0..s {
            let n = rng.range(28) - 14;
            let snow = if y < 14 && rng.range(3) != 0 { 45 } else { 0 };
            img.set_pixel(
                x,
                y,
                Color::from_rgba8(
                    (68 + n + snow).clamp(25, 165) as u8,
                    (64 + n + snow).clamp(25, 160) as u8,
                    (60 + n + snow).clamp(25, 155) as u8,
                    255,
                ),
            );
        }
    }
    ImageTexture::create_from_image(&img).unwrap()
}

fn make_road_tex(rng: &mut Rng) -> Gd<ImageTexture> {
    let s = 64;
    let mut img = Image::create_empty(s, s, false, Format::RGBA8).unwrap();
    for y in 0..s {
        for x in 0..s {
            let n = rng.range(14) - 7;
            let col = if (30..=33).contains(&x) && (y % 32) < 18 {
                Color::from_rgb(0.85, 0.70, 0.15)
            } else {
                Color::from_rgba8((46 + n) as u8, (48 + n) as u8, (52 + n) as u8, 255)
            };
            img.set_pixel(x, y, col);
        }
    }
    ImageTexture::create_from_image(&img).unwrap()
}

fn add_trimesh_recursive(node: &Gd<Node>) {
    if let Ok(mi) = node.clone().try_cast::<MeshInstance3D>() {
        let mut mi = mi;
        mi.create_trimesh_collision();
    }
    for i in 0..node.get_child_count() {
        if let Some(ch) = node.get_child(i) {
            add_trimesh_recursive(&ch);
        }
    }
}

#[derive(GodotClass)]
#[class(base=Node3D)]
pub struct WorldBuilder {
    base: Base<Node3D>,
}

#[godot_api]
impl INode3D for WorldBuilder {
    fn init(base: Base<Node3D>) -> Self {
        Self { base }
    }

    fn ready(&mut self) {
        if self.base().get_child_count() > 0 {
            return;
        }
        let mut rng = Rng(1337);

        // 1. Земля 140x160
        if let Some(parent) = self.base().get_parent() {
            if let Some(mut gm) = parent.try_get_node_as::<MeshInstance3D>("Ground/MeshInstance3D") {
                if let Some(mut pm) = gm.get_mesh().and_then(|m| m.try_cast::<PlaneMesh>().ok()) {
                    pm.set_size(Vector2::new(140.0, 160.0));
                }
                let mut gmat = StandardMaterial3D::new_gd();
                gmat.set_texture(godot::classes::base_material_3d::TextureParam::ALBEDO, &make_grass_tex(&mut rng));
                gmat.set_texture_filter(TextureFilter::NEAREST);
                gmat.set_uv1_scale(Vector3::new(20.0, 24.0, 1.0));
                gm.set_surface_override_material(0, &gmat);
            }
        }

        // 2. Трава (1400 кустов через MultiMesh)
        let mut blade = BoxMesh::new_gd();
        blade.set_size(Vector3::new(0.09, 0.38, 0.09));
        let mut bmat = StandardMaterial3D::new_gd();
        bmat.set_albedo(Color::from_rgba8(28, 72, 22, 255));
        blade.set_material(&bmat);

        let mut mm = MultiMesh::new_gd();
        mm.set_transform_format(TransformFormat::TRANSFORM_3D);
        mm.set_instance_count(1400);
        mm.set_mesh(&blade);
        for i in 0..1400 {
            let gx = (rng.range(1240) - 620) as f32 / 10.0;
            let gz = (rng.range(1400) - 700) as f32 / 10.0;
            if (gx.abs() < 4.2 && gz > -2.0) || (gz > 17.5 && gz < 26.5 && gx.abs() < 48.0) {
                mm.set_instance_transform(i, Transform3D::new(Basis::from_scale(Vector3::ZERO), Vector3::new(0.0, -5.0, 0.0)));
            } else {
                mm.set_instance_transform(i, Transform3D::new(Basis::IDENTITY, Vector3::new(gx, 0.19, gz)));
            }
        }
        let mut mmi = MultiMeshInstance3D::new_alloc();
        mmi.set_multimesh(&mm);
        self.base_mut().add_child(&mmi);

        // 3. Перевал на z = 0
        let mut rock_mesh = CylinderMesh::new_gd();
        rock_mesh.set_top_radius(0.0);
        rock_mesh.set_bottom_radius(1.0);
        rock_mesh.set_height(1.0);
        rock_mesh.set_radial_segments(5);
        rock_mesh.set_cap_top(false);
        let mut rmat = StandardMaterial3D::new_gd();
        rmat.set_texture(godot::classes::base_material_3d::TextureParam::ALBEDO, &make_rock_tex(&mut rng));
        rmat.set_texture_filter(TextureFilter::NEAREST);
        rock_mesh.set_material(&rmat);

        let mut add_mtn = |x: f32, z: f32, w: f32, h: f32, rot_y: f32| {
            let mut mi = MeshInstance3D::new_alloc();
            mi.set_mesh(&rock_mesh);
            mi.set_position(Vector3::new(x, h * 0.5, z));
            mi.set_rotation(Vector3::new(0.0, rot_y, 0.0));
            mi.set_scale(Vector3::new(w, h, w));
            self.base_mut().add_child(&mi);
        };
        let mut x = -60.0;
        while x <= -9.5 { let rot_y = rng.range(360) as f32 * 0.017453; let rh = rng.range(4) as f32; add_mtn(x, 0.0, 5.8, 8.5 + rh, rot_y); x += 7.0; }
        let mut x = 9.5;
        while x <= 60.0 { let rot_y = rng.range(360) as f32 * 0.017453; let rh = rng.range(4) as f32; add_mtn(x, 0.0, 5.8, 8.5 + rh, rot_y); x += 7.0; }

        let mut pending: Vec<Gd<Node>> = Vec::new();
        let add_wall = |pending: &mut Vec<Gd<Node>>, pos: Vector3, sz: Vector3| {
            let mut sb = StaticBody3D::new_alloc();
            let mut cs = CollisionShape3D::new_alloc();
            let mut bs = BoxShape3D::new_gd();
            bs.set_size(sz);
            cs.set_shape(&bs);
            sb.set_position(pos);
            sb.add_child(&cs);
            pending.push(sb.upcast());
        };
        add_wall(&mut pending, Vector3::new(-35.6, 6.0, 0.0), Vector3::new(56.8, 12.0, 5.0));
        add_wall(&mut pending, Vector3::new(35.6, 6.0, 0.0), Vector3::new(56.8, 12.0, 5.0));

        // 4. Дороги
        let mut add_road = |pending: &mut Vec<Gd<Node>>, name: &str, pos: Vector3, sz: Vector2, rot_y: f32, uv_y: f32| {
            let mut rm = PlaneMesh::new_gd();
            rm.set_size(sz);
            let mut rmat2 = StandardMaterial3D::new_gd();
            rmat2.set_texture(godot::classes::base_material_3d::TextureParam::ALBEDO, &make_road_tex(&mut rng));
            rmat2.set_texture_filter(TextureFilter::NEAREST);
            rmat2.set_cull_mode(CullMode::DISABLED);
            rmat2.set_uv1_scale(Vector3::new(1.0, uv_y, 1.0));
            rm.set_material(&rmat2);
            let mut ri = MeshInstance3D::new_alloc();
            ri.set_name(name);
            ri.set_mesh(&rm);
            ri.set_position(pos);
            ri.set_rotation(Vector3::new(0.0, rot_y, 0.0));
            pending.push(ri.upcast());
        };
        add_road(&mut pending, "RoadMain", Vector3::new(0.0, 0.04, 34.0), Vector2::new(8.0, 68.0), 0.0, 11.0);
        add_road(&mut pending, "RoadTown", Vector3::new(0.0, 0.045, 22.0), Vector2::new(8.0, 96.0), 1.5708, 16.0);

        // 5. Здания города
        let add_solid_box = |pending: &mut Vec<Gd<Node>>, pos: Vector3, sz: Vector3, col: Color, collide: bool| {
            let mut bm = BoxMesh::new_gd();
            bm.set_size(sz);
            let mut m = StandardMaterial3D::new_gd();
            m.set_albedo(col);
            bm.set_material(&m);
            let mut mi = MeshInstance3D::new_alloc();
            mi.set_mesh(&bm);
            mi.set_position(pos);
            pending.push(mi.upcast());
            if collide {
                add_wall(pending, pos, sz);
            }
        };
        // Склад
        add_solid_box(&mut pending, Vector3::new(-24.0, 2.1, 44.0), Vector3::new(10.0, 4.2, 8.0), Color::from_rgba8(135, 110, 88, 255), true);
        add_solid_box(&mut pending, Vector3::new(-24.0, 4.45, 44.0), Vector3::new(10.8, 0.6, 8.8), Color::from_rgba8(90, 42, 32, 255), false);
        add_solid_box(&mut pending, Vector3::new(-19.0, 1.3, 44.0), Vector3::new(0.25, 2.6, 1.6), Color::from_rgba8(65, 38, 18, 255), false);

        // Супермаркет
        add_solid_box(&mut pending, Vector3::new(-34.0, 2.4, 12.0), Vector3::new(0.6, 4.8, 14.0), Color::from_rgb(0.76, 0.74, 0.70), true);
        add_solid_box(&mut pending, Vector3::new(-28.0, 2.4, 5.0), Vector3::new(12.0, 4.8, 0.6), Color::from_rgb(0.76, 0.74, 0.70), true);
        add_solid_box(&mut pending, Vector3::new(-28.0, 2.4, 19.0), Vector3::new(12.0, 4.8, 0.6), Color::from_rgb(0.76, 0.74, 0.70), true);
        add_solid_box(&mut pending, Vector3::new(-22.0, 2.4, 7.5), Vector3::new(0.6, 4.8, 5.0), Color::from_rgb(0.76, 0.74, 0.70), true);
        add_solid_box(&mut pending, Vector3::new(-22.0, 2.4, 16.5), Vector3::new(0.6, 4.8, 5.0), Color::from_rgb(0.76, 0.74, 0.70), true);
        add_solid_box(&mut pending, Vector3::new(-28.0, 4.95, 12.0), Vector3::new(13.0, 0.4, 15.0), Color::from_rgb(0.16, 0.43, 0.65), false);
        add_solid_box(&mut pending, Vector3::new(-29.0, 0.7, 12.0), Vector3::new(1.4, 1.4, 6.0), Color::from_rgb(0.43, 0.29, 0.17), true);

        // Полицейский участок
        add_solid_box(&mut pending, Vector3::new(32.0, 2.6, 34.0), Vector3::new(12.0, 5.2, 10.0), Color::from_rgb(0.27, 0.32, 0.41), true);
        add_solid_box(&mut pending, Vector3::new(32.0, 5.45, 34.0), Vector3::new(12.6, 0.5, 10.6), Color::from_rgb(0.14, 0.16, 0.23), false);
        add_solid_box(&mut pending, Vector3::new(32.0, 5.95, 30.0), Vector3::new(1.4, 0.5, 0.8), Color::from_rgb(0.1, 0.35, 1.0), false);

        // 6. Забор загона
        let mut wood_mat = StandardMaterial3D::new_gd();
        wood_mat.set_albedo(Color::from_rgb(0.48, 0.31, 0.16));
        let add_plank = |pending: &mut Vec<Gd<Node>>, pos: Vector3, sz: Vector3| {
            let mut bm = BoxMesh::new_gd();
            bm.set_size(sz);
            bm.set_material(&wood_mat);
            let mut mi = MeshInstance3D::new_alloc();
            mi.set_mesh(&bm);
            mi.set_position(pos);
            pending.push(mi.upcast());
        };
        let (px0, px1, pz0, pz1) = (-6.5, 6.5, -30.0, -18.0);
        let mut x = px0;
        while x <= px1 + 0.1 {
            add_plank(&mut pending, Vector3::new(x, 0.75, pz0), Vector3::new(0.16, 1.5, 0.16));
            add_plank(&mut pending, Vector3::new(x, 0.75, pz1), Vector3::new(0.16, 1.5, 0.16));
            if x < px1 - 0.5 {
                add_plank(&mut pending, Vector3::new(x + 1.08, 0.48, pz0), Vector3::new(2.18, 0.20, 0.08));
                add_plank(&mut pending, Vector3::new(x + 1.08, 1.10, pz0), Vector3::new(2.18, 0.20, 0.08));
                add_plank(&mut pending, Vector3::new(x + 1.08, 0.48, pz1), Vector3::new(2.18, 0.20, 0.08));
                add_plank(&mut pending, Vector3::new(x + 1.08, 1.10, pz1), Vector3::new(2.18, 0.20, 0.08));
            }
            x += 2.16;
        }
        let mut z = pz0;
        while z <= pz1 + 0.1 {
            add_plank(&mut pending, Vector3::new(px0, 0.75, z), Vector3::new(0.16, 1.5, 0.16));
            add_plank(&mut pending, Vector3::new(px1, 0.75, z), Vector3::new(0.16, 1.5, 0.16));
            if z < pz1 - 0.5 {
                add_plank(&mut pending, Vector3::new(px0, 0.48, z + 1.0), Vector3::new(0.08, 0.20, 2.02));
                add_plank(&mut pending, Vector3::new(px0, 1.10, z + 1.0), Vector3::new(0.08, 0.20, 2.02));
                add_plank(&mut pending, Vector3::new(px1, 0.48, z + 1.0), Vector3::new(0.08, 0.20, 2.02));
                add_plank(&mut pending, Vector3::new(px1, 1.10, z + 1.0), Vector3::new(0.08, 0.20, 2.02));
            }
            z += 2.0;
        }
        add_wall(&mut pending, Vector3::new(0.0, 0.8, pz0), Vector3::new(13.2, 1.6, 0.4));
        add_wall(&mut pending, Vector3::new(0.0, 0.8, pz1), Vector3::new(13.2, 1.6, 0.4));
        add_wall(&mut pending, Vector3::new(px0, 0.8, -24.0), Vector3::new(0.4, 1.6, 12.2));
        add_wall(&mut pending, Vector3::new(px1, 0.8, -24.0), Vector3::new(0.4, 1.6, 12.2));

        // Сарай и дерево (.glb)
        let mut rl = ResourceLoader::singleton();
        let mut add_prop_glb = |path: &str, name: &str, pos: Vector3| {
            if !rl.exists(path) { return; }
            if let Some(sc) = rl.load(path).and_then(|r| r.try_cast::<PackedScene>().ok()) {
                if let Some(mut node) = sc.instantiate().and_then(|n| n.try_cast::<Node3D>().ok()) {
                    node.set_name(name);
                    node.set_position(pos);
                    self.base_mut().add_child(&node);
                    add_trimesh_recursive(&node.upcast::<Node>());
                }
            }
        };
        add_prop_glb("res://assets/resources/other/shed.glb", "Shed", Vector3::new(14.5, 0.0, -10.0));
        add_prop_glb("res://assets/resources/other/tree.glb", "Tree", Vector3::new(-15.5, 0.0, -38.0));

        // Ахмед, 2 Миньона и Кабаньезе
        let mut add_billboard = |path: &str, name: &str, pos: Vector3, target_h: f32| {
            if !rl.exists(path) { return; }
            if let Some(tex) = rl.load(path).and_then(|r| r.try_cast::<Texture2D>().ok()) {
                let mut sp = Sprite3D::new_alloc();
                sp.set_name(name);
                sp.set_texture(&tex);
                sp.set_billboard_mode(BillboardMode::FIXED_Y);
                let h = tex.get_height() as f32;
                if h > 0.0 { sp.set_pixel_size(target_h / h); }
                sp.set_position(pos);
                self.base_mut().add_child(&sp);
            }
        };
        add_billboard("res://assets/images/ahmed.png", "Ahmed", Vector3::new(0.0, 1.35, 24.0), 2.7);
        add_billboard("res://assets/images/micro.jpg", "Minion0", Vector3::new(-3.5, 1.10, -21.0), 2.1);
        add_billboard("res://assets/images/micro.jpg", "Minion1", Vector3::new(3.5, 1.10, -27.0), 2.1);

        if rl.exists("res://assets/resources/kabanyeze/kabanyeze.glb") {
            if let Some(sc) = rl.load("res://assets/resources/kabanyeze/kabanyeze.glb").and_then(|r| r.try_cast::<PackedScene>().ok()) {
                if let Some(mut pig) = sc.instantiate().and_then(|n| n.try_cast::<Node3D>().ok()) {
                    pig.set_name("Kabanyeze");
                    pig.set_position(Vector3::new(0.0, 0.0, -24.0));
                    pig.set_scale(Vector3::new(2.0, 2.0, 2.0));
                    self.base_mut().add_child(&pig);
                }
            }
        }

        if !Engine::singleton().is_editor_hint() {
            let mut gm = GameManager::new_alloc();
            gm.set_name("GameManager");
            self.base_mut().add_child(&gm);
        }
    
        for n in pending {
            self.base_mut().add_child(&n);
        }
    }
}
