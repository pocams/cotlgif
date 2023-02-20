use clap::Parser;
use color_eyre::eyre::eyre;
use cotlgif_http::{HttpActor, HttpOptions, OutputType};
use cotlgif_render::{Frame, FrameHandler, HandleFrameError, RenderMetadata, SpineActor};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::thread;
use tracing::{debug, error, info};
use tracing_subscriber::EnvFilter;

use cotlgif_common::{ActorConfig, CustomSize, SkinColours};
use serde::Deserialize;

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
            listen: self.listen,
            spoilers_host: self.spoilers_host.clone(),
            public: self.public,
            dev: self.dev,
        }
    }
}

#[derive(Deserialize, Debug)]
pub struct Config {
    pub actors: Vec<ActorConfig>,
}

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "info,cotlgif=debug")
    }

    let toml_bytes = std::fs::read("config.toml")
            .map_err(|e| eyre!("Reading config.toml: {:?}", e))?;

    let toml_str: String = String::from_utf8(toml_bytes)
        .map_err(|e| eyre!("Invalid utf-8 in config.toml: {:?}", e))?;

    let config: Config = toml::from_str(&toml_str)
        .map_err(|e| eyre!("Parsing config.toml: {}", e))?;

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
        http_actors.push(HttpActor::new(
            actor,
            &spine_actor.skins,
            &spine_actor.animations,
        ));
        spine_actors.insert(actor.slug.clone(), spine_actor);
        actors.push(actor);
    }

    let (render_request_sender, mut render_request_receiver) = tokio::sync::mpsc::channel(16);

    let runtime = Arc::new(tokio::runtime::Runtime::new()?);
    thread::spawn(move || {
        runtime.block_on(cotlgif_http::serve(
            args.get_http_options(),
            http_actors,
            SkinColours::load(),
            render_request_sender,
        ))
    });

    while let Some(http_render_request) = render_request_receiver.blocking_recv() {
        debug!(
            "Render request received: {:?}",
            http_render_request.render_request
        );
        // The slug has to be correct, since the http service checked it - if not we want to know, so crash
        let spine_actor = spine_actors
            .get(&http_render_request.render_request.actor_slug)
            .unwrap();

        // I tried to use Box<dyn> to prevent this insane situation, but I couldn't make it out of
        // the dark forest of type errors
        let buf_renderer = match http_render_request.render_request.custom_size {
            CustomSize::DefaultSize => match http_render_request.output_type {
                OutputType::Gif => RenderBufferer::new(
                    cotlgif_imgproc::GifRenderer::new(http_render_request.writer),
                    1000,
                ),
                OutputType::Apng => RenderBufferer::new(
                    cotlgif_imgproc::ApngRenderer::new(http_render_request.writer),
                    1000,
                ),
                OutputType::Png => RenderBufferer::new(
                    cotlgif_imgproc::PngRenderer::new(http_render_request.writer),
                    1000,
                ),
            },

            CustomSize::Discord128x128 => match http_render_request.output_type {
                OutputType::Gif => RenderBufferer::new(
                    cotlgif_imgproc::ResizeWrapper::new(
                        128,
                        128,
                        cotlgif_imgproc::GifRenderer::new(http_render_request.writer),
                    )
                    .unwrap(),
                    1000,
                ),
                OutputType::Apng => RenderBufferer::new(
                    cotlgif_imgproc::ResizeWrapper::new(
                        128,
                        128,
                        cotlgif_imgproc::ApngRenderer::new(http_render_request.writer),
                    )
                    .unwrap(),
                    1000,
                ),
                OutputType::Png => RenderBufferer::new(
                    cotlgif_imgproc::ResizeWrapper::new(
                        128,
                        128,
                        cotlgif_imgproc::PngRenderer::new(http_render_request.writer),
                    )
                    .unwrap(),
                    1000,
                ),
            },
        };

        let frame_handler: Box<dyn FrameHandler> = if http_render_request.render_request.slots_to_draw.is_some() {
            Box::new(Recropper::new(buf_renderer))
        } else {
            Box::new(buf_renderer)
        };

        match cotlgif_render::render(
            spine_actor,
            http_render_request.render_request,
            frame_handler,
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
    sender: crossbeam_channel::Sender<BufferMessage>,
}

impl FrameHandler for RenderBufferer {
    fn set_metadata(&mut self, metadata: RenderMetadata) {
        if self.sender.send(BufferMessage::Metadata(metadata)).is_err() {
            error!("RenderBufferer send metadata failed");
        }
    }

    fn handle_frame(&mut self, frame: Frame) -> Result<(), HandleFrameError> {
        self.sender
            .send(BufferMessage::Frame(frame))
            .map_err(|_| HandleFrameError::PermanentError)
    }
}

impl Drop for RenderBufferer {
    fn drop(&mut self) {
        info!("Buffered render finished");
    }
}

impl RenderBufferer {
    pub fn new<FH: FrameHandler + Send + 'static>(
        mut inner: FH,
        buffer_size: usize,
    ) -> RenderBufferer {
        let (sender, receiver) = crossbeam_channel::bounded(buffer_size);

        thread::spawn(move || {
            loop {
                match receiver.recv() {
                    Ok(BufferMessage::Frame(f)) => match inner.handle_frame(f) {
                        Ok(_) => {}
                        Err(HandleFrameError::TemporaryError) => {}
                        Err(HandleFrameError::PermanentError) => {
                            error!("RenderBufferer: permanent receiver error");
                            break;
                        }
                    },
                    Ok(BufferMessage::Metadata(m)) => inner.set_metadata(m),
                    // This means the sender went away
                    Err(_) => break,
                }
            }
        });

        RenderBufferer { sender }
    }
}

pub struct Recropper<FH: FrameHandler + Send + 'static> {
    frames: Vec<Frame>,
    metadata: Option<RenderMetadata>,
    top: usize,
    left: usize,
    bottom: usize,
    right: usize,
    frame_handler: FH
}

impl<FH> Recropper<FH> where FH: FrameHandler + Send + 'static {
    fn new(frame_handler: FH) -> Recropper<FH> {
        Recropper {
            frames: Vec::new(),
            metadata: None,
            top: usize::MAX,
            left: usize::MAX,
            bottom: 0,
            right: 0,
            frame_handler
        }
    }
}

impl<FH> FrameHandler for Recropper<FH> where FH: FrameHandler + Send + 'static {
    fn set_metadata(&mut self, metadata: RenderMetadata) {
        self.metadata = Some(metadata);
    }

    fn handle_frame(&mut self, frame: Frame) -> Result<(), HandleFrameError> {
        let frame_height = self.metadata.as_ref().unwrap().frame_height;
        let frame_width = self.metadata.as_ref().unwrap().frame_width;

        for row in 0..frame_height {
            let row_start = frame_width * 4 * row;
            let row_end = frame_width * 4 * (row + 1);

            if row < self.top {
                for pixel in (row_start..row_end).step_by(4) {
                    if frame.pixel_data[pixel + 3] != 0 {
                        self.top = row;
                        break;
                    }
                }
            }

            if row > self.bottom {
                for pixel in (row_start..row_end).step_by(4) {
                    if frame.pixel_data[pixel + 3] != 0 {
                        self.bottom = row;
                        break;
                    }
                }
            }
        }

        for col in 0..frame_width {
            let col_start = col * 4;
            let col_end = (frame_width * 4 * (frame_height - 1)) + col_start;

            if col < self.left {
                for pixel in (col_start..col_end).step_by(frame_width * 4) {
                    if frame.pixel_data[pixel + 3] != 0 {
                        // debug!("frame {} col {}, pix set {} {:?}", frame.frame_number, col, pixel, &frame.pixel_data[pixel..pixel+4]);
                        self.left = col;
                        break;
                    }
                }
            }

            if col > self.right {
                for pixel in (col_start..col_end).step_by(frame_width * 4) {
                    if frame.pixel_data[pixel + 3] != 0 {
                        self.right = col;
                        break;
                    }
                }
            }
        }

        self.frames.push(frame);
        Ok(())
    }
}

impl<FH> Drop for Recropper<FH> where FH: FrameHandler + Send + 'static {
    fn drop(&mut self) {
        // debug!("recropper: top {} bot {} left {} right {}", self.top, self.bottom, self.left, self.right);
        if let Some(mut md) = self.metadata.take() {
            let old_width = md.frame_width;
            let new_height = self.bottom - self.top;
            let new_width = self.right - self.left;
            md.frame_height = new_height;
            md.frame_width = new_width;
            self.frame_handler.set_metadata(md);

            for mut frame in self.frames.drain(0..) {
                let mut new_pixel_data = Vec::with_capacity((new_height) * (new_width) * 4);
                for row in self.top..self.bottom {
                    let row_start = old_width * row * 4;
                    for col in self.left..self.right {
                        let col_start = row_start + (4 * col);
                        new_pixel_data.extend_from_slice(&frame.pixel_data[col_start..(col_start + 4)])
                    }
                }
                frame.width = new_width as u32;
                frame.height = new_height as u32;
                frame.pixel_data = new_pixel_data;
                if self.frame_handler.handle_frame(frame).is_err() { break }
            }
        }
    }
}
