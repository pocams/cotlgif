use std::num::NonZeroU32;

use fast_image_resize::PixelType;

use cotlgif_render::{Frame, FrameHandler, HandleFrameError, RenderMetadata};

pub struct ResizeWrapper<FH: FrameHandler + 'static> {
    target_width: NonZeroU32,
    target_height: NonZeroU32,
    mul_div: fast_image_resize::MulDiv,
    resizer: fast_image_resize::Resizer,
    frame_handler: FH,
}

#[derive(Debug, Copy, Clone)]
pub enum ResizerError {
    ZeroDimension,
}

impl<FH: FrameHandler + 'static> ResizeWrapper<FH> {
    pub fn new(
        target_width: usize,
        target_height: usize,
        next_handler: FH,
    ) -> Result<ResizeWrapper<FH>, ResizerError> {
        let algorithm =
            fast_image_resize::ResizeAlg::Convolution(fast_image_resize::FilterType::Lanczos3);

        Ok(ResizeWrapper {
            target_width: NonZeroU32::new(target_width as u32)
                .ok_or(ResizerError::ZeroDimension)?,
            target_height: NonZeroU32::new(target_height as u32)
                .ok_or(ResizerError::ZeroDimension)?,
            mul_div: Default::default(),
            resizer: fast_image_resize::Resizer::new(algorithm),
            frame_handler: next_handler,
        })
    }
}

impl<FH: FrameHandler> FrameHandler for ResizeWrapper<FH> {
    fn set_metadata(&mut self, metadata: RenderMetadata) {
        self.frame_handler.set_metadata(RenderMetadata {
            frame_count: metadata.frame_count,
            frame_delay: metadata.frame_delay,
            frame_width: self.target_width.get() as usize,
            frame_height: self.target_height.get() as usize,
        });
    }

    fn handle_frame(&mut self, frame: Frame) -> Result<(), HandleFrameError> {
        // RenderTexture lives on the GPU, so this could be done quicker with some GPU-based
        // algorithm, but SFML doesn't have fancy resize algorithms on the GPU right now.
        let mut resize_img = fast_image_resize::Image::from_vec_u8(
            NonZeroU32::new(frame.width).ok_or(HandleFrameError::PermanentError)?,
            NonZeroU32::new(frame.height).ok_or(HandleFrameError::PermanentError)?,
            frame.pixel_data,
            PixelType::U8x4,
        )
        .map_err(|_| HandleFrameError::PermanentError)?;

        // According to the docs this is required
        self.mul_div
            .multiply_alpha_inplace(&mut resize_img.view_mut())
            .map_err(|_| HandleFrameError::PermanentError)?;

        let mut destination_image =
            fast_image_resize::Image::new(self.target_width, self.target_height, PixelType::U8x4);

        self.resizer
            .resize(&resize_img.view(), &mut destination_image.view_mut())
            .map_err(|_| HandleFrameError::PermanentError)?;

        self.mul_div
            .divide_alpha_inplace(&mut destination_image.view_mut())
            .map_err(|_| HandleFrameError::PermanentError)?;

        self.frame_handler.handle_frame(Frame {
            frame_number: frame.frame_number,
            pixel_data: destination_image.into_vec(),
            width: self.target_width.get(),
            height: self.target_height.get(),
            timestamp: frame.timestamp,
        })
    }
}
