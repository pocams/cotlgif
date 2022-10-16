use std::collections::HashMap;
use std::io::BufWriter;
use std::path::Path;
use std::process::abort;
use std::sync::Arc;
use std::{mem, thread};
use std::num::NonZeroU32;
use fast_image_resize::PixelType;
use gifski::progress::NoProgress;
use gifski::Settings;
use imgref::{Img, ImgRef, ImgVec};
use rusty_spine::{Atlas, SkeletonBinary, SkeletonJson, Color, AnimationStateData, SkeletonController, Skeleton, SkeletonData, Bone};
use sfml::graphics::{IntRect, PrimitiveType, RenderStates, RenderTarget, RenderTexture, Texture, Transform, Vertex, Color as SfmlColor};
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, info};
use once_cell::sync::OnceCell;
use rgb::FromSlice;
use serde::Serialize;
use sfml::SfBox;
use sfml::system::Vector2f;
use sfml::graphics::BlendMode as SfmlBlendMode;
use sfml::graphics::blend_mode::{Factor as BlendFactor, Equation as BlendEquation};
use rusty_spine::BlendMode as SpineBlendMode;

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


fn spine_init() {
    static SPINE_STATE: OnceCell<()> = OnceCell::new();
    SPINE_STATE.get_or_init(|| {
        // We can't panic and unwind through C code, so just abort if we run into errors
        // in the callbacks

        rusty_spine::extension::set_create_texture_cb(|atlas_page, path| {
            info!("Loading texture from {path}");

            let mut tex = Texture::new().unwrap_or_else(|| {
                eprintln!("Error creating new texture");
                abort();
            });

            tex.load_from_file(path, IntRect::new(0, 0, 0, 0)).unwrap_or_else(|e| {
                eprintln!("Error loading texture from {path}: {e:?}");
                abort();
            });

            tex.set_smooth(true);

            atlas_page.renderer_object().set(tex);
        });

        rusty_spine::extension::set_dispose_texture_cb(|atlas_page| unsafe {
            atlas_page.renderer_object().dispose::<Texture>();
        });

        ()
    });
}

#[derive(Serialize)]
pub struct Skin {
    pub name: String,
}

#[derive(Serialize)]
pub struct Animation {
    pub name: String,
    pub duration: f32
}

#[derive(Serialize)]
pub struct Actor {
    pub name: String,
    pub description: String,
    pub skins: Vec<Skin>,
    pub animations: Vec<Animation>,
    #[serde(skip)]
    atlas: Arc<Atlas>,
    #[serde(skip)]
    skeleton_data: Arc<SkeletonData>,
    #[serde(skip)]
    animation_state_data: Arc<AnimationStateData>,
}

#[derive(Debug)]
pub struct RenderParameters {
    pub skins: Vec<String>,
    pub animation: String,
    pub scale: f32,
    pub antialiasing: u32,
    pub start_time: f32,
    pub end_time: f32,
    pub frame_delay: f32,
    pub background_color: Color,
    pub color1: Option<Color>,
    pub color2: Option<Color>,
}

impl Actor {
    pub async fn new<P: AsRef<Path>>(name: String, description: String, skeleton_path: P, atlas_path: P) -> color_eyre::Result<Actor> {
        spine_init();

        let atlas = Arc::new(Atlas::new_from_file(atlas_path)?);

        let skeleton_data = match skeleton_path.as_ref().extension().map(|f| f.to_string_lossy()) {
            Some(ext) if ext == "json" => {
                let skeleton_json = SkeletonJson::new(atlas.clone());
                Arc::new(skeleton_json.read_skeleton_data_file(skeleton_path)?)
            },
            _ => {
                let skeleton_binary = SkeletonBinary::new(atlas.clone());
                Arc::new(skeleton_binary.read_skeleton_data_file(skeleton_path)?)
            }
        };

        let animation_state_data = Arc::new(AnimationStateData::new(skeleton_data.clone()));

        let mut skins = Vec::with_capacity(skeleton_data.skins_count() as usize);
        for skin in skeleton_data.skins() {
            skins.push(Skin {
                name: skin.name().to_owned()
            })
        };

        let mut animations = Vec::with_capacity(skeleton_data.animations_count() as usize);
        for animation in skeleton_data.animations() {
            animations.push(Animation {
                name: animation.name().to_owned(),
                duration: animation.duration()
            })
        };

        Ok(Actor {
            name,
            description,
            skins,
            animations,
            atlas,
            skeleton_data,
            animation_state_data
        })
    }

    fn apply_color(&self, controller: &mut SkeletonController, color1: &Option<Color>, color2: &Option<Color>) {
        let color1 = color1.unwrap_or(Color { r: 1.0, g: 1.0, b: 1.0, a: 1.0 });
        let color2 = color2.unwrap_or(Color { r: 1.0, g: 1.0, b: 1.0, a: 1.0 });
        if self.name == "follower" {
            for mut slot in controller.skeleton.slots_mut() {
                match slot.data().name() {
                    "ARM_LEFT_SKIN" |
                    "ARM_RIGHT_SKIN" |
                    "LEG_LEFT_SKIN" |
                    "LEG_RIGHT_SKIN" |
                    "HEAD_SKIN_BTM" => {
                        let c = slot.color_mut();
                        c.set_r(color1.r);
                        c.set_g(color1.g);
                        c.set_b(color1.b);
                        c.set_a(color1.a);
                    },
                    "HEAD_SKIN_TOP" => {
                        let c = slot.color_mut();
                        c.set_r(color2.r);
                        c.set_g(color2.g);
                        c.set_b(color2.b);
                        c.set_a(color2.a);
                    },
                    _ => {}
                }
            }
        }
    }

    pub async fn render_gif(&self, params: RenderParameters) -> Vec<u8> {
        let mut controller = SkeletonController::new(self.skeleton_data.clone(), self.animation_state_data.clone());

        let aa_factor = if params.antialiasing == 0 { 1 } else { params.antialiasing };
        let render_scale = params.scale * aa_factor as f32;
        debug!("Scale x{}, AA x{}, total render scale {}", params.scale, aa_factor, render_scale);

        debug!("{}-{} will be {} frames ({} fps)", params.start_time, params.end_time, (params.end_time - params.start_time) / params.frame_delay, 1.0 / params.frame_delay);

        let background_color = SfmlColor::rgba(
            (params.background_color.r * 255.0).round() as u8,
            (params.background_color.g * 255.0).round() as u8,
            (params.background_color.b * 255.0).round() as u8,
            (params.background_color.a * 255.0).round() as u8,
        );

        let mut skin = rusty_spine::Skin::new("render_gif");
        for skin_name in &params.skins {
            skin.add_skin(controller.skeleton.data().skins().find(|s| s.name() == skin_name).unwrap().as_ref());
        }

        debug!("Created skin from {} requested", params.skins.len());

        controller.skeleton.set_skin(&skin);
        controller.skeleton.set_scale([render_scale, render_scale]);
        controller.skeleton.set_to_setup_pose();

        debug!("Applying colors: {:?}, {:?}", params.color1, params.color2);
        self.apply_color(&mut controller, &params.color1, &params.color2);

        debug!("Finding bounding box...");
        // Run through the animation once and grab all the min/max X and Y. This is actually pretty
        // fast (tens of ms) compared to all the other crazy shit we're doing
        let mut min_x = f32::MAX;
        let mut max_x = f32::MIN;
        let mut min_y = f32::MAX;
        let mut max_y = f32::MIN;
        controller.animation_state.clear_tracks();
        controller.animation_state.set_animation_by_name(0, &params.animation, true).unwrap();
        controller.update(params.start_time);
        let mut time = params.start_time;
        while time <= params.end_time {
            for r in controller.renderables() {
                if r.color.a < 0.001 { continue };
                for [x, y] in r.vertices {
                    min_x = min_x.min(x);
                    min_y = min_y.min(y);
                    max_x = max_x.max(x);
                    max_y = max_y.max(y);
                }
            }
            controller.update(params.frame_delay);
            time += params.frame_delay;
        }
        debug!("Bounding box: ({min_x}, {min_y}) - ({max_x}, {max_y})");

        let render_width = (max_x - min_x).ceil() as u32;
        let render_height = (max_y - min_y).ceil() as u32;
        let final_width = render_width / aa_factor;
        let final_height = render_height / aa_factor;
        debug!("Final scale is {}x{}, render will be {}x{}", final_width, final_height, render_width, render_height);

        controller.skeleton.set_x(-min_x);
        controller.skeleton.set_y(-min_y);

        let mut target = RenderTexture::new(render_width, render_height).unwrap();

        let mut render_states = RenderStates::new(
            BLEND_NORMAL,
            Transform::IDENTITY,
            None,
            None
        );

        debug!("Initializing gifski..");
        let (gs_collector, gs_writer) = gifski::new(Settings::default()).unwrap();
        let writer_thread = thread::spawn(move || {
            let mut buf = Vec::new();
            gs_writer.write(&mut buf, &mut NoProgress {}).unwrap();
            buf
        });

        controller.animation_state.clear_tracks();
        controller.animation_state.set_animation_by_name(0, &params.animation, true).unwrap();
        controller.update(params.start_time);
        let mut time = params.start_time;
        let mut elapsed_time = 0.0;
        let mut frame = 0;
        while time <= params.end_time {
            debug!("Processing frame {}", frame);
            target.clear(background_color);

            let renderables = controller.renderables();
            let mut min_x = f32::MAX;
            let mut max_x = f32::MIN;
            for renderable in renderables.iter() {
                if renderable.color.a < 0.001 { continue };
                let color = SfmlColor::rgba(
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

                let mut vertexes = Vec::with_capacity(renderable.indices.len());
                for i in &renderable.indices {
                    let v = renderable.vertices[*i as usize];
                    let t = renderable.uvs[*i as usize];
                    let t = [t[0] * texture_size.x as f32, t[1] * texture_size.y as f32];
                    vertexes.push(Vertex::new(
                        Vector2f::new(v[0], v[1]), color, Vector2f::new(t[0], t[1])
                    ));

                    min_x = min_x.min(renderable.vertices[*i as usize][0]);
                    max_x = max_x.max(renderable.vertices[*i as usize][0]);
                }

                target.draw_primitives(vertexes.as_slice(), PrimitiveType::TRIANGLES, &render_states);
            }
            debug!("min x {min_x} max x {max_x}");
            debug!("Rendered {} objects", renderables.len());

            // Sucks a bit to have to copy the image twice, but sfml Image doesn't have a way to
            // give us ownership of the pixel data.
            let mut image = target.texture().copy_to_image().unwrap().pixel_data().to_vec();

            if aa_factor == 1 {
                debug!("Skipping antialiasing");
                // No antialiasing, so just use the image as-is
                let img = ImgVec::new(Vec::from(image.as_rgba()), render_width as usize, render_height as usize);
                gs_collector.add_frame_rgba(frame, img, elapsed_time).unwrap();
            } else {
                debug!("AA: resizing to {}x{}", final_width, final_height);
                // RenderTexture lives on the GPU, so this could be done quicker with some GPU-based
                // algorithm, but SFML doesn't have fancy resize algorithms on the GPU right now.
                let mut resize_img = fast_image_resize::Image::from_vec_u8(
                    NonZeroU32::new(render_width).unwrap(),
                    NonZeroU32::new(render_height).unwrap(),
                    image,
                    PixelType::U8x4
                ).unwrap();

                // According to the docs this is required
                let alpha_mul_div = fast_image_resize::MulDiv::default();
                alpha_mul_div
                    .multiply_alpha_inplace(&mut resize_img.view_mut())
                    .unwrap();

                let mut destination_image = fast_image_resize::Image::new(
                    NonZeroU32::new(final_width).unwrap(),
                    NonZeroU32::new(final_height).unwrap(),
                    PixelType::U8x4
                );

                let mut resizer = fast_image_resize::Resizer::new(
                    fast_image_resize::ResizeAlg::Convolution(fast_image_resize::FilterType::Lanczos3),
                );
                resizer.resize(&resize_img.view(), &mut destination_image.view_mut()).unwrap();

                alpha_mul_div.divide_alpha_inplace(&mut destination_image.view_mut()).unwrap();
                debug!("AA: resize finished");

                let img = ImgVec::new(Vec::from(destination_image.into_vec().as_rgba()), final_width as usize, final_height as usize);
                gs_collector.add_frame_rgba(frame, img, elapsed_time).unwrap();
            }

            frame += 1;
            time += params.frame_delay;
            elapsed_time += params.frame_delay as f64;
            controller.update(params.frame_delay);
        }
        drop(gs_collector);
        debug!("Rendering finished, waiting for gif...");
        let buf = writer_thread.join().unwrap();
        debug!("Returning {}-byte GIF", buf.len());
        buf
    }
}
