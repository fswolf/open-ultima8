//! The view in cutscenes. Inferred from the scripts: the opening holds the camera (setCenterOn 0),
//! glides to the campfire, centres on Devon (NPC 2) while he talks, then glides back to the
//! Avatar's head (its x, y, z + 20) and follows it again (setCenterOn 1).
use super::Ctx;
use crate::usecode::Request;

/// Camera::scrollTo(x, y, z, speed): glide there; returns a process that ends on arrival.
pub fn scroll_to(c: &mut Ctx) -> u32 {
    let to = (c.arg2(0) as i32, c.arg2(2) as i32, c.arg1(4) as i32);
    let pid = c.vm.spawn_external(0, CAMERA);
    c.vm.requests.push(Request::CameraScroll { pid, to });
    pid as u32
}

/// Camera::moveTo(x, y, z, map): look there at once (ScummVM's CameraProcess::I_moveTo).
pub fn move_to(c: &mut Ctx) -> u32 {
    let to = (c.arg2(0) as i32, c.arg2(2) as i32, c.arg1(4) as i32);
    c.vm.requests.push(Request::CameraMove(to));
    0
}

/// Camera::setCenterOn(item): follow it; 0 = stay where it is.
pub fn set_center_on(c: &mut Ctx) -> u32 {
    let id = c.arg2(0);
    c.vm.requests.push(Request::CameraFollow((id != 0).then_some(id)));
    0
}

/// Process type of a camera glide.
const CAMERA: u16 = 0x208;
