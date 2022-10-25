use std::borrow::Cow;
use std::collections::HashMap;
use std::ops::Deref;
use std::str::FromStr;
use std::sync::Arc;

use axum::{Extension, Json, Router};
use axum::body::StreamBody;
use axum::extract::{Host, Path, Query};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, get_service};
use color_eyre::Report;
use css_color_parser2::Color as CssColor;
use rusty_spine::Color;
use serde_json::json;
use tokio::task::spawn_blocking;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing::{debug, info, warn};
use tracing_subscriber::EnvFilter;

use crate::actors::{Actor, RenderParameters, Slug};
use crate::colours::SkinColours;

mod actors;
mod colours;

const SPOILERS_HOST: &str = "cotl-spoilers.xl0.org";

#[derive(Debug, Copy, Clone)]
enum OutputType {
    Gif,
    Apng,
    Png,
    Mp4,
}

impl OutputType {
    fn mime_type(&self) -> &'static str {
        match self {
            OutputType::Gif => "image/gif",
            OutputType::Apng | OutputType::Png => "image/png",
            OutputType::Mp4 => "video/mp4",
        }
    }

    fn extension(&self) -> &'static str {
        match self {
            OutputType::Gif => "gif",
            OutputType::Apng | OutputType::Png => "png",
            OutputType::Mp4 => "mp4",
        }
    }
}

impl Default for OutputType {
    fn default() -> Self {
        OutputType::Apng
    }
}

impl FromStr for OutputType {
    type Err = JsonError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_ref() {
            "gif" | ".gif" => Ok(OutputType::Gif),
            "png" | ".png" => Ok(OutputType::Png),
            "apng" | ".apng" => Ok(OutputType::Apng),
            "mp4" | ".mp4" => Ok(OutputType::Mp4),
            _ => Err(json_400("Invalid format, expected gif, png, apng, mp4"))
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
    slot_colours: HashMap<String, Color>,
    background_colour: Option<Color>,
    fps: Option<u32>,
    only_head: Option<bool>,
    download: Option<bool>,
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
                "scale" => sp.scale = Some(value.parse().map_err(|e| json_400(format!("scale: {e:?}")))?),
                "antialiasing" => sp.antialiasing = Some(value.parse().map_err(|e| json_400(format!("antialiasing: {e:?}")))?),
                "start_time" => sp.start_time = Some(value.parse().map_err(|e| json_400(format!("start_time: {e:?}")))?),
                "end_time" => sp.end_time = Some(value.parse().map_err(|e| json_400(format!("end_time: {e:?}")))?),
                "background" => sp.background_colour = Some(color_from_string(value.as_str()).map_err(|e| json_400(format!("background_color: {}", e)))?),
                "fps" => sp.fps = Some(value.parse().map_err(|e| json_400(format!("fps: {e:?}")))?),
                "only_head" => sp.only_head = Some(value.parse().map_err(|e| json_400(format!("only_head: {e:?}")))?),
                "download" => sp.download = Some(value.parse().map_err(|e| json_400(format!("download: {e:?}")))?),
                // Skin colour parameters
                "HEAD_SKIN_TOP" | "HEAD_SKIN_BTM" | "MARKINGS" | "ARM_LEFT_SKIN" | "ARM_RIGHT_SKIN" | "LEG_LEFT_SKIN" | "LEG_RIGHT_SKIN" => {
                    sp.slot_colours.insert(key.clone(), color_from_string(value.as_str()).map_err(|e| json_400(format!("{}: {}", key, e)))?);
                }
                _ => return Err(json_400(Cow::from(format!("Invalid parameter {:?}", key))))
            }
        }
        Ok(sp)
    }
}

impl SkinParameters {
    fn into_render_parameters(self) -> Result<RenderParameters, JsonError> {
        let fps = (self.fps.unwrap_or(50) as f32).max(1.0);

        Ok(RenderParameters {
            skins: self.add_skin,
            animation: self.animation.ok_or(json_400("animation= is required"))?,
            scale: self.scale.unwrap_or(1.0),
            antialiasing: self.antialiasing.unwrap_or(1),
            start_time: self.start_time.unwrap_or(0.0),
            end_time: self.end_time.unwrap_or(1.0),
            frame_delay: 1.0 / fps,
            background_colour: self.background_colour.unwrap_or_default(),
            slot_colours: self.slot_colours,
            only_head: self.only_head.unwrap_or(false)
        })
    }
}

fn color_from_string(s: &str) -> color_eyre::Result<Color> {
    let css_color = s.parse::<CssColor>()?;
    debug!("css color: {:?}", css_color);
    Ok(Color::new_rgba(css_color.r as f32 / 255.0, css_color.g as f32 / 255.0, css_color.b as f32 / 255.0, css_color.a))
}

struct JsonError {
    message: String,
    status: StatusCode
}

fn json_400(s: impl Into<String>) -> JsonError {
    JsonError { message: s.into(), status: StatusCode::BAD_REQUEST }
}

fn json_404(s: impl Into<String>) -> JsonError {
    JsonError { message: s.into(), status: StatusCode::NOT_FOUND }
}

impl IntoResponse for JsonError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({"error": self.message}))).into_response()
    }
}

async fn load_actors() -> color_eyre::Result<Vec<Arc<Actor>>> {
    Ok(vec![
        Arc::new(Actor::new("player".to_owned(), "Player".to_owned(), "cotl/player-main.skel", "cotl/player-main.atlas").await?),
        Arc::new(Actor::new("follower".to_owned(), "Follower".to_owned(), "cotl/Follower.skel", "cotl/Follower.atlas").await?),
        Arc::new(Actor::new("ratau".to_owned(), "Ratau".to_owned(), "cotl/RatNPC.skel", "cotl/RatNPC.atlas").await?),
        Arc::new(Actor::new("fox".to_owned(), "Fox".to_owned(), "cotl/Fox.skel", "cotl/Fox.atlas").await?),
    ])
}

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "info,cotlgif=debug")
    }

    tracing_subscriber::fmt::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let actors = Arc::new(load_actors().await?);
    let skin_colours = Arc::new(SkinColours::load());

    let serve_dir_service = get_service(ServeDir::new("static"))
        .handle_error(|err| async move {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Unhandled internal error: {}", err),
            )
        });

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
        .layer(TraceLayer::new_for_http());

    info!("Starting server");
    axum::Server::bind(&"0.0.0.0:3000".parse().unwrap())
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
                Response::builder()
                    .header("Content-Type", "text/html")
                    .body(String::from_utf8(f).unwrap())
                    .unwrap()
            )
        }
        Err(e) => {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Response::builder()
                    .body(format!("{:?}", e))
                    .unwrap()
            )
        }
    }
}

async fn get_spoiler_js(Host(host): Host) -> impl IntoResponse {
    let body = if host == SPOILERS_HOST {
        "window.spoilersEnabled = true;\n"
    } else {
        "\n"
    };

    Response::builder()
        .header("Content-Type", "text/javascript")
        .body(body.to_owned())
        .unwrap()
}

async fn get_v1(Extension(actors): Extension<Arc<Vec<Arc<Actor>>>>) -> impl IntoResponse {
    let actor_json: Vec<_> = actors.iter().map(|actor| {
        let mut m = HashMap::new();
        m.insert("description", actor.description.to_owned());
        m.insert("url", format!("/v1/{}", actor.name));
        m
    }).collect();

    Json(json!({
        "actors": actor_json
    }))
}

async fn get_v1_actor(
    Extension(actors): Extension<Arc<Vec<Arc<Actor>>>>,
    Path(actor_name): Path<String>,
    Host(host): Host
) -> impl IntoResponse {
    let show_spoilers = host == SPOILERS_HOST || host.starts_with("localhost");
    info!("Request host {}, spoilers {}", host, show_spoilers);

    if let Some(actor) = actors.iter().find(|a| a.name == actor_name) {
        if show_spoilers {
            (StatusCode::OK, Json(serde_json::to_value(actor.deref()).unwrap()))
        } else {
            (StatusCode::OK, Json(actor.serialize_without_spoilers()))

        }
    } else {
        (StatusCode::NOT_FOUND, Json(json!({"error": "no such actor"})))
    }
}

async fn get_v1_colours(
    Extension(skin_colours): Extension<Arc<SkinColours>>,
    Path(actor_name): Path<String>,
    Host(_host): Host
) -> impl IntoResponse {
    if actor_name != "follower" {
        // Only followers have colour sets
        return (StatusCode::NOT_FOUND, Json(json!({"error": "no colours available for actor"})))
    }

    (StatusCode::OK, Json(serde_json::to_value(skin_colours.deref()).unwrap()))
}

async fn get_v1_skin(
    Extension(actors): Extension<Arc<Vec<Arc<Actor>>>>,
    Path((actor_name, skin_name)): Path<(String, String)>,
    Query(params): Query<Vec<(String, String)>>
) -> Result<impl IntoResponse, JsonError> {
    let mut params = SkinParameters::try_from(params)?;
    debug!("params: {:?}", params);

    let actor = actors.iter().find(|a| a.name == actor_name).ok_or(json_404("No such actor"))?.clone();

    let animation_name = params.animation.as_deref().ok_or(json_400("animation= parameter is required"))?;
    let animation = actor.animations.iter().find(|anim| anim.name == animation_name).ok_or(json_404("No such animation for actor"))?;

    if params.end_time.is_none() {
        params.end_time = Some(animation.duration);
    }

    params.add_skin.insert(0, skin_name);
    for add_skin in &params.add_skin {
        if actor.skins.iter().find(|s| &s.name == add_skin).is_none() {
            return Err(json_400(format!("No such skin for actor: {add_skin:?}")))
        }
    }

    let output_type = params.output_type.unwrap_or_default();

    let (tx, rx) = futures_channel::mpsc::unbounded::<Result<Vec<u8>, Report>>();

    let mut builder = Response::builder()
        .header("Content-Type", output_type.mime_type());

    if params.download.unwrap_or(false) {
        let disposition = format!("attachment; filename=\"{}-{}.{}\"", actor.name, animation.slug(), output_type.extension());
        builder = builder.header("Content-Disposition", disposition);
    }

    let render_params = params.into_render_parameters()?;
    if actor.name != "follower" && !render_params.slot_colours.is_empty() {
        return Err(json_400(format!("Only follower supports slot colours")));
    }

    spawn_blocking(move || {
        let render = match output_type {
            OutputType::Gif => actor.render_gif(render_params, tx),
            OutputType::Apng => actor.render_apng(render_params, tx),
            OutputType::Png => actor.render_png(render_params, tx),
            OutputType::Mp4 => actor.render_ffmpeg(render_params, tx),
        };
        if let Err(e) = render {
            warn!("Failed to render: {:?}", e);
        }
    });

    Ok(builder.body(StreamBody::from(rx)).unwrap())
}
