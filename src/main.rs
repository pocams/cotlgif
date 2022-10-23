use std::borrow::Cow;
use std::collections::HashMap;
use std::error::Error;
use std::ffi::OsStr;
use std::num::{ParseFloatError, ParseIntError};
use std::ops::Deref;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use axum::extract::{BodyStream, Host, Path, Query};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json, Router};
use axum::body::{Body, StreamBody};
use axum::http::StatusCode;
use axum::routing::{get, get_service};
use color_eyre::eyre::eyre;
use color_eyre::Report;
use rusty_spine::Color;
use tracing_subscriber::EnvFilter;
use tower_http::trace::TraceLayer;
use serde_json::json;
use serde::Deserialize;
use css_color_parser2::Color as CssColor;
use tokio::spawn;
use tokio::sync::mpsc::unbounded_channel;
use tokio::task::spawn_blocking;
use tracing::{debug, info, warn};
use tower_http::services::{ServeDir, ServeFile};

use crate::actors::{Actor, RenderParameters};

mod actors;

/*
URLs:

  /v1/(player, follower)/(baseskin)/(animation).(gif, png)
    ?add_skin=a,b,c
    ?antialiasing=<int>
    ?start_time=<float>
    ?end_time=<float> (only for gif)
    ?color1=RRGGBB (only for follower?)
    ?color2=RRGGBB (only for follower?)

  /v1/
  [
    {
      "url": "/v1/player/",
      "description": "Player"
    }
  ]

  /v1/player/
    {
      "description": "Player",
      "animations": [
        {
          "name": "idle",
          "duration": 0.7
        }
      ],
      "skins": [
        {
          "name": "Lamb"
        {
      ]
    }
*/

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
    color1: Option<Color>,
    color2: Option<Color>,
    color3: Option<Color>,
    background_color: Option<Color>,
    fps: Option<u32>
}

impl TryFrom<Vec<(String, String)>> for SkinParameters {
    type Error = JsonError;

    fn try_from(params: Vec<(String, String)>) -> Result<SkinParameters, Self::Error> {
        let mut sp = SkinParameters::default();
        for (key, value) in params.into_iter() {
            match key.to_ascii_lowercase().as_str() {
                "format" => sp.output_type = Some(value.parse()?),
                "add_skin" => sp.add_skin.push(value),
                "animation" => sp.animation = Some(value),
                "scale" => sp.scale = Some(value.parse().map_err(|e| json_400(format!("scale: {e:?}")))?),
                "antialiasing" => sp.antialiasing = Some(value.parse().map_err(|e| json_400(format!("antialiasing: {e:?}")))?),
                "start_time" => sp.start_time = Some(value.parse().map_err(|e| json_400(format!("start_time: {e:?}")))?),
                "end_time" => sp.end_time = Some(value.parse().map_err(|e| json_400(format!("end_time: {e:?}")))?),
                "color" => {
                    let c = color_from_string(value.as_str()).map_err(|e| json_400(format!("color: {}", e)))?;
                    sp.color1 = Some(c);
                    sp.color2 = Some(c);
                    sp.color3 = Some(c);
                },
                "color1" => sp.color1 = Some(color_from_string(value.as_str()).map_err(|e| json_400(format!("color1: {}", e)))?),
                "color2" => sp.color2 = Some(color_from_string(value.as_str()).map_err(|e| json_400(format!("color2: {}", e)))?),
                "color3" => sp.color3 = Some(color_from_string(value.as_str()).map_err(|e| json_400(format!("color3: {}", e)))?),
                "background_color" => sp.background_color = Some(color_from_string(value.as_str()).map_err(|e| json_400(format!("background_color: {}", e)))?),
                "fps" => sp.fps = Some(value.parse().map_err(|e| json_400(format!("fps: {e:?}")))?),
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
            background_color: self.background_color.unwrap_or_default(),
            color1: self.color1,
            color2: self.color2,
            color3: self.color3,
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

    let serve_dir_service = get_service(ServeDir::new("static"))
        .handle_error(|err| async move {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Unhandled internal error: {}", err),
            )
        });

    let app = Router::new()
        .route("/v1", get(get_v1))
        .route("/v1/:actor", get(get_v1_actor))
        .route("/v1/:actor/:skin", get(get_v1_skin))
        // If we don't match any routes, try serving static files from /static
        .fallback(serve_dir_service)
        .layer(Extension(actors))
        .layer(TraceLayer::new_for_http());

    info!("Starting server");
    axum::Server::bind(&"0.0.0.0:3000".parse().unwrap())
        .serve(app.into_make_service())
        .await
        .unwrap();

    Ok(())
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
    // let show_spoilers = host == "cotl-spoilers.xl0.org" || host.starts_with("localhost");
    let show_spoilers = host == "cotl-spoilers.xl0.org";
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

    let render_params = params.into_render_parameters()?;

    let (tx, rx) = futures_channel::mpsc::unbounded::<Result<Vec<u8>, Report>>();


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

    Ok(Response::builder()
        .header("Content-Type", output_type.mime_type())
        .body(StreamBody::from(rx))
        .unwrap())
}
