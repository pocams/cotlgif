use std::fs::File;
use std::ops::{Add, Div, Mul};
use std::ptr::NonNull;
use std::thread;
use fnv::FnvHashMap;
use image::{DynamicImage, GenericImageView, Pixel, Rgba, RgbaImage};
use image::ColorType::Rgba8;
use itertools::Itertools;
use spine::animation::{AnimationState, AnimationStateData};
use spine::geometry::Vertex;
use spine::render::Renderer;
use spine::skeleton::{Skeleton, SkeletonBinary, SkeletonData, SkeletonJson};
use spine::sys::{spAtlasPage, spPositionMode_SP_POSITION_MODE_FIXED, spSkeleton_setToSetupPose};
use triangle::lib32::{Point, Triangle};
use gifski;
use gifski::progress::NoProgress;
use imgref;
use imgref::{Img, ImgRef, ImgVec};
use rgb::FromSlice;

mod impls;

const AA_SAMPLES: u32 = 4;

pub struct DummyRenderer {
    textures: FnvHashMap<usize, String>
}

/*
impl DummyRenderer {
    fn new() -> DummyRenderer {
        DummyRenderer {
            textures: Default::default()
        }
    }
}

impl Renderer for DummyRenderer {
    type Texture = String;
    type Frame = ();

    fn build_texture(&self, image: &spine::image::DynamicImage) -> spine::Result<Self::Texture> {
        println!("building tex");

        Ok(format!("{}x{} {}b", image.width(), image.height(), image.as_bytes().len()))
    }

    fn add_texture(&mut self, id: usize, texture: Self::Texture) {
        println!("adding tex {}", id);
        self.textures.insert(id, texture);
    }

    fn get_texture(&self, id: &usize) -> Option<&Self::Texture> {
        println!("getting tex {}", id);
        self.textures.get(id)
    }

    fn render_mesh(&self, vertices: &[Vertex], texture: &Self::Texture, _frame: &mut Self::Frame) -> spine::Result<()> {
        println!("Rendering texture {} on {} vertices {:?}", texture, vertices.len(), vertices);
        // for triangle in vertices.chunks(3) {
        //     println!("   {:?}", triangle);
        // }
        Ok(())
    }
}
*/

trait AsPoints {
    fn position(&self) -> Point;
    fn texture_coords(&self) -> Point;
    fn texture_scaled(&self, width: u32, height: u32) -> Point;
}

impl AsPoints for Vertex {
    fn position(&self) -> Point {
        Point { x: self.in_position[0], y: self.in_position[1], z: 0.0 }
    }

    fn texture_coords(&self) -> Point {
        Point { x: self.in_texture_coords[0], y: self.in_texture_coords[1], z: 0.0 }
    }

    fn texture_scaled(&self, width: u32, height: u32) -> Point {
        Point {
            x: self.in_texture_coords[0] * width as f32,
            y: (-self.in_texture_coords[1]) * height as f32,
            z: 0.0
        }
    }
}

trait InterpolatePixels {
    fn linear_sample(&self, x: f32, y: f32) -> F32Rgba;
}

#[derive(Debug)]
struct F32Rgba {
    r: f32,
    g: f32,
    b: f32,
    a: f32
}

impl F32Rgba {
    fn new() -> F32Rgba {
        F32Rgba {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0
        }
    }
}

impl From<&Rgba<u8>> for F32Rgba {
    fn from(p: &Rgba<u8>) -> Self {
        F32Rgba {
            r: p.0[0] as f32,
            g: p.0[1] as f32,
            b: p.0[2] as f32,
            a: p.0[3] as f32
        }
    }
}

impl From<&F32Rgba> for Rgba<u8> {
    fn from(f: &F32Rgba) -> Self {
        Rgba([f.r.round() as u8, f.g.round() as u8, f.b.round() as u8, f.a.round() as u8])
    }
}

impl Mul<f32> for F32Rgba {
    type Output = F32Rgba;

    fn mul(self, rhs: f32) -> Self::Output {
        F32Rgba {
            r: self.r * rhs,
            g: self.g * rhs,
            b: self.b * rhs,
            a: self.a * rhs
        }
    }
}

impl Div<f32> for F32Rgba {
    type Output = F32Rgba;

    fn div(self, rhs: f32) -> Self::Output {
        F32Rgba {
            r: self.r / rhs,
            g: self.g / rhs,
            b: self.b / rhs,
            a: self.a / rhs
        }
    }
}

impl Add<F32Rgba> for F32Rgba {
    type Output = F32Rgba;

    fn add(self, rhs: F32Rgba) -> Self::Output {
        F32Rgba {
            r: self.r + rhs.r,
            g: self.g + rhs.g,
            b: self.b + rhs.b,
            a: self.a + rhs.a
        }
    }
}

impl InterpolatePixels for RgbaImage {
    fn linear_sample(&self, x: f32, y: f32) -> F32Rgba {
        let mut x0 = x.floor() as u32;
        let mut x1 = x.ceil() as u32;
        let mut y0 = y.floor() as u32;
        let mut y1 = y.ceil() as u32;

        if x1 == self.width() { x1 = x0; }
        if y1 == self.height() { y1 = y0; }

        let x0_proximity = 1.0 - (x - x.floor());
        let x1_proximity = 1.0 - x0_proximity;
        let y0_proximity = 1.0 - (y - y.floor());
        let y1_proximity = 1.0 - y0_proximity;

        if x0 >= self.width() || y0 >= self.height() {
            panic!("Point {}, {} outside image ({}x{})", x0, y0, self.width(), self.height())
        }

        let p00: F32Rgba = self.get_pixel(x0, y0).into();
        let p01: F32Rgba = self.get_pixel(x0, y1).into();
        let p10: F32Rgba = self.get_pixel(x1, y0).into();
        let p11: F32Rgba = self.get_pixel(x1, y1).into();
        let p00_10 = (p00 * x0_proximity) + (p10 * x1_proximity);
        let p01_11 = (p01 * x0_proximity) + (p11 * x1_proximity);
        let p = (p00_10 * y0_proximity) + (p01_11 * y1_proximity);
        p
    }
}

pub struct ImageRenderer {
    textures: FnvHashMap<usize, RgbaImage>
}

impl ImageRenderer {
    fn new() -> ImageRenderer {
        ImageRenderer {
            textures: Default::default()
        }
    }
}

fn has_pointish(t: &Triangle, pt: Point) -> bool {
    fn sign(a: &Point, b: &Point, c: &Point) -> f32 {
        ((a.x - c.x) * (b.y - c.y) - (b.x - c.x) * (a.y - c.y)) as f32
    }
    let d1 = sign(&pt, &t.a, &t.b);
    let d2 = sign(&pt, &t.b, &t.c);
    let d3 = sign(&pt, &t.c, &t.a);
    let has_neg = (d1 <= 0.0) || (d2 <= 0.0) || (d3 <= 0.0);
    let has_pos = (d1 >= 0.0) || (d2 >= 0.0) || (d3 >= 0.0);
    !(has_neg && has_pos)
}


/* void _AtlasPage_createTexture (AtlasPage* self, const char* path){
	Texture* texture = new Texture();
	if (!texture->loadFromFile(path)) return;

	if (self->magFilter == SP_ATLAS_LINEAR) texture->setSmooth(true);
	if (self->uWrap == SP_ATLAS_REPEAT && self->vWrap == SP_ATLAS_REPEAT) texture->setRepeated(true);

	self->rendererObject = texture;
	Vector2u size = texture->getSize();
	self->width = size.x;
	self->height = size.y;
}
*/

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


impl Renderer for ImageRenderer {
    type Texture = RgbaImage;
    type Frame = RgbaImage;

    fn build_texture(&self, texture: NonNull<spAtlasPage>) -> spine::Result<Self::Texture> {
        Ok(image.to_rgba8())
    }

    fn add_texture(&mut self, id: usize, texture: Self::Texture) {
        self.textures.insert(id, texture);
    }

    fn get_texture(&self, id: &usize) -> Option<&Self::Texture> {
        self.textures.get(id)
    }

    fn render_mesh(&self, vertices: &[Vertex], texture: &Self::Texture, frame: &mut Self::Frame) -> spine::Result<()> {
        println!("Rendering {} triangles", vertices.len() / 3);
        let tex_width = texture.width();
        let tex_height = texture.height();
        let frame_height = frame.height();
        for (a, b, c) in vertices.iter().tuples() {
            let position_tri = Triangle::new(a.position(), b.position(), c.position());
            if !(position_tri.a.x.is_normal()
                && position_tri.a.y.is_normal()
                && position_tri.b.x.is_normal()
                && position_tri.b.y.is_normal()
                && position_tri.c.x.is_normal()
                && position_tri.c.y.is_normal()) {
                panic!("Weird floats in vertices: {:?}", position_tri);
            }
            let texture_tri = Triangle::new(a.texture_scaled(tex_width, tex_height), b.texture_scaled(tex_width, tex_height), c.texture_scaled(tex_width, tex_height));

            /*
            let [ttl, tbr] = texture_tri.aabb();
            let mut tx = ttl.x;
            let mut ty = ttl.y;
            while ty < tbr.y {
                while tx < tbr.x {
                    tx += 1.0;
                    if texture_tri.has_point(Point { x: tx, y: ty, z: 0.0 }) {
                        t.put_pixel(tx as u32, ty as u32, Rgba([0, 255, 0, 255]));
                    }
                }
                ty += 1.0;
                tx = ttl.x;
            }
             */

            // "Axis-aligned bounding box"
            let [pos_top_left, pos_bottom_right] = position_tri.aabb();

            for y in pos_top_left.y.floor() as u32 ..= pos_bottom_right.y.ceil() as u32 {
                for x in pos_top_left.x.floor() as u32 ..= pos_bottom_right.x.ceil() as u32 {
                    let mut hits = 0;
                    for sub_x in 0..AA_SAMPLES {
                        for sub_y in 0..AA_SAMPLES {
                            let ss_x = x as f32 + (sub_x as f32 / (AA_SAMPLES as f32));
                            let ss_y = y as f32 + (sub_y as f32 / (AA_SAMPLES as f32));
                            let position_point = Point { x: ss_x, y: ss_y, z: 0.0 };
                            // dbg!(x, y, sub_x, sub_y, ss_x, ss_y);
                            if position_tri.has_point(position_point) { hits += 1 }
                            // if has_pointish(&position_tri, position_point) { hits += 1 }

                            // let position_bary = position_tri.cartesian_to_barycentric(&position_point);
                            // let texture_point = texture_tri.barycentric_to_cartesian(&position_bary);
                            // let tex = texture.linear_sample(texture_point.x, texture_point.y);
                            // tex_sample = tex_sample + (tex / 16.0);
                        }
                    }

                    let yy = frame_height - y;
                    if hits > 0 {
                        hits = 16;
                        let old_px = frame.get_pixel_mut(x, frame_height - y);
                        let position_point = Point { x: x as f32, y: y as f32, z: 0.0 };
                        let position_bary = position_tri.cartesian_to_barycentric(&position_point);
                        let texture_point = texture_tri.barycentric_to_cartesian(&position_bary);
                        let mut tex = texture.linear_sample(texture_point.x, texture_point.y);
                        // If alpha is essentially 0, skip drawing the pixel
                        if tex.a < 0.001 { continue }

                        tex.a *= hits as f32 / (AA_SAMPLES * AA_SAMPLES) as f32;
                        // let tex = texture.get_pixel(texture_point.x.round() as u32, texture_point.y.round() as u32);
                        old_px.blend(&(&tex).into());

                        // *old_px = (&tex).into();
                    }
                }
            }
        }
        Ok(())
    }
}

pub fn main() {
    let mut renderer = ImageRenderer::new();

    // let atlas = renderer.new_atlas("axie/axie.atlas").unwrap();
    // let mut skeleton_json = SkeletonJson::new(&atlas);
    // skeleton_json.set_scale(1.0);
    // let skeleton_data = SkeletonData::from_json_file("axie/axie.json", skeleton_json).unwrap();

    let atlas = renderer.new_atlas("/Users/mark/dev/cotlgif/cotl/Follower.atlas").unwrap();
    let mut skeleton_binary = SkeletonBinary::new(&atlas);
    skeleton_binary.set_scale(1.0);
    let skeleton_data = SkeletonData::from_binary_file("/Users/mark/dev/cotlgif/cotl/Follower.skel", skeleton_binary).unwrap();

    let animation_state_data = AnimationStateData::new(&skeleton_data);
    let mut skeleton = Skeleton::new(&skeleton_data);

    skeleton.set_skin_by_name("Dog");
    skeleton.f();

    let bounds = skeleton.get_bounds();
    skeleton.set_x((bounds.x_max - bounds.x_min) / 2.0);
    skeleton.set_y((bounds.y_max - bounds.y_min) / 2.0);

    let mut animation_state = AnimationState::new(&animation_state_data);
    animation_state.set_animation_by_name(0, "Dissenters/dissenter-listening", true).unwrap();

    let (gs_collector, gs_writer) = gifski::new(Default::default()).unwrap();

    let t = thread::spawn(move || {
        let f = File::create("gif.gif").unwrap();
        gs_writer.write(f, &mut (NoProgress {})).unwrap();
    });

    let mut pos = 0.0;
    let increment = 1.0 / 60.0;
    for i in 0..150 {
        println!("{}", i);
        let mut frame = RgbaImage::new(1600, 1200);
        animation_state.apply(&mut skeleton);
        skeleton.update_world_transform();
        renderer.render(&mut skeleton, &mut frame).unwrap();
        // frame.save(&format!("frame{i}.png")).unwrap();
        animation_state.update(increment as f32);

        let f = ImgVec::new(frame.as_rgba().into(), 1600, 1200);
        println!("add");
        gs_collector.add_frame_rgba(i, f, pos).unwrap();
        println!("added");
        pos += increment;
    }

    drop(gs_collector);
    println!("joining");
    t.join().unwrap();

    // for i in 0..40 {
    //     let mut frame = RgbaImage::new(1600, 1200);
    //     skeleton.update_world_transform();
    //     animation_state.apply(&mut skeleton);
    //     renderer.render(&mut skeleton, &mut frame).unwrap();
    //     frame.save(&format!("frame{i}.png")).unwrap();
    //     animation_state.update(0.1);
    //     break;
    // }
}

// pub fn main() {
//     let sdl_context = sdl2::init().unwrap();
//     let video_subsystem = sdl_context.video().unwrap();
//
//     let window = video_subsystem.window("rust-sdl2 demo", 800, 600)
//         .position_centered()
//         .build()
//         .unwrap();
//
//     let mut canvas = window.into_canvas().build().unwrap();
//     let mut renderer = SdlRenderer::new(&canvas);
//
//     let atlas = renderer.new_atlas("axie/axie.atlas").unwrap();
//     let mut skeleton_json = SkeletonJson::new(&atlas);
//     skeleton_json.set_scale(1.0);
//
//     let mut skeleton_data = SkeletonData::from_json_file("axie/axie.json", skeleton_json).unwrap();
//     let asd = AnimationStateData::new(&skeleton_data);
//     let mut skel = Skeleton::new(&skeleton_data);
//
//     let Bounds { y_min, y_max, .. } = skel.get_bounds();
//     skel.set_y((y_min - y_max) / 2.0);
//
//     let mut animation_state = AnimationState::new(&asd);
//     animation_state.set_animation_by_name(0, "action/idle", true).unwrap();
//
//     let mut event_pump = sdl_context.event_pump().unwrap();
//     'running: loop {
//         canvas.clear();
//
//         animation_state.update(0.1);
//         animation_state.apply(&mut skel);
//         skel.update_world_transform();
//
//         renderer.render(&mut skel, &mut canvas).unwrap();
//
//         for event in event_pump.poll_iter() {
//             match event {
//                 Event::Quit {..} |
//                 Event::KeyDown { keycode: Some(Keycode::Escape), .. } => {
//                     break 'running
//                 },
//                 _ => {}
//             }
//         }
//
//         canvas.present();
//         ::std::thread::sleep(Duration::new(0, 1_000_000_000u32 / 60));
//     }
// }
//
