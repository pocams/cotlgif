use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::thread;
use tracing_subscriber::EnvFilter;
use cotlgif_http::{HttpActor, HttpOptions, HttpRenderRequest};
use cotlgif_render::{RenderError, SpineActor};
use clap::Parser;
use tracing::{debug, info, warn, error};
use color_eyre::eyre::eyre;
use serde::Deserialize;
use cotlgif_common::{ActorConfig, SkinColours};

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
    public: bool,

    /// Use index.dev.html so we refer to JS served by `npm run dev`
    #[arg(long)]
    dev: bool,
}

impl Args {
    fn get_http_options(&self) -> HttpOptions {
        HttpOptions {
            listen: self.listen.clone(),
            spoilers_host: self.spoilers_host.clone(),
            public: self.public,
            dev: self.dev
        }
    }
}

#[derive(Deserialize, Debug)]
pub struct Config {
    pub actors: Vec<ActorConfig>
}

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "info,cotlgif=debug")
    }

    let config: Config = toml::from_slice(
        &std::fs::read("config.toml").map_err(|e| eyre!("Reading config.toml: {:?}", e))?
    ).map_err(|e| eyre!("Parsing config.toml: {}", e))?;

    tracing_subscriber::fmt::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let args = Args::parse();
    let mut actors = Vec::with_capacity(config.actors.len());

    let mut http_actors = Vec::with_capacity(config.actors.len());
    let mut spine_actors = HashMap::with_capacity(config.actors.len());

    for actor in &config.actors {
        debug!(actor=?actor, "Loading actor");
        let spine_actor = SpineActor::load(&actor.atlas, &actor.skeleton)?;
        http_actors.push(HttpActor::new(actor, &spine_actor.skins, &spine_actor.animations));
        spine_actors.insert(actor.slug.clone(), spine_actor);
        actors.push(actor);
    }

    let (render_request_sender, mut render_request_receiver) = tokio::sync::mpsc::channel(16);

    let runtime = Arc::new(tokio::runtime::Runtime::new()?);
    thread::spawn(move || runtime.block_on(
        cotlgif_http::serve(
            args.get_http_options(),
            http_actors,
            SkinColours::load(),
            render_request_sender
        )
    ));

    while let Some(http_render_request) = render_request_receiver.blocking_recv() {
        // The slug has to be correct, since the http service checked it - if not we want to know, so crash
        let spine_actor = spine_actors.get(&http_render_request.render_request.actor_slug).unwrap();
        let gif_renderer = cotlgif_imgproc::GifRenderer::new(http_render_request.writer);

        match cotlgif_render::render(
            spine_actor,
            http_render_request.render_request,
            Box::new(gif_renderer)
        ) {
            Ok(_) => info!("Render finished"),
            Err(e) => error!("Render error: {:?}", e),
        };
    }

    Ok(())
}

/*
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

*/
