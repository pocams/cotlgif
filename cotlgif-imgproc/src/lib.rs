mod resize;

use gifski::progress::NoProgress;
use gifski::Settings;
use imgref::ImgVec;
use png::{BitDepth, ColorType};
use rgb::FromSlice;
use std::{io, thread};
use thiserror::Error;
use tracing::{debug, error, warn};

use cotlgif_render::{Frame, FrameHandler, HandleFrameError, RenderMetadata};

pub use crate::resize::ResizeWrapper;

#[derive(Error, Debug)]
pub enum RenderError {
    #[error("encoder failure: {0}")]
    EncodeError(String),
}

pub struct GifRenderer {
    collector: gifski::Collector,
}

impl GifRenderer {
    pub fn new<W: io::Write + Send + 'static>(output: W) -> GifRenderer
    where
        W: io::Write + Send + 'static,
    {
        let settings = Settings {
            quality: 75,
            ..Default::default()
        };
        let (gs_collector, gs_writer) = gifski::new(settings).unwrap();

        // Start a thread to send output to `output` as it's generated
        thread::spawn(move || {
            let mut progress = NoProgress {};
            if let Err(e) = gs_writer.write(output, &mut progress) {
                warn!("Failed writing output: {:?}", e);
            };
        });

        GifRenderer {
            collector: gs_collector,
        }
    }
}

impl FrameHandler for GifRenderer {
    fn set_metadata(&mut self, metadata: RenderMetadata) {
        debug!("GifRenderer metadata {:?}", metadata);
    }

    fn handle_frame(&mut self, frame: Frame) -> Result<(), HandleFrameError> {
        let img = ImgVec::new(
            Vec::from(frame.pixel_data.as_rgba()),
            frame.width as usize,
            frame.height as usize,
        );
        self.collector
            .add_frame_rgba(frame.frame_number as usize, img, frame.timestamp)
            .map_err(|e| {
                error!("add_frame_rgba: {:?}", e);
                HandleFrameError::PermanentError
            })
    }
}

impl Drop for GifRenderer {
    fn drop(&mut self) {
        debug!("GifRenderer finished");
    }
}

pub struct ApngRenderer<W>
where
    W: io::Write + Send + 'static,
{
    writer: Option<png::Writer<W>>,
    last_timestamp: f64,
    output: Option<W>,
}

impl<W: io::Write + Send + 'static> ApngRenderer<W> {
    pub fn new(output: W) -> ApngRenderer<W> {
        ApngRenderer {
            writer: None,
            last_timestamp: 0.0,
            output: Some(output),
        }
    }
}

impl<W: io::Write + Send + 'static> FrameHandler for ApngRenderer<W> {
    fn set_metadata(&mut self, metadata: RenderMetadata) {
        debug!("ApngRenderer metadata {:?}", metadata);
        let mut encoder = png::Encoder::new(
            self.output.take().unwrap(),
            metadata.frame_width as u32,
            metadata.frame_height as u32,
        );
        encoder.set_color(ColorType::Rgba);
        encoder.set_depth(BitDepth::Eight);

        if let Err(e) = encoder.set_animated(metadata.frame_count, 0) {
            error!("set_animated: {:?}", e);
            // Leave self.writer unset - we will abort on the first handle_frame() call
            return;
        }

        if let Ok(mut writer) = encoder.write_header() {
            // Set the first frame's delay, since we won't have it available in handle_frame()
            match writer.set_frame_delay((1000.0 * metadata.frame_delay).round() as u16, 1000) {
                Ok(_) => {
                    self.writer = Some(writer);
                }
                Err(e) => {
                    error!("set_frame_delay(): {:?}", e);
                }
            };
        }
    }

    fn handle_frame(&mut self, frame: Frame) -> Result<(), HandleFrameError> {
        match self.writer.as_mut() {
            Some(writer) => {
                if frame.frame_number != 0 {
                    // Only set the frame delay if we aren't on the first frame - if this is frame 0,
                    // the delay was set in set_metadata()
                    let frame_delay =
                        (1000.0 * (frame.timestamp - self.last_timestamp)).round() as u16;
                    // debug!("frame delay: {:?}", frame_delay);
                    writer.set_frame_delay(frame_delay, 1000).map_err(|e| {
                        error!("set_frame_delay(): {:?}", e);
                        HandleFrameError::PermanentError
                    })?;
                }
                writer
                    .write_image_data(frame.pixel_data.as_slice())
                    .map_err(|e| {
                        error!("write_image_data(): {:?}", e);
                        HandleFrameError::PermanentError
                    })?;
                self.last_timestamp = frame.timestamp;
                Ok(())
            }
            None => {
                error!("handle_frame(): no writer!");
                Err(HandleFrameError::PermanentError)
            }
        }
    }
}

impl<W: io::Write + Send + 'static> Drop for ApngRenderer<W> {
    fn drop(&mut self) {
        debug!("ApngRenderer finished");
    }
}

pub struct PngRenderer<W>
where
    W: io::Write + Send + 'static,
{
    writer: Option<png::Writer<W>>,
    output: Option<W>,
}

impl<W: io::Write + Send + 'static> PngRenderer<W> {
    pub fn new(output: W) -> PngRenderer<W> {
        PngRenderer {
            writer: None,
            output: Some(output),
        }
    }
}

impl<W: io::Write + Send + 'static> FrameHandler for PngRenderer<W> {
    fn set_metadata(&mut self, metadata: RenderMetadata) {
        debug!("PngRenderer metadata {:?}", metadata);
        let mut encoder = png::Encoder::new(
            self.output.take().unwrap(),
            metadata.frame_width as u32,
            metadata.frame_height as u32,
        );
        encoder.set_color(ColorType::Rgba);
        encoder.set_depth(BitDepth::Eight);

        if let Ok(writer) = encoder.write_header() {
            self.writer = Some(writer);
        }
    }

    fn handle_frame(&mut self, frame: Frame) -> Result<(), HandleFrameError> {
        match self.writer.as_mut() {
            Some(writer) => {
                writer
                    .write_image_data(frame.pixel_data.as_slice())
                    .map_err(|e| {
                        error!("write_image_data(): {:?}", e);
                        HandleFrameError::PermanentError
                    })?;
                Ok(())
            }
            None => {
                error!("handle_frame(): no writer!");
                Err(HandleFrameError::PermanentError)
            }
        }
    }
}

impl<W: io::Write + Send + 'static> Drop for PngRenderer<W> {
    fn drop(&mut self) {
        debug!("PngRenderer finished");
    }
}

/*
pub fn render_ffmpeg(&self, params: RenderParameters, response_sender: futures_channel::mpsc::UnboundedSender<Result<Vec<u8>, Report>>) -> Result<(), Report> {
    let fps = (1.0 / params.frame_delay).round() as u32;
    let prepared_params = self.prepare_render(params)?;
    debug!("prepared params: {:?}", prepared_params);

    let mut ffmpeg = Command::new("ffmpeg")
        .args([
            "-f", "rawvideo",
            "-pixel_format", "rgba",
            "-video_size", &format!("{}x{}", prepared_params.final_width, prepared_params.final_height),
            "-framerate", &format!("{}", fps),
            "-i", "pipe:",
            "-vcodec", "vp8",
            "-deadline", "realtime",
            // Output fragmented video - otherwise we can't write mp4 to a non-seekable medium
            //"-movflags", "frag_keyframe+empty_moov",
            "-an",  // Audio - none
            "-f", "webm",
            "-auto-alt-ref", "0",
            "pipe:",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    thread::scope(move |scope| {
        let mut writer = ChannelWriter::new(response_sender);
        let mut stdin = ffmpeg.stdin.take().unwrap();
        let mut stdout = ffmpeg.stdout.take().unwrap();
        let stderr = ffmpeg.stderr.take().unwrap();

        // Log ffmpeg's stderr
        scope.spawn(move || {
            for line in BufReader::new(stderr).lines() {
                match line {
                    Ok(l) => debug!("ffmpeg: {}", l),
                    Err(e) => debug!("ffmpeg error: {:?}", e),
                }
            }
        });

        // Feed rendered frames into ffmpeg's stdin
        scope.spawn(move || self.render(prepared_params, |frame| {
            stdin.write_all(frame.pixel_data)?;
            Ok(())
        }));

        // Send ffmpeg's stdout straight to the client
        scope.spawn(move || io::copy(&mut stdout, &mut writer));
    });

    info!("Finished handling request");
    Ok(())
}
*/
