use regex::Regex;
use serde::{Deserialize, Deserializer, Serialize};
use serde::de::Error;

#[derive(Deserialize, Serialize, Debug, Clone)]
enum ActorCategory {
    None,
    NPCs,
    Bosses,
    Minibosses,
    Enemies,
    Others,
    Objects,
    Unused,
}

impl Default for ActorCategory {
    fn default() -> Self {
        ActorCategory::None
    }
}

fn default_scale() -> f32 { 1.0 }

fn deserialize_regex<'de, D>(deserializer: D) -> Result<Option<Regex>, D::Error> where D: Deserializer<'de> {
    // There must be some way to just borrow the &str and compile the regex, but this gets called
    // so seldom it's not a huge deal
    let s = String::deserialize(deserializer)?;
    Ok(Some(Regex::new(&s).map_err(|e| D::Error::custom(format!("{:?}", e)))?))
}

#[derive(Deserialize, Debug, Clone)]
pub struct ActorConfig {
    name: String,
    slug: String,
    atlas: String,
    skeleton: String,
    #[serde(default)]
    category: ActorCategory,
    #[serde(default)]
    is_spoiler: bool,
    default_skins: Vec<String>,
    default_animation: String,
    #[serde(default="default_scale")]
    default_scale: f32,
    #[serde(deserialize_with="deserialize_regex", default)]
    spoiler_skins: Option<Regex>,
    #[serde(deserialize_with="deserialize_regex", default)]
    spoiler_animations: Option<Regex>,
    #[serde(default)]
    has_slot_colours: bool,
}

#[derive(Deserialize)]
struct ActorConfigFile {
    actors: Vec<ActorConfig>
}

