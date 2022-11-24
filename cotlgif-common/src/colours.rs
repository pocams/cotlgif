use std::collections::HashMap;

use serde::{Deserialize, Serialize, Serializer};

const COLOUR_DATA: &str = include_str!("../../assets/worshipper_data.json");

#[derive(Deserialize, Copy, Clone, Debug, Default)]
pub struct CommonColour {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32
}

impl Serialize for CommonColour {
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
    sets: Vec<HashMap<String, CommonColour>>
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SkinColours {
    global: Vec<HashMap<String, CommonColour>>,
    skins: Vec<FollowerSkins>
}

impl SkinColours {
    pub fn load() -> SkinColours {
        serde_json::from_str(COLOUR_DATA).unwrap()
    }

    pub fn colour_set_from_index(&self, skin_name: &str, index: usize) -> Option<HashMap<String, CommonColour>> {
        let mut index = index;
        for follower_skin_set in &self.skins {
            if follower_skin_set.skins.iter().any(|s| s == skin_name) {
                if index < follower_skin_set.sets.len() {
                    return Some(follower_skin_set.sets[index].clone())
                } else {
                    // Reduce index by the number of custom sets available for this follower
                    index -= follower_skin_set.sets.len();
                }
                break;
            }
        }

        self.global.get(index).map(|h| h.to_owned())
    }

    /*
    fn colours_for_skin(&self, skin_name: &str) -> Vec<HashMap<String, Colour>> {
        let mut colours = self.global.clone();
        for follower_skin_set in &self.skins {
            if follower_skin_set.skins.iter().any(|s| s == skin_name) {
                colours.extend(follower_skin_set.sets.iter().cloned())
            }
        }
        colours
    }
    */
}
