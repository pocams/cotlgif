use std::collections::HashMap;
use serde::{Serialize, Deserialize, Deserializer, Serializer};
use crate::actors::Skin;

const COLOUR_DATA: &str = include_str!("../worshipper_data.json");

#[derive(Deserialize, Copy, Clone, Debug)]
pub struct Colour {
    r: f32,
    g: f32,
    b: f32,
    a: f32,
}

impl Serialize for Colour {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer {
        if self.a > 0.9999 {
            serializer.serialize_str(&format!("#{:02x}{:02x}{:02x}",
                                              (self.r * 255.0).round() as u32,
                                              (self.g * 255.0).round() as u32,
                                              (self.b * 255.0).round() as u32))
        } else {
            serializer.serialize_str(&format!("#{:02x}{:02x}{:02x}{:02x}",
                                              (self.r * 255.0).round() as u32,
                                              (self.g * 255.0).round() as u32,
                                              (self.b * 255.0).round() as u32,
                                              (self.a * 255.0).round() as u32,
            ))
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct FollowerSkins {
    name: String,
    skins: Vec<String>,
    sets: Vec<HashMap<String, Colour>>
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SkinColours {
    global: Vec<HashMap<String, Colour>>,
    skins: Vec<FollowerSkins>
}

impl SkinColours {
    pub(crate) fn load() -> SkinColours {
        serde_json::from_str(COLOUR_DATA).unwrap()
    }

    fn colours_for_skin(&self, skin_name: &str) -> Vec<HashMap<String, Colour>> {
        let mut colours = self.global.clone();
        for follower_skin_set in &self.skins {
            if follower_skin_set.skins.iter().any(|s| s == skin_name) {
                colours.extend(follower_skin_set.sets.iter().cloned())
            }
        }
        colours
    }
}
