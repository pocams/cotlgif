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

mod actors;
mod colours;
mod util;
mod resize;

const CACHE_CONTROL_SHORT: &str = "max-age=60";

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

#[derive(Deserialize, Serialize, Debug, Clone)]
enum Category {
    None,
    NPCs,
    Bosses,
    Minibosses,
    Enemies,
    Others,
    Objects,
    Unused,
}

impl Default for Category {
    fn default() -> Self {
        Category::None
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct ActorConfig {
    name: String,
    slug: String,
    atlas: String,
    skeleton: String,
    #[serde(default)]
    category: Category,
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

impl ActorConfig {
    fn petpet() -> ActorConfig {
        ActorConfig {
            name: "Petpet".to_owned(),
            slug: "petpet".to_owned(),
            atlas: "assets/petpet.atlas".to_owned(),
            skeleton: "assets/petpet.skel".to_owned(),
            category: Category::None,
            is_spoiler: true,
            default_skins: vec!["default".to_owned()],
            default_animation: "petpet".to_owned(),
            default_scale: 1.0,
            spoiler_skins: None,
            spoiler_animations: None,
            has_slot_colours: false
        }
    }
}

fn default_scale() -> f32 { 1.0 }

fn deserialize_regex<'de, D>(deserializer: D) -> Result<Option<Regex>, D::Error> where D: Deserializer<'de> {
    // There must be some way to just borrow the &str and compile the regex, but this gets called
    // so seldom it's not a huge deal
    let s = String::deserialize(deserializer)?;
    Ok(Some(Regex::new(&s).map_err(|e| D::Error::custom(format!("{:?}", e)))?))
}

#[derive(Deserialize)]
struct Config {
    actors: Vec<ActorConfig>
}

#[derive(Debug)]
struct Actor {
    config: ActorConfig,
    spine: SpineActor
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


#[derive(Debug, Default)]
struct SkinParameters {
    output_type: Option<OutputType>,
    animation: Option<String>,
    add_skin: Vec<String>,
    scale: Option<f32>,
    antialiasing: Option<u32>,
    start_time: Option<f32>,
    end_time: Option<f32>,
    colour_set: Option<u32>,
    slot_colours: HashMap<String, Color>,
    background_colour: Option<Color>,
    fps: Option<u32>,
    only_head: Option<bool>,
    download: Option<bool>,
    petpet: Option<bool>,
    text_parameters: Option<TextParameters>
}

// Convert a list of HTTP GET query parameters to SkinParameters
impl TryFrom<Vec<(String, String)>> for SkinParameters {
    type Error = JsonError;

    fn try_from(params: Vec<(String, String)>) -> Result<SkinParameters, Self::Error> {
        let mut sp = SkinParameters::default();
        for (key, value) in params.into_iter() {
            match key.as_str() {
                "format" => sp.output_type = Some(value.parse()?),
                "add_skin" => sp.add_skin.push(value),
                "animation" => sp.animation = Some(value),
                "scale" => sp.scale = Some(value.parse().map_err(|e| util::json_400(format!("scale: {e:?}")))?),
                "antialiasing" => sp.antialiasing = Some(value.parse().map_err(|e| util::json_400(format!("antialiasing: {e:?}")))?),
                "start_time" => sp.start_time = Some(value.parse().map_err(|e| util::json_400(format!("start_time: {e:?}")))?),
                "end_time" => sp.end_time = Some(value.parse().map_err(|e| util::json_400(format!("end_time: {e:?}")))?),
                "colour_set" => sp.colour_set = Some(value.parse().map_err(|e| util::json_400(format!("colour_set: {e:?}")))?),
                "background" => sp.background_colour = Some(color_from_string(value.as_str()).map_err(|e| util::json_400(format!("background_color: {}", e)))?),
                "fps" => sp.fps = Some(value.parse().map_err(|e| util::json_400(format!("fps: {e:?}")))?),
                "only_head" => sp.only_head = Some(value.parse().map_err(|e| util::json_400(format!("only_head: {e:?}")))?),
                "download" => sp.download = Some(value.parse().map_err(|e| util::json_400(format!("download: {e:?}")))?),
                // Skin colour parameters
                "HEAD_SKIN_TOP" | "HEAD_SKIN_BTM" | "MARKINGS" | "ARM_LEFT_SKIN" | "ARM_RIGHT_SKIN" | "LEG_LEFT_SKIN" | "LEG_RIGHT_SKIN" => {
                    sp.slot_colours.insert(key.clone(), color_from_string(value.as_str()).map_err(|e| util::json_400(format!("{}: {}", key, e)))?);
                }
                "petpet" => sp.petpet = Some(value.parse().map_err(|e| util::json_400(format!("petpet: {e:?}")))?),
                _ => return Err(util::json_400(Cow::from(format!("Invalid parameter {:?}", key))))
            }
        }
        Ok(sp)
    }
}

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
        })
    }
}

fn color_from_string(s: &str) -> color_eyre::Result<Color> {
    let css_color = s.parse::<CssColor>()?;
    Ok(Color::new_rgba(css_color.r as f32 / 255.0, css_color.g as f32 / 255.0, css_color.b as f32 / 255.0, css_color.a))
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

    let serve_dir_service = get_service(ServeDir::new("static"))
        .handle_error(|err| async move {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Unhandled internal error: {}", err),
            )
        });

    let listen_host = args.listen;
    let app = Router::new()
        .route("/", get(get_index))
        .route("/init.js", get(get_spoiler_js))
        .route("/v1", get(get_v1))
        .route("/v1/:actor", get(get_v1_actor))
        .route("/v1/:actor/colours", get(get_v1_colours))
        .route("/v1/:actor/:skin", get(get_v1_skin))
        // If we don't match any routes, try serving static files from /static
        .fallback(serve_dir_service)
        .layer(Extension(actors))
        .layer(Extension(skin_colours))
        .layer(Extension(args))
        .layer(TraceLayer::new_for_http());

    info!("Starting server");
    axum::Server::bind(&listen_host)
        .serve(app.into_make_service())
        .await
        .unwrap();

    Ok(())
}

async fn get_index(Host(host): Host) -> impl IntoResponse {
    let filename = if host.starts_with("localhost:") {
        "html/index.dev.html"
    } else {
        "html/index.html"
    };

    match tokio::fs::read(filename).await {
        Ok(f) => {
            (
                StatusCode::OK,
                [
                    (header::CACHE_CONTROL, CACHE_CONTROL_SHORT),
                    (header::CONTENT_TYPE, "text/html")
                ],
                String::from_utf8(f).unwrap()
            )
        }
        Err(e) => {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                [
                    (header::CACHE_CONTROL, CACHE_CONTROL_SHORT),
                    (header::CONTENT_TYPE, "text/plain")
                ],
                format!("{:?}", e)
            )
        }
    }
}

async fn get_spoiler_js(Host(host): Host, Extension(args): Extension<Arc<Args>>) -> impl IntoResponse {
    let body = if host == args.spoilers_host {
        "window.spoilersEnabled = true;\n"
    } else {
        "\n"
    };

    (
        [
            (header::CONTENT_TYPE, "text/javascript"),
            (header::CACHE_CONTROL, CACHE_CONTROL_SHORT)
        ],
        body
    )
}

async fn get_v1(
    Extension(actors): Extension<Arc<Vec<Arc<Actor>>>>,
    Extension(args): Extension<Arc<Args>>,
    Host(host): Host
    ) -> impl IntoResponse {
    let show_spoilers = host == args.spoilers_host;

    let actor_json: Vec<_> = actors.iter()
        .filter(|actor| show_spoilers || !actor.config.is_spoiler)
        .map(|actor| {
            json!({
                "name": actor.config.name,
                "slug": actor.config.slug,
                "category": actor.config.category,
                "default_skins": actor.config.default_skins,
                "default_animation": actor.config.default_animation,
                "default_scale": actor.config.default_scale,
            })
        }
    ).collect();

    (
        [(header::CACHE_CONTROL, CACHE_CONTROL_SHORT)],
        Json(json!({"actors": actor_json}))
    )
}

async fn get_v1_actor(
    Extension(actors): Extension<Arc<Vec<Arc<Actor>>>>,
    Extension(args): Extension<Arc<Args>>,
    Path(actor_slug): Path<String>,
    Host(host): Host
) -> impl IntoResponse {
    let show_spoilers = host == args.spoilers_host;
    info!("Request host {}, spoilers {}", host, show_spoilers);

    // Does the actor exist?
    if let Some(actor) = actors.iter().find(|a| a.config.slug == actor_slug) {
        debug!("found actor: {:?}", actor);
        // as_json() will return None if we're not showing spoilers and the whole actor is a spoiler
        if let Some(json) = actor.as_json(show_spoilers) {
            return (
                StatusCode::OK,
                [(header::CACHE_CONTROL, CACHE_CONTROL_SHORT)],
                Json(json)
            )
        }
    }
    (StatusCode::NOT_FOUND, [(header::CACHE_CONTROL, CACHE_CONTROL_SHORT)], Json(json!({"error": "no such actor"})))
}

async fn get_v1_colours(
    Extension(skin_colours): Extension<Arc<SkinColours>>,
    Path(actor_name): Path<String>,
    Host(_host): Host
) -> impl IntoResponse {
    let json = if actor_name == "follower" {
        // Only followers have colour sets
        serde_json::to_value(skin_colours.deref()).unwrap()
    } else {
        json!([])
    };

    (
        [(header::CACHE_CONTROL, CACHE_CONTROL_SHORT)],
        Json(json)
    )
}

async fn get_v1_skin(
    Extension(actors): Extension<Arc<Vec<Arc<Actor>>>>,
    Extension(args): Extension<Arc<Args>>,
    Extension(skin_colours): Extension<Arc<SkinColours>>,
    Path((actor_slug, skin_name)): Path<(String, String)>,
    Query(params): Query<Vec<(String, String)>>
) -> Result<impl IntoResponse, JsonError> {
    let mut params = SkinParameters::try_from(params)?;
    debug!("params: {:?}", params);

    let actor = actors.iter().find(|a| a.config.slug == actor_slug).ok_or_else(|| util::json_404("No such actor"))?.clone();

    let animation_name = params.animation.as_deref().ok_or_else(|| util::json_400("animation= parameter is required"))?;
    let animation = actor.spine.animations.iter().find(|anim| anim.name == animation_name).ok_or_else(|| util::json_404("No such animation for actor"))?;

    if params.end_time.is_none() {
        params.end_time = Some(animation.duration);
    }

    params.add_skin.insert(0, skin_name);
    for add_skin in &params.add_skin {
        if !actor.spine.skins.iter().any(|s| &s.name == add_skin) {
            return Err(util::json_400(format!("No such skin for actor: {add_skin:?}")))
        }
    }

    let output_type = params.output_type.unwrap_or_default();

    let (tx, rx) = futures_channel::mpsc::unbounded::<Result<Vec<u8>, Report>>();

    let mut builder = Response::builder()
        .header("Content-Type", output_type.mime_type());

    if params.download.unwrap_or(false) {
        let disposition = format!("attachment; filename=\"{}-{}.{}\"", actor.config.name, animation.slug(), output_type.extension());
        builder = builder.header("Content-Disposition", disposition);
    }

    let mut render_params = params.into_render_parameters(skin_colours)?;

    if args.public {
        render_params.apply_reasonable_limits();
    }

    spawn_blocking(move || {
        let render = match output_type {
            OutputType::Gif => actor.spine.render_gif(render_params, tx),
            OutputType::Apng => actor.spine.render_apng(render_params, tx),
            OutputType::Png => actor.spine.render_png(render_params, tx),
            OutputType::Mp4 => actor.spine.render_ffmpeg(render_params, tx),
        };
        if let Err(e) = render {
            warn!("Failed to render: {:?}", e);
        }
    });

    Ok(builder.body(StreamBody::from(rx)).unwrap())
}
