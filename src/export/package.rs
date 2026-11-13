use std::{fs::File, io::{BufWriter, Read, Write}, path::{Path, PathBuf}};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use crate::{domain::{now, Project}, paths::{contained_file, safe_name}, repository::Repository, AppError, Result};
use super::docx::render;

pub fn export(repo: &Repository, project: &Project) -> Result<Value> {
    let root=project.working_dir()?;
    let title=safe_name(&project.name);
    let markdown=project.artifact("document").and_then(|v|v.get("markdown")).and_then(Value::as_str).ok_or_else(||AppError::Workflow("缺少可导出的正文".into()))?;
    for path in image_paths(markdown) { contained_file(&root,&path)?; }
    let md_path=root.join(format!("{title}.md"));
    let docx_path=root.join(format!("{title}.docx"));
    std::fs::write(&md_path,markdown)?;
    render(project,&docx_path)?;
    if let Some(artifacts)=project.artifacts.as_object() {
        for (stage,artifact) in artifacts {
            if artifact.get("content").is_some() {
                let directory=match stage.as_str() { "research"=>"research","review"=>"reviews",_=>"versions" };
                std::fs::write(root.join(directory).join(format!("{stage}.json")),serde_json::to_string_pretty(artifact)?)?;
            }
        }
    }
    std::fs::write(root.join("research/evidence.json"),serde_json::to_string_pretty(&repo.evidence_list(&project.id,100_000)?)?)?;
    std::fs::write(root.join("reviews/history.json"),serde_json::to_string_pretty(&repo.versions(&project.id,100_000,0)?)?)?;
    let mut manifest=json!({"name":title,"project_id":project.id,"revision":project.revision,"created_at":project.created_at,"exported_at":now(),"approval":project.artifact("review"),"documents":[md_path.file_name().unwrap().to_string_lossy(),docx_path.file_name().unwrap().to_string_lossy()],"assets":repo.assets(&project.id)?,"notice":"网络图片仅用于研究参考，可检索不代表取得再发布授权。本文档不包含 API Key。","files":[]});
    let files=project_files(&root)?;
    if files.len()>10_000 || files.iter().try_fold(0u64,|sum,path|sum.checked_add(path.metadata().ok()?.len())).is_none_or(|total|total>2*1024*1024*1024) {
        return Err(AppError::Invalid("归档文件数量或总大小超过安全限制".into()));
    }
    for path in &files {
        let relative=path.strip_prefix(&root).unwrap().to_string_lossy().replace('\\',"/");
        manifest["files"].as_array_mut().unwrap().push(json!({"path":relative,"sha256":hash_file(path)?,"bytes":path.metadata()?.len()}));
    }
    std::fs::write(root.join("manifest.json"),serde_json::to_string_pretty(&manifest)?)?;
    let zip_path=root.join(format!("{title}-v{}.zip",project.revision));
    let temp=root.join(format!("{title}.tmp.zip"));
    let mut archive=zip::ZipWriter::new(BufWriter::new(File::create(&temp)?));
    let options=zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for path in project_files(&root)? {
        let relative=path.strip_prefix(&root).unwrap().to_string_lossy().replace('\\',"/");
        archive.start_file(format!("{title}/{relative}"),options).map_err(|error|AppError::Other(error.to_string()))?;
        std::io::copy(&mut File::open(path)?,&mut archive)?;
    }
    archive.finish().map_err(|error|AppError::Other(error.to_string()))?;
    std::fs::rename(&temp,&zip_path)?;
    Ok(json!({"directory":root,"markdown":md_path,"docx":docx_path,"zip":zip_path}))
}

fn image_paths(markdown: &str) -> Vec<String> {
    let mut result=vec![];
    let mut rest=markdown;
    while let Some(start)=rest.find("![") {
        let Some(mid)=rest[start..].find("](") else { break; };
        let end=rest[start+mid+2..].find(')').map(|index|start+mid+2+index).unwrap_or(rest.len());
        result.push(rest[start+mid+2..end].to_string());
        rest=&rest[end..];
    }
    result
}

fn project_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files=vec![];
    for entry in std::fs::read_dir(root)? {
        let path=entry?.path();
        if path.is_dir() { files.extend(project_files(&path)?); }
        else if path.is_file() && path.extension().and_then(|v|v.to_str())!=Some("zip") && path.extension().and_then(|v|v.to_str())!=Some("pdf") && path.file_name().and_then(|v|v.to_str())!=Some("manifest.json") && !path.file_name().and_then(|v|v.to_str()).unwrap_or("").contains("secret") { files.push(path); }
    }
    files.sort();
    Ok(files)
}

fn hash_file(path: &Path) -> Result<String> {
    let mut file=File::open(path)?;
    let mut hasher=Sha256::new();
    let mut buffer=[0u8;65536];
    loop {
        let count=file.read(&mut buffer)?;
        if count==0 { break; }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}",hasher.finalize()))
}
