use crate::{domain::Project, paths::contained_file, Result};

pub fn preview(project: &Project) -> Result<(String,String)> {
    let markdown=project.artifact("document").and_then(|v|v.get("markdown")).and_then(serde_json::Value::as_str).unwrap_or("");
    let mut options=comrak::Options::default();
    options.extension.table=true;
    options.extension.strikethrough=true;
    options.extension.autolink=true;
    options.extension.tasklist=true;
    let rendered=comrak::markdown_to_html(markdown,&options);
    let mut builder=ammonia::Builder::default();
    builder.tags(["p","br","h1","h2","h3","h4","ul","ol","li","strong","em","blockquote","code","pre","table","thead","tbody","tr","th","td","a","img","hr"].into_iter().collect());
    builder.generic_attributes(["align"].into_iter().collect());
    builder.tag_attributes(std::collections::HashMap::from([
        ("a",["href","target"].into_iter().collect()),
        ("img",["src","alt","loading"].into_iter().collect()),
        ("th",["colspan","rowspan"].into_iter().collect()),
        ("td",["colspan","rowspan"].into_iter().collect()),
    ]));
    builder.url_schemes(["http","https"].into_iter().collect());
    builder.link_rel(Some("noopener noreferrer"));
    let html=builder.clean(&rendered).to_string();
    let root=project.working_dir()?;
    let mut output=String::new();
    let mut rest=html.as_str();
    while let Some(start)=rest.find("<img ") {
        output.push_str(&rest[..start]);
        let end=rest[start..].find('>').map(|i|start+i+1).unwrap_or(rest.len());
        let tag=&rest[start..end];
        if let Some(src_start)=tag.find("src=\"") {
            let src=&tag[src_start+5..];
            if let Some(src_end)=src.find('"') {
                let path=&src[..src_end];
                if contained_file(&root,path).is_ok() {
                    output.push_str(&tag.replace(&format!("src=\"{path}\""),&format!("src=\"/api/projects/{}/asset?path={}\"",project.id,urlencoding(path))));
                }
            }
        }
        rest=&rest[end..];
    }
    output.push_str(rest);
    Ok((output,markdown.into()))
}

fn urlencoding(value: &str) -> String { percent_encoding::utf8_percent_encode(value,percent_encoding::NON_ALPHANUMERIC).to_string() }
