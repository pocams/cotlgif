use crate::util::{json_400, JsonError, OutputType};
use crate::HttpActor;
use cotlgif_common::{
    CommonColour, CustomSize, Flip, RenderRequest, SkinColours, SomeIfValid, TextParameters,
};
use css_color_parser2::{Color, ColorParseError};
use std::borrow::Cow;
use std::collections::HashMap;
use std::num::NonZeroU32;
use tracing::warn;

#[derive(Debug, Default)]
pub(crate) struct SkinParameters {
    pub output_type: Option<OutputType>,
    pub animation: Option<String>,
    pub add_skin: Vec<String>,
    pub scale: Option<f32>,
    pub antialiasing: Option<u32>,
    pub start_time: Option<f32>,
    pub end_time: Option<f32>,
    pub colour_set: Option<u32>,
    pub slot_colours: HashMap<String, CommonColour>,
    pub background_colour: Option<CommonColour>,
    pub fps: Option<u32>,
    pub only_head: Option<bool>,
    pub download: Option<bool>,
    pub petpet: Option<bool>,
    pub flip: Flip,
    pub custom_size: CustomSize,
    pub top_text: Option<TextParameters>,
    pub bottom_text: Option<TextParameters>,
}

impl SkinParameters {
    // Don't love the weird coupling here tbh
    pub fn render_request(
        &self,
        actor: &HttpActor,
        skin_name: &str,
        enable_spoilers: bool,
        skin_colours: &SkinColours,
    ) -> Result<RenderRequest, JsonError> {
        let mut skins = vec![skin_name.to_string()];
        skins.extend(self.add_skin.iter().cloned());
        let antialiasing = self
            .antialiasing
            .and_then(NonZeroU32::new)
            .unwrap_or_else(|| NonZeroU32::new(1).unwrap());
        let fps = self
            .fps
            .and_then(NonZeroU32::new)
            .unwrap_or_else(|| NonZeroU32::new(50).unwrap());

        if skins
            .iter()
            .any(|s| !actor.is_valid_skin(s, enable_spoilers))
        {
            return Err(json_400("invalid skin for actor".to_string()));
        }

        let Some(animation) = &self.animation else {
            return Err(json_400("animation parameter is required".to_string()));
        };

        if !actor.is_valid_animation(animation, enable_spoilers) {
            return Err(json_400("Invalid animation"));
        }

        let mut slot_colours = self.slot_colours.clone();
        if let Some(colour_set) = self.colour_set {
            if let Some(colour_set_map) =
                skin_colours.colour_set_from_index(skin_name, colour_set as usize)
            {
                for (slot, colour) in colour_set_map.into_iter() {
                    slot_colours.insert(slot, colour);
                }
            }
        }

        Ok(RenderRequest {
            actor_slug: actor.config.slug.to_string(),
            skins,
            animation: animation.to_string(),
            scale: self.scale.unwrap_or(1.0),
            antialiasing,
            start_time: self.start_time.unwrap_or(0.0),
            end_time: self.end_time.unwrap_or_else(|| {
                actor
                    .all_animations
                    .iter()
                    .find(|a| a.name == *animation)
                    .map(|a| a.duration)
                    // The actor just returned true for is_valid_animation(), so we know the animation is good
                    .unwrap()
            }),
            fps,
            background_colour: self.background_colour.unwrap_or_default(),
            slot_colours,
            slots_to_draw: if self.only_head.unwrap_or(false) {
                actor.config.head_slots.clone()
            } else {
                None
            },
            petpet: self.petpet.unwrap_or_default(),
            flip: self.flip,
            custom_size: self.custom_size,
            top_text: self.top_text.clone().some_if_valid(),
            bottom_text: self.bottom_text.clone().some_if_valid(),
        })
    }

    pub fn apply_reasonable_limits(&mut self) {
        if self.add_skin.len() > 10 {
            warn!(
                "LIMITS: add_skin reducing from {} to 10",
                self.add_skin.len()
            );
            self.add_skin.truncate(10);
        }

        if let Some(scale) = self.scale.as_mut() {
            if *scale > 3.0 {
                warn!("LIMITS: scale reducing from {} to 3.0", scale);
                *scale = 3.0;
            }
        }

        if let Some(antialiasing) = self.antialiasing.as_mut() {
            if *antialiasing > 4 {
                warn!("LIMITS: antialiasing reducing from {} to 4", antialiasing);
                *antialiasing = 4;
            }
        }

        if let Some(start_time) = self.start_time.as_mut() {
            if *start_time > 30.0 {
                warn!("LIMITS: start_time reducing from {} to 30", start_time);
                *start_time = 30.0;
            }
        }

        if let Some(end_time) = self.end_time.as_mut() {
            if *end_time > 60.0 {
                warn!("LIMITS: end_time reducing from {} to 60", end_time);
                *end_time = 60.0
            }
        }

        if let Some(fps) = self.fps.as_mut() {
            if *fps > 120 {
                warn!("LIMITS: fps reducing from {} to 120", fps);
                *fps = 120;
            }
        }

        if let Some(t) = self.top_text.as_mut() {
            if t.size > 200 {
                warn!("LIMITS: top_text size decreasing from {} to 200", t.size);
                t.size = 200;
            }

            if t.text.len() > 100 {
                warn!(
                    "LIMITS: top_text length truncating from {} to 100",
                    t.text.len()
                );
                t.text.truncate(100);
            }
        }

        if let Some(t) = self.bottom_text.as_mut() {
            if t.size > 200 {
                warn!("LIMITS: bottom size decreasing from {} to 200", t.size);
                t.size = 200;
            }

            if t.text.len() > 100 {
                warn!(
                    "LIMITS: bottom length truncating from {} to 100",
                    t.text.len()
                );
                t.text.truncate(100);
            }
        }
    }
}

fn parse_color(s: &str) -> Result<CommonColour, ColorParseError> {
    let css_color: Color = s.parse()?;
    Ok(CommonColour {
        r: css_color.r as f32 / 255.0,
        g: css_color.g as f32 / 255.0,
        b: css_color.b as f32 / 255.0,
        a: css_color.a,
    })
}

impl TryFrom<Vec<(String, String)>> for SkinParameters {
    type Error = JsonError;

    fn try_from(params: Vec<(String, String)>) -> Result<SkinParameters, Self::Error> {
        let mut sp = SkinParameters::default();
        for (key, value) in params.into_iter() {
            match key.as_str() {
                "format" => sp.output_type = Some(value.parse()?),
                "add_skin" => sp.add_skin.push(value),
                "animation" => sp.animation = Some(value),
                "scale" => {
                    sp.scale = Some(
                        value
                            .parse()
                            .map_err(|e| json_400(format!("scale: {e:?}")))?,
                    )
                }
                "antialiasing" => {
                    sp.antialiasing = Some(
                        value
                            .parse()
                            .map_err(|e| json_400(format!("antialiasing: {e:?}")))?,
                    )
                }
                "start_time" => {
                    sp.start_time = Some(
                        value
                            .parse()
                            .map_err(|e| json_400(format!("start_time: {e:?}")))?,
                    )
                }
                "end_time" => {
                    sp.end_time = Some(
                        value
                            .parse()
                            .map_err(|e| json_400(format!("end_time: {e:?}")))?,
                    )
                }
                "colour_set" => {
                    sp.colour_set = Some(
                        value
                            .parse()
                            .map_err(|e| json_400(format!("colour_set: {e:?}")))?,
                    )
                }
                "background" => {
                    sp.background_colour = Some(
                        parse_color(&value)
                            .map_err(|e| json_400(format!("background_color: {}", e)))?,
                    )
                }
                "fps" => sp.fps = Some(value.parse().map_err(|e| json_400(format!("fps: {e:?}")))?),
                "only_head" => {
                    sp.only_head = Some(
                        value
                            .parse()
                            .map_err(|e| json_400(format!("only_head: {e:?}")))?,
                    )
                }
                "download" => {
                    sp.download = Some(
                        value
                            .parse()
                            .map_err(|e| json_400(format!("download: {e:?}")))?,
                    )
                }
                // Skin colour parameters
                "HEAD_SKIN_TOP" | "HEAD_SKIN_BTM" | "MARKINGS" | "ARM_LEFT_SKIN"
                | "ARM_RIGHT_SKIN" | "LEG_LEFT_SKIN" | "LEG_RIGHT_SKIN" => {
                    sp.slot_colours.insert(
                        key.clone(),
                        parse_color(&value).map_err(|e| json_400(format!("{}: {}", key, e)))?,
                    );
                }
                "petpet" => {
                    sp.petpet = Some(
                        value
                            .parse()
                            .map_err(|e| json_400(format!("petpet: {e:?}")))?,
                    )
                }
                "flip" => {
                    sp.flip = match value.as_str() {
                        "horizontal" => Flip::Horizontal,
                        "none" => Flip::NoFlip,
                        _ => {
                            return Err(json_400(
                                "flip: expected 'horizontal' or 'none'".to_string(),
                            ))
                        }
                    }
                }
                "custom_size" => {
                    sp.custom_size = match value.as_str() {
                        "discord128x128" => CustomSize::Discord128x128,
                        "none" => CustomSize::DefaultSize,
                        _ => {
                            return Err(json_400(
                                "custom_size: expected 'discord128x128' or 'none'".to_string(),
                            ))
                        }
                    }
                }
                "top_text" | "top_text_font" | "top_text_size" => {
                    let mut t = sp.top_text.get_or_insert_with(Default::default);
                    match key.as_str() {
                        "top_text" => t.text = value,
                        "top_text_font" => {
                            t.font = value
                                .parse()
                                .map_err(|_| json_400("top_text_font: unknown font".to_string()))?
                        }
                        "top_text_size" => {
                            t.size = value
                                .parse()
                                .map_err(|e| json_400(format!("top_text_size: {}", e)))?
                        }
                        _ => unreachable!(),
                    }
                }
                "bottom_text" | "bottom_text_font" | "bottom_text_size" => {
                    let mut t = sp.bottom_text.get_or_insert_with(Default::default);
                    match key.as_str() {
                        "bottom_text" => t.text = value,
                        "bottom_text_font" => {
                            t.font = value.parse().map_err(|_| {
                                json_400("bottom_text_font: unknown font".to_string())
                            })?
                        }
                        "bottom_text_size" => {
                            t.size = value
                                .parse()
                                .map_err(|e| json_400(format!("bottom_text_size: {}", e)))?
                        }
                        _ => unreachable!(),
                    }
                }
                _ => return Err(json_400(Cow::from(format!("Invalid parameter {:?}", key)))),
            }
        }
        Ok(sp)
    }
}
