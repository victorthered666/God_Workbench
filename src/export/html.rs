use crate::{domain::Project, Result};

/// 独立 HTML 版策划文档：与 assets/ 相对路径同目录发布，双击即可离线阅读。
/// 渲染管线与预览一致（comrak + ammonia 允许列表），差异仅在图片保留相对路径。
pub fn render(project: &Project, title: &str, markdown: &str) -> Result<String> {
    let mut options=comrak::Options::default();
    options.extension.table=true;
    options.extension.strikethrough=true;
    options.extension.autolink=true;
    options.extension.tasklist=true;
    let rendered=comrak::markdown_to_html(markdown,&options);
    let mut builder=ammonia::Builder::default();
    builder.tags(["p","br","h1","h2","h3","h4","ul","ol","li","strong","em","blockquote","code","pre","table","thead","tbody","tr","th","td","a","img","hr","figure","figcaption"].into_iter().collect());
    builder.generic_attributes(["align"].into_iter().collect());
    builder.tag_attributes(std::collections::HashMap::from([
        ("a",["href","target"].into_iter().collect()),
        ("img",["src","alt","loading"].into_iter().collect()),
        ("th",["colspan","rowspan"].into_iter().collect()),
        ("td",["colspan","rowspan"].into_iter().collect()),
    ]));
    builder.url_schemes(["http","https"].into_iter().collect());
    builder.link_rel(Some("noopener noreferrer"));
    let body=builder.clean(&rendered).to_string();
    let generated=chrono::Utc::now().format("%Y-%m-%d");
    Ok(format!(r#"<!doctype html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<style>
:root {{ --ink:#24373B; --deep:#153C42; --accent:#BF593B; --muted:#6E8080; --paper:#FBFAF6; --line:#DCE5DF; }}
* {{ box-sizing:border-box; }}
body {{ margin:0; background:var(--paper); color:var(--ink);
  font-family:"Microsoft YaHei","PingFang SC","Noto Sans CJK SC",sans-serif; line-height:1.75; }}
.cover {{ background:var(--deep); color:#fff; padding:72px 48px 56px; }}
.cover .kicker {{ letter-spacing:.4em; font-size:13px; color:#C3D5C7; }}
.cover h1 {{ margin:.4em 0 .2em; font-size:40px; line-height:1.25; }}
.cover .meta {{ color:#D6E5DB; font-size:14px; }}
main {{ max-width:960px; margin:0 auto; padding:40px 24px 80px; }}
h1,h2,h3,h4 {{ color:var(--deep); line-height:1.35; }}
h1 {{ font-size:28px; border-bottom:3px solid var(--deep); padding-bottom:.3em; }}
h2 {{ font-size:23px; margin-top:2em; }}
h3 {{ font-size:19px; }}
blockquote {{ margin:1em 0; padding:.6em 1em; border-left:4px solid var(--accent); background:#F6F1E7; color:#58716C; }}
table {{ border-collapse:collapse; width:100%; margin:1em 0; font-size:14px; }}
th,td {{ border:1px solid var(--line); padding:6px 10px; vertical-align:top; }}
th {{ background:var(--deep); color:#fff; text-align:left; }}
tr:nth-child(even) td {{ background:#F3F6F2; }}
img {{ max-width:100%; height:auto; display:block; margin:1em auto; border-radius:4px; }}
code {{ background:#EEF2ED; padding:.1em .4em; border-radius:3px; font-size:.92em; }}
pre {{ background:#EEF2ED; padding:12px; overflow:auto; border-radius:6px; }}
pre code {{ background:none; padding:0; }}
a {{ color:var(--accent); }}
hr {{ border:none; border-top:1px solid var(--line); margin:2em 0; }}
footer {{ max-width:960px; margin:0 auto; padding:0 24px 40px; color:var(--muted); font-size:13px; }}
@media print {{
  .cover {{ padding:40px 24px; }}
  body {{ background:#fff; }}
  main {{ max-width:none; padding:16px; }}
  table {{ font-size:12px; }}
}}
</style>
</head>
<body>
<header class="cover">
  <div class="kicker">G A M E&nbsp; D E S I G N&nbsp; D O C U M E N T</div>
  <h1>{title}</h1>
  <div class="meta">系统 · 规则 · 体验 · 制作&nbsp;&nbsp;|&nbsp;&nbsp;版本 1.0&nbsp;&nbsp;|&nbsp;&nbsp;生成日期 {generated}</div>
</header>
<main>
{body}
</main>
<footer>参考图仅用于设计研究，不代表取得再发布授权。本文档由「神的使者」工作台生成，项目 ID {project_id}。</footer>
</body>
</html>
"#, project_id=project.id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn project() -> Project {
        serde_json::from_value(json!({
            "id":"test-project","name":"测试文档","purpose":"测试","reference_text":"参考",
            "platform":"","audience":"","status":"approved","revision":1,"review_round":1,
            "artifacts":{"document":{"markdown":"# 标题"}},
            "created_at":"2026-11-16T00:00:00Z","updated_at":"2026-11-16T00:00:00Z"
        })).unwrap()
    }
    #[test]
    fn renders_standalone_html_with_table_and_cover() {
        let html=render(&project(),"测试文档","# 标题\n\n| 操作 | 反馈 |\n| --- | --- |\n| 采集 | 入包 |").unwrap();
        assert!(html.starts_with("<!doctype html>"));
        assert!(html.contains("<html lang=\"zh-CN\">"));
        assert!(html.contains("<meta charset=\"utf-8\">"));
        assert!(html.contains("G A M E"));
        assert!(html.contains("<table>"));
        assert!(html.contains("test-project"));
        assert!(!html.contains("script"),"ammonia must strip scripts");
    }
    #[test]
    fn strips_scripts_from_markdown() {
        let html=render(&project(),"t","# 安全\n\n<script>alert(1)</script>\n\n正文").unwrap();
        assert!(!html.contains("<script"));
        assert!(html.contains("正文"));
    }
}
