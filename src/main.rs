use std::collections::HashSet;
use sfml::graphics::{Color, IntRect, PrimitiveType, RenderStates, RenderTarget, RenderWindow, Texture, Transform, Vertex, VertexBuffer, VertexBufferUsage};
use sfml::window::{ContextSettings, Event, Style};
use std::sync::Arc;
use std::task::Context;
use rusty_spine::{AnimationStateData, Atlas, Bone, SkeletonBinary, SkeletonController, SkeletonJson, SkeletonRenderable, Skin};
use sfml::SfBox;
use sfml::system::{Vector2, Vector2f};
use sfml::graphics::BlendMode as SfmlBlendMode;
use sfml::graphics::blend_mode::{Factor as BlendFactor, Equation as BlendEquation};
use rusty_spine::BlendMode as SpineBlendMode;

struct SavedTexture {
    tex: SfBox<Texture>
}

const BLEND_NORMAL: SfmlBlendMode = SfmlBlendMode {
    color_src_factor: BlendFactor::SrcAlpha,
    color_dst_factor: BlendFactor::OneMinusSrcAlpha,
    color_equation: BlendEquation::Add,
    alpha_src_factor: BlendFactor::SrcAlpha,
    alpha_dst_factor: BlendFactor::OneMinusSrcAlpha,
    alpha_equation: BlendEquation::Add
};

const BLEND_ADDITIVE: SfmlBlendMode = SfmlBlendMode {
    color_src_factor: BlendFactor::SrcAlpha,
    color_dst_factor: BlendFactor::One,
    color_equation: BlendEquation::Add,
    alpha_src_factor: BlendFactor::SrcAlpha,
    alpha_dst_factor: BlendFactor::One,
    alpha_equation: BlendEquation::Add
};

const BLEND_MULTIPLY: SfmlBlendMode = SfmlBlendMode {
    color_src_factor: BlendFactor::DstColor,
    color_dst_factor: BlendFactor::OneMinusSrcAlpha,
    color_equation: BlendEquation::Add,
    alpha_src_factor: BlendFactor::DstColor,
    alpha_dst_factor: BlendFactor::OneMinusSrcAlpha,
    alpha_equation: BlendEquation::Add
};

const BLEND_SCREEN: SfmlBlendMode = SfmlBlendMode {
    color_src_factor: BlendFactor::One,
    color_dst_factor: BlendFactor::OneMinusSrcColor,
    color_equation: BlendEquation::Add,
    alpha_src_factor: BlendFactor::One,
    alpha_dst_factor: BlendFactor::OneMinusSrcColor,
    alpha_equation: BlendEquation::Add
};

fn main() {
    let mut settings: ContextSettings = Default::default();
    settings.antialiasing_level = 16;

    let mut window = RenderWindow::new(
        (2000, 1600),
        "Render test",
        Style::CLOSE,
        &settings,
    );
    println!("used settings: {:?}", window.settings());

    // window.set_framerate_limit(60);

    rusty_spine::extension::set_create_texture_cb(|atlas_page, path| {
        println!("create texture {:?} {:?}", atlas_page, path);
        let mut tex = Texture::new().unwrap();
        tex.load_from_file(path, IntRect::new(0, 0, 0, 0)).unwrap();
        tex.set_smooth(true);
        println!("new texture is {:?}", tex.size());
        atlas_page.renderer_object().set(tex);
    });

    rusty_spine::extension::set_dispose_texture_cb(|atlas_page| unsafe {
        println!("dispose texture {:?}", atlas_page);
        atlas_page.renderer_object().dispose::<Texture>();
    });

    let atlas_path = "cotl/Follower.atlas";
    let skel_path = "cotl/Follower.skel";
    let atlas = Arc::new(Atlas::new_from_file(atlas_path).unwrap());
    let skeleton_binary = SkeletonBinary::new(atlas);
    let skeleton_data = Arc::new(skeleton_binary.read_skeleton_data_file(skel_path).unwrap());

    for anim in skeleton_data.animations() {
        println!("anim {:?}", anim.name());
    }

    for skin in skeleton_data.skins() {
        println!("skin {:?}", skin.name());
    }


    // let animation_state_data = {
    //     let mut asd =AnimationStateData::new(skeleton_data.clone());
    //     asd.set_mix_by_name("walk", "jump", 0.2);
    //     asd.set_mix_by_name("jump", "run", 0.2);
    //     Arc::new(asd)
    // };
    let animation_state_data = Arc::new(AnimationStateData::new(skeleton_data.clone()));
    let mut skeleton_controller =
        SkeletonController::new(skeleton_data.clone(), animation_state_data);

    let mut skin = Skin::new("mixed");
    skin.add_skin(skeleton_data.skins().find(|s| s.name() == "Fennec Fox").unwrap().as_ref());
    skin.add_skin(skeleton_data.skins().find(|s| s.name() == "Clothes/Holiday").unwrap().as_ref());

    skeleton_controller.animation_state.set_animation_by_name(0, "action", true).unwrap();
    skeleton_controller.skeleton.set_skin(&skin);
    skeleton_controller.skeleton.set_to_setup_pose();
    skeleton_controller.skeleton.set_scale([4.0, 4.0]);
    skeleton_controller.skeleton.set_y(1500.0);
    skeleton_controller.skeleton.set_x(900.0);
    Bone::set_y_down(true);

    let color_slots: HashSet<String> = vec![
        "ARM_LEFT_SKIN",
        "ARM_RIGHT_SKIN",
        "LEG_LEFT_SKIN",
        "LEG_RIGHT_SKIN",
        "HEAD_SKIN_BTM",
    ].into_iter().map(|s| s.to_owned()).collect();

    for mut slot in skeleton_controller.skeleton.slots_mut() {
        let n = slot.data().name().to_owned();
        println!("slot: {:?}", n);
        let c = slot.color_mut();
        if color_slots.contains(&n) {
            c.set_r(0.8);
            c.set_b(0.3);
            c.set_g(0.6);
        }
    }

    let mut clock = sfml::system::Clock::start();

    while window.is_open() {

        while let Some(ev) = window.poll_event() {
            match ev {
                Event::Closed => window.close(),
                _ => {}
            }
        }

        let t = clock.elapsed_time();
        clock.restart();

        // println!("{:?}", t.as_seconds());
        skeleton_controller.update(t.as_seconds());
        window.clear(Color::rgba(80, 80, 80, 255));

        let mut render_states = RenderStates::new(
            BLEND_NORMAL,
            Transform::IDENTITY,
            None,
            None
        );

        let renderables = skeleton_controller.renderables();
        for renderable in renderables.iter() {
            let color = Color::rgba(
                (renderable.color.r * 255.0).round() as u8,
                (renderable.color.g * 255.0).round() as u8,
                (renderable.color.b * 255.0).round() as u8,
                (renderable.color.a * 255.0).round() as u8,
            );

            let texture = unsafe { &*(renderable.attachment_renderer_object.unwrap() as *const SfBox<Texture>) };
            let texture_size = texture.size();
            render_states.set_texture(Some(texture));

            render_states.blend_mode = match renderable.blend_mode {
                SpineBlendMode::Normal => BLEND_NORMAL,
                SpineBlendMode::Additive => BLEND_ADDITIVE,
                SpineBlendMode::Multiply => BLEND_MULTIPLY,
                SpineBlendMode::Screen => BLEND_SCREEN,
            };

            let mut vertexes = Vec::new();
            for i in &renderable.indices {
                let v = renderable.vertices[*i as usize];
                let t = renderable.uvs[*i as usize];
                let t = [t[0] * texture_size.x as f32, t[1] * texture_size.y as f32];
                vertexes.push(Vertex::new(
                    Vector2f::new(v[0], v[1]), color, Vector2f::new(t[0], t[1])
                ));
            }

            window.draw_primitives(vertexes.as_slice(), PrimitiveType::TRIANGLES, &render_states);
        }

        window.display();

    }
}
