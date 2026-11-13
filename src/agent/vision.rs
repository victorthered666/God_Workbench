use std::{io::Cursor, path::Path};
use base64::{engine::general_purpose::STANDARD, Engine};
use image::{codecs::jpeg::JpegEncoder, DynamicImage, GenericImageView, ImageReader};
use serde_json::{json, Value};
use crate::{config::MAX_VISION_BYTES, paths::contained_file};

pub fn image_blocks(root: &Path, paths: &[String]) -> Vec<Value> {
    let mut blocks=vec![];
    let mut total=0usize;
    for relative in paths.iter().take(6) {
        let Ok(path)=contained_file(root,relative) else { continue; };
        let Ok(mut reader)=ImageReader::open(path) else { continue; };
        let mut limits=image::Limits::default(); limits.max_image_width=Some(4096); limits.max_image_height=Some(4096); limits.max_alloc=Some(128*1024*1024);
        reader.limits(limits);
        let Ok(source)=reader.decode() else { continue; };
        if u64::from(source.width())*u64::from(source.height())>12_000_000 { continue; }
        let mut image=source.resize(1920,1920,image::imageops::FilterType::Lanczos3).to_rgb8();
        let mut quality=85;
        let encoded=loop {
            let mut buffer=Cursor::new(Vec::new());
            if JpegEncoder::new_with_quality(&mut buffer,quality).encode_image(&DynamicImage::ImageRgb8(image.clone())).is_err() { break None; }
            let bytes=buffer.into_inner();
            if bytes.len()<=2*1024*1024 { break Some(bytes); }
            if image.width()>640 && image.height()>640 {
                image=image::imageops::resize(&image,image.width()*3/4,image.height()*3/4,image::imageops::FilterType::Lanczos3);
            } else if quality>55 { quality-=10; } else { break None; }
        };
        let Some(encoded)=encoded else { continue; };
        if total+encoded.len()>MAX_VISION_BYTES { break; }
        total+=encoded.len();
        blocks.push(json!({"type":"image_url","image_url":{"url":format!("data:image/jpeg;base64,{}",STANDARD.encode(encoded))}}));
    }
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn caps_image_count_and_payload() {
        let dir=tempfile::tempdir().unwrap();
        let image=image::RgbImage::from_pixel(2400,1800,image::Rgb([100,140,130]));
        image.save(dir.path().join("large.png")).unwrap();
        let blocks=image_blocks(dir.path(),&vec!["large.png".into();20]);
        assert!(blocks.len()<=6);
        assert!(serde_json::to_string(&blocks).unwrap().len()<12_000_000);
    }
}
