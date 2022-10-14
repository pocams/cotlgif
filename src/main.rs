use sfml::graphics::{BlendMode, Color, IntRect, PrimitiveType, RenderStates, RenderTarget, RenderWindow, Texture, Transform, Vertex, VertexBuffer, VertexBufferUsage};
use sfml::window::{ContextSettings, Event, Style};
use std::sync::Arc;
use std::task::Context;
use rusty_spine::{AnimationStateData, Atlas, Bone, SkeletonController, SkeletonJson, SkeletonRenderable};
use sfml::SfBox;
use sfml::system::{Vector2, Vector2f};

struct SavedTexture {
    tex: SfBox<Texture>
}

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
        println!("new texture is {:?}", tex.size());
        atlas_page.renderer_object().set(tex);
    });

    rusty_spine::extension::set_dispose_texture_cb(|atlas_page| unsafe {
        println!("dispose texture {:?}", atlas_page);
        atlas_page.renderer_object().dispose::<Texture>();
    });

    let atlas_path = "spineboy/spineboy.atlas";
    let json_path = "spineboy/spineboy-pro.json";
    let atlas = Arc::new(Atlas::new_from_file(atlas_path).unwrap());
    let skeleton_json = SkeletonJson::new(atlas);
    let skeleton_data = Arc::new(skeleton_json.read_skeleton_data_file(json_path).unwrap());
    // let animation_state_data = {
    //     let mut asd =AnimationStateData::new(skeleton_data.clone());
    //     asd.set_mix_by_name("walk", "jump", 0.2);
    //     asd.set_mix_by_name("jump", "run", 0.2);
    //     Arc::new(asd)
    // };
    let animation_state_data = Arc::new(AnimationStateData::new(skeleton_data.clone()));
    let mut skeleton_controller =
        SkeletonController::new(skeleton_data.clone(), animation_state_data);
    skeleton_controller.animation_state.set_animation_by_name(0, "walk", true).unwrap();
    skeleton_controller.animation_state.add_animation_by_name(1, "jump", false, 3.0).unwrap();
    skeleton_controller.animation_state.add_empty_animation(1, 1.0, 0.5);
    skeleton_controller.skeleton.set_scale([1.8, 1.8]);
    skeleton_controller.skeleton.set_y(1500.0);
    skeleton_controller.skeleton.set_x(400.0);
    Bone::set_y_down(true);

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
        window.clear(Color::TRANSPARENT);

        let mut tex = Texture::new().unwrap();
        tex.load_from_file("spineboy/spineboy.png", IntRect::new(0, 0, 0, 0)).unwrap();

        let renderables = skeleton_controller.renderables();
        for renderable in renderables.iter() {
            let tex = unsafe { &*(renderable.attachment_renderer_object.unwrap() as *const SfBox<Texture>) };
            let texsz = tex.size();

            let mut vertexes = Vec::new();
            for i in &renderable.indices {
                let v = renderable.vertices[*i as usize];
                let t = renderable.uvs[*i as usize];
                let t = [t[0] * texsz.x as f32, t[1] * texsz.y as f32];
                let c = Color::rgba(
                    (renderable.color.r * 255.0).round() as u8,
                    (renderable.color.g * 255.0).round() as u8,
                    (renderable.color.b * 255.0).round() as u8,
                    (renderable.color.a * 255.0).round() as u8,
                );
                vertexes.push(Vertex::new(
                    Vector2f::new(v[0], v[1]), c, Vector2f::new(t[0], t[1])
                ));
                // vertexes.push(Vertex::with_pos_coords(
                //     Vector2f::new(v[0], v[1]), Vector2f::new(t[0], t[1])
                // ));
            }

            let st = RenderStates::new(
                // FIXME
                BlendMode::default(),
                Transform::IDENTITY,
                Some(&tex),
                None
            );

            window.draw_primitives(vertexes.as_slice(), PrimitiveType::TRIANGLES, &st);

            // let slot = skeleton_controller
            //     .skeleton
            //     .slot_at_index(renderable.slot_index)
            //     .unwrap();

        }

        window.display();

    }
}
