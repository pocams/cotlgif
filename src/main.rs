use std::borrow::Cow;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::ops::Deref;
use std::sync::Arc;

use axum::{Extension, Json, Router};
use axum::body::StreamBody;
use axum::extract::{Host, Path, Query};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, get_service};
use clap::Parser;
use color_eyre::eyre::eyre;
use color_eyre::Report;
use css_color_parser2::Color as CssColor;
use regex::Regex;
use rusty_spine::Color;
use serde::{Deserialize, Deserializer, Serialize};
use serde::de::Error;
use serde_json::json;
use tokio::task::spawn_blocking;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing::{debug, info, warn};
use tracing_subscriber::EnvFilter;

use util::{JsonError, OutputType, Slug};

use crate::actors::{RenderParameters, SpineActor};
use crate::colours::SkinColours;
use crate::text::TextParameters;

mod actors;
mod colours;
mod resize;
mod text;
mod util;
mod http;
mod actor;
mod render;
mod petpet;
mod format;


#[derive(Parser)]
struct Args {
    /// Host and port to listen on
    #[arg(short, long, default_value = "0.0.0.0:3000")]
    listen: SocketAddr,

    /// Show spoilers when accessed via this vhost
    #[arg(long, default_value = "")]
    spoilers_host: String,

    /// Limit parameters to try to avoid abuse
    #[arg(long)]
    public: bool
}

impl Actor {
    fn as_json(&self, include_spoilers: bool) -> Option<serde_json::Value> {
        if !include_spoilers {
            if self.config.is_spoiler {
                None
            } else {
                let animations: Vec<_> = if let Some(regex) = &self.config.spoiler_animations {
                    self.spine.animations.iter().filter(|a| !regex.is_match(&a.name)).collect()
                } else {
                    self.spine.animations.iter().collect()
                };

                let skins: Vec<_> = if let Some(regex) = &self.config.spoiler_skins {
                    self.spine.skins.iter().filter(|s| !regex.is_match(&s.name)).collect()
                } else {
                    self.spine.skins.iter().collect()
                };

                Some(
                    json!({
                        "name": self.config.name,
                        "slug": self.config.slug,
                        "category": self.config.category,
                        "skins": skins,
                        "animations": animations,
                    })
                )
            }
        } else {
            Some(json!({
                "name": self.config.name,
                "slug": self.config.slug,
                "category": self.config.category,
                "skins": self.spine.skins,
                "animations": self.spine.animations,
            }))
        }
    }
}



// Convert a list of HTTP GET query parameters to SkinParameters
impl SkinParameters {
    fn into_render_parameters(self, skin_colours: Arc<SkinColours>) -> Result<RenderParameters, JsonError> {
        let fps = (self.fps.unwrap_or(50) as f32).max(1.0);

        let mut final_colours: HashMap<String, Color> = if let Some(colour_set) = self.colour_set {
            skin_colours.colour_set_from_index(&self.add_skin[0], colour_set as usize)
                .unwrap_or_default()
                .into_iter()
                .map(|(slot, colour)| (slot, colour.into()))
                .collect()
        } else {
            Default::default()
        };

        for (slot, color) in self.slot_colours.into_iter() {
            final_colours.insert(slot, color);
        }

        Ok(RenderParameters {
            skins: self.add_skin,
            animation: self.animation.ok_or_else(|| util::json_400("animation= is required"))?,
            scale: self.scale.unwrap_or(1.0).max(0.1),
            antialiasing: self.antialiasing.unwrap_or(1),
            start_time: self.start_time.unwrap_or(0.0),
            end_time: self.end_time.unwrap_or(1.0),
            frame_delay: 1.0 / fps,
            background_colour: self.background_colour.unwrap_or_default(),
            slot_colours: final_colours,
            only_head: self.only_head.unwrap_or(false),
            petpet: self.petpet.unwrap_or(false),
            text_parameters: self.text_parameters
        })
    }
}

async fn load_actors(config: &Config) -> color_eyre::Result<Vec<Arc<Actor>>> {
    let mut actors = Vec::with_capacity(config.actors.len());
    for actor_config in config.actors.iter() {
        actors.push(Arc::new(
            Actor {
                config: actor_config.clone(),
                spine: SpineActor::from_config(actor_config)?
            }))
    }
    Ok(actors)
}

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "info,cotlgif=debug")
    }

    let config: Config = toml::from_slice(
        &tokio::fs::read("config.toml").await.map_err(|e| eyre!("Reading config.toml: {:?}", e))?
    ).map_err(|e| eyre!("Parsing config.toml: {}", e))?;

    tracing_subscriber::fmt::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let args = Arc::new(Args::parse());

    let actors = Arc::new(load_actors(&config).await?);
    let skin_colours = Arc::new(SkinColours::load());


    Ok(())
}

