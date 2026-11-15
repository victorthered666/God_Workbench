use std::{io::Write, net::{IpAddr, SocketAddr}, path::Path, sync::Arc, time::Duration};
use futures_util::StreamExt;
use image::{ImageFormat, ImageReader};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use crate::{config::MAX_IMAGE_DOWNLOAD, domain::id, AppError, Result};

pub fn validate_public_url(url: &str) -> Result<reqwest::Url> {
    let parsed=reqwest::Url::parse(url).map_err(|_|AppError::Tool("图片地址无效".into()))?;
    if parsed.scheme()!="https" || parsed.host_str().is_none() || !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(AppError::Tool("图片仅允许不带凭据的 HTTPS 地址".into()));
    }
    if let Ok(ip)=parsed.host_str().unwrap().parse::<IpAddr>() {
        validate_ip(ip)?;
    }
    Ok(parsed)
}

fn validate_ip(ip: IpAddr) -> Result<()> {
    let blocked=match ip {
        IpAddr::V4(ip)=>ip.is_private()||ip.is_loopback()||ip.is_link_local()||ip.is_unspecified()||ip.is_broadcast()||ip.is_documentation()||ip.is_multicast(),
        IpAddr::V6(ip)=>ip.is_loopback()||ip.is_unspecified()||ip.is_unique_local()||ip.is_unicast_link_local()||ip.is_multicast(),
    };
    if blocked { Err(AppError::Tool("图片地址指向本机、内网或保留网络，已拒绝".into())) } else { Ok(()) }
}

/// 在连接建立时做一次性 DNS 解析并只返回通过校验的公网地址。
/// 此前“先手动解析校验、再让 reqwest 二次解析”存在 DNS rebinding TOCTOU：
/// 恶意域名可在两次解析间切换到内网地址。
#[derive(Clone)]
struct PublicOnlyResolver;
impl reqwest::dns::Resolve for PublicOnlyResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        Box::pin(async move {
            // Name 实现 Deref<Target=str>
            let addresses=tokio::net::lookup_host((name.as_str(),0)).await?;
            let public:Vec<SocketAddr>=addresses.into_iter().filter(|address|validate_ip(address.ip()).is_ok()).collect();
            if public.is_empty() {
                return Err(Box::new(std::io::Error::new(std::io::ErrorKind::Other,"图片域名没有可用的公网地址")) as Box<dyn std::error::Error + Send + Sync>);
            }
            Ok(Box::new(public.into_iter()) as Box<dyn Iterator<Item=SocketAddr> + Send>)
        })
    }
}

pub async fn download(url: &str, root: &Path) -> Result<(String,Value)> {
    let client=reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::none())
        .dns_resolver(Arc::new(PublicOnlyResolver))
        .build()?;
    let mut current=validate_public_url(url)?;
    for _ in 0..4 {
        let mut response=client.get(current.clone()).header(reqwest::header::USER_AGENT,"GDDHarness/0.2 (local research)").send().await
            .map_err(|error|AppError::Tool(format!("图片请求失败：{}",crate::paths::truncate_utf8(&error.to_string(),200))))?;
        if response.status().is_redirection() {
            let location=response.headers().get(reqwest::header::LOCATION).and_then(|v|v.to_str().ok()).ok_or_else(||AppError::Tool("图片重定向缺少目标".into()))?;
            current=validate_public_url(&current.join(location).map_err(|_|AppError::Tool("图片重定向地址无效".into()))?.to_string())?;
            continue;
        }
        if !response.status().is_success() { return Err(AppError::Tool(format!("图片下载失败 HTTP {}",response.status()))); }
        let mime=response.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v|v.to_str().ok()).unwrap_or("").split(';').next().unwrap_or("").to_lowercase();
        let (_format,extension)=match mime.as_str() { "image/png"=>(ImageFormat::Png,"png"),"image/jpeg"=>(ImageFormat::Jpeg,"jpg"),"image/webp"=>(ImageFormat::WebP,"webp"),_=>return Err(AppError::Tool("不支持的图片类型".into())) };
        let filename=format!("assets/images/{}.{}",id(),extension);
        let path=root.join(&filename);
        std::fs::create_dir_all(path.parent().unwrap())?;
        let mut hasher=Sha256::new();
        let mut bytes=0usize;
        let mut file=std::fs::File::create(&path)?;
        while let Some(chunk)=response.chunk().await? {
            bytes=bytes.checked_add(chunk.len()).ok_or_else(||AppError::Tool("图片尺寸溢出".into()))?;
            if bytes>MAX_IMAGE_DOWNLOAD { drop(file); let _=std::fs::remove_file(&path); return Err(AppError::Tool("图片超过 12 MiB 限制".into())); }
            hasher.update(&chunk); file.write_all(&chunk)?;
        }
        drop(file);
        // 任何后续校验失败都必须清掉半成品文件，避免未审核字节混入导出归档。
        let finish=|| -> Result<(u32,u32)> {
            let mut reader=ImageReader::open(&path)?.with_guessed_format()?;
            let mut limits=image::Limits::default(); limits.max_image_width=Some(8192); limits.max_image_height=Some(8192); limits.max_alloc=Some(192*1024*1024);
            reader.limits(limits);
            let dimensions=reader.into_dimensions()?;
            if u64::from(dimensions.0)*u64::from(dimensions.1)>40_000_000 { return Err(AppError::Tool("图片像素超出处理限制".into())); }
            Ok(dimensions)
        };
        let dimensions=match finish() {
            Ok(dimensions)=>dimensions,
            Err(error)=>{ let _=std::fs::remove_file(&path); return Err(error); }
        };
        let digest=format!("{:x}",hasher.finalize());
        return Ok((filename,json!({"bytes":bytes,"width":dimensions.0,"height":dimensions.1,"format":extension,"sha256":digest,"source_url":url})));
    }
    Err(AppError::Tool("图片重定向次数过多".into()))
}
