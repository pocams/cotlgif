use std::io::BufReader;
use std::process::{Command, Stdio};
use std::{io, thread};
use gifski::progress::NoProgress;
use gifski::Settings;
use imgref::ImgVec;
use png::{BitDepth, ColorType};
use tracing::{debug, info, warn, error};
use rgb::FromSlice;
use thiserror::Error;

use cotlgif_render::{Frame, FrameHandler, HandleFrameError, RenderMetadata};

#[derive(Error, Debug)]
pub enum RenderError {
    #[error("encoder failure: {0}")]
    EncodeError(String)
}

pub struct GifRenderer {
    collector: gifski::Collector
}

impl GifRenderer {
    pub fn new<W: io::Write + Send + 'static>(output: W) -> GifRenderer where W: io::Write + Send + 'static {
        let settings = Settings { quality: 75, ..Default::default() };
        let (gs_collector, gs_writer) = gifski::new(settings).unwrap();

        // Start a thread to send output to `output` as it's generated
        thread::spawn(move || {
            let mut progress = NoProgress {};
            if let Err(e) = gs_writer.write(output, &mut progress) {
                warn!("Failed writing output: {:?}", e);
            };
        });

        GifRenderer {
            collector: gs_collector
        }
    }
}

impl FrameHandler for GifRenderer {
    fn set_metadata(&mut self, _: RenderMetadata) {}

    fn handle_frame(&mut self, frame: Frame) -> Result<(), HandleFrameError> {
        let img = ImgVec::new(Vec::from(frame.pixel_data.as_rgba()), frame.width as usize, frame.height as usize);
        self.collector.add_frame_rgba(frame.frame_number as usize, img, frame.timestamp)
            .map_err(|e| {
                error!("add_frame_rgba: {:?}", e);
                HandleFrameError::PermanentError
            })
    }
}

struct ApngRenderer<'a, W> where W: io::Write + Send + 'static {
    encoder: Option<png::Encoder<'a, W>>,
    writer: Option<png::Writer<W>>,
    last_timestamp: f64,
    output: Option<W>
}

impl<'a, W: io::Write + Send + 'static> ApngRenderer<'a, W> {
    fn new(output: W) -> ApngRenderer<'a, W> {
        ApngRenderer {
            encoder: None,
            writer: None,
            last_timestamp: 0.0,
            output: Some(output)
        }
    }
}

impl<W: io::Write + Send + 'static> FrameHandler for ApngRenderer<'_, W> {
    fn set_metadata(&mut self, metadata: RenderMetadata) {
        let mut encoder = png::Encoder::new(self.output.take().unwrap(), metadata.frame_width as u32, metadata.frame_height as u32);
        encoder.set_color(ColorType::Rgba);
        encoder.set_depth(BitDepth::Eight);

        if let Err(e) = encoder.set_animated(metadata.frame_count, 0) {
            error!("set_animated: {:?}", e);
            // Leave self.writer unset - we will abort on the first handle_frame() call
            return
        }

        if let Ok(writer) = encoder.write_header() {
            self.writer = Some(writer);
        }
    }

    fn handle_frame(&mut self, frame: Frame) -> Result<(), HandleFrameError> {
        match self.writer.as_mut() {
            Some(writer) => {
                writer.set_frame_delay((1000.0 * (frame.timestamp - self.last_timestamp)).round() as u16, 1000)
                    .map_err(|e| {
                        error!("set_frame_delay(): {:?}", e);
                        HandleFrameError::PermanentError
                    })?;
                writer.write_image_data(frame.pixel_data.as_slice())
                    .map_err(|e| {
                        error!("write_image_data(): {:?}", e);
                        HandleFrameError::PermanentError
                    })?;
                Ok(())
            },
            None => {
                error!("handle_frame(): no writer!");
                Err(HandleFrameError::PermanentError)
            }

        }
    }
}

/*
pub fn render_png(&self, mut params: RenderParameters, response_sender: futures_channel::mpsc::UnboundedSender<Result<Vec<u8>, Report>>) -> Result<(), Report> {
    params.end_time = params.start_time;
    let prepared_params = self.prepare_render(params)?;
    debug!("prepared params: {:?}", prepared_params);

    let writer = ChannelWriter::new(response_sender);
    let mut encoder = png::Encoder::new(writer, prepared_params.final_width as u32, prepared_params.final_height as u32);
    encoder.set_color(ColorType::Rgba);
    encoder.set_depth(BitDepth::Eight);
    let mut png_writer = encoder.write_header()?;

    self.render(prepared_params, |frame| {
        png_writer.write_image_data(frame.pixel_data)?;
        Ok(())
    });

    png_writer.finish()?;
    info!("Finished handling request");
    Ok(())
}

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
