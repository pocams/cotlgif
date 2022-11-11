use std::{io, thread};
use std::collections::HashMap;
use std::default::Default;
use std::io::{BufRead, BufReader, Write};
use std::process::{abort, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use color_eyre::eyre::{ErrReport, eyre};
use color_eyre::Report;
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
use sfml::graphics::{Color as SfmlColor, IntRect, PrimitiveType, RenderStates, RenderTarget, RenderTexture, Texture, Transform, Vertex};
use sfml::graphics::blend_mode::{Equation as BlendEquation, Factor as BlendFactor};
use sfml::graphics::BlendMode as SfmlBlendMode;
use sfml::SfBox;
use sfml::system::Vector2f;
use tracing::{debug, info, warn};

use crate::{ActorConfig, resize};
use crate::util::{ChannelWriter, Slug};

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
const PETPET_TIMESCALE: f32 = 3.0;
const PETPET_NATIVE_WIDTH: f32 = 100.0;

fn only_head_includes(slot_name: &str) -> bool {
    static ONLY_HEAD: OnceCell<Regex> = OnceCell::new();
    let only_head = ONLY_HEAD.get_or_init(|| Regex::new(
        r"^(HEAD_SKIN_.*|MARKINGS|EXTRA_(TOP|BTM)|Face Colouring|MOUTH|HOOD|EYE_.*|HeadAccessory|HAT|MASK|Tear\d|Crown_Particle\d)$"
    ).unwrap());

    only_head.is_match(slot_name)
}

fn get_petpet_actor() -> &'static SpineActor {
    static ACTOR: OnceCell<SpineActor> = OnceCell::new();
    ACTOR.get_or_init(||
        SpineActor::from_config(
            &ActorConfig::petpet()
        ).unwrap()
    )
}

fn apply_petpet(controller: &mut SkeletonController, petpet_state: &str, original_offset: (f32, f32), original_scale: f32) {
    /*
        https://benisland.neocities.org/petpet/main.js
        { x: 0, y: 0, w: 0, h: 0 },
        { x: -4, y: 12, w: 4, h: -12 },
        { x: -12, y: 18, w: 12, h: -18 },
        { x: -8, y: 12, w: 4, h: -12 },
        { x: -4, y: 0, w: 0, h: 0 },
     */
    let squish_factor = 0.7;
    let (scale, position): ((f32, f32), (f32, f32)) = match petpet_state {
        "petpet0" => ((0.0, 0.0), (0.0, 0.0)),
        "petpet1" => ((0.3, -0.2), (4.0, 0.0)),
        "petpet2" => ((0.5, -0.3), (12.0, 0.0)),
        "petpet3" => ((0.4, -0.2), (4.0, 0.0)),
        "petpet4" => ((0.2, 0.0), (0.0, 0.0)),
        other => panic!("bad petpet state {}", other),
    };
    controller.skeleton.set_scale([(1.0 + (scale.0 * squish_factor)) * original_scale, (1.0 + (scale.1 * squish_factor)) * original_scale]);
    controller.skeleton.set_position([original_offset.0 + position.0, original_offset.1 + position.1]);
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

        
    });
}

#[derive(Serialize, Debug)]
pub struct Skin {
    pub name: String,
}

impl Slug for Animation {
    fn slug(&self) -> String {
        <Animation as Slug>::slugify_string(&self.name)
    }
}

#[derive(Serialize, Debug)]
pub struct Animation {
    pub name: String,
    pub duration: f32
}

impl Slug for Skin {
    fn slug(&self) -> String {
        <Skin as Slug>::slugify_string(&self.name)
    }
}

#[derive(Debug)]
pub struct SpineActor {
    pub skins: Vec<Skin>,
    pub animations: Vec<Animation>,
    has_slot_colours: bool,
    #[allow(dead_code)]
    atlas: Arc<Atlas>,
    skeleton_data: Arc<SkeletonData>,
    animation_state_data: Arc<AnimationStateData>,
    mutex: std::sync::Mutex<()>
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
    pub petpet: bool,
}

impl RenderParameters {
    pub fn apply_reasonable_limits(&mut self) {
        if self.skins.len() > 7 {
            debug!("LIMITS: skins reducing from {} to 7", self.skins.len());
            self.skins.truncate(7);
        }
        if self.scale > 3.0 {
            debug!("LIMITS: scale reducing from {} to 3.0", self.scale);
            self.scale = 3.0;
        }
        if self.antialiasing > 4 {
            debug!("LIMITS: antialiasing reducing from {} to 4", self.antialiasing);
            self.antialiasing = 4;
        }
        if self.start_time > 60.0 {
            debug!("LIMITS: start_time increasing from {} to 60", self.start_time);
            self.start_time = 60.0;
        }
        if self.end_time > 60.0 {
            debug!("LIMITS: end_time increasing from {} to 60", self.end_time);
            self.end_time = 60.0;
        }
        if self.frame_delay < 1.0 / 120.0 {
            debug!("LIMITS: frame_delay increasing from {} to 1/120", self.frame_delay);
            self.frame_delay = 1.0 / 120.0;
        }
    }

    fn render_scale(&self) -> f32 {
        let aa_factor = if self.antialiasing == 0 { 1 } else { self.antialiasing };
        self.scale * aa_factor as f32
    }
}

#[derive(Debug)]
struct PreparedRenderParameters {
    parameters: RenderParameters,
    skin: rusty_spine::Skin,
    frame_count: u32,
    render_width: usize,
    render_height: usize,
    final_width: usize,
    final_height: usize,
    x_offset: f32,
    y_offset: f32,
    petpet: bool,
}

struct Frame<'a> {
    frame_number: u32,
    pixel_data: &'a [u8],
    width: usize,
    height: usize,
    timestamp: f64
}

impl SpineActor {
    pub fn from_config(config: &crate::ActorConfig) -> color_eyre::Result<SpineActor> {
        spine_init();

        let atlas = Arc::new(Atlas::new_from_file(&config.atlas)?);

        let skeleton_data = if config.skeleton.ends_with(".json") {
            let skeleton_json = SkeletonJson::new(atlas.clone());
            Arc::new(skeleton_json.read_skeleton_data_file(&config.skeleton)?)
        } else {
            let skeleton_binary = SkeletonBinary::new(atlas.clone());
            Arc::new(skeleton_binary.read_skeleton_data_file(&config.skeleton)?)
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

        Ok(SpineActor {
            skins,
            animations,
            has_slot_colours: config.has_slot_colours,
            atlas,
            skeleton_data,
            animation_state_data,
            mutex: std::sync::Mutex::new(())
        })
    }

    fn prepare_render(&self, parameters: RenderParameters) -> color_eyre::Result<PreparedRenderParameters> {
        let _guard = self.mutex.lock().unwrap();

        let mut controller = SkeletonController::new(self.skeleton_data.clone(), self.animation_state_data.clone());
        let render_scale = parameters.render_scale();
        let aa_factor = if parameters.antialiasing == 0 { 1 } else { parameters.antialiasing };

        debug!("Scale x{}, AA x{}, total render scale {}", parameters.scale, aa_factor, render_scale);

        debug!("{}-{} will be {} frames ({} fps)", parameters.start_time, parameters.end_time, (parameters.end_time - parameters.start_time) / parameters.frame_delay, 1.0 / parameters.frame_delay);

        // Make sure we don't duplicate skin names - not sure if this is crucial but I was getting
        // some mysterious crashes before.
        let skin_name = format!("{}", SKIN_NUMBER.fetch_add(1, Ordering::SeqCst));
        let mut skin = rusty_spine::Skin::new(&skin_name);
        for skin_name in &parameters.skins {
            skin.add_skin(controller.skeleton.data().skins().find(|s| s.name() == skin_name).unwrap().as_ref());
        }

        controller.skeleton.set_skin(&skin);
        controller.skeleton.set_scale([render_scale, render_scale]);
        controller.skeleton.set_to_setup_pose();

        if parameters.only_head {
            for mut slot in controller.skeleton.slots_mut() {
                if !only_head_includes(slot.data().name()) {
                    // Make the slot transparent
                    slot.color_mut().set_a(0.0);
                }
            }
        }

        debug!("Finding bounding box...");
        // Run through the animation once and grab all the min/max X and Y. This is actually pretty
        // fast (tens of ms) compared to all the other crazy shit we're doing
        let mut min_x = f32::MAX;
        let mut max_x = f32::MIN;
        let mut min_y = f32::MAX;
        let mut max_y = f32::MIN;

        let mut controllers = vec![];

        controller.animation_state.clear_tracks();
        controller.animation_state.set_animation_by_name(0, &parameters.animation, true).unwrap();
        controllers.push(controller);

        let petpet_guard = if parameters.petpet {
            let petpet = get_petpet_actor();
            let mut petpet_controller = SkeletonController::new(petpet.skeleton_data.clone(), petpet.animation_state_data.clone());
            petpet_controller.skeleton.set_skin_by_name("default").unwrap();
            petpet_controller.animation_state.set_animation_by_name(0, "petpet", true).unwrap();
            petpet_controller.animation_state.set_timescale(PETPET_TIMESCALE);
            controllers.push(petpet_controller);
            Some(petpet.mutex.lock().unwrap())
        } else {
            None
        };

        for controller in &mut controllers {
            controller.update(parameters.start_time);
        };

        let mut time = parameters.start_time;
        let mut frame_count = 0;
        while time <= parameters.end_time {
            for controller in &mut controllers {
                for r in controller.renderables().iter() {
                    if r.color.a < 0.001 { continue };
                    for [x, y] in &r.vertices {
                        min_x = min_x.min(*x);
                        min_y = min_y.min(*y);
                        max_x = max_x.max(*x);
                        max_y = max_y.max(*y);
                    }
                }
                controller.update(parameters.frame_delay);
            }
            time += parameters.frame_delay;
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
            parameters,
            skin,
            frame_count,
            render_width,
            render_height,
            final_width,
            final_height,
            x_offset: -min_x,
            y_offset: -min_y,
            petpet: petpet_guard.is_some(),
        })
    }

    fn render(&self, prepared_params: PreparedRenderParameters, mut frame_callback: impl FnMut(&Frame) -> Result<(), ErrReport>) {
        let _guard = self.mutex.lock().unwrap();

        let mut controller = SkeletonController::new(self.skeleton_data.clone(), self.animation_state_data.clone());

        let params = prepared_params.parameters;
        let render_scale = params.render_scale();

        let background_colour = SfmlColor::rgba(
            (params.background_colour.r * 255.0).round() as u8,
            (params.background_colour.g * 255.0).round() as u8,
            (params.background_colour.b * 255.0).round() as u8,
            (params.background_colour.a * 255.0).round() as u8,
        );

        controller.skeleton.set_skin(&prepared_params.skin);
        controller.skeleton.set_scale([render_scale, render_scale]);
        controller.skeleton.set_to_setup_pose();

        // Only follower supports slot colours and only_head
        if self.has_slot_colours {
            for mut slot in controller.skeleton.slots_mut() {
                if params.only_head && !only_head_includes(slot.data().name()) {
                    // Make the slot transparent
                    slot.color_mut().set_a(0.0);
                } else if let Some(colour) = &params.slot_colours.get(slot.data().name()) {
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

        let mut controllers = vec![controller];

        let _petpet_guard = if prepared_params.petpet {
            let petpet = get_petpet_actor();
            let mut petpet_controller = SkeletonController::new(petpet.skeleton_data.clone(), petpet.animation_state_data.clone());
            petpet_controller.skeleton.set_skin_by_name("default").unwrap();
            petpet_controller.animation_state.set_animation_by_name(0, "petpet", true).unwrap();
            petpet_controller.animation_state.set_timescale(PETPET_TIMESCALE);
            let petpet_scale = if prepared_params.render_width > prepared_params.render_height {
                (prepared_params.render_width as f32 / PETPET_NATIVE_WIDTH) * 0.9 * (prepared_params.render_height as f32 / prepared_params.render_width as f32)
            } else {
                (prepared_params.render_width as f32 / PETPET_NATIVE_WIDTH) * 0.9
            };
            petpet_controller.skeleton.set_scale([petpet_scale, petpet_scale]);
            // petpet_controller.skeleton.set_x(3.0);
            petpet_controller.skeleton.set_y(prepared_params.render_height as f32 - (35.0 * petpet_scale));

            controllers.push(petpet_controller);
            Some(petpet.mutex.lock().unwrap())
        } else {
            None
        };

        for controller in &mut controllers {
            controller.update(params.start_time);
        }

        let mut time = params.start_time;
        let mut elapsed_time = 0.0;
        let mut frame = 0;
        while time <= params.end_time {
            if params.petpet {
                // Work around a bit of borrow checker nonsense - we can't pass controllers[0] as mutable
                // if we're holding a borrow from controllers[1], so just copy the string :<
                let petpet_state = controllers[1].skeleton.slot_at_index(0).unwrap().attachment().unwrap().name().to_owned();
                apply_petpet(&mut controllers[0], &petpet_state, (prepared_params.x_offset, prepared_params.y_offset), render_scale);
            }
            // debug!("Processing frame {}", frame);
            target.clear(background_colour);

            for controller in &mut controllers {
                let renderables = controller.renderables();
                for renderable in renderables.iter() {
                    if renderable.color.a < 0.001 { continue };

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

                controller.update(params.frame_delay);
            }

            // Sucks a bit to have to copy the image twice, but sfml Image doesn't have a way to
            // give us ownership of the pixel data.
            let image = target.texture().copy_to_image().unwrap().pixel_data().to_vec();

            let raw_image = if params.antialiasing <= 1 {
                // debug!("Skipping antialiasing");
                // No antialiasing, so just use the image as-is
                image
            } else {
                resize::resize((prepared_params.render_width, prepared_params.render_height), (prepared_params.final_width, prepared_params.final_height), image)
            };

            let f = Frame {
                frame_number: frame,
                pixel_data: &raw_image,
                width: prepared_params.final_width,
                height: prepared_params.final_height,
                timestamp: elapsed_time
            };

            if let Err(e) = frame_callback(&f) {
                // If we can't send the frames anywhere, there's no point carrying on with rendering them
                warn!("frame callback failed {:?}, aborting render", e);
                break;
            }

            frame += 1;
            time += params.frame_delay;
            elapsed_time += params.frame_delay as f64;
        }
        info!("Finished rendering");
    }

    pub fn render_gif(&self, params: RenderParameters, response_sender: futures_channel::mpsc::UnboundedSender<Result<Vec<u8>, Report>>) -> Result<(), Report> {
        let mut settings = Settings { quality: 75, ..Default::default() };
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
                stdin.write_all(frame.pixel_data)?;
                Ok(())
            }));

            // Send ffmpeg's stdout straight to the client
            scope.spawn(move || io::copy(&mut stdout, &mut writer));
        });

        info!("Finished handling request");
        Ok(())
    }
}
