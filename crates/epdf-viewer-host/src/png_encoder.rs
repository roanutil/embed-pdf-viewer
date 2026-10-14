//! The asset engine's image encoder: `pages.renderEncoded` needs one, and
//! a stamp preview is a PNG. Straight alpha in, straight alpha out.

use epdf_engine::error::{EngineError, EngineErrorCode};
use epdf_engine::features::render::raster::PageRaster;
use epdf_engine::features::server::{EncodedImage, ImageEncoder, RenderEncode};

pub struct PngEncoder;

impl ImageEncoder for PngEncoder {
    fn encode(
        &self,
        raster: &PageRaster,
        encode: &RenderEncode,
    ) -> Result<EncodedImage, EngineError> {
        if !matches!(encode, RenderEncode::Png) {
            return Err(EngineError::with_message(
                EngineErrorCode::NotImplemented,
                "the native asset engine encodes png only",
            ));
        }
        let row = raster.width as usize * 4;
        let mut packed = Vec::with_capacity(row * raster.height as usize);
        for y in 0..raster.height as usize {
            let start = y * raster.stride as usize;
            let line = raster.rgba.get(start..start + row).ok_or_else(|| {
                failed(format!("row {y} is past the end of a {}-byte raster", raster.rgba.len()))
            })?;
            packed.extend_from_slice(line);
        }
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, raster.width, raster.height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().map_err(|e| failed(e.to_string()))?;
            writer.write_image_data(&packed).map_err(|e| failed(e.to_string()))?;
        }
        Ok(EncodedImage {
            content_type: "image/png".into(),
            width: raster.width,
            height: raster.height,
            bytes,
        })
    }
}

fn failed(detail: String) -> EngineError {
    EngineError::with_message(EngineErrorCode::Unknown, format!("png encoding failed: {detail}"))
}
