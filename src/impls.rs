use std::ffi::{c_char, c_int};

use spine::sys::spAtlasPage;
use image::{DynamicImage, GenericImageView};


#[no_mangle]
pub extern "C" fn _spAtlasPage_createTexture(page: *mut spAtlasPage, path: *const c_char) {
    #[inline]
    fn read_texture_file(path: *const c_char) -> Result<DynamicImage> {
        let path = to_str(path)?;
        image::open(path).map_err(Error::invalid_data)
    }

    let texture = read_texture_file(path).unwrap();
    let (width, height) = texture.dimensions();

    unsafe {
        (*page).width = width as c_int;
        (*page).height = height as c_int;
        (*page).rendererObject = Box::into_raw(Box::new(texture)) as *mut c_void;
    }
}

#[no_mangle]
pub extern "C" fn _spAtlasPage_disposeTexture(page: *mut spAtlasPage) {
    unsafe {
        Box::from_raw((*page).rendererObject as *mut DynamicImage);
    }
}
