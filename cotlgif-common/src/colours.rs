use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::io::Error;

use serde::{Deserialize, Serialize, Serializer};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum WorshipperDataError {
    #[error("failed to load worshipper_data.json: {0}")]
    LoadError(String),
    #[error("failed to parse worshipper_data.json: {0}")]
    ParseError(String),
}

impl From<io::Error> for WorshipperDataError {
    fn from(value: Error) -> Self {
        WorshipperDataError::LoadError(value.to_string())
    }
}

impl From<serde_json::Error> for WorshipperDataError {
    fn from(value: serde_json::Error) -> Self {
        WorshipperDataError::ParseError(value.to_string())
    }
}

#[derive(Deserialize, Copy, Clone, Debug, Default)]
pub struct CommonColour {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Serialize for CommonColour {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if self.a > 0.9999 {
            serializer.serialize_str(&format!(
                "#{:02x}{:02x}{:02x}",
                (self.r * 255.0).round() as u32,
                (self.g * 255.0).round() as u32,
                (self.b * 255.0).round() as u32
            ))
        } else {
            serializer.serialize_str(&format!(
                "#{:02x}{:02x}{:02x}{:02x}",
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
    sets: Vec<HashMap<String, CommonColour>>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SkinColours {
    global: Vec<HashMap<String, CommonColour>>,
    skins: Vec<FollowerSkins>,
}

impl SkinColours {
    pub fn load() -> Result<SkinColours, WorshipperDataError> {
        Ok(serde_json::from_reader(File::open(
            "assets/worshipper_data.json",
        )?)?)
    }

    pub fn colour_set_from_index(
        &self,
        skin_name: &str,
        index: usize,
    ) -> Option<HashMap<String, CommonColour>> {
        let mut index = index;
        for follower_skin_set in &self.skins {
            if follower_skin_set.skins.iter().any(|s| s == skin_name) {
                if index < follower_skin_set.sets.len() {
                    return Some(follower_skin_set.sets[index].clone());
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
