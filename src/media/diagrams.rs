use std::path::Path;
use ab_glyph::{FontArc, PxScale};
use image::{Rgb, RgbImage};
use imageproc::drawing::{draw_filled_circle_mut, draw_filled_rect_mut, draw_hollow_rect_mut, draw_line_segment_mut, draw_text_mut};
use imageproc::rect::Rect;
use crate::{domain::id, AppError, Result};

pub fn font(bold: bool) -> Result<FontArc> {
    let candidates=if bold { &["C:/Windows/Fonts/msyhbd.ttc","C:/Windows/Fonts/simhei.ttf","C:/Windows/Fonts/msyh.ttc"][..] } else { &["C:/Windows/Fonts/msyh.ttc","C:/Windows/Fonts/simhei.ttf","C:/Windows/Fonts/msyhbd.ttc"][..] };
    for path in candidates {
        if let Ok(bytes)=std::fs::read(path) {
            if let Ok(font)=FontArc::try_from_vec(bytes) { return Ok(font); }
        }
    }
    Err(AppError::Tool("未找到可用中文字体，无法生成中文图示".into()))
}

pub fn text(image: &mut RgbImage, x: i32, y: i32, content: &str, size: f32, color: Rgb<u8>, bold: bool) -> Result<()> {
    let font=font(bold)?;
    draw_text_mut(image,color,x,y,PxScale::from(size),&font,content);
    Ok(())
}

pub fn flowchart(root: &Path, title: &str, steps: &[String]) -> Result<String> {
    if !(2..=8).contains(&steps.len()) { return Err(AppError::Tool("流程图需要 2–8 个步骤".into())); }
    let width=1200;
    let height=260+144*steps.len() as u32;
    let mut image=RgbImage::from_pixel(width,height,Rgb([245,242,234]));
    text(&mut image,64,45,&title.chars().take(100).collect::<String>(),38.0,Rgb([23,54,59]),true)?;
    for (index,step) in steps.iter().enumerate() {
        let top=145+index as i32*144;
        let fill=if index%2==0{Rgb([225,236,230])}else{Rgb([229,233,239])};
        imageproc::drawing::draw_filled_rect_mut(&mut image,Rect::at(130,top).of_size(940,92),fill);
        draw_hollow_rect_mut(&mut image,Rect::at(130,top).of_size(940,92),Rgb([119,146,140]));
        draw_filled_circle_mut(&mut image,(87,top+46),23,Rgb([203,96,64]));
        text(&mut image,79,top+28,&(index+1).to_string(),23.0,Rgb([255,255,255]),true)?;
        text(&mut image,170,top+28,&step.chars().take(55).collect::<String>(),28.0,Rgb([24,50,56]),false)?;
        if index<steps.len()-1 {
            draw_line_segment_mut(&mut image,(600.0,(top+94)as f32),(600.0,(top+134)as f32),Rgb([203,96,64]));
            draw_line_segment_mut(&mut image,(590.0,(top+120)as f32),(600.0,(top+136)as f32),Rgb([203,96,64]));
            draw_line_segment_mut(&mut image,(610.0,(top+120)as f32),(600.0,(top+136)as f32),Rgb([203,96,64]));
        }
    }
    let filename=format!("{}.png",id());
    let path=root.join("assets/diagrams").join(&filename);
    std::fs::create_dir_all(path.parent().unwrap())?;
    image.save(&path)?;
    Ok(format!("![{}](assets/diagrams/{})\n\n*自绘流程图：{}*",title,filename,steps.join(" → ")))
}

pub fn wireframe(root: &Path, title: &str, regions: &[serde_json::Value]) -> Result<String> {
    if !(2..=8).contains(&regions.len()) { return Err(AppError::Tool("线框图需要 2–8 个界面区域".into())); }
    let mut image=RgbImage::from_pixel(1400,960,Rgb([245,242,234]));
    text(&mut image,50,36,&title.chars().take(100).collect::<String>(),36.0,Rgb([23,54,59]),true)?;
    draw_hollow_rect_mut(&mut image,Rect::at(45,110).of_size(1310,795),Rgb([40,75,81]));
    let mut legend=vec![];
    for (index,region) in regions.iter().enumerate() {
        let number=|key:&str,default:f64|region.get(key).and_then(serde_json::Value::as_f64).unwrap_or(default);
        let (x,y,w,h)=(number("x",0.1),number("y",0.1),number("w",0.3),number("h",0.2));
        if !(0.0..=1.0).contains(&x)||!(0.0..=1.0).contains(&y)||w<=0.0||h<=0.0||x+w>1.0||y+h>1.0 { return Err(AppError::Tool("线框图区域必须位于画面内".into())); }
        let rect=Rect::at(75+(x*1250.0)as i32,145+(y*710.0)as i32).of_size((w*1250.0)as u32,(h*710.0)as u32);
        imageproc::drawing::draw_filled_rect_mut(&mut image,rect,Rgb([230,236,233]));
        draw_hollow_rect_mut(&mut image,rect,Rgb([103,137,131]));
        let name=region.get("name").and_then(serde_json::Value::as_str).unwrap_or("界面区域");
        text(&mut image,rect.left()+16,rect.top()+14,&format!("{}  {}",index+1,name.chars().take(38).collect::<String>()),25.0,Rgb([29,58,64]),true)?;
        legend.push(format!("{}. {}",index+1,region.get("interaction").and_then(serde_json::Value::as_str).unwrap_or(name).chars().take(160).collect::<String>()));
    }
    let filename=format!("{}.png",id());
    let path=root.join("assets/diagrams").join(&filename);
    std::fs::create_dir_all(path.parent().unwrap())?;
    image.save(&path)?;
    Ok(format!("![{}](assets/diagrams/{})\n\n*交互标注：{}*",title,filename,legend.join("；")))
}
