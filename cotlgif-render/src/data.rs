use once_cell::sync::OnceCell;
use regex::Regex;
use sfml::graphics::blend_mode::{BlendMode, Equation, Factor};
use std::collections::HashMap;
use std::num::NonZeroU32;
use thiserror::Error;

pub struct RenderRequest {
    pub actor: String,
    pub skins: Vec<String>,
    pub animation: String,
    pub scale: f32,
    pub antialiasing: NonZeroU32,
    pub start_time: f32,
    pub end_time: f32,
    pub fps: NonZeroU32,
    pub background_colour: rusty_spine::Color,
    pub slot_colours: HashMap<String, rusty_spine::Color>,
    pub only_head: bool,
    pub petpet: bool,
    pub frame_callback: Box<FrameCallback>,
    // pub text_parameters: Option<TextParameters>
}

impl RenderRequest {
    pub(crate) fn frame_delay(&self) -> f32 {
        1.0 / self.fps.get() as f32
    }

    pub(crate) fn frame_count(&self) -> u32 {
        ((self.end_time - self.start_time) / self.frame_delay()).ceil() as u32
    }

    pub(crate) fn should_draw_slot(&self, slot_name: &str) -> bool {
        static ONLY_HEAD: OnceCell<Regex> = OnceCell::new();
        let only_head = ONLY_HEAD.get_or_init(|| Regex::new(
            r"^(HEAD_SKIN_.*|MARKINGS|EXTRA_(TOP|BTM)|Face Colouring|MOUTH|HOOD|EYE_.*|HeadAccessory|HAT|MASK|Tear\d|Crown_Particle\d)$"
        ).unwrap());

        !self.only_head || only_head.is_match(slot_name)
    }
}

pub type FrameCallback = dyn Fn(&Frame) -> Result<(), FrameCallbackError>;

#[derive(Error, Debug)]
pub enum FrameCallbackError {
    #[error("temporary failure, keep sending frames")]
    TemporaryError,
    #[error("permanent failure, no more frames can be handled")]
    PermanentError,
}

#[derive(Error, Debug)]
pub enum RenderError {
    #[error("skin `{0}` not found")]
    SkinNotFound(String),
    #[error("animation `{0}` not found")]
    AnimationNotFound(String),
    #[error("nothing rendered - zero-size image")]
    NothingRendered,
    #[error("failed to create RenderTexture")]
    TextureFailed,
}

#[derive(Error, Debug)]
pub enum LoadError {
    #[error("skeleton load failed: {0}")]
    SkeletonLoadError(String),
    #[error("atlas load failed: {0}")]
    AtlasLoadError(String),
}

#[derive(Debug)]
pub struct SpineSkin {
    pub name: String,
}

#[derive(Debug)]
pub struct SpineAnimation {
    pub name: String,
    pub duration: f32,
}

pub(crate) fn spine_to_sfml(spine_color: &rusty_spine::Color) -> sfml::graphics::Color {
    sfml::graphics::Color::rgba(
        (spine_color.r * 255.0).round() as u8,
        (spine_color.g * 255.0).round() as u8,
        (spine_color.b * 255.0).round() as u8,
        (spine_color.a * 255.0).round() as u8,
    )
}

pub(crate) const BLEND_NORMAL: BlendMode = BlendMode {
    color_src_factor: Factor::SrcAlpha,
    color_dst_factor: Factor::OneMinusSrcAlpha,
    color_equation: Equation::Add,
    alpha_src_factor: Factor::SrcAlpha,
    alpha_dst_factor: Factor::OneMinusSrcAlpha,
    alpha_equation: Equation::Add,
};

pub(crate) const BLEND_ADDITIVE: BlendMode = BlendMode {
    color_src_factor: Factor::SrcAlpha,
    color_dst_factor: Factor::One,
    color_equation: Equation::Add,
    alpha_src_factor: Factor::SrcAlpha,
    alpha_dst_factor: Factor::One,
    alpha_equation: Equation::Add,
};

pub(crate) const BLEND_MULTIPLY: BlendMode = BlendMode {
    color_src_factor: Factor::DstColor,
    color_dst_factor: Factor::OneMinusSrcAlpha,
    color_equation: Equation::Add,
    alpha_src_factor: Factor::DstColor,
    alpha_dst_factor: Factor::OneMinusSrcAlpha,
    alpha_equation: Equation::Add,
};

pub(crate) const BLEND_SCREEN: BlendMode = BlendMode {
    color_src_factor: Factor::One,
    color_dst_factor: Factor::OneMinusSrcColor,
    color_equation: Equation::Add,
    alpha_src_factor: Factor::One,
    alpha_dst_factor: Factor::OneMinusSrcColor,
    alpha_equation: Equation::Add,
};
