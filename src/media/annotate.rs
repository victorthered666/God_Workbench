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
    let mut caption=format!("![{}]({})\n\n*标注：{}*",md_text(title),relative,legend.iter().map(|item| md_text(item)).collect::<Vec<_>>().join("；"));
    caption.push_str(&source_links(source_url,source_page));
    Ok(caption)
}

/// 视觉标注不可用（模型故障/返回空标签）时的回退：直接嵌入未标注原图，
/// 而不是让整条 select_image 失败——此前一失败图片就永远进不了正文。
pub fn plain_markdown(path: &str, title: &str, source_url: &str, source_page: &str) -> String {
    format!("![{}]({})\n\n*网络参考图（未标注）*{}",md_text(title),path,source_links(source_url,source_page))
}

fn source_links(source_url: &str, source_page: &str) -> String {
    let mut tail=String::new();
    if !source_url.is_empty() {
        tail.push_str(&format!("\n\n原图：[检索来源]({})",source_url));
        if !source_page.is_empty() { tail.push_str(&format!(" · [来源网页]({})",source_page)); }
        tail.push_str("。可检索不代表取得使用授权。");
    }
    tail
}

// title/label 来自模型输出，需转义以免破坏图片链接或注入 Markdown 结构。
fn md_text(text: &str) -> String { text.chars().filter(|c|*c!='\n'&&*c!='\r').map(|c| if matches!(c,'['|']'|'('|')') { ' ' } else { c }).take(160).collect() }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plain_markdown_escapes_and_links_sources() {
        let md=plain_markdown("assets/images/a.jpg","怪猎[截图](测试)","https://cdn.example/x.jpg","https://game.example/page");
        assert!(md.starts_with("![怪猎 截图  测试 ](assets/images/a.jpg)"),"{md}");
        assert!(md.contains("*网络参考图（未标注）*"));
        assert!(md.contains("[检索来源](https://cdn.example/x.jpg)"));
        assert!(md.contains("[来源网页](https://game.example/page)"));
        assert!(md.contains("可检索不代表取得使用授权"));
    }
    #[test]
    fn plain_markdown_without_source_has_no_links() {
        let md=plain_markdown("assets/images/b.jpg","无来源图","","");
        assert!(!md.contains("检索来源"));
    }
}
