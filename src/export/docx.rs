use std::{fs::File, io::{BufWriter, Read, Seek, Write}, path::Path};
use docx_rs::*;
use crate::{domain::Project, export::cover::calc_title_layout, paths::contained_file, AppError, Result};

fn text(content: &str, size: usize, color: &str, bold: bool) -> Run {
    let run=Run::new().add_text(content).size(size).color(color).fonts(RunFonts::new().east_asia("Microsoft YaHei").ascii("Arial").hi_ansi("Arial"));
    if bold { run.bold() } else { run }
}

fn paragraph(content: &str, size: usize, color: &str, bold: bool) -> Paragraph {
    Paragraph::new().add_run(text(content,size,color,bold)).line_spacing(LineSpacing::new().line(312).after(140))
}

fn heading(content: &str, level: usize) -> Paragraph {
    let size=match level {1=>32,2=>30,_=>28};
    Paragraph::new().style(&format!("Heading{level}")).add_run(text(content,size,"153C42",true)).line_spacing(LineSpacing::new().before(if level==1{360}else{240}).after(160))
}

fn cell(content: &str, header: bool) -> TableCell {
    let paragraph=paragraph(content,17,if header{"FFFFFF"}else{"24373B"},header);
    let cell=TableCell::new().add_paragraph(paragraph).width(1600,WidthType::Dxa);
    if header { cell.shading(Shading::new().fill("153C42").color("auto")) } else { cell }
}

fn cover_block(title: &str) -> Result<Table> {
    let (size,lines)=calc_title_layout(title,9906,40,26)?;
    let mut cell=TableCell::new().shading(Shading::new().fill("153C42").color("auto"));
    cell=cell.add_paragraph(Paragraph::new().add_run(text("G A M E  D E S I G N  D O C U M E N T",18,"C3D5C7",false)).line_spacing(LineSpacing::new().before(1200).after(500)));
    for line in lines {
        cell=cell.add_paragraph(Paragraph::new().add_run(text(&line,size*2,"FFFFFF",true)).line_spacing(LineSpacing::new().line((size*23)as i32).after(120)));
    }
    cell=cell.add_paragraph(paragraph("系统 · 规则 · 体验 · 制作",24,"D6E5DB",false));
    Ok(Table::without_borders(vec![TableRow::new(vec![cell]).row_height(7500.0).height_rule(HeightRule::Exact)]).layout(TableLayoutType::Fixed))
}

pub fn render(project: &Project, output: &Path) -> Result<()> {
    let title=crate::paths::safe_name(&project.name);
    let markdown=project.artifact("document").and_then(|v|v.get("markdown")).and_then(serde_json::Value::as_str).unwrap_or("");
    let footer=Footer::new().add_paragraph(Paragraph::new().add_run(text(&format!("{title}  ·  第 "),16,"6E8080",false)).add_run(Run::new().add_field_char(FieldCharType::Begin,false).add_instr_text(InstrText::PAGE(InstrPAGE::new())).add_field_char(FieldCharType::End,false)).align(AlignmentType::Center));
    let mut document=Docx::new().page_margin(PageMargin::new().top(1247).bottom(1134).left(1361).right(1361)).footer(footer);
    document=document.add_table(cover_block(&title)?);
    document=document.add_paragraph(paragraph("游戏策划工作文档",28,"BF593B",true));
    document=document.add_paragraph(paragraph(&format!("版本 1.0 | 生成日期 {}",chrono::Utc::now().format("%Y-%m-%d")),20,"667A79",false));
    document=document.add_paragraph(paragraph("参考图仅用于设计研究，不代表取得再发布授权。",18,"667A79",false));
    document=document.add_paragraph(paragraph("目录",48,"153C42",true).page_break_before(true));
    document=document.add_table_of_contents(TableOfContents::with_instr_text("TOC \\o \"1-3\" \\h \\z \\u").heading_styles_range(1,3));
    document=document.add_paragraph(paragraph("打开 Word / WPS 后，请右键目录选择“更新域”以刷新页码。",18,"6E8080",false));
    document=document.add_paragraph(paragraph("正文",18,"6E8080",false).page_break_before(true));
    let root=project.working_dir()?;
    let mut figure=0;
    let mut lines=markdown.replace("\r\n","\n");
    let lines:Vec<&str>=lines.lines().map(str::trim).collect();
    let mut index=0;
    while index<lines.len() {
        let line=lines[index];
        if line.is_empty() { index+=1; continue; }
        if line.starts_with('|') && index+1<lines.len() && lines[index+1].starts_with("| ---") {
            let headers=split_table(line);
            index+=2;
            let mut rows=vec![TableRow::new(headers.iter().map(|value|cell(value,true)).collect()).cant_split()];
            while index<lines.len() && lines[index].starts_with('|') {
                rows.push(TableRow::new(split_table(lines[index]).iter().map(|value|cell(value,false)).collect()).cant_split());
                index+=1;
            }
            document=document.add_table(Table::new(rows).layout(TableLayoutType::Fixed));
            document=document.add_paragraph(Paragraph::new());
            continue;
        }
        if line.starts_with("![") {
            if let Some(end)=line.find("](") {
                let alt=&line[2..end];
                let path=line[end+2..].trim_end_matches(')');
                match contained_file(&root,path) {
                    Ok(path)=> {
                        let bytes=std::fs::read(&path)?;
                        let dimensions=image::image_dimensions(&path)?;
                        let width=5_800_000u32;
                        let height=(u64::from(width)*u64::from(dimensions.1)/u64::from(dimensions.0))as u32;
                        figure+=1;
                        document=document.add_paragraph(Paragraph::new().add_run(Run::new().add_image(Pic::new(&bytes).size(width,height))).align(AlignmentType::Center));
                        document=document.add_paragraph(paragraph(&format!("图 {figure}  {alt}"),18,"5F7474",false).align(AlignmentType::Center));
                    },
                    Err(_)=>document=document.add_paragraph(paragraph(&format!("[缺失图片：{path}]"),20,"A34430",false)),
                }
            }
            index+=1;
            continue;
        }
        if let Some(content)=line.strip_prefix("### ") { document=document.add_paragraph(heading(content,3)); }
        else if let Some(content)=line.strip_prefix("## ") { document=document.add_paragraph(heading(content,2)); }
        else if let Some(content)=line.strip_prefix("# ") { document=document.add_paragraph(heading(content,1)); }
        else if let Some(content)=line.strip_prefix("> ") { document=document.add_paragraph(paragraph(content,18,"58716C",false)); }
        else if let Some(content)=line.strip_prefix("- ") { document=document.add_paragraph(paragraph(content,24,"24373B",false).style("ListParagraph")); }
        else { document=document.add_paragraph(paragraph(line,24,"000000",false).indent(None,Some(SpecialIndentType::FirstLine(480)),None,None)); }
        index+=1;
    }
    let file=File::create(output)?;
    document.pack(BufWriter::new(file)).map_err(|error|AppError::Other(format!("DOCX 生成失败：{error}")))?;
    enable_field_updates(output)
}

fn split_table(line: &str) -> Vec<String> { line.trim_matches('|').split('|').map(|cell|cell.trim().replace("\\|","|")).collect() }

fn enable_field_updates(path: &Path) -> Result<()> {
    let input=File::open(path)?;
    let mut archive=zip::ZipArchive::new(input).map_err(|e|AppError::Other(format!("DOCX 读取失败：{e}")))?;
    let temp=path.with_extension("tmp.docx");
    let output=File::create(&temp)?;
    let mut writer=zip::ZipWriter::new(BufWriter::new(output));
    let options=zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for index in 0..archive.len() {
        let mut file=archive.by_index(index).map_err(|e|AppError::Other(e.to_string()))?;
        let name=file.name().to_string();
        writer.start_file(&name,options).map_err(|e|AppError::Other(e.to_string()))?;
        if name=="word/settings.xml" {
            let mut xml=String::new(); file.read_to_string(&mut xml)?;
            if !xml.contains("updateFields") { xml=xml.replace("</w:settings>","<w:updateFields w:val=\"true\"/></w:settings>"); }
            writer.write_all(xml.as_bytes())?;
        } else { std::io::copy(&mut file,&mut writer)?; }
    }
    writer.finish().map_err(|e|AppError::Other(e.to_string()))?;
    std::fs::rename(temp,path)?;
    Ok(())
}
