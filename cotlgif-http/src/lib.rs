use std::net::SocketAddr;
use std::ops::Deref;

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use argon2::Argon2;
use axum::body::StreamBody;
use axum::extract::{ConnectInfo, FromRef, Host, Path, Query, State};
use axum::http::{header, Request, StatusCode};
use axum::middleware::{from_fn_with_state, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, get_service, post};
use axum::{Json, Router, TypedHeader};
use cookie::time::OffsetDateTime;
use cookie::{Expiration, SameSite};
use governor::clock::DefaultClock;
use governor::state::keyed::DefaultKeyedStateStore;
use governor::{Quota, RateLimiter};

use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation};
use nonzero_ext::nonzero;
use password_hash::{PasswordHashString, PasswordVerifier};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::mpsc;
use tower_cookies::{Cookie, CookieManagerLayer, Cookies};
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing::{debug, error, info, warn};

use crate::params::SkinParameters;
use crate::util::{json_400, json_500, ChannelWriter, JsonError};
use cotlgif_common::{slugify_string, ActorConfig, SkinColours, SpineAnimation, SpineSkin};

mod params;
mod util;
mod x_real_ip;

use crate::x_real_ip::XRealIp;
pub use util::{HttpRenderRequest, OutputType};

const CACHE_CONTROL_SHORT: &str = "max-age=60";
const JWT_VALID_FOR: Duration = Duration::from_secs(86400 * 30);
const JWT_ISSUER: &str = "The One Who Validates";
const JWT_COOKIE: &str = "login";
const AUTHENTICATION_RATE_LIMIT: Quota =
    Quota::per_minute(nonzero!(15u32)).allow_burst(nonzero!(2u32));

pub struct AuthenticationOptions {
    /// Require a password for API interactions
    pub password_hash: PasswordHashString,
    /// Shared secret for JWT signing
    pub jwt_secret_key: String,

    jwt_encoding_key: EncodingKey,
    jwt_decoding_key: DecodingKey,
}

impl AuthenticationOptions {
    pub fn new(
        password_hash: String,
        jwt_secret_key: String,
    ) -> Result<AuthenticationOptions, password_hash::errors::Error> {
        let jwt_encoding_key = EncodingKey::from_secret(jwt_secret_key.as_bytes());
        let jwt_decoding_key = DecodingKey::from_secret(jwt_secret_key.as_bytes());
        Ok(AuthenticationOptions {
            password_hash: PasswordHashString::new(&password_hash)?,
            jwt_secret_key,
            jwt_encoding_key,
            jwt_decoding_key,
        })
    }
}

#[derive(Deserialize)]
struct LoginBody {
    pub password: String,
}

pub struct HttpOptions {
    /// Host and port to listen on
    pub listen: SocketAddr,
    /// Show spoilers when accessed via this vhost
    pub spoilers_host: String,
    /// Limit parameters to try to avoid abuse
    pub public: bool,
    /// Use index.dev.html so we refer to JS served by `npm run dev`
    pub dev: bool,
    /// Require authentication for API interactions if set
    pub authentication_options: Option<AuthenticationOptions>,
}

impl HttpOptions {
    fn should_enable_spoilers(&self, host: &str) -> bool {
        debug!("enable spoilers? {} == {}", host, self.spoilers_host);
        host == self.spoilers_host
    }

    fn authentication_required(&self) -> bool {
        self.authentication_options.is_some()
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
    pub fn new(
        actor_config: &ActorConfig,
        skins: &Vec<SpineSkin>,
        animations: &Vec<SpineAnimation>,
    ) -> HttpActor {
        let nonspoiler_skins = if actor_config.is_spoiler {
            Vec::new()
        } else {
            skins
                .iter()
                .filter(|s| {
                    actor_config
                        .spoiler_skins
                        .as_ref()
                        .map(|r| !r.is_match(&s.name))
                        .unwrap_or(true)
                })
                .cloned()
                .collect()
        };

        let nonspoiler_animations = if actor_config.is_spoiler {
            Vec::new()
        } else {
            animations
                .iter()
                .filter(|a| {
                    actor_config
                        .spoiler_animations
                        .as_ref()
                        .map(|r| !r.is_match(&a.name))
                        .unwrap_or(true)
                })
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

    pub fn features(&self) -> Vec<&'static str> {
        let mut features = vec![];
        if self.config.head_slots.is_some() {
            features.push("only_head");
        }
        if self.config.has_slot_colours {
            features.push("slot_colours");
        }
        features
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
            self.nonspoiler_animations
                .iter()
                .any(|s| s.name == animation_name)
        }
    }

    fn as_json(&self, include_spoilers: bool) -> Option<serde_json::Value> {
        if !include_spoilers && self.config.is_spoiler {
            return None;
        }

        Some(json!({
                "name": self.config.name,
                "slug": self.config.slug,
                "category": self.config.category,
                "skins": if include_spoilers { &self.all_skins } else { &self.nonspoiler_skins },
                "animations": if include_spoilers { &self.all_animations } else { &self.nonspoiler_animations }
        }))
    }
}

type AuthenticationRateLimiter = RateLimiter<String, DefaultKeyedStateStore<String>, DefaultClock>;

#[derive(Clone, FromRef)]
struct AppState {
    options: Arc<HttpOptions>,
    actors: Arc<Vec<HttpActor>>,
    skin_colours: Arc<SkinColours>,
    render_request_channel: mpsc::Sender<HttpRenderRequest>,
    authentication_rate_limiter: Arc<AuthenticationRateLimiter>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    iat: usize,  // Issued at
    exp: usize,  // Expiry
    iss: String, // Issuer
}

impl Claims {
    fn cookie_expiry(&self) -> Expiration {
        Expiration::DateTime(OffsetDateTime::from_unix_timestamp(self.exp as i64).unwrap())
    }
}

impl Default for Claims {
    fn default() -> Self {
        let now = SystemTime::now();
        let expiry = now + JWT_VALID_FOR;
        Claims {
            iat: now
                .duration_since(SystemTime::UNIX_EPOCH)
                .expect("SystemTime before UNIX_EPOCH")
                .as_secs() as usize,
            exp: expiry
                .duration_since(SystemTime::UNIX_EPOCH)
                .expect("SystemTime before UNIX_EPOCH")
                .as_secs() as usize,
            iss: JWT_ISSUER.to_string(),
        }
    }
}

pub async fn serve(
    options: HttpOptions,
    actors: Vec<HttpActor>,
    skin_colours: SkinColours,
    render_request_channel: mpsc::Sender<HttpRenderRequest>,
) {
    let serve_dir_service = get_service(ServeDir::new("static")).handle_error(|err| async move {
        // There was some unexpected error serving a static file
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Unhandled internal error: {}", err),
        )
    });

    let listen_host = options.listen;

    let state = AppState {
        options: Arc::new(options),
        actors: Arc::new(actors),
        skin_colours: Arc::new(skin_colours),
        render_request_channel,
        authentication_rate_limiter: Arc::new(RateLimiter::keyed(AUTHENTICATION_RATE_LIMIT)),
    };

    let v1_router = Router::new()
        .route("/", get(get_v1))
        // Handle trailing slashes in v1 api since axum transparently fixed these up in 0.5, in api v2 this can be stricter
        .route("/:actor", get(get_v1_actor))
        .route("/:actor/", get(get_v1_actor))
        .route("/:actor/colours", get(get_v1_colours))
        .route("/:actor/colours/", get(get_v1_colours))
        .route("/:actor/:skin", get(get_v1_skin))
        .route("/:actor/:skin/", get(get_v1_skin))
        .with_state(state.clone())
        .layer(from_fn_with_state(state.clone(), authentication));

    let app = Router::new()
        .route("/", get(get_index))
        .route("/login", post(handle_login))
        .route("/logout", post(handle_logout))
        .route("/init.js", get(get_init_js))
        .nest("/v1", v1_router)
        .nest_service("/static", serve_dir_service)
        .with_state(state)
        .layer(CookieManagerLayer::new())
        .layer(TraceLayer::new_for_http());

    info!("Starting server");
    axum::Server::bind(&listen_host)
        .serve(app.into_make_service_with_connect_info::<SocketAddr>())
        .await
        .unwrap();
}

async fn authentication<B>(
    State(options): State<Arc<HttpOptions>>,
    cookies: Cookies,
    request: Request<B>,
    next: Next<B>,
) -> Response {
    if let Some(authentication_options) = options.authentication_options.as_ref() {
        // Authentication is enabled, so make sure the user is authenticated
        if let Some(cookie) = cookies.get(JWT_COOKIE) {
            let mut validation = Validation::default();
            validation.set_issuer(&[JWT_ISSUER]);
            match jsonwebtoken::decode::<Claims>(
                cookie.value(),
                &authentication_options.jwt_decoding_key,
                &validation,
            ) {
                Ok(claims) => {
                    // Authentication is required and was successful - fall through to the body
                    // of the function
                    debug!("JWT accepted: {:?}", claims);
                }
                Err(e) => {
                    warn!("JWT validation failed: {:?}", e);
                    return JsonError {
                        status: StatusCode::UNAUTHORIZED,
                        message: "authentication required".to_string(),
                    }
                    .into_response();
                }
            }
        } else {
            debug!("No JWT presented");
            return JsonError {
                status: StatusCode::UNAUTHORIZED,
                message: "authentication required".to_string(),
            }
            .into_response();
        }
    }

    // At this point, either authentication is disabled or it was successful

    // Pass the request down the middleware stack and return its response
    next.run(request).await
}

async fn get_index(State(options): State<Arc<HttpOptions>>) -> impl IntoResponse {
    let filename = if options.dev {
        "html/index.dev.html"
    } else {
        "html/index.html"
    };

    match tokio::fs::read(filename).await {
        Ok(f) => (
            StatusCode::OK,
            [
                (header::CACHE_CONTROL, CACHE_CONTROL_SHORT),
                (header::CONTENT_TYPE, "text/html"),
            ],
            String::from_utf8(f).unwrap(),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            [
                (header::CACHE_CONTROL, CACHE_CONTROL_SHORT),
                (header::CONTENT_TYPE, "text/plain"),
            ],
            format!("{:?}", e),
        ),
    }
}

async fn handle_login(
    State(options): State<Arc<HttpOptions>>,
    State(rate_limiter): State<Arc<AuthenticationRateLimiter>>,
    x_real_ip: Option<TypedHeader<XRealIp>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    cookies: Cookies,
    Json(body): Json<LoginBody>,
) -> impl IntoResponse {
    let client_ip = if addr.ip().is_loopback() {
        // The real connecting IP is localhost, so we want to use the X-Real-IP header
        if let Some(xri) = x_real_ip.as_ref() {
            // We have an X-Real-IP, use it
            xri.0 .0.to_string()
        } else {
            // No X-Real-IP, so just fall back to the connecting IP
            addr.to_string()
        }
    } else {
        // The real connecting IP isn't localhost, so we don't trust X-Real-IP
        addr.to_string()
    };

    if rate_limiter.check_key(&client_ip).is_err() {
        warn!(
            "{}: too many authentication attempts, rate limiting",
            client_ip
        );
        return JsonError {
            message: "too many authentication attempts".to_string(),
            status: StatusCode::TOO_MANY_REQUESTS,
        }
        .into_response();
    }

    let Some(authentication_options) = options.authentication_options.as_ref() else {
        warn!("{}: Attempted login but authentication disabled", client_ip);
        return json_400("authentication not available").into_response();
    };

    let argon2 = Argon2::default();
    let password_hash = authentication_options.password_hash.password_hash();
    match argon2.verify_password(body.password.as_bytes(), &password_hash) {
        Ok(_) => {
            info!("{}: Successful login", client_ip);
            let claims = Claims::default();
            let jwt = match jsonwebtoken::encode(
                &Header::default(),
                &claims,
                &authentication_options.jwt_encoding_key,
            ) {
                Ok(jwt) => jwt,
                Err(e) => {
                    error!("Encoding JWT failed: {:?}", e);
                    return json_500("jwt encoding failure").into_response();
                }
            };

            let mut cookie = Cookie::new(JWT_COOKIE, jwt);
            cookie.set_expires(claims.cookie_expiry());
            cookie.set_http_only(true);
            // Set the cookie to secure unless we're in dev (running on localhost)
            cookie.set_secure(!options.dev);
            // Allow the cookie to be sent with 3rd party embedded image requests
            cookie.set_same_site(SameSite::None);
            cookies.add(cookie);

            (StatusCode::OK, Json(json!({"result": "login successful"}))).into_response()
        }

        Err(e) => {
            warn!("{}: Invalid login attempt: {:?}", client_ip, e);
            JsonError {
                message: "invalid credentials".to_string(),
                status: StatusCode::UNAUTHORIZED,
            }
            .into_response()
        }
    }
}

async fn handle_logout(cookies: Cookies) -> impl IntoResponse {
    cookies.remove(Cookie::named(JWT_COOKIE));

    (StatusCode::OK, Json(json!({"result": "logout successful"})))
}

async fn get_init_js(
    Host(host): Host,
    State(options): State<Arc<HttpOptions>>,
) -> impl IntoResponse {
    let mut body = String::from("\n");

    if options.should_enable_spoilers(&host) {
        body.push_str("window.spoilersEnabled = true;\n");
    }

    if options.authentication_required() {
        body.push_str("window.authenticationRequired = true;\n");
    }

    (
        [
            (header::CONTENT_TYPE, "text/javascript"),
            (header::CACHE_CONTROL, CACHE_CONTROL_SHORT),
        ],
        body,
    )
}

async fn get_v1(
    State(actors): State<Arc<Vec<HttpActor>>>,
    State(options): State<Arc<HttpOptions>>,
    Host(host): Host,
) -> impl IntoResponse {
    let show_spoilers = options.should_enable_spoilers(&host);

    let actor_json: Vec<_> = actors
        .iter()
        .filter(|actor| show_spoilers || !actor.config.is_spoiler)
        .map(|actor| {
            json!({
                "name": actor.config.name,
                "slug": actor.config.slug,
                "category": actor.config.category,
                "default_skins": actor.config.default_skins,
                "default_animation": actor.config.default_animation,
                "default_scale": actor.config.default_scale,
                "features": actor.features(),
            })
        })
        .collect();

    (
        [(header::CACHE_CONTROL, CACHE_CONTROL_SHORT)],
        Json(json!({ "actors": actor_json })),
    )
}

async fn get_v1_actor(
    State(actors): State<Arc<Vec<HttpActor>>>,
    State(options): State<Arc<HttpOptions>>,
    Path(actor_slug): Path<String>,
    Host(host): Host,
) -> impl IntoResponse {
    // Does the actor exist?
    if let Some(actor) = actors.iter().find(|a| a.config.slug == actor_slug) {
        // as_json() will return None if we're not showing spoilers and the whole actor is a spoiler
        if let Some(json) = actor.as_json(options.should_enable_spoilers(&host)) {
            return (
                StatusCode::OK,
                [(header::CACHE_CONTROL, CACHE_CONTROL_SHORT)],
                Json(json),
            );
        }
    }

    (
        StatusCode::NOT_FOUND,
        [(header::CACHE_CONTROL, CACHE_CONTROL_SHORT)],
        Json(json!({"error": "no such actor"})),
    )
}

async fn get_v1_colours(
    State(skin_colours): State<Arc<SkinColours>>,
    Path(actor_name): Path<String>,
    Host(_host): Host,
) -> impl IntoResponse {
    let json = if actor_name == "follower" {
        // Only followers have colour sets
        serde_json::to_value(skin_colours.deref()).unwrap()
    } else {
        json!([])
    };

    ([(header::CACHE_CONTROL, CACHE_CONTROL_SHORT)], Json(json))
}

async fn get_v1_skin(
    State(actors): State<Arc<Vec<HttpActor>>>,
    State(options): State<Arc<HttpOptions>>,
    State(skin_colours): State<Arc<SkinColours>>,
    State(render_request_channel): State<mpsc::Sender<HttpRenderRequest>>,
    Path((actor_slug, skin_name)): Path<(String, String)>,
    Query(params): Query<Vec<(String, String)>>,
    Host(host): Host,
) -> Result<impl IntoResponse, JsonError> {
    let actor = actors
        .iter()
        .find(|a| a.config.slug == actor_slug)
        .ok_or_else(|| util::json_404("No such actor"))?;
    let enable_spoilers = options.should_enable_spoilers(&host);

    if actor.config.is_spoiler && !enable_spoilers {
        return Err(util::json_404("No such actor"));
    }

    let mut params = SkinParameters::try_from(params)?;
    if options.public {
        params.apply_reasonable_limits();
    }

    let output_type = params.output_type.unwrap_or_default();
    let render_request =
        params.render_request(actor, &skin_name, enable_spoilers, &skin_colours)?;

    let mut builder = Response::builder().header("Content-Type", output_type.mime_type());

    if params.download.unwrap_or(false) {
        builder = builder.header(
            "Content-Disposition",
            format!(
                "attachment; filename=\"{}-{}.{}\"",
                actor.config.name,
                slugify_string(&render_request.animation),
                output_type.extension()
            ),
        );
    }

    let (tx, rx) = futures_channel::mpsc::unbounded::<Result<Vec<u8>, tokio::io::Error>>();
    let writer = ChannelWriter::new(tx);

    render_request_channel
        .send(HttpRenderRequest {
            render_request,
            output_type,
            writer: Box::new(writer),
        })
        .await
        .map_err(|e| json_500(format!("Internal server error: {}", e)))?;

    Ok(builder.body(StreamBody::from(rx)).unwrap())
}
