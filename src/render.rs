use std::{io, thread};
use std::collections::HashMap;
use std::default::Default;
use std::io::{BufRead, BufReader, Write};
use std::num::NonZeroU32;
use std::ops::{Deref, DerefMut};
use std::process::{abort, Command, Stdio};
use std::sync::{Arc, Mutex};
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
use rusty_spine::{AnimationStateData, Atlas, Color, SkeletonBinary, SkeletonController, SkeletonData, SkeletonJson, SkeletonRenderable};
use rusty_spine::BlendMode as SpineBlendMode;
use serde::Serialize;
use sfml::graphics::{Color as SfmlColor, FloatRect, IntRect, PrimitiveType, RenderStates, RenderTarget, RenderTexture, Texture, Transform, Vertex};
use sfml::graphics::blend_mode::{Equation as BlendEquation, Factor as BlendFactor};
use sfml::graphics::BlendMode as SfmlBlendMode;
use sfml::SfBox;
use sfml::system::Vector2f;
use toml::Value::Float;
use tracing::{debug, info, warn};
use thiserror::Error;

use crate::text::TextParameters;
use crate::util::{ChannelWriter, Slug};

#[derive(Error, Debug)]
pub enum FrameCallbackError {
    #[error("temporary failure, keep sending frames")]
    TemporaryError,
    #[error("permanent failure, no more frames can be handled")]
    PermanentError,
}

type FrameCallback = impl FnMut(&Frame) -> Result<(), FrameCallbackError>;

#[derive(Error, Debug)]
pub enum RenderError {
    #[error("skin `{0}` not found")]
    SkinNotFound(String),
    #[error("animation `{0}` not found")]
    AnimationNotFound(String),
    #[error("nothing rendered - zero-size image")]
    NothingRendered,
    #[error("failed to create rendertexture")]
    TextureFailed,
}

#[derive(Debug)]
pub struct RenderRequest {
    pub actor: String,
    pub skins: Vec<String>,
    pub animation: String,
    pub scale: f32,
    pub antialiasing: NonZeroU32,
    pub start_time: f32,
    pub end_time: f32,
    pub fps: NonZeroU32,
    pub background_colour: Color,
    pub slot_colours: HashMap<String, Color>,
    pub only_head: bool,
    pub petpet: bool,
    // pub text_parameters: Option<TextParameters>
}

fn spine_to_sfml(spine_color: &Color) -> SfmlColor {
    SfmlColor::rgba(
        (spine_color.r * 255.0).round() as u8,
        (spine_color.g * 255.0).round() as u8,
        (spine_color.b * 255.0).round() as u8,
        (spine_color.a * 255.0).round() as u8,
    )
}

impl RenderRequest {
    fn frame_delay(&self) -> f32 {
        1.0 / self.fps as f32
    }

    fn frame_count(&self) -> u32 {
        ((self.end_time - self.start_time) / self.frame_delay()).ceil() as u32
    }

    fn should_draw_slot(&self, slot_name: &str) -> bool {
        static ONLY_HEAD: OnceCell<Regex> = OnceCell::new();
        let only_head = ONLY_HEAD.get_or_init(|| Regex::new(
            r"^(HEAD_SKIN_.*|MARKINGS|EXTRA_(TOP|BTM)|Face Colouring|MOUTH|HOOD|EYE_.*|HeadAccessory|HAT|MASK|Tear\d|Crown_Particle\d)$"
        ).unwrap());

        !self.only_head || only_head.is_match(slot_name)
    }
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


#[derive(Debug)]
struct SpineActor {
    pub skins: Vec<SpineSkin>,
    pub animations: Vec<SpineAnimation>,
    skeleton_data: Arc<SkeletonData>,
    animation_state_data: Arc<AnimationStateData>,
    #[allow(dead_code)]
    atlas: Arc<Atlas>,
}

#[derive(Serialize, Debug)]
pub struct SpineSkin {
    pub name: String,
}

#[derive(Serialize, Debug)]
pub struct SpineAnimation {
    pub name: String,
    pub duration: f32,
}

/*
impl RenderParameters {
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

 */


struct Frame<'a> {
    frame_number: u32,
    pixel_data: Vec<u8>,
    width: usize,
    height: usize,
    timestamp: f64
}


impl SpineActor {
    pub fn load(atlas_path: &str, skeleton_path: &str) -> Result<SpineActor, RenderError> {
        spine_init();

        let atlas = Arc::new(Atlas::new_from_file(atlas_path)?);

        let skeleton_data = Arc::new(
            if skeleton_path.ends_with(".json") {
                let skeleton_json = SkeletonJson::new(atlas.clone());
                skeleton_json.read_skeleton_data_file(&skeleton_path)?
            } else {
                let skeleton_binary = SkeletonBinary::new(atlas.clone());
                skeleton_binary.read_skeleton_data_file(&skeleton_path)?
            }
        );

        let animation_state_data = Arc::new(AnimationStateData::new(skeleton_data.clone()));

        let skins = skeleton_data.skins()
            .map(|s| SpineSkin { name: s.name().to_owned() })
            .collect();

        let animations = skeleton_data.animations()
            .map(|a| SpineAnimation { name: a.name().to_owned(), duration: a.duration() })
            .collect();

        Ok(SpineActor {
            skins,
            animations,
            atlas,
            skeleton_data,
            animation_state_data,
        })
    }

    pub fn new_skeleton_controller(&self) -> SkeletonController {
        SkeletonController::new(self.skeleton_data.clone(), self.animation_state_data.clone())
    }
}

fn get_bounding_box(skeleton_controller: &mut SkeletonController, frame_count: u32, frame_delay: f32) -> FloatRect {
    // Run through the requested number of frames and grab all the min/max X and Y of the vertices.
    // This is actually pretty fast (tens of ms) compared to all the other crazy shit we're doing

    let mut min_x = f32::MAX;
    let mut max_x = f32::MIN;
    let mut min_y = f32::MAX;
    let mut max_y = f32::MIN;

    for _ in 0..frame_count {
        for r in controller.renderables().iter() {
            if r.color.a < 0.001 { continue };
            for [x, y] in &r.vertices {
                min_x = min_x.min(*x);
                min_y = min_y.min(*y);
                max_x = max_x.max(*x);
                max_y = max_y.max(*y);
            }
        }
        skeleton_controller.update(frame_delay);
    }

    FloatRect::new(min_x, min_y, max_x - min_x, max_y - min_y)
}

fn render(actor: &SpineActor, request: RenderRequest) -> Result<(), RenderError> {
    let mut controller = actor.new_skeleton_controller();

    // Keep the custom skin around until the end of the function if we create one
    let _custom_skin = if request.skins.len() == 1 {
        // Just one skin, we can just use set_skin_by_name()
        controller.skeleton.set_skin_by_name(request.skins[0].as_str())
            .map_err(|_| RenderError::SkinNotFound(request.skins[0].to_owned()))?;
        None
    } else {
        // Multiple skins stacked up - we'll have to build a custom skin
        let mut skin = rusty_spine::Skin::new("custom");
        for skin_name in &parameters.skins {
            let skeleton_skin = controller.skeleton.data()
                .skins()
                .find(|s| s.name() == skin_name)
                .ok_or_else(|| Err(RenderError::SkinNotFound(skin_name.to_owned())))?;
            skin.add_skin(skeleton_skin.as_ref());
        };
        controller.skeleton.set_skin(&skin);
        Some(skin)
    };

    controller.skeleton.set_scale([request.scale, request.scale]);

    // If there are slots we shouldn't draw, make them transparent (set alpha=0)
    for mut slot in controller.skeleton.slots_mut() {
        let slot_name = slot.data().name();
        if !request.should_draw_slot(slot_name) {
            slot.color_mut().set_a(0.0);
        } else if let Some(color) = request.slot_colours.get(slot_name) {
            *slot.color_mut() = *color;
        }
    }

    controller.animation_state.clear_tracks();
    controller.animation_state.set_animation_by_name(0, &parameters.animation, true)
        .map_err(|_| RenderError::AnimationNotFound(parameters.animation.to_owned()))?;
    controller.update(request.start_time);

    let bounding_box = get_bounding_box(&mut controller, request.frame_count(), request.frame_delay());
    debug!("Bounding box: {:?}", bounding_box);

    // Bail out early if we didn't render anything at all
    if bounding_box.width < 1.0 || bounding_box.height < 1.0 {
        return Err(RenderError::NothingRendered)
    };

    // Move the skeleton into the center of the bounding box
    controller.skeleton.set_x(-bounding_box.left);
    controller.skeleton.set_y(-bounding_box.top);

    // Reset the animation after we rendered it to calculate the bounding box
    controller.animation_state.clear_tracks();
    controller.animation_state.set_animation_by_name(0, &parameters.animation, true)
        .map_err(|_| RenderError::AnimationNotFound(parameters.animation.to_owned()))?;
    controller.update(request.start_time);

    let mut controllers = vec![controller];
    if request.petpet {
        controllers.push(petpetcontroller);
    }

    let mut target = RenderTexture::new(bounding_box.width.ceil() as u32, bounding_box.height.ceil() as u32)
        .ok_or_else(|| Err(RenderError::TextureFailed))?;

    let mut render_states = RenderStates::new(
        BLEND_NORMAL,
        Transform::IDENTITY,
        None,
        None
    );

    let background_color = spine_to_sfml(&request.background_colour);
    let mut time = request.start_time;
    let mut elapsed_time = 0.0;
    let mut frame = 0;
    let mut vertex_buffer = Vec::with_capacity(256);
    while frame < request.frame_count() {
        // if prepared_params.parameters.petpet {
        //     // Work around a bit of borrow checker nonsense - we can't pass controllers[0] as mutable
        //     // if we're holding a borrow from controllers[1], so just copy the string :<
        //     let petpet_state = controllers[1].skeleton.slot_at_index(0).unwrap().attachment().unwrap().name().to_owned();
        //     apply_petpet(&mut controllers[0], &petpet_state, (prepared_params.x_offset, prepared_params.y_offset), render_scale);
        // }
        // debug!("Processing frame {}", frame);
        target.clear(background_color);

        for controller in &mut controllers {
            let renderables = controller.renderables();
            for renderable in renderables.iter() {
                if renderable.color.a < 0.001 { continue };

                let color = spine_to_sfml(&renderable.color);

                let texture = unsafe { &*(renderable.attachment_renderer_object.unwrap() as *const SfBox<Texture>) };
                let texture_size = texture.size();
                render_states.set_texture(Some(texture));

                render_states.blend_mode = match renderable.blend_mode {
                    SpineBlendMode::Normal => BLEND_NORMAL,
                    SpineBlendMode::Additive => BLEND_ADDITIVE,
                    SpineBlendMode::Multiply => BLEND_MULTIPLY,
                    SpineBlendMode::Screen => BLEND_SCREEN,
                };

                vertex_buffer.clear();

                for i in &renderable.indices {
                    let vertex = renderable.vertices[*i as usize];
                    let uv_raw = renderable.uvs[*i as usize];
                    let uv = [uv_raw[0] * texture_size.x as f32, uv_raw[1] * texture_size.y as f32];
                    vertex_buffer.push(Vertex::new(
                        Vector2f::new(vertex[0], vertex[1]), color, Vector2f::new(uv[0], uv[1])
                    ));
                }

                target.draw_primitives(vertex_buffer.as_slice(), PrimitiveType::TRIANGLES, &render_states);
            }

            controller.update(request.frame_delay());
        }

        // Sucks a bit to have to copy the image twice, but sfml Image doesn't have a way to
        // give us ownership of the pixel data.
        let image = target.texture().copy_to_image().unwrap().pixel_data().to_vec();

            // let raw_image = if prepared_params.parameters.antialiasing <= 1 {
            //     // debug!("Skipping antialiasing");
            //     // No antialiasing, so just use the image as-is
            //     image
            // } else {
            //     resize::resize((prepared_params.render_width, prepared_params.render_height), (prepared_params.final_width, prepared_params.final_height), image)
            // };
            //
        let f = Frame {
            frame_number: frame,
            pixel_data: image,
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
        time += prepared_params.parameters.frame_delay;
        elapsed_time += prepared_params.parameters.frame_delay as f64;
    }
    info!("Finished rendering");
}
