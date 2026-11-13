use std::path::Path;
use image::{DynamicImage, ImageReader, Rgb, RgbImage};
use imageproc::drawing::{draw_filled_circle_mut, draw_hollow_circle_mut};
use crate::{domain::{id, Annotation}, paths::contained_file, AppError, Result};
use super::diagrams::text;

pub fn annotate(root: &Path, path: &str, title: &str, labels: &[Annotation], source_url: &str, source_page: &str) -> Result<String> {
    if labels.is_empty() || labels.len()>8 { return Err(AppError::Tool("每张图片需要 1–8 个标注".into())); }
    let source=contained_file(root,path)?;
    let original=ImageReader::open(source)?.decode()?;
    let mut image=if original.width()*original.height()>12_000_000 || original.width().max(original.height())>3200 {
        original.resize(3200,3200,image::imageops::FilterType::Lanczos3).to_rgb8()
    } else { original.to_rgb8() };
    let (width,height)=image.dimensions();
    if u64::from(width)*u64::from(height)>12_000_000 { return Err(AppError::Tool("图片像素超出处理限制".into())); }
    let header=(height/8).max(70);
    let mut canvas=RgbImage::from_pixel(width,height+header,Rgb([244,241,233]));
    image::imageops::replace(&mut canvas,&image,0,header.into());
    text(&mut canvas,22,18,&title.chars().take(100).collect::<String>(),(width/36).max(22)as f32,Rgb([21,43,50]),true)?;
    let mut legend=vec![];
    for (index,label) in labels.iter().enumerate() {
        if !(0.0..=1.0).contains(&label.x)||!(0.0..=1.0).contains(&label.y) { return Err(AppError::Tool("标注坐标必须在 0–1 范围内".into())); }
        let (x,y)=((label.x*width as f64)as i32,(label.y*height as f64)as i32+header as i32);
        let radius=(width.min(height)/36).max(18)as i32;
        draw_filled_circle_mut(&mut canvas,(x,y),radius,Rgb([209,93,59]));
        draw_hollow_circle_mut(&mut canvas,(x,y),radius,Rgb([255,255,255]));
        text(&mut canvas,x-radius/3,y-radius/2,&(index+1).to_string(),radius as f32,Rgb([255,255,255]),true)?;
        legend.push(format!("{}. {}",index+1,label.text.chars().take(120).collect::<String>()));
    }
    let filename=format!("{}.png",id());
    let output=root.join("assets/images").join(&filename);
    canvas.save(output)?;
    let relative=format!("assets/images/{filename}");
    let mut caption=format!("![{}]({})\n\n*标注：{}*",title,relative,legend.join("；"));
    if !source_url.is_empty() {
        caption.push_str(&format!("\n\n原图：[检索来源]({})",source_url));
        if !source_page.is_empty() { caption.push_str(&format!(" · [来源网页]({})",source_page)); }
        caption.push_str("。可检索不代表取得使用授权。");
    }
    Ok(caption)
}
