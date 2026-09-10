/// BGRA pixels, `stride * height` bytes, `stride == width * 4`.
///
/// The owning half of the buffer contract: the allocation is Rust's, its
/// length was checked against the layout before it was made, and it outlives
/// the `FPDF_BITMAP` that wrote into it. Host adapters hand it onward under
/// `epdf_engine::buffer`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bitmap {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub bgra: Vec<u8>,
}
