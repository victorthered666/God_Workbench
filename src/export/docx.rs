use std::{fs::File, io::{BufWriter, Read, Seek, Write}, path::Path};
use docx_rs::*;
use crate::{domain::Project, export::cover::calc_title_layout, paths::contained_file, AppError, Result};

/// 页面内容宽度（twips）：A4 11906 − 左右边距 1361×2。
const CONTENT_WIDTH: usize = 9184;

fn fonts() -> RunFonts { RunFonts::new().east_asia("Microsoft YaHei").ascii("Arial").hi_ansi("Arial") }

fn text(content: &str, size: usize, color: &str, bold: bool) -> Run {
    let run=Run::new().add_text(content).size(size).color(color).fonts(fonts());
    if bold { run.bold() } else { run }
}

fn paragraph(content: &str, size: usize, color: &str, bold: bool) -> Paragraph {
    Paragraph::new().add_run(text(content,size,color,bold)).line_spacing(LineSpacing::new().line(312).after(140))
}

fn heading(content: &str, level: usize) -> Paragraph {
    let size=match level {1=>32,2=>30,_=>28};
    Paragraph::new().style(&format!("Heading{level}")).add_run(text(content,size,"153C42",true)).line_spacing(LineSpacing::new().before(if level==1{360}else{240}).after(160))
}

/// 目录域按大纲级别收集条目：Heading1-3 样式必须真实存在并带 outline level，
/// 否则 TOC 域永远收集不到任何条目（此前 styles.xml 只有 Normal/ToC*）。
fn heading_style(id: &str, name: &str, level: usize, size: usize) -> Style {
    Style::new(id,StyleType::Paragraph).name(name).based_on("Normal").next("Normal").outline_lvl(level)
        .size(size).bold().color("153C42").fonts(fonts())
        .line_spacing(LineSpacing::new().before(if level==0{360}else{240}).after(160))
}

fn cell(content: &str, header: bool, width: usize) -> TableCell {
    // 单元格文本不经过 Markdown 解析：**加粗**/*强调* 的星号会原样露出，直接剥掉。
    // 乘号在本文档体系里一律用 ×，不存在需要保留的字面星号。
    let cleaned=content.replace('*',"");
    let paragraph=paragraph(&cleaned,17,if header{"FFFFFF"}else{"24373B"},header);
    let cell=TableCell::new().add_paragraph(paragraph).width(width,WidthType::Dxa);
    if header { cell.shading(Shading::new().fill("153C42").color("auto")) } else { cell }
}

/// 按各列最长内容的字符数加权分配列宽（下限 900），并写入 tblGrid。
/// 此前 tblGrid 为空、Fixed 布局下 Word 拿不到列宽，表格渲染成挤压重叠的乱码。
fn column_widths(headers: &[String], rows: &[Vec<String>]) -> Vec<usize> {
    let count=headers.len().max(1);
    let mut weights=vec![4usize;count];
    for (index,header) in headers.iter().enumerate().take(count) {
        weights[index]=weights[index].max(header.chars().count().min(40));
    }
    for row in rows {
        for (index,value) in row.iter().enumerate().take(count) {
            weights[index]=weights[index].max(value.chars().count().min(40));
        }
    }
    let total:usize=weights.iter().sum();
    let mut widths:Vec<usize>=weights.iter().map(|w|CONTENT_WIDTH*w/total.max(1)).collect();
    for width in widths.iter_mut() { if *width<900 { *width=900; } }
    let overflow:usize=widths.iter().sum::<usize>().saturating_sub(CONTENT_WIDTH);
    if overflow>0 {
        let widest=widths.iter().enumerate().max_by_key(|(_,w)|**w).map(|(i,_)|i).unwrap_or(0);
        widths[widest]=widths[widest].saturating_sub(overflow);
    }
    widths
}

fn content_table(headers: &[String], rows: &[Vec<String>]) -> Table {
    let widths=column_widths(headers,rows);
    let mut table_rows=vec![TableRow::new(headers.iter().enumerate().map(|(index,value)|cell(value,true,widths[index.min(widths.len())])).collect()).cant_split()];
    for row in rows {
        table_rows.push(TableRow::new(row.iter().enumerate().map(|(index,value)|cell(value,false,widths.get(index).copied().unwrap_or(900))).collect()).cant_split());
    }
    Table::new(table_rows).set_grid(widths).width(CONTENT_WIDTH,WidthType::Dxa).layout(TableLayoutType::Fixed)
}

fn cover_block(title: &str) -> Result<Table> {
    let (size,lines)=calc_title_layout(title,9906,40,26)?;
    let mut cell=TableCell::new().shading(Shading::new().fill("153C42").color("auto")).width(CONTENT_WIDTH,WidthType::Dxa);
    cell=cell.add_paragraph(Paragraph::new().add_run(text("G A M E  D E S I G N  D O C U M E N T",18,"C3D5C7",false)).line_spacing(LineSpacing::new().before(1200).after(500)));
    for line in lines {
        cell=cell.add_paragraph(Paragraph::new().add_run(text(&line,size*2,"FFFFFF",true)).line_spacing(LineSpacing::new().line((size*23)as i32).after(120)));
    }
    cell=cell.add_paragraph(paragraph("系统 · 规则 · 体验 · 制作",24,"D6E5DB",false));
    Ok(Table::without_borders(vec![TableRow::new(vec![cell]).row_height(7500.0).height_rule(HeightRule::Exact)]).set_grid(vec![CONTENT_WIDTH]).width(CONTENT_WIDTH,WidthType::Dxa).layout(TableLayoutType::Fixed))
}

/// 页码域必须拆成独立 run（begin/instr/separate/占位结果/end），
/// 全挤在一个 run 里是畸形 XML，Word/WPS 严格模式会把页脚渲染成乱码。
fn page_footer(title: &str) -> Footer {
    Footer::new().add_paragraph(
        Paragraph::new()
            .add_run(text(&format!("{title}  ·  第 "),16,"6E8080",false))
            .add_run(Run::new().size(16).color("6E8080").fonts(fonts()).add_field_char(FieldCharType::Begin,false))
            .add_run(Run::new().size(16).color("6E8080").fonts(fonts()).add_instr_text(InstrText::PAGE(InstrPAGE::new())))
            .add_run(Run::new().size(16).color("6E8080").fonts(fonts()).add_field_char(FieldCharType::Separate,false))
            .add_run(text("1",16,"6E8080",false))
            .add_run(Run::new().size(16).color("6E8080").fonts(fonts()).add_field_char(FieldCharType::End,false))
            .align(AlignmentType::Center)
    )
}

pub fn render(project: &Project, output: &Path) -> Result<()> {
    let title=crate::paths::safe_name(&project.name);
    let markdown=project.artifact("document").and_then(|v|v.get("markdown")).and_then(serde_json::Value::as_str).unwrap_or("");
    let mut document=Docx::new().page_margin(PageMargin::new().top(1247).bottom(1134).left(1361).right(1361)).footer(page_footer(&title));
    document=document
        .add_style(heading_style("Heading1","heading 1",0,32))
        .add_style(heading_style("Heading2","heading 2",1,30))
        .add_style(heading_style("Heading3","heading 3",2,28))
        .add_style(Style::new("ListParagraph",StyleType::Paragraph).name("List Paragraph").based_on("Normal").next("Normal").indent(Some(480),None,None,None));
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
        if line.starts_with('|') && index+1<lines.len() && is_table_separator(lines[index+1]) {
            let headers=split_table(line);
            index+=2;
            let mut rows=vec![];
            while index<lines.len() && lines[index].starts_with('|') {
                rows.push(split_table(lines[index]));
                index+=1;
            }
            document=document.add_table(content_table(&headers,&rows));
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
                        let height=(u64::from(width)*u64::from(dimensions.1)/u64::from(dimensions.0)).min(u64::from(u32::MAX))as u32;
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

fn split_table(line: &str) -> Vec<String> {
    // 先按未转义的 | 切分，再还原转义；直接 split 会把单元格内的 \| 当分隔符。
    let mut cells=Vec::new();
    let mut current=String::new();
    let mut escaped=false;
    for ch in line.trim_matches('|').chars() {
        if escaped {
            if ch=='|' { current.push('|'); } else { current.push('\\'); current.push(ch); }
            escaped=false;
        } else if ch=='\\' { escaped=true; }
        else if ch=='|' { cells.push(current.trim().to_string()); current.clear(); }
        else { current.push(ch); }
    }
    if escaped { current.push('\\'); }
    cells.push(current.trim().to_string());
    cells
}

/// 结构化识别表头分隔行：| --- | :---: | 等变体都接受。
fn is_table_separator(line: &str) -> bool {
    let cells=split_table(line);
    cells.len()>=2 && cells.iter().all(|cell| {
        let trimmed=cell.trim();
        let body=trimmed.trim_matches(':');
        !body.is_empty() && body.chars().all(|c| c=='-')
    })
}

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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn column_widths_weight_by_content_and_stay_within_page() {
        let headers=vec!["ID".to_string(),"名称".to_string(),"验收标准".to_string()];
        let rows=vec![vec!["F-001".to_string(),"主动技能槽扩容与技能联动系统".to_string(),"AC-4.1.1：6 槽全部解锁后，测试组中 ≥ 50% 的玩家在 10 场战斗中使用了 ≥ 4 种不同技能组合。".to_string()]];
        let widths=column_widths(&headers,&rows);
        assert_eq!(widths.len(),3);
        assert!(widths.iter().all(|w|*w>=900),"no column below floor: {widths:?}");
        assert!(widths.iter().sum::<usize>()<=CONTENT_WIDTH+2,"total within page: {} vs {CONTENT_WIDTH}",widths.iter().sum::<usize>());
        assert!(widths[2]>widths[0],"longest column widest: {widths:?}");
    }
    #[test]
    fn column_widths_single_column_takes_full_width() {
        let widths=column_widths(&["内容".to_string()],&[vec!["很长的中文内容".to_string()]]);
        assert_eq!(widths,vec![CONTENT_WIDTH]);
    }
}
