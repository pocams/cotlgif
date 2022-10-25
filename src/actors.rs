use std::{io, thread};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::num::NonZeroU32;
use std::path::Path;
use std::process::{abort, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use color_eyre::eyre::{ErrReport, eyre};
use color_eyre::Report;
use fast_image_resize::PixelType;
use gifski::progress::NoProgress;
use gifski::Settings;
use imgref::ImgVec;
use once_cell::sync::OnceCell;
use png::{BitDepth, ColorType};
use regex::Regex;
use rgb::FromSlice;
use rusty_spine::{AnimationStateData, Atlas, Color, SkeletonBinary, SkeletonController, SkeletonData, SkeletonJson};
use rusty_spine::BlendMode as SpineBlendMode;
use serde::Serialize;
use serde_json::json;
use sfml::graphics::{Color as SfmlColor, IntRect, PrimitiveType, RenderStates, RenderTarget, RenderTexture, Texture, Transform, Vertex};
use sfml::graphics::blend_mode::{Equation as BlendEquation, Factor as BlendFactor};
use sfml::graphics::BlendMode as SfmlBlendMode;
use sfml::SfBox;
use sfml::system::Vector2f;
use tracing::{debug, info, warn};

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

static SKIN_NUMBER: AtomicI64 = AtomicI64::new(0);

pub trait Slug {
    fn slugify_string(s: &str) -> String {
        static NON_ALPHA: OnceCell<Regex> = OnceCell::new();
        static LOWER_UPPER: OnceCell<Regex> = OnceCell::new();
        let non_alpha = NON_ALPHA.get_or_init(|| Regex::new(r"[^A-Za-z0-9]").unwrap());
        let lower_upper = LOWER_UPPER.get_or_init(|| Regex::new(r"([a-z])([A-Z])").unwrap());
        let s = non_alpha.replace(s, "-");
        let s = lower_upper.replace(&s, "$2-$1");
        s.to_ascii_lowercase().to_owned()
    }

    fn slug(&self) -> String;
}

fn only_head_includes(slot_name: &str) -> bool {
    static ONLY_HEAD: OnceCell<Regex> = OnceCell::new();
    let only_head = ONLY_HEAD.get_or_init(|| Regex::new(
        r"^(HEAD_SKIN_.*|MARKINGS|EXTRA_(TOP|BTM)|Face Colouring|MOUTH|HOOD|EYE_.*|HeadAccessory|HAT|MASK|Tear\d|Crown_Particle\d)$"
    ).unwrap());

    only_head.is_match(slot_name)
}

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

// We want to own the image because we're going to mutate it before we resize it
fn resize(from: (usize, usize), to: (usize, usize), image: Vec<u8>) -> Vec<u8> {
    debug!("AA: resizing to {}x{}", to.0, to.1);
    // RenderTexture lives on the GPU, so this could be done quicker with some GPU-based
    // algorithm, but SFML doesn't have fancy resize algorithms on the GPU right now.

    let mut resize_img = fast_image_resize::Image::from_vec_u8(
        NonZeroU32::new(from.0 as u32).unwrap(),
        NonZeroU32::new(from.1 as u32).unwrap(),
        image.to_vec(),
        PixelType::U8x4
    ).unwrap();

    // According to the docs this is required
    let alpha_mul_div = fast_image_resize::MulDiv::default();
    alpha_mul_div
        .multiply_alpha_inplace(&mut resize_img.view_mut())
        .unwrap();

    let mut destination_image = fast_image_resize::Image::new(
        NonZeroU32::new(to.0 as u32).unwrap(),
        NonZeroU32::new(to.1 as u32).unwrap(),
        PixelType::U8x4
    );

    let mut resizer = fast_image_resize::Resizer::new(
        fast_image_resize::ResizeAlg::Convolution(fast_image_resize::FilterType::Lanczos3),
    );
    resizer.resize(&resize_img.view(), &mut destination_image.view_mut()).unwrap();

    alpha_mul_div.divide_alpha_inplace(&mut destination_image.view_mut()).unwrap();
    debug!("AA: resize finished");
    destination_image.into_vec()
}

#[derive(Serialize)]
pub struct Skin {
    pub name: String,
}

impl Slug for Animation {
    fn slug(&self) -> String {
        <Animation as Slug>::slugify_string(&self.name)
    }
}

impl Skin {
    fn is_spoiler(&self, actor: &str) -> bool {
        static HIDE_FOLLOWER: OnceCell<Regex> = OnceCell::new();
        static HIDE_PLAYER: OnceCell<Regex> = OnceCell::new();
        let hide_follower = HIDE_FOLLOWER.get_or_init(|| Regex::new(
            r"^(Archer|Badger\d?|Raccoon\d?|Rhino\d?|.*HorseTown.*|Clothes/(Hooded_Lvl[2345]|NoHouse.*|Rags.*|Robes.*|Warrior)|Hats/Chef|HorseKing|default)$"
        ).unwrap());
        let hide_player = HIDE_PLAYER.get_or_init(|| Regex::new(
            "^(Goat|Owl|Snake|default|effects-top)$"
        ).unwrap());

        if actor == "follower" {
            hide_follower.is_match(&self.name)
        } else if actor == "player" {
            hide_player.is_match(&self.name)
        } else if actor == "ratau" {
            false
        } else {
            panic!("Unexpected actor for is_spoiler: {}", actor)
        }
    }
}

#[derive(Serialize)]
pub struct Animation {
    pub name: String,
    pub duration: f32
}

impl Slug for Skin {
    fn slug(&self) -> String {
        <Skin as Slug>::slugify_string(&self.name)
    }
}

impl Animation {
    fn is_spoiler(&self, actor: &str) -> bool {
        static HIDE_FOLLOWER: OnceCell<Regex> = OnceCell::new();
        static HIDE_PLAYER: OnceCell<Regex> = OnceCell::new();
        let hide_follower = HIDE_FOLLOWER.get_or_init(|| Regex::new(
            r"^(Buildings/(enter-portal|exit-portal|portal-loop)|Emotions/emotion-insane|Fishing/.*|Food/food-fillbowl|Ghost/.*|Insane/.*|OldStuff/.*|Possessed/.*|Prison/(stocks-dead.*|stocks-die.*)|TESTING|astrologer|attack-.*|ball|barracks-training|bend-knee|bow-attack.*|bubble.*|convertBUBBLE|cook|devotion/devotion-refused?|hurt-.*|scarify|spawn-in-base-old|studying|sword-.*)$"
        ).unwrap());
        let hide_player = HIDE_PLAYER.get_or_init(|| Regex::new(
            r"^(altar-hop|attack-.*OLD|.*blunderbuss.*|attack-combo3-axe-test|.*chalice.*|grabber-.*|grapple-.*|intro/goat-.*|lute-.*|oldstuff/.*|shield.*|slide|specials.*|teleport-.*|testing|throw|unconverted.*|warp-out-down-(alt|old)|zipline.*)$"
        ).unwrap());

        if actor == "follower" {
            hide_follower.is_match(&self.name)
        } else if actor == "player" {
            hide_player.is_match(&self.name)
        } else if actor == "ratau" {
            false
        } else {
            panic!("Unexpected actor for is_spoiler: {}", actor)
        }
    }
}

#[derive(Serialize)]
pub struct Actor {
    pub name: String,
    pub description: String,
    pub skins: Vec<Skin>,
    pub animations: Vec<Animation>,
    #[serde(skip)]
    #[allow(dead_code)]
    atlas: Arc<Atlas>,
    #[serde(skip)]
    skeleton_data: Arc<SkeletonData>,
    #[serde(skip)]
    animation_state_data: Arc<AnimationStateData>,
    #[serde(skip)]
    actor_mutex: std::sync::Mutex<()>
}

impl Actor {
    pub fn serialize_without_spoilers(&self) -> serde_json::Value {
        let skins: Vec<serde_json::Value> = self.skins.iter().filter(|s| !s.is_spoiler(&self.name)).map(|s| json!({"name": s.name})).collect();
        let animations: Vec<serde_json::Value> = self.animations.iter().filter(|a| !a.is_spoiler(&self.name)).map(|a| json!({"name": a.name, "duration": a.duration})).collect();
        json!({
            "name": self.name,
            "description": self.description,
            "skins": skins,
            "animations": animations
        })
    }
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
    pub background_colour: Color,
    pub slot_colours: HashMap<String, Color>,
    pub only_head: bool,
}

impl RenderParameters {
    fn render_scale(&self) -> f32 {
        let aa_factor = if self.antialiasing == 0 { 1 } else { self.antialiasing };
        self.scale * aa_factor as f32
    }
}

#[derive(Debug)]
struct PreparedRenderParameters {
    parameters: RenderParameters,
    frame_count: u32,
    render_width: usize,
    render_height: usize,
    final_width: usize,
    final_height: usize,
    x_offset: f32,
    y_offset: f32,
}

struct Frame<'a> {
    frame_number: u32,
    pixel_data: &'a [u8],
    width: usize,
    height: usize,
    timestamp: f64
}

struct ChannelWriter {
    sender: Option<futures_channel::mpsc::UnboundedSender<Result<Vec<u8>, Report>>>
}

impl ChannelWriter {
    fn new(sender: futures_channel::mpsc::UnboundedSender<Result<Vec<u8>, Report>>) -> ChannelWriter {
        ChannelWriter { sender: Some(sender) }
    }
}

impl io::Write for ChannelWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.sender.as_ref().unwrap().unbounded_send(Ok(buf.to_vec())).map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "Receiver closed"))?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        drop(self.sender.take());
        Ok(())
    }
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
            animation_state_data,
            actor_mutex: std::sync::Mutex::new(())
        })
    }

    fn prepare_render(&self, params: RenderParameters) -> color_eyre::Result<PreparedRenderParameters> {
        let _guard = self.actor_mutex.lock().unwrap();

        let mut controller = SkeletonController::new(self.skeleton_data.clone(), self.animation_state_data.clone());
        let render_scale = params.render_scale();
        let aa_factor = if params.antialiasing == 0 { 1 } else { params.antialiasing };

        debug!("Scale x{}, AA x{}, total render scale {}", params.scale, aa_factor, render_scale);

        debug!("{}-{} will be {} frames ({} fps)", params.start_time, params.end_time, (params.end_time - params.start_time) / params.frame_delay, 1.0 / params.frame_delay);

        // can we avoid having to make the skin twice?
        let skin_name = format!("{}", SKIN_NUMBER.fetch_add(1, Ordering::SeqCst));
        let mut skin = rusty_spine::Skin::new(&skin_name);
        for skin_name in &params.skins {
            skin.add_skin(controller.skeleton.data().skins().find(|s| s.name() == skin_name).unwrap().as_ref());
        }

        controller.skeleton.set_skin(&skin);
        controller.skeleton.set_scale([render_scale, render_scale]);
        controller.skeleton.set_to_setup_pose();

        for slot in controller.skeleton.slots() {
            debug!("slot: {}", slot.data().name());
        }

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
        let mut frame_count = 0;
        while time <= params.end_time {
            for r in controller.renderables().iter() {
                if r.color.a < 0.001 { continue };
                if params.only_head {
                    let slot = controller.skeleton.slot_at_index(r.slot_index).unwrap();
                    if !only_head_includes(slot.data().name()) {
                        debug!("skip (only head): {}", slot.data().name());
                        continue
                    }
                }
                for [x, y] in &r.vertices {
                    min_x = min_x.min(*x);
                    min_y = min_y.min(*y);
                    max_x = max_x.max(*x);
                    max_y = max_y.max(*y);
                }
            }
            controller.update(params.frame_delay);
            time += params.frame_delay;
            frame_count += 1;
        }
        debug!("Bounding box: ({min_x}, {min_y}) - ({max_x}, {max_y})");

        let render_width = (max_x - min_x).ceil() as usize;
        let render_height = (max_y - min_y).ceil() as usize;
        let final_width = render_width / aa_factor as usize;
        let final_height = render_height / aa_factor as usize;
        if render_width == 0 || render_height == 0 || final_width == 0 || final_height == 0 {
            return Err(eyre!("Render failed, zero-size image"));
        }
        debug!("Final scale is {}x{}, render will be {}x{}", final_width, final_height, render_width, render_height);

        Ok(PreparedRenderParameters {
            parameters: params,
            frame_count,
            render_width,
            render_height,
            final_width,
            final_height,
            x_offset: -min_x,
            y_offset: -min_y,
        })
    }

    fn render(&self, prepared_params: PreparedRenderParameters, mut frame_callback: impl FnMut(&Frame) -> Result<(), ErrReport>) {
        let _guard = self.actor_mutex.lock().unwrap();

        let mut controller = SkeletonController::new(self.skeleton_data.clone(), self.animation_state_data.clone());

        let params = prepared_params.parameters;
        let render_scale = params.render_scale();

        let background_colour = SfmlColor::rgba(
            (params.background_colour.r * 255.0).round() as u8,
            (params.background_colour.g * 255.0).round() as u8,
            (params.background_colour.b * 255.0).round() as u8,
            (params.background_colour.a * 255.0).round() as u8,
        );

        let skin_name = format!("{}", SKIN_NUMBER.fetch_add(1, Ordering::SeqCst));
        let mut skin = rusty_spine::Skin::new(&skin_name);
        for skin_name in &params.skins {
            skin.add_skin(controller.skeleton.data().skins().find(|s| s.name() == skin_name).unwrap().as_ref());
        }

        debug!("Created skin from {} requested", params.skins.len());

        controller.skeleton.set_skin(&skin);
        controller.skeleton.set_scale([render_scale, render_scale]);
        controller.skeleton.set_to_setup_pose();

        // Only follower has slot colours
        if self.name == "follower" {
            for mut slot in controller.skeleton.slots_mut() {
                if let Some(colour) = &params.slot_colours.get(slot.data().name()) {
                    let c = slot.color_mut();
                    c.set_r(colour.r);
                    c.set_g(colour.g);
                    c.set_b(colour.b);
                    c.set_a(colour.a);
                }
            }
        }

        controller.skeleton.set_x(prepared_params.x_offset);
        controller.skeleton.set_y(prepared_params.y_offset);

        let mut target = RenderTexture::new(prepared_params.render_width as u32, prepared_params.render_height as u32).unwrap();

        let mut render_states = RenderStates::new(
            BLEND_NORMAL,
            Transform::IDENTITY,
            None,
            None
        );

        controller.animation_state.clear_tracks();
        controller.animation_state.set_animation_by_name(0, &params.animation, true).unwrap();
        controller.update(params.start_time);
        let mut time = params.start_time;
        let mut elapsed_time = 0.0;
        let mut frame = 0;
        while time <= params.end_time {
            // debug!("Processing frame {}", frame);
            target.clear(background_colour);

            let renderables = controller.renderables();
            for renderable in renderables.iter() {
                if renderable.color.a < 0.001 { continue };
                if params.only_head {
                    let slot = controller.skeleton.slot_at_index(renderable.slot_index).unwrap();
                    if !only_head_includes(slot.data().name()) {
                        debug!("skip (only head): {}", slot.data().name());
                        continue
                    }
                }

                let colour = SfmlColor::rgba(
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
                        Vector2f::new(v[0], v[1]), colour, Vector2f::new(t[0], t[1])
                    ));
                }

                target.draw_primitives(vertexes.as_slice(), PrimitiveType::TRIANGLES, &render_states);
            }
            // debug!("Rendered {} objects", renderables.len());

            // Sucks a bit to have to copy the image twice, but sfml Image doesn't have a way to
            // give us ownership of the pixel data.
            let image = target.texture().copy_to_image().unwrap().pixel_data().to_vec();

            let raw_image = if params.antialiasing <= 1 {
                // debug!("Skipping antialiasing");
                // No antialiasing, so just use the image as-is
                image
            } else {
                resize((prepared_params.render_width, prepared_params.render_height), (prepared_params.final_width, prepared_params.final_height), image)
            };

            let f = Frame {
                frame_number: frame,
                pixel_data: &raw_image,
                width: prepared_params.final_width,
                height: prepared_params.final_height,
                timestamp: elapsed_time
            };

            if let Err(e) = frame_callback(&f) {
                warn!("frame callback failed {:?}, aborting render", e);
                break;
            }

            frame += 1;
            time += params.frame_delay;
            elapsed_time += params.frame_delay as f64;
            controller.update(params.frame_delay);
        }
        info!("Finished rendering");
    }

    pub fn render_gif(&self, params: RenderParameters, response_sender: futures_channel::mpsc::UnboundedSender<Result<Vec<u8>, Report>>) -> Result<(), Report> {
        let mut settings = Settings::default();
        settings.quality = 75;
        let (gs_collector, gs_writer) = gifski::new(settings).unwrap();
        let writer = ChannelWriter::new(response_sender);
        thread::spawn(move || {
            if let Err(e) = gs_writer.write(writer, &mut NoProgress {}) {
                warn!("Failed writing output: {:?}", e);
            };
        });

        debug!("render params: {:?}", params);
        let prepared_params = self.prepare_render(params)?;
        debug!("prepared params: {:?}", prepared_params);

        thread::scope(|scope| {
            scope.spawn(|| self.render(prepared_params, |frame| {
                let img = ImgVec::new(Vec::from(frame.pixel_data.as_rgba()), frame.width, frame.height);
                gs_collector.add_frame_rgba(frame.frame_number as usize, img, frame.timestamp)?;
                Ok(())
            }));
        });
        info!("Finished handling request");
        Ok(())
    }

    pub fn render_apng(&self, params: RenderParameters, response_sender: futures_channel::mpsc::UnboundedSender<Result<Vec<u8>, Report>>) -> Result<(), Report> {
        let frame_delay = params.frame_delay;
        let prepared_params = self.prepare_render(params)?;
        debug!("prepared params: {:?}", prepared_params);

        let writer = ChannelWriter::new(response_sender);
        let mut encoder = png::Encoder::new(writer, prepared_params.final_width as u32, prepared_params.final_height as u32);
        encoder.set_color(ColorType::Rgba);
        encoder.set_depth(BitDepth::Eight);
        encoder.set_animated(prepared_params.frame_count, 0)?;
        let mut png_writer = encoder.write_header()?;

        thread::scope(|scope| {
            scope.spawn(|| self.render(prepared_params, |frame| {
                png_writer.set_frame_delay((frame_delay * 1000.0) as u16, 1000)?;
                png_writer.write_image_data(frame.pixel_data)?;
                Ok(())
            }));
        });

        // // Delay data for the final frame
        // png_writer.set_frame_delay((frame_delay * 1000.0) as u16, 1000)?;
        png_writer.finish()?;
        info!("Finished handling request");
        Ok(())
    }

    pub fn render_png(&self, mut params: RenderParameters, response_sender: futures_channel::mpsc::UnboundedSender<Result<Vec<u8>, Report>>) -> Result<(), Report> {
        params.end_time = params.start_time;
        let prepared_params = self.prepare_render(params)?;
        debug!("prepared params: {:?}", prepared_params);

        let writer = ChannelWriter::new(response_sender);
        let mut encoder = png::Encoder::new(writer, prepared_params.final_width as u32, prepared_params.final_height as u32);
        encoder.set_color(ColorType::Rgba);
        encoder.set_depth(BitDepth::Eight);
        let mut png_writer = encoder.write_header()?;

        self.render(prepared_params, |frame| {
            png_writer.write_image_data(frame.pixel_data)?;
            Ok(())
        });

        png_writer.finish()?;
        info!("Finished handling request");
        Ok(())
    }

    pub fn render_ffmpeg(&self, params: RenderParameters, response_sender: futures_channel::mpsc::UnboundedSender<Result<Vec<u8>, Report>>) -> Result<(), Report> {
        let fps = (1.0 / params.frame_delay).round() as u32;
        let prepared_params = self.prepare_render(params)?;
        debug!("prepared params: {:?}", prepared_params);

        let mut ffmpeg = Command::new("ffmpeg")
            .args([
                "-f", "rawvideo",
                "-pixel_format", "rgba",
                "-video_size", &format!("{}x{}", prepared_params.final_width, prepared_params.final_height),
                "-framerate", &format!("{}", fps),
                "-i", "pipe:",
                "-vcodec", "vp8",
                "-deadline", "realtime",
                // Output fragmented video - otherwise we can't write mp4 to a non-seekable medium
                //"-movflags", "frag_keyframe+empty_moov",
                "-an",  // Audio - none
                "-f", "webm",
                "-auto-alt-ref", "0",
                "pipe:",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        thread::scope(move |scope| {
            let mut writer = ChannelWriter::new(response_sender);
            let mut stdin = ffmpeg.stdin.take().unwrap();
            let mut stdout = ffmpeg.stdout.take().unwrap();
            let stderr = ffmpeg.stderr.take().unwrap();

            // Log ffmpeg's stderr
            scope.spawn(move || {
                for line in BufReader::new(stderr).lines() {
                    match line {
                        Ok(l) => debug!("ffmpeg: {}", l),
                        Err(e) => debug!("ffmpeg error: {:?}", e),
                    }
                }
            });

            // Feed rendered frames into ffmpeg's stdin
            scope.spawn(move || self.render(prepared_params, |frame| {
                stdin.write(frame.pixel_data)?;
                Ok(())
            }));

            // Send ffmpeg's stdout straight to the client
            scope.spawn(move || io::copy(&mut stdout, &mut writer));
        });

        info!("Finished handling request");
        Ok(())
    }
}
