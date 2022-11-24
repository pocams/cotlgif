use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
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
use regex::Regex;
use serde::Serialize;
use serde_json::json;
use tokio::sync::mpsc;
use tokio::task::spawn_blocking;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing::{debug, info, warn};
use cotlgif_common::{ActorCategory, ActorConfig, RenderRequest, SkinColours, slugify_string, SpineAnimation, SpineSkin};
use crate::params::SkinParameters;
use crate::util::{ChannelWriter, json_500, JsonError, OutputType};

mod params;
mod util;

pub use util::HttpRenderRequest;

const CACHE_CONTROL_SHORT: &str = "max-age=60";

pub struct HttpOptions {
    /// Host and port to listen on
    pub listen: SocketAddr,
    /// Show spoilers when accessed via this vhost
    pub spoilers_host: String,
    /// Limit parameters to try to avoid abuse
    pub public: bool,
    /// Use index.dev.html so we refer to JS served by `npm run dev`
    pub dev: bool,
}

impl HttpOptions {
    fn should_enable_spoilers(&self, host: &str) -> bool {
        host == self.spoilers_host
    }
}

pub struct HttpActor {
    config: ActorConfig,
    pub all_skins: Vec<SpineSkin>,
    pub all_animations: Vec<SpineAnimation>,
    pub nonspoiler_skins: Vec<SpineSkin>,
    pub nonspoiler_animations: Vec<SpineAnimation>,
}

impl HttpActor {
    pub fn new(actor_config: &ActorConfig, skins: &Vec<SpineSkin>, animations: &Vec<SpineAnimation>) -> HttpActor {
        let nonspoiler_skins = if actor_config.is_spoiler {
            Vec::new()
        } else {
            skins.iter()
                .filter(|s| actor_config.spoiler_skins.as_ref().map(|r| !r.is_match(&s.name)).unwrap_or(true))
                .cloned()
                .collect()
        };

        let nonspoiler_animations = if actor_config.is_spoiler {
            Vec::new()
        } else {
            animations.iter()
                .filter(|a| actor_config.spoiler_animations.as_ref().map(|r| !r.is_match(&a.name)).unwrap_or(true))
                .cloned()
                .collect()
        };

        HttpActor {
            config: actor_config.clone(),
            all_skins: skins.to_owned(),
            all_animations: animations.to_owned(),
            nonspoiler_skins,
            nonspoiler_animations,
        }
    }

    pub fn is_valid_skin(&self, skin_name: &str, include_spoilers: bool) -> bool {
        if include_spoilers {
            self.all_skins.iter().any(|s| s.name == skin_name)
        } else {
            self.nonspoiler_skins.iter().any(|s| s.name == skin_name)
        }
    }

    pub fn is_valid_animation(&self, animation_name: &str, include_spoilers: bool) -> bool {
        if include_spoilers {
            self.all_animations.iter().any(|s| s.name == animation_name)
        } else {
            self.nonspoiler_animations.iter().any(|s| s.name == animation_name)
        }
    }

    fn as_json(&self, include_spoilers: bool) -> Option<serde_json::Value> {
        if !include_spoilers && self.config.is_spoiler {
            return None
        }

        Some(
            json!({
                    "name": self.config.name,
                    "slug": self.config.slug,
                    "category": self.config.category,
                    "skins": if include_spoilers { &self.all_skins } else { &self.nonspoiler_skins },
                    "animations": if include_spoilers { &self.all_animations } else { &self.nonspoiler_animations },
            })
        )
    }
}

pub async fn serve(options: HttpOptions, actors: Vec<HttpActor>, skin_colours: SkinColours, render_request_channel: mpsc::Sender<HttpRenderRequest>) {
    let serve_dir_service = get_service(ServeDir::new("static"))
        .handle_error(|err| async move {
            // There was some unexpected error serving a static file
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
        .layer(Extension(Arc::new(options)))
        .layer(Extension(Arc::new(actors)))
        .layer(Extension(Arc::new(skin_colours)))
        .layer(Extension(render_request_channel))
        .layer(TraceLayer::new_for_http());

    info!("Starting server");
    axum::Server::bind(&listen_host)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn get_index(
    Extension(options): Extension<Arc<HttpOptions>>
) -> impl IntoResponse {
    let filename = if options.dev {
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

async fn get_spoiler_js(Host(host): Host, Extension(options): Extension<Arc<HttpOptions>>) -> impl IntoResponse {
    let body = if options.should_enable_spoilers(&host) {
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
    Extension(actors): Extension<Arc<Vec<HttpActor>>>,
    Extension(options): Extension<Arc<HttpOptions>>,
    Host(host): Host
) -> impl IntoResponse {
    let show_spoilers = options.should_enable_spoilers(&host);

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
    Extension(actors): Extension<Arc<Vec<HttpActor>>>,
    Extension(options): Extension<Arc<HttpOptions>>,
    Path(actor_slug): Path<String>,
    Host(host): Host
) -> impl IntoResponse {
    // Does the actor exist?
    if let Some(actor) = actors.iter().find(|a| a.config.slug == actor_slug) {
        // as_json() will return None if we're not showing spoilers and the whole actor is a spoiler
        if let Some(json) = actor.as_json(options.should_enable_spoilers(&host)) {
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
    Extension(actors): Extension<Arc<Vec<HttpActor>>>,
    Extension(options): Extension<Arc<HttpOptions>>,
    Extension(skin_colours): Extension<Arc<SkinColours>>,
    Extension(render_request_channel): Extension<mpsc::Sender<HttpRenderRequest>>,
    Path((actor_slug, skin_name)): Path<(String, String)>,
    Query(params): Query<Vec<(String, String)>>,
    Host(host): Host
) -> Result<impl IntoResponse, JsonError> {
    let actor = actors.iter().find(|a| a.config.slug == actor_slug).ok_or_else(|| util::json_404("No such actor"))?;
    let enable_spoilers = options.should_enable_spoilers(&host);

    if actor.config.is_spoiler && !enable_spoilers {
        return Err(util::json_404("No such actor"));
    }

    let mut params = SkinParameters::try_from(params)?;
    if options.public { params.apply_reasonable_limits(); }

    let render_request = params.render_request(&actor, &skin_name, enable_spoilers)?;

    let output_type = params.output_type.unwrap_or_default();
    let mut builder = Response::builder()
        .header("Content-Type", output_type.mime_type());

    if params.download.unwrap_or(false) {
        builder = builder.header(
            "Content-Disposition",
            format!("attachment; filename=\"{}-{}.{}\"", actor.config.name, slugify_string(&render_request.animation), output_type.extension())
        );
    }

    let (tx, rx) = futures_channel::mpsc::unbounded::<Result<Vec<u8>, tokio::io::Error>>();
    let writer = ChannelWriter::new(tx);

    render_request_channel.send(HttpRenderRequest {
        render_request,
        output_type,
        writer: Box::new(writer)
    }).await
        .map_err(|e| json_500(format!("Internal server error: {}", e)))?;

    Ok(builder.body(StreamBody::from(rx)).unwrap())
}
