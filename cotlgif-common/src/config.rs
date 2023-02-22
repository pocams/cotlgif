use regex::Regex;
use serde::de::Error;
use serde::{Deserialize, Deserializer, Serialize};

fn deserialize_regex<'de, D>(deserializer: D) -> Result<Option<Regex>, D::Error>
where
    D: Deserializer<'de>,
{
    // There must be some way to just borrow the &str and compile the regex, but this gets called
    // so seldom it's not a huge deal
    let s = String::deserialize(deserializer)?;
    Ok(Some(
        Regex::new(&s).map_err(|e| Error::custom(format!("{:?}", e)))?,
    ))
}

fn default_scale() -> f32 {
    1.0
}

#[derive(Deserialize, Serialize, Debug, Copy, Clone)]
pub enum ActorCategory {
    None,
    NPCs,
    Bosses,
    Minibosses,
    Enemies,
    Others,
    Objects,
    Unused,
    Uncategorized,
}

impl Default for ActorCategory {
    fn default() -> Self {
        ActorCategory::None
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct ActorConfig {
    pub name: String,
    pub slug: String,
    pub atlas: String,
    pub skeleton: String,
    #[serde(default)]
    pub category: ActorCategory,
    #[serde(default)]
    pub is_spoiler: bool,
    #[serde(default)]
    pub default_skins: Vec<String>,
    #[serde(default)]
    pub default_animation: String,
    #[serde(deserialize_with = "deserialize_regex", default)]
    pub head_slots: Option<Regex>,
    #[serde(default = "default_scale")]
    pub default_scale: f32,
    #[serde(deserialize_with = "deserialize_regex", default)]
    pub spoiler_skins: Option<Regex>,
    #[serde(deserialize_with = "deserialize_regex", default)]
    pub spoiler_animations: Option<Regex>,
    #[serde(default)]
    pub has_slot_colours: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SpineSkin {
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SpineAnimation {
    pub name: String,
    pub duration: f32,
}
