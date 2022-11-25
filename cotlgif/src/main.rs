use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::thread;
use tracing_subscriber::EnvFilter;
use cotlgif_http::{HttpActor, HttpOptions, HttpRenderRequest, OutputType};
use cotlgif_render::{Frame, FrameHandler, HandleFrameError, RenderError, RenderMetadata, SpineActor};
use clap::Parser;
use tracing::{debug, info, warn, error};
use color_eyre::eyre::eyre;
use crossbeam_channel::RecvError;
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
        debug!("Render request received: {:?}", http_render_request.render_request);
        // The slug has to be correct, since the http service checked it - if not we want to know, so crash
        let spine_actor = spine_actors.get(&http_render_request.render_request.actor_slug).unwrap();
        let buf_renderer = match http_render_request.output_type {
            OutputType::Gif => RenderBufferer::new(cotlgif_imgproc::GifRenderer::new(http_render_request.writer), 1000),
            OutputType::Apng => RenderBufferer::new(cotlgif_imgproc::ApngRenderer::new(http_render_request.writer), 1000),
            OutputType::Png => RenderBufferer::new(cotlgif_imgproc::PngRenderer::new(http_render_request.writer), 1000),
        };

        match cotlgif_render::render(
            spine_actor,
            http_render_request.render_request,
            Box::new(buf_renderer)
        ) {
            Ok(_) => info!("Render finished"),
            Err(e) => error!("Render error: {:?}", e),
        };
    }

    Ok(())
}


enum BufferMessage {
    Metadata(RenderMetadata),
    Frame(Frame),
}

pub struct RenderBufferer {
    sender: crossbeam_channel::Sender<BufferMessage>
}

impl FrameHandler for RenderBufferer {
    fn set_metadata(&mut self, metadata: RenderMetadata) {
        if let Err(_) = self.sender.send(BufferMessage::Metadata(metadata)) {
            error!("RenderBufferer send metadata failed");
        }
    }

    fn handle_frame(&mut self, frame: Frame) -> Result<(), HandleFrameError> {
        self.sender.send(BufferMessage::Frame(frame))
            .map_err(|_| HandleFrameError::PermanentError)
    }
}

impl Drop for RenderBufferer {
    fn drop(&mut self) {
        info!("Buffered render finished");
    }
}

impl RenderBufferer {
    pub fn new<FH: FrameHandler + Send + 'static>(mut inner: FH, buffer_size: usize) -> RenderBufferer {
        let (sender, receiver) = crossbeam_channel::bounded(buffer_size);

        thread::spawn(move || {
            loop {
                match receiver.recv() {
                    Ok(BufferMessage::Frame(f)) => {
                        match inner.handle_frame(f) {
                            Ok(_) => {}
                            Err(HandleFrameError::TemporaryError) => {}
                            Err(HandleFrameError::PermanentError) => {
                                error!("RenderBufferer: permanent receiver error");
                                break;
                            }
                        }
                    },
                    Ok(BufferMessage::Metadata(m)) => inner.set_metadata(m),
                    // This means the sender went away
                    Err(_) => break
                }
            }
        });

        RenderBufferer {
            sender
        }
    }
}
