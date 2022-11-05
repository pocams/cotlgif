use std::num::NonZeroU32;

use fast_image_resize::PixelType;
use tracing::debug;

// We want to own the image because we're going to mutate it before we resize it
pub fn resize(from: (usize, usize), to: (usize, usize), image: Vec<u8>) -> Vec<u8> {
    debug!("AA: resizing to {}x{}", to.0, to.1);
    // RenderTexture lives on the GPU, so this could be done quicker with some GPU-based
    // algorithm, but SFML doesn't have fancy resize algorithms on the GPU right now.

    let mut resize_img = fast_image_resize::Image::from_vec_u8(
        NonZeroU32::new(from.0 as u32).unwrap(),
        NonZeroU32::new(from.1 as u32).unwrap(),
        image.to_vec(),
        PixelType::U8x4
    ).unwrap();

    // According to the docs this is required
    let alpha_mul_div = fast_image_resize::MulDiv::default();
    alpha_mul_div
        .multiply_alpha_inplace(&mut resize_img.view_mut())
        .unwrap();

    let mut destination_image = fast_image_resize::Image::new(
        NonZeroU32::new(to.0 as u32).unwrap(),
        NonZeroU32::new(to.1 as u32).unwrap(),
        PixelType::U8x4
    );

    let mut resizer = fast_image_resize::Resizer::new(
        fast_image_resize::ResizeAlg::Convolution(fast_image_resize::FilterType::Lanczos3),
    );
    resizer.resize(&resize_img.view(), &mut destination_image.view_mut()).unwrap();

    alpha_mul_div.divide_alpha_inplace(&mut destination_image.view_mut()).unwrap();
    debug!("AA: resize finished");
    destination_image.into_vec()
}
