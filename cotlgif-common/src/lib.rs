pub struct Frame {
    pub frame_number: u32,
    pub pixel_data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub timestamp: f64,
}
