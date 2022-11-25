use crate::CommonColour;
use once_cell::sync::OnceCell;
use regex::Regex;
use std::collections::HashMap;
use std::num::NonZeroU32;

#[derive(Debug)]
pub struct RenderRequest {
    pub actor_slug: String,
    pub skins: Vec<String>,
    pub animation: String,
    pub scale: f32,
    pub antialiasing: NonZeroU32,
    pub start_time: f32,
    pub end_time: f32,
    pub fps: NonZeroU32,
    pub background_colour: CommonColour,
    pub slot_colours: HashMap<String, CommonColour>,
    pub only_head: bool,
    pub petpet: bool,
}

impl RenderRequest {
    pub fn frame_delay(&self) -> f32 {
        1.0 / self.fps.get() as f32
    }

    pub fn frame_count(&self) -> u32 {
        if self.end_time == self.start_time {
            1
        } else {
            ((self.end_time - self.start_time) / self.frame_delay()).ceil() as u32
        }
    }

    pub fn should_draw_slot(&self, slot_name: &str) -> bool {
        static ONLY_HEAD: OnceCell<Regex> = OnceCell::new();
        let only_head = ONLY_HEAD.get_or_init(|| Regex::new(
            r"^(HEAD_SKIN_.*|MARKINGS|EXTRA_(TOP|BTM)|Face Colouring|MOUTH|HOOD|EYE_.*|HeadAccessory|HAT|MASK|Tear\d|Crown_Particle\d)$"
        ).unwrap());

        !self.only_head || only_head.is_match(slot_name)
    }
}
