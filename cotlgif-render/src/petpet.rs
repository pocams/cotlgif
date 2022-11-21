use once_cell::sync::OnceCell;

use crate::spine::SpineActor;
use rusty_spine::{Skeleton, SkeletonController};

const PETPET_TIMESCALE: f32 = 3.0;
const PETPET_NATIVE_WIDTH: f32 = 100.0;
const PETPET_Y_OFFSET: f32 = 35.0;

pub fn get_petpet_frame(attachment_name: &str) -> Option<i32> {
    match attachment_name {
        "petpet0" => Some(0),
        "petpet1" => Some(1),
        "petpet2" => Some(2),
        "petpet3" => Some(3),
        "petpet4" => Some(4),
        _ => None,
    }
}

pub fn petpet_controller(target_width: f32, target_height: f32) -> SkeletonController {
    static PETPET_ACTOR: OnceCell<SpineActor> = OnceCell::new();
    let petpet_actor = PETPET_ACTOR
        .get_or_init(|| SpineActor::load("assets/petpet.atlas", "assets/petpet.skel").unwrap());

    let mut petpet_controller = petpet_actor.new_skeleton_controller();

    petpet_controller
        .skeleton
        .set_skin_by_name("default")
        .unwrap();

    petpet_controller
        .animation_state
        .set_animation_by_name(0, "petpet", true)
        .unwrap();
    petpet_controller
        .animation_state
        .set_timescale(PETPET_TIMESCALE);

    let petpet_scale = if target_width > target_height {
        (target_width / PETPET_NATIVE_WIDTH) * 0.9 * (target_height / target_width)
    } else {
        (target_width / PETPET_NATIVE_WIDTH) * 0.9
    };

    petpet_controller
        .skeleton
        .set_scale([petpet_scale, petpet_scale]);
    petpet_controller
        .skeleton
        .set_y(target_height - (PETPET_Y_OFFSET * petpet_scale));

    petpet_controller
}

pub(crate) fn apply_petpet_squish(
    target: &mut Skeleton,
    petpet_frame: i32,
    original_offset: (f32, f32),
    original_scale: f32,
) {
    /*
       https://benisland.neocities.org/petpet/main.js
       { x: 0, y: 0, w: 0, h: 0 },
       { x: -4, y: 12, w: 4, h: -12 },
       { x: -12, y: 18, w: 12, h: -18 },
       { x: -8, y: 12, w: 4, h: -12 },
       { x: -4, y: 0, w: 0, h: 0 },
    */
    let squish_factor = 0.7;
    let (scale, position): ((f32, f32), (f32, f32)) = match petpet_frame {
        0 => ((0.0, 0.0), (0.0, 0.0)),
        1 => ((0.3, -0.2), (4.0, 0.0)),
        2 => ((0.5, -0.3), (12.0, 0.0)),
        3 => ((0.4, -0.2), (4.0, 0.0)),
        4 => ((0.2, 0.0), (0.0, 0.0)),
        other => panic!("bad petpet frame {}", other),
    };

    target.set_scale([
        (1.0 + (scale.0 * squish_factor)) * original_scale,
        (1.0 + (scale.1 * squish_factor)) * original_scale,
    ]);
    target.set_position([
        original_offset.0 + position.0,
        original_offset.1 + position.1,
    ]);
}
