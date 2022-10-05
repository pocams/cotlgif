use fnv::FnvHashMap;
use image::{DynamicImage, GenericImageView, Pixel, RgbaImage};
use itertools::Itertools;
use spine::animation::{AnimationState, AnimationStateData};
use spine::geometry::Vertex;
use spine::render::Renderer;
use spine::skeleton::{Skeleton, SkeletonData, SkeletonJson};
use triangle::lib32::{Point, Triangle};

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
            y: -self.in_texture_coords[1] * height as f32,
            z: 0.0
        }
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

impl Renderer for ImageRenderer {
    type Texture = RgbaImage;
    type Frame = RgbaImage;

    fn build_texture(&self, image: &DynamicImage) -> spine::Result<Self::Texture> {
        Ok(image.to_rgba8())
    }

    fn add_texture(&mut self, id: usize, texture: Self::Texture) {
        self.textures.insert(id, texture);
    }

    fn get_texture(&self, id: &usize) -> Option<&Self::Texture> {
        self.textures.get(id)
    }

    fn render_mesh(&self, vertices: &[Vertex], texture: &Self::Texture, frame: &mut Self::Frame) -> spine::Result<()> {
        // println!("Rendering texture {:?} on {} vertices {:?}", texture, vertices.len(), vertices);
        // println!("Rendering texture on {} vertices {:?}", vertices.len(), vertices);

        let tex_width = texture.width();
        let tex_height = texture.height();
        let frame_height = frame.height();
        for (a, b, c) in vertices.iter().tuples() {
            let position_tri = Triangle::new(a.position(), b.position(), c.position());
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
            let mut x = pos_top_left.x;
            let mut y = pos_top_left.y;
            while y <= pos_bottom_right.y {
                while x <= pos_bottom_right.x {
                    let position_point = Point { x, y, z: 0.0 };
                    if position_tri.has_point(position_point) {
                        // This point is inside the destination triangle, so copy it from the texture
                        let position_bary = position_tri.cartesian_to_barycentric(&position_point);
                        let texture_point = texture_tri.barycentric_to_cartesian(&position_bary);
                        // No interpolation at all right now
                        let px = texture.get_pixel(texture_point.x as u32, texture_point.y as u32);

                        // Invert the y, because texture space has inverted y coordinate
                        let y = frame_height - position_point.y as u32;
                        let old_px = frame.get_pixel_mut(position_point.x as u32, y);
                        old_px.blend(px);
                        // println!("{:?} (from {:?}) at {:?} => {:?} => {:?}", px, texture_point, position_point, position_bary, texture_point);

                        // t.put_pixel(texture_point.x as u32, texture_point.y as u32, Rgba([255, 0, 0, 255]));
                    }
                    x += 1.0;
                }
                y += 1.0;
                x = pos_top_left.x;
            }
        }
        Ok(())
    }
}

pub fn main() {
    let mut renderer = ImageRenderer::new();
    let atlas = renderer.new_atlas("axie/axie.atlas").unwrap();

    let mut skeleton_json = SkeletonJson::new(&atlas);
    skeleton_json.set_scale(1.0);
    let skeleton_data = SkeletonData::from_json_file("axie/axie.json", skeleton_json).unwrap();
    let animation_state_data = AnimationStateData::new(&skeleton_data);
    let mut skeleton = Skeleton::new(&skeleton_data);

    let bounds = skeleton.get_bounds();
    skeleton.set_x((bounds.x_max - bounds.x_min) / 2.0);
    skeleton.set_y((bounds.y_max - bounds.y_min) / 2.0);

    let mut animation_state = AnimationState::new(&animation_state_data);
    animation_state.set_animation_by_name(0, "action/idle", true).unwrap();

    for i in 0..40 {
        let mut frame = RgbaImage::new(1600, 1200);
        skeleton.update_world_transform();
        animation_state.apply(&mut skeleton);
        renderer.render(&mut skeleton, &mut frame).unwrap();
        frame.save(&format!("frame{i}.png")).unwrap();
        animation_state.update(0.1);
    }
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
