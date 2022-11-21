use std::borrow::Cow;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::ops::Deref;
use std::sync::Arc;
use axum::http::{header, StatusCode};
use axum::{Extension, Json, Router};
use axum::body::StreamBody;
use axum::extract::{Host, Path, Query};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, get_service};
use color_eyre::Report;
use rusty_spine::Color;
use serde::Serialize;
use serde_json::json;
use tokio::task::spawn_blocking;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing::{debug, info, warn};
use crate::text::TextParameters;
use crate::{Actor, Args, color_from_string, get_index, get_spoiler_js, get_v1, get_v1_actor, get_v1_colours, get_v1_skin, util};
use crate::colours::SkinColours;
use crate::util::{JsonError, OutputType, Slug};

const CACHE_CONTROL_SHORT: &str = "max-age=60";

pub struct HttpOptions {
    /// Host and port to listen on
    listen: SocketAddr,
    /// Show spoilers when accessed via this vhost
    spoilers_host: String,
    /// Limit parameters to try to avoid abuse
    public: bool
}


// FIXME don't want these structs like this anymore
impl Slug for Animation {
    fn slug(&self) -> String {
        <Animation as Slug>::slugify_string(&self.name)
    }
}

#[derive(Serialize, Debug)]
pub struct Animation {
    pub name: String,
    pub duration: f32
}

impl Slug for Skin {
    fn slug(&self) -> String {
        <Skin as Slug>::slugify_string(&self.name)
    }
}



// FIXME run on param struct from this file, not from render.rs
pub fn apply_reasonable_limits(&mut self) {
    if self.skins.len() > 7 {
        debug!("LIMITS: skins reducing from {} to 7", self.skins.len());
        self.skins.truncate(7);
    }
    if self.scale > 3.0 {
        debug!("LIMITS: scale reducing from {} to 3.0", self.scale);
        self.scale = 3.0;
    }
    if self.antialiasing > 4 {
        debug!("LIMITS: antialiasing reducing from {} to 4", self.antialiasing);
        self.antialiasing = 4;
    }
    if self.start_time > 60.0 {
        debug!("LIMITS: start_time increasing from {} to 60", self.start_time);
        self.start_time = 60.0;
    }
    if self.end_time > 60.0 {
        debug!("LIMITS: end_time increasing from {} to 60", self.end_time);
        self.end_time = 60.0;
    }
    if self.frame_delay < 1.0 / 120.0 {
        debug!("LIMITS: frame_delay increasing from {} to 1/120", self.frame_delay);
        self.frame_delay = 1.0 / 120.0;
    }
    if let Some(t) = self.text_parameters.as_mut() {
        if t.font_size > 200 {
            debug!("LIMITS: font_size decreasing from {} to 200", t.font_size);
            t.font_size = 200;
        }
        if let Some(top_text) = t.top_text.as_mut() {
            if top_text.len() > 100 {
                debug!("LIMITS: top_text length decreasing from {} to 100", top_text.len());
                top_text.truncate(100);
            }
        }
        if let Some(bottom_text) = t.bottom_text.as_mut() {
            if bottom_text.len() > 100 {
                debug!("LIMITS: bottom_text length decreasing from {} to 100", bottom_text.len());
                bottom_text.truncate(100);
            }
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
                "top_text" | "bottom_text" | "font" | "font_size" => {
                    sp.text_parameters.get_or_insert_with(|| Default::default()).set_from_params(key, value)?;
                }
                _ => return Err(util::json_400(Cow::from(format!("Invalid parameter {:?}", key))))
            }
        }
        Ok(sp)
    }
}

fn color_from_string(s: &str) -> color_eyre::Result<Color> {
    let css_color = s.parse::<CssColor>()?;
    Ok(Color::new_rgba(css_color.r as f32 / 255.0, css_color.g as f32 / 255.0, css_color.b as f32 / 255.0, css_color.a))
}

async fn serve(options: HttpOptions) {
    let serve_dir_service = get_service(ServeDir::new("static"))
        .handle_error(|err| async move {
            // There was some error serving a static file other than "not found"
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Unhandled internal error: {}", err),
            )
        });

    let listen_host = options.listen;
    let app = Router::new()
        .route("/", get(get_index))
        .route("/init.js", get(get_spoiler_js))
        .route("/v1", get(get_v1))
        .route("/v1/:actor", get(get_v1_actor))
        .route("/v1/:actor/colours", get(get_v1_colours))
        .route("/v1/:actor/:skin", get(get_v1_skin))
        .nest("/static", serve_dir_service)
        .layer(Extension(actors))
        .layer(Extension(skin_colours))
        .layer(TraceLayer::new_for_http());

    info!("Starting server");
    axum::Server::bind(&listen_host)
        .serve(app.into_make_service())
        .await
        .unwrap();
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
