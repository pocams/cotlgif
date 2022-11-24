use std::ops::Deref;
use std::process::abort;
use std::sync::Arc;

use once_cell::sync::OnceCell;

use rusty_spine::BlendMode as SpineBlendMode;
use rusty_spine::{
    AnimationStateData, Atlas, SkeletonBinary, SkeletonController, SkeletonData, SkeletonJson,
};
use sfml::graphics::{
    FloatRect, IntRect, PrimitiveType, RenderStates, RenderTarget, RenderTexture, Texture,
    Transform, Vertex,
};

use sfml::system::Vector2f;
use sfml::SfBox;
use tracing::{debug, info, warn};

use crate::data::{spine_to_sfml, LoadError, BLEND_ADDITIVE, BLEND_MULTIPLY, BLEND_NORMAL, BLEND_SCREEN, RenderMetadata, common_to_sfml, common_to_spine};
use crate::{Frame, FrameHandler, HandleFrameError, RenderError};
use cotlgif_common::{SpineSkin, SpineAnimation, RenderRequest};

use crate::petpet::{apply_petpet_squish, get_petpet_frame, petpet_controller};

fn spine_init() {
    static SPINE_STATE: OnceCell<()> = OnceCell::new();
    SPINE_STATE.get_or_init(|| {
        // We can't panic and unwind through C code, so just abort if we run into errors
        // in the callbacks

        rusty_spine::extension::set_create_texture_cb(|atlas_page, path| {
            info!("Loading texture from {path}");

            let mut tex = Texture::new().unwrap_or_else(|| {
                eprintln!("Error creating new texture");
                abort();
            });

            tex.load_from_file(path, IntRect::new(0, 0, 0, 0))
                .unwrap_or_else(|e| {
                    eprintln!("Error loading texture from {path}: {e:?}");
                    abort();
                });

            tex.set_smooth(true);

            atlas_page.renderer_object().set(tex);
        });

        rusty_spine::extension::set_dispose_texture_cb(|atlas_page| unsafe {
            atlas_page.renderer_object().dispose::<Texture>();
        });
    });
}

#[derive(Debug)]
pub struct SpineActor {
    pub skins: Vec<SpineSkin>,
    pub animations: Vec<SpineAnimation>,
    skeleton_data: Arc<SkeletonData>,
    animation_state_data: Arc<AnimationStateData>,
    #[allow(dead_code)]
    atlas: Arc<Atlas>,
}

impl SpineActor {
    pub fn load(atlas_path: &str, skeleton_path: &str) -> Result<SpineActor, LoadError> {
        spine_init();

        let atlas = Arc::new(
            Atlas::new_from_file(atlas_path)
                .map_err(|e| LoadError::AtlasLoadError(e.to_string()))?,
        );

        let skeleton_data = Arc::new(if skeleton_path.ends_with(".json") {
            let skeleton_json = SkeletonJson::new(atlas.clone());
            skeleton_json
                .read_skeleton_data_file(&skeleton_path)
                .map_err(|e| LoadError::SkeletonLoadError(e.to_string()))?
        } else {
            let skeleton_binary = SkeletonBinary::new(atlas.clone());
            skeleton_binary
                .read_skeleton_data_file(&skeleton_path)
                .map_err(|e| LoadError::SkeletonLoadError(e.to_string()))?
        });

        let animation_state_data = Arc::new(AnimationStateData::new(skeleton_data.clone()));

        let skins = skeleton_data
            .skins()
            .map(|s| SpineSkin {
                name: s.name().to_owned(),
            })
            .collect();

        let animations = skeleton_data
            .animations()
            .map(|a| SpineAnimation {
                name: a.name().to_owned(),
                duration: a.duration(),
            })
            .collect();

        Ok(SpineActor {
            skins,
            animations,
            atlas,
            skeleton_data,
            animation_state_data,
        })
    }

    pub fn new_skeleton_controller(&self) -> SkeletonController {
        SkeletonController::new(
            self.skeleton_data.clone(),
            self.animation_state_data.clone(),
        )
    }
}

fn get_bounding_box(
    skeleton_controller: &mut SkeletonController,
    frame_count: u32,
    frame_delay: f32,
) -> FloatRect {
    // Run through the requested number of frames and grab all the min/max X and Y of the vertices.
    // This is actually pretty fast (tens of ms) compared to all the other crazy shit we're doing

    let mut min_x = f32::MAX;
    let mut max_x = f32::MIN;
    let mut min_y = f32::MAX;
    let mut max_y = f32::MIN;

    for _ in 0..frame_count {
        for r in skeleton_controller.renderables().iter() {
            if r.color.a < 0.001 {
                continue;
            };
            for [x, y] in &r.vertices {
                min_x = min_x.min(*x);
                min_y = min_y.min(*y);
                max_x = max_x.max(*x);
                max_y = max_y.max(*y);
            }
        }
        skeleton_controller.update(frame_delay);
    }

    FloatRect::new(min_x, min_y, max_x - min_x, max_y - min_y)
}

pub fn render(actor: &SpineActor, mut request: RenderRequest, mut frame_handler: Box<dyn FrameHandler>) -> Result<(), RenderError> {
    let mut controller = actor.new_skeleton_controller();

    // Keep the custom skin around until the end of the function if we create one
    let _custom_skin = if request.skins.len() == 1 {
        // Just one skin, we can just use set_skin_by_name()
        controller
            .skeleton
            .set_skin_by_name(request.skins[0].as_str())
            .map_err(|_| RenderError::SkinNotFound(request.skins[0].to_owned()))?;
        None
    } else {
        // Multiple skins stacked up - we'll have to build a custom skin
        let mut skin = rusty_spine::Skin::new("custom");
        for skin_name in &request.skins {
            let data = controller.skeleton.data();
            let skeleton_skin = data
                .skins()
                .find(|s| s.name() == skin_name)
                .ok_or_else(|| RenderError::SkinNotFound(skin_name.to_owned()))?;
            skin.add_skin(skeleton_skin.as_ref());
        }
        controller.skeleton.set_skin(&skin);
        Some(skin)
    };

    controller
        .skeleton
        .set_scale([request.scale, request.scale]);

    // If there are slots we shouldn't draw, make them transparent (set alpha=0)
    for mut slot in controller.skeleton.slots_mut() {
        let slot_data = slot.data();
        let slot_name = slot_data.name();
        if !request.should_draw_slot(slot_name) {
            slot.color_mut().set_a(0.0);
        } else if let Some(color) = request.slot_colours.get(slot_name) {
            *slot.color_mut() = common_to_spine(color);
        }
    }

    controller.animation_state.clear_tracks();
    controller
        .animation_state
        .set_animation_by_name(0, &request.animation, true)
        .map_err(|_| RenderError::AnimationNotFound(request.animation.to_owned()))?;
    controller.update(request.start_time);

    let bounding_box = get_bounding_box(
        &mut controller,
        request.frame_count(),
        request.frame_delay(),
    );
    debug!("Bounding box: {:?}", bounding_box);

    // Bail out early if we didn't render anything at all
    if bounding_box.width < 1.0 || bounding_box.height < 1.0 {
        return Err(RenderError::NothingRendered);
    };

    let x_offset = -bounding_box.left;
    let y_offset = -bounding_box.top;
    let target_width = bounding_box.width.ceil() as u32;
    let target_height = bounding_box.height.ceil() as u32;

    // Move the skeleton into the center of the bounding box
    controller.skeleton.set_x(x_offset);
    controller.skeleton.set_y(y_offset);

    // Reset the animation after we rendered it to calculate the bounding box
    controller.animation_state.clear_tracks();
    controller
        .animation_state
        .set_animation_by_name(0, &request.animation, true)
        .map_err(|_| RenderError::AnimationNotFound(request.animation.to_owned()))?;
    controller.update(request.start_time);

    let mut petpet_controller = if request.petpet {
        Some(petpet_controller(bounding_box.width, bounding_box.height))
    } else {
        None
    };

    let mut target = RenderTexture::new(target_width, target_height)
        .ok_or_else(|| RenderError::TextureFailed)?;

    let mut render_states = RenderStates::new(BLEND_NORMAL, Transform::IDENTITY, None, None);

    let background_color = common_to_sfml(&request.background_colour);
    let mut time = request.start_time;
    let mut elapsed_time = 0.0;
    let mut frame = 0;
    let mut vertex_buffer = Vec::with_capacity(256);

    frame_handler.set_metadata(RenderMetadata {
        frame_count: request.frame_count(),
        frame_width: target_width as usize,
        frame_height: target_height as usize
    });

    while frame < request.frame_count() {
        target.clear(background_color);

        if let Some(pc) = petpet_controller.as_ref() {
            let petpet_frame = get_petpet_frame(
                pc.skeleton
                    .slot_at_index(0)
                    .unwrap()
                    .attachment()
                    .unwrap()
                    .name(),
            )
            .unwrap();
            apply_petpet_squish(
                &mut controller.skeleton,
                petpet_frame,
                (x_offset, y_offset),
                request.scale,
            );
        }

        for render_controller in [Some(&mut controller), petpet_controller.as_mut()] {
            if let Some(rc) = render_controller {
                let renderables = rc.renderables();
                for renderable in renderables.iter() {
                    if renderable.color.a < 0.001 {
                        continue;
                    };

                    let color = spine_to_sfml(&renderable.color);

                    let texture = unsafe {
                        &*(renderable.attachment_renderer_object.unwrap() as *const SfBox<Texture>)
                    };
                    let texture_size = texture.size();
                    render_states.set_texture(Some(texture));

                    render_states.blend_mode = match renderable.blend_mode {
                        SpineBlendMode::Normal => BLEND_NORMAL,
                        SpineBlendMode::Additive => BLEND_ADDITIVE,
                        SpineBlendMode::Multiply => BLEND_MULTIPLY,
                        SpineBlendMode::Screen => BLEND_SCREEN,
                    };

                    vertex_buffer.clear();

                    for i in &renderable.indices {
                        let vertex = renderable.vertices[*i as usize];
                        let uv_raw = renderable.uvs[*i as usize];
                        let uv = [
                            uv_raw[0] * texture_size.x as f32,
                            uv_raw[1] * texture_size.y as f32,
                        ];
                        vertex_buffer.push(Vertex::new(
                            Vector2f::new(vertex[0], vertex[1]),
                            color,
                            Vector2f::new(uv[0], uv[1]),
                        ));
                    }

                    target.draw_primitives(
                        vertex_buffer.as_slice(),
                        PrimitiveType::TRIANGLES,
                        &render_states,
                    );
                }

                rc.update(request.frame_delay());
            }
        }

        // Sucks a bit to have to copy the image twice, but sfml Image doesn't have a way to
        // give us ownership of the pixel data.
        let pixel_data = target
            .texture()
            .copy_to_image()
            .unwrap()
            .pixel_data()
            .to_vec();

        let f = Frame {
            frame_number: frame,
            pixel_data,
            width: target_width,
            height: target_height,
            timestamp: elapsed_time,
        };

        match frame_handler.handle_frame(f) {
            Ok(_) => {}
            Err(HandleFrameError::TemporaryError) => {
                warn!("Frame callback returned a temporary error")
            }
            Err(HandleFrameError::PermanentError) => {
                warn!("Frame callback failed, aborting render");
                break;
            }
        }

        frame += 1;
        time += request.frame_delay();
        elapsed_time += request.frame_delay() as f64;
    }

    info!("Finished rendering");
    Ok(())
}
