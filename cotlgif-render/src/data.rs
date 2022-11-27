use sfml::graphics::blend_mode::{BlendMode, Equation, Factor};

use cotlgif_common::CommonColour;
use thiserror::Error;

#[derive(Debug)]
pub struct RenderMetadata {
    pub frame_count: u32,
    pub frame_delay: f32,
    pub frame_width: usize,
    pub frame_height: usize,
}

#[derive(Debug)]
pub struct Frame {
    pub frame_number: u32,
    pub pixel_data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub timestamp: f64,
}

#[derive(Error, Debug)]
pub enum HandleFrameError {
    #[error("temporary failure, keep sending frames")]
    TemporaryError,
    #[error("permanent failure, no more frames can be handled")]
    PermanentError,
}

pub trait FrameHandler: Send {
    fn set_metadata(&mut self, metadata: crate::RenderMetadata);
    fn handle_frame(&mut self, frame: Frame) -> Result<(), HandleFrameError>;
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

pub(crate) fn spine_to_sfml(spine_color: &rusty_spine::Color) -> sfml::graphics::Color {
    sfml::graphics::Color::rgba(
        (spine_color.r * 255.0).round() as u8,
        (spine_color.g * 255.0).round() as u8,
        (spine_color.b * 255.0).round() as u8,
        (spine_color.a * 255.0).round() as u8,
    )
}

pub(crate) fn common_to_sfml(common_colour: &CommonColour) -> sfml::graphics::Color {
    sfml::graphics::Color::rgba(
        (common_colour.r * 255.0).round() as u8,
        (common_colour.g * 255.0).round() as u8,
        (common_colour.b * 255.0).round() as u8,
        (common_colour.a * 255.0).round() as u8,
    )
}

pub(crate) fn common_to_spine(common_colour: &CommonColour) -> rusty_spine::Color {
    rusty_spine::Color::new_rgba(
        common_colour.r,
        common_colour.g,
        common_colour.b,
        common_colour.a,
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
