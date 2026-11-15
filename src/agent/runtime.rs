use std::{fs, path::{Path, PathBuf}, time::Duration};
use async_trait::async_trait;
use futures_util::StreamExt;
use schemars::schema_for;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use crate::{config::{MAX_MESSAGE_BYTES, MAX_TOOL_RESULT_BYTES}, domain::{AgentResult, AgentSettings, Annotations, Project, Role}, media, paths::truncate_utf8, repository::Repository, secrets::SecretStore, tools::{deepseek::DeepSeek, doubao::Doubao, http::ToolHttp}, AppError, Result};
use super::{session, vision};

/// 显式声明本次调用的产出类型；绝不从 project.status 推断，
/// 否则中断恢复等场景会把审核输出误当框架校验。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentTask {
    Research,
    Framework,
    RevisionPlan,
    Review,
    Document,
    DocumentPatch,
    InteractionSupplement,
    Resources,
    DocumentAudit,
}

impl AgentTask {
    pub fn role(&self) -> Role {
        match self {
            Self::Research => Role::Research,
            Self::Framework | Self::RevisionPlan | Self::Review => Role::Architect,
            Self::Document | Self::DocumentPatch => Role::Writer,
            Self::InteractionSupplement => Role::Interaction,
            Self::Resources => Role::Producer,
            Self::DocumentAudit => Role::Architect,
        }
    }
}

#[async_trait]
pub trait AgentExecutor: Send + Sync {
    async fn run(&self, task: AgentTask, project: &Project, instruction: String, feedback: &str) -> Result<Value>;
    async fn annotate(&self, project: &Project, path: &str, title: &str) -> Result<Vec<crate::domain::Annotation>>;
}

#[derive(Clone)]
pub struct AgentRuntime {
    pub repo: Repository,
    pub secrets: SecretStore,
    pub root: PathBuf,
}

impl AgentRuntime {
    pub fn new(repo: Repository, secrets: SecretStore, root: impl AsRef<Path>) -> Self { Self { repo, secrets, root:root.as_ref().into() } }

    pub fn settings(&self, role: Role) -> Result<AgentSettings> {
        let default:AgentSettings=serde_yaml::from_str(&fs::read_to_string(self.root.join(format!("config/agents/{}.yaml",role.as_str())))?)?;
        let saved=self.repo.setting(&format!("agent:{}",role.as_str()),json!({}))?;
        let mut merged=serde_json::to_value(default)?;
        if let Some(values)=saved.as_object() { for (key,value) in values { if key!="api_key" { merged[key]=value.clone(); } } }
        let mut settings:AgentSettings=serde_json::from_value(merged)?;
        if !(0.0..=2.0).contains(&settings.temperature) || !(512..=65536).contains(&settings.max_tokens) || settings.base_url.is_empty() { return Err(AppError::Agent("模型参数配置不合法".into())); }
        if settings.thinking.is_empty() { settings.thinking = "enabled".into(); }
        if settings.thinking != "enabled" { return Err(AppError::Agent("按当前策略思考模式必须开启（enabled）；如需关闭请在配置策略中解除限制".into())); }
        // 思考 token 计入输出预算；保底 64K 防止长文档推理挤占内容空间。
        if settings.max_tokens<65536 { settings.max_tokens=65536; }
        if settings.base_url.starts_with("http://") && !settings.base_url.starts_with("http://127.0.0.1") && !settings.base_url.starts_with("http://localhost") { return Err(AppError::Agent("模型 Base URL 必须使用 HTTPS".into())); }
        let prompt=self.repo.setting(&format!("prompt:{}",role.as_str()),Value::String(fs::read_to_string(self.root.join(format!("agents/{}.md",role.as_str())))?))?;
        settings.prompt=prompt.as_str().unwrap_or_default().to_string();
        Ok(settings)
    }

    pub fn system_prompt(&self, role: Role, settings: &AgentSettings) -> Result<String> {
        let mut skills=String::new();
        for name in &settings.skills {
            if !name.chars().all(|c|c.is_ascii_alphanumeric()||c=='-') { return Err(AppError::Agent("Skill 名称不合法".into())); }
            skills.push_str(&fs::read_to_string(self.root.join(format!("skills/{name}.md")))?);
            skills.push_str("\n\n");
        }
        Ok(format!("{}\n\n以下是本角色的 Skill 工作规范：\n{}",settings.prompt,skills))
    }

    /// 以 SSE 流式调用模型：token 持续回流避免长生成被网关/超时掐断。
    /// 累积 delta 内容与分片工具调用，重组为标准 Chat Completions 载荷。
    async fn stream_chat_completion(&self, client:&reqwest::Client, base_url:&str, key:&str, body:Value) -> Result<Value> {
        let response=client.post(format!("{base_url}/chat/completions")).bearer_auth(key).json(&body).send().await
            .map_err(|error|AppError::Agent(format!("模型连接失败：{}",truncate_utf8(&error.to_string(),200))))?;
        let status=response.status();
        if !status.is_success() {
            let mut response=response;
            let bytes=crate::tools::http::bounded_body(&mut response).await?;
            let detail=serde_json::from_slice::<Value>(&bytes).ok().and_then(|v|v.pointer("/error/message").and_then(Value::as_str).map(str::to_owned)).unwrap_or_default();
            return Err(AppError::Agent(format!("模型调用失败 HTTP {status}：{}",self.secrets.redact(&detail))));
        }
        let mut content=String::new();
        let mut tool_calls:std::collections::BTreeMap<u64,Value>=Default::default();
        let mut current_tool_index:Option<u64>=None;
        let mut finish:Option<String>=None;
        let mut received=0usize;
        // 原始字节缓冲：多字节 UTF-8 序列可能被网络分块切开，必须等整行
        // 到齐后再解码，不能对单个 chunk 做 from_utf8_lossy（会丢字/坏 JSON）。
        let mut buffer:Vec<u8>=Vec::new();
        let mut stream=response.bytes_stream();
        fn process_line(line:&str,
                        content:&mut String,
                        tool_calls:&mut std::collections::BTreeMap<u64,Value>,
                        current_tool_index:&mut Option<u64>,
                        finish:&mut Option<String>) {
            let line=line.trim_end();
            // SSE 允许 data: 后零或一个空格
            let Some(data)=line.strip_prefix("data:") else { return; };
            let data=data.strip_prefix(' ').unwrap_or(data).trim();
            if data.is_empty() || data=="[DONE]" { return; }
            let Ok(delta)=serde_json::from_str::<Value>(data) else { return; };
            if let Some(text)=delta.pointer("/choices/0/delta/content").and_then(Value::as_str) { content.push_str(text); }
            if let Some(reason)=delta.pointer("/choices/0/finish_reason").and_then(Value::as_str) { *finish=Some(reason.to_string()); }
            if let Some(deltas)=delta.pointer("/choices/0/delta/tool_calls").and_then(Value::as_array) {
                for call in deltas {
                    // 无 index 的分片跟随最近出现的 index（分片按调用顺序到达）
                    let index=call.get("index").and_then(Value::as_u64).or(*current_tool_index).unwrap_or(0);
                    *current_tool_index=Some(index);
                    let entry=tool_calls.entry(index).or_insert_with(||json!({"id":"","type":"function","function":{"name":"","arguments":""}}));
                    if let Some(id)=call.get("id").and_then(Value::as_str) { if !id.is_empty() { entry["id"]=json!(id); } }
                    if let Some(name)=call.pointer("/function/name").and_then(Value::as_str) { if !name.is_empty() { entry["function"]["name"]=json!(name); } }
                    if let Some(args)=call.pointer("/function/arguments").and_then(Value::as_str) {
                        let mut accumulated=entry.pointer("/function/arguments").and_then(Value::as_str).unwrap_or("").to_string();
                        accumulated.push_str(args);
                        entry["function"]["arguments"]=json!(accumulated);
                    }
                }
            }
        }
        loop {
            let chunk=match stream.next().await {
                Some(chunk)=>chunk.map_err(|error|AppError::Agent(format!("模型流式响应中断：{}",truncate_utf8(&error.to_string(),200))))?,
                None=>break,
            };
            received+=chunk.len();
            if received>64*1024*1024 { return Err(AppError::Agent("模型流式响应超过 64 MiB，已停止读取".into())); }
            buffer.extend_from_slice(&chunk);
            while let Some(position)=buffer.iter().position(|byte| *byte==b'\n') {
                let line_bytes:Vec<u8>=buffer.drain(..=position).collect();
                let line=std::str::from_utf8(&line_bytes)
                    .map_err(|_|AppError::Agent("模型流式响应包含无效 UTF-8".into()))?;
                process_line(line,&mut content,&mut tool_calls,&mut current_tool_index,&mut finish);
            }
        }
        // 服务端可能不以换行结束最后一条事件
        if !buffer.is_empty() {
            let line=std::str::from_utf8(&buffer)
                .map_err(|_|AppError::Agent("模型流式响应包含无效 UTF-8".into()))?;
            process_line(line,&mut content,&mut tool_calls,&mut current_tool_index,&mut finish);
        }
        let tool_list:Vec<Value>=tool_calls.into_values().collect();
        let message=if tool_list.is_empty() {
            json!({"role":"assistant","content":content})
        } else {
            json!({"role":"assistant","content":content,"tool_calls":tool_list})
        };
        Ok(json!({"choices":[{"message":message,"finish_reason":finish.unwrap_or_else(||"stop".into())}]}))
    }

    async fn call_tool(&self, role: Role, project: &Project, name: &str, arguments: Value) -> Result<Value> {
        let http=ToolHttp::new(self.secrets.clone())?;
        match (role,name) {
            (Role::Research,"search_web") => DeepSeek::new(http).search(&self.repo,&project.id,arguments.get("query").and_then(Value::as_str).unwrap_or(""),arguments.get("count").and_then(Value::as_u64).unwrap_or(5).clamp(1,10)as u8).await,
            (Role::Research,"fetch_page") => DeepSeek::new(http).fetch(&self.repo,&project.id,arguments.get("url").and_then(Value::as_str).unwrap_or("")).await,
            (Role::Writer|Role::Interaction,"image_search") => Doubao::new(http).search(&self.repo,&project.id,arguments.get("query").and_then(Value::as_str).unwrap_or(""),arguments.get("count").and_then(Value::as_u64).unwrap_or(3).clamp(1,5)as u8).await,
            (Role::Writer|Role::Interaction,"select_image") => {
                let url=arguments.get("url").and_then(Value::as_str).unwrap_or("");
                let source_page=arguments.get("source_page").and_then(Value::as_str).unwrap_or("");
                let title=arguments.get("title").and_then(Value::as_str).unwrap_or("参考图");
                let root=project.working_dir()?;
                let (path,metadata)=media::download::download(url,&root).await?;
                let labels=self.annotate(project,&path,title).await?;
                let markdown=media::annotate::annotate(&root,&path,title,&labels,url,source_page)?;
                let mut metadata_with_labels=metadata.as_object().cloned().unwrap_or_default();
                metadata_with_labels.insert("labels".into(),serde_json::to_value(&labels)?);
                self.repo.asset(&project.id,url,source_page,&path,metadata.get("sha256").and_then(Value::as_str).unwrap_or(""),Value::Object(metadata_with_labels))?;
                Ok(Value::String(markdown))
            },
            (Role::Writer|Role::Interaction,"flowchart") => Ok(Value::String(media::diagrams::flowchart(&project.working_dir()?,arguments.get("title").and_then(Value::as_str).unwrap_or("核心流程"),&arguments.get("steps").and_then(Value::as_array).map(|items|items.iter().filter_map(Value::as_str).map(str::to_owned).collect::<Vec<_>>()).unwrap_or_default())?)),
            (Role::Writer|Role::Interaction,"wireframe") => Ok(Value::String(media::diagrams::wireframe(&project.working_dir()?,arguments.get("title").and_then(Value::as_str).unwrap_or("交互线框"),arguments.get("regions").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]))?)),
            _ => Err(AppError::Tool(format!("角色无权调用工具 {name}"))),
        }
    }

    async fn execute<T:AgentResult>(&self, role: Role, project: &Project, instruction: String, feedback: &str, images: Vec<Value>) -> Result<T> {
        self.execute_with_model::<T>(role,project,instruction,feedback,images,None).await
    }

    async fn execute_with_model<T:AgentResult>(&self, role: Role, project: &Project, instruction: String, feedback: &str, images: Vec<Value>, model_override: Option<String>) -> Result<T> {
        self.execute_inner::<T>(role,project,instruction,feedback,images,model_override,false).await
    }

    /// 视觉标注专用：禁用工具，防止 select_image → 标注 → select_image 无限递归。
    async fn execute_vision<T:AgentResult>(&self, project: &Project, instruction: String, images: Vec<Value>, model: &str) -> Result<T> {
        self.execute_inner::<T>(Role::Writer,project,instruction,"",images,Some(model.to_string()),true).await
    }

    async fn execute_inner<T:AgentResult>(&self, role: Role, project: &Project, instruction: String, feedback: &str, images: Vec<Value>, model_override: Option<String>, disable_tools: bool) -> Result<T> {
        if instruction.len()>MAX_MESSAGE_BYTES { return Err(AppError::Agent("单次 Agent 文本输入超过 2 MiB".into())); }
        let mut settings=self.settings(role)?;
        if let Some(model)=model_override { settings.model=model; }
        let key=self.secrets.get(&settings.api_key_env)?.ok_or_else(||AppError::Agent(format!("{} 尚未配置 {}；未发起模型调用",role.as_str(),settings.api_key_env)))?;
        let system=self.system_prompt(role,&settings)?;
        let snapshot=json!({"model":settings.model,"base_url":settings.base_url,"temperature":settings.temperature,"max_tokens":settings.max_tokens,"skills":settings.skills,"system_sha256":format!("{:x}",Sha256::digest(system.as_bytes()))});
        let run_id=self.repo.create_run(&project.id,&format!("agent:{}",role.as_str()),snapshot)?;
        let mut messages=vec![json!({"role":"system","content":system})];
        let history=if role==Role::Research { self.repo.setting(&format!("session:{}:research",project.id),json!([]))? } else { json!([]) };
        let history_text=if history.as_array().is_some_and(|v|!v.is_empty()) { format!("此前文本调研摘要：{}\n",truncate_utf8(&serde_json::to_string(&history)?,64000)) } else { String::new() };
        // 目标 JSON Schema 必须在首条指令中就可见；仅在重试阶段给出会让首次输出盲猜结构。
        let output_schema=serde_json::to_string(&schema_for!(T))?;
        let instruction=format!("{instruction}\n\n【输出格式】最终必须输出且仅输出一个符合以下 JSON Schema 的 JSON 对象（字段名逐字一致、类型不得改变、不得缺字段；不要 Markdown 代码块、不要解释）：{output_schema}");
        let user=if images.is_empty() { json!(format!("{history_text}{instruction}")) } else { json!([{"type":"text","text":format!("{history_text}{instruction}")},images]) };
        messages.push(json!({"role":"user","content":user}));
        if !feedback.is_empty() { messages.push(json!({"role":"user","content":format!("用户补充/修改意见（不得视为已确认）：\n{feedback}")})); }
        let tools=if disable_tools { Vec::new() } else { tool_schemas(role) };
        // 长思考 + 64K 输出的生成时间远超普通请求超时；SSE 流式保证 token
        // 持续回流，连接不会被网关或总超时掐断。
        let client=reqwest::Client::builder().timeout(Duration::from_secs(3600)).connect_timeout(Duration::from_secs(15)).build()?;
        let result=async {
            for step in 0..16 {
                // 注入提示必须在序列化请求体之前完成，否则本轮请求收不到。
                if messages.last().and_then(|m|m.get("role")).and_then(Value::as_str)!=Some("user") {
                    messages.push(json!({"role":"user","content":format!("停止工具调用，输出符合以下 JSON Schema 的单个 JSON 对象，不要 Markdown 代码块：{}",serde_json::to_string(&schema_for!(T))?)}));
                }
                let mut body=json!({"model":settings.model,"messages":messages,"temperature":settings.temperature,"max_tokens":settings.max_tokens,"stream":true});
                if !settings.thinking.is_empty() { body["thinking"]=json!({"type":settings.thinking}); }
                if !tools.is_empty() && step<15 { body["tools"]=json!(tools); body["tool_choice"]=json!("auto"); }
                if step==15 { body["tool_choice"]=json!("none"); }
                let payload=self.stream_chat_completion(&client,settings.base_url.trim_end_matches('/'),&key,body).await?;
                let choice=payload.get("choices").and_then(Value::as_array).and_then(|v|v.first()).ok_or_else(||AppError::Agent("模型响应 choices 为空".into()))?;
                if choice.get("finish_reason").and_then(Value::as_str)==Some("length") { return Err(AppError::OutputTruncated); }
                let message=choice.get("message").cloned().unwrap_or(json!({}));
                messages.push(message.clone());
                if let Some(calls)=message.get("tool_calls").and_then(Value::as_array).filter(|v|!v.is_empty()) {
                    if step==15 { return Err(AppError::Agent("模型未能按要求停止工具调用".into())); }
                    if calls.len()>8 {
                        self.repo.event(&project.id,"warning",&format!("模型单轮请求 {} 个工具调用，超出上限，仅执行前 8 个",calls.len()))?;
                    }
                    for call in calls.iter().take(8) {
                        let name=call.pointer("/function/name").and_then(Value::as_str).unwrap_or("");
                        let result=async {
                            let arguments:Value=serde_json::from_str(call.pointer("/function/arguments").and_then(Value::as_str).unwrap_or("{}"))?;
                            self.call_tool(role,project,name,arguments).await
                        }.await;
                        let (text,failed)=match result { Ok(value)=>(value,false),Err(error)=>(json!({"error":error.to_string()}),true) };
                        self.repo.event(&project.id,if failed{"tool_error"}else{"tool"},&format!("{} 调用 {}：{}",role.as_str(),name,if failed{"失败"}else{"成功"}))?;
                        let serialized=text.to_string();
                        let bounded=if serialized.len()>MAX_TOOL_RESULT_BYTES { format!("{}…[工具结果已截断]",truncate_utf8(&serialized,MAX_TOOL_RESULT_BYTES)) } else { serialized };
                        messages.push(json!({"role":"tool","tool_call_id":call.get("id").cloned().unwrap_or(json!("")),"content":bounded}));
                    }
                    continue;
                }
                let content=message.get("content").and_then(Value::as_str).unwrap_or("");
                if content.trim().is_empty() { return Err(AppError::Agent("模型返回空正文".into())); }
                match parse_result::<T>(content) {
                    Ok(result)=>return Ok(result),
                    Err(error) if step>=14=>return Err(error),
                    Err(error)=>messages.push(json!({"role":"user","content":format!("上次结果不是有效结构：{}。仅输出符合 Schema 的 JSON 对象。",truncate_utf8(&error.to_string(),800))})),
                }
            }
            Err(AppError::Agent("模型调用轮次已用尽".into()))
        }.await;
        let succeeded=result.is_ok();
        self.repo.finish_run(&run_id,if succeeded{None}else{Some("Agent 执行失败；详见项目事件")})?;
        if let Err(error)=session::save(&self.repo,&project.id,role.as_str(),&messages) {
            // 会话只是连续性优化：保存失败不应丢弃已成功的模型产出。
            let _=self.repo.event(&project.id,"warning",&format!("会话保存失败：{error}"));
        }
        result
    }
}

#[async_trait]
impl AgentExecutor for AgentRuntime {
    async fn run(&self, task: AgentTask, project: &Project, instruction: String, feedback: &str) -> Result<Value> {
        let role=task.role();
        match task {
            AgentTask::Research => Ok(serde_json::to_value(self.execute::<crate::domain::ResearchReport>(role,project,instruction,feedback,vec![]).await?)?),
            AgentTask::Framework => Ok(serde_json::to_value(self.execute::<crate::domain::Framework>(role,project,instruction,feedback,vec![]).await?)?),
            AgentTask::RevisionPlan => Ok(serde_json::to_value(self.execute::<crate::domain::ImpactPlan>(role,project,instruction,feedback,vec![]).await?)?),
            AgentTask::Review => Ok(serde_json::to_value(self.execute::<crate::domain::Review>(role,project,instruction,feedback,vec![]).await?)?),
            AgentTask::Document => Ok(serde_json::to_value(self.execute::<crate::domain::DocumentDraft>(role,project,instruction,feedback,vec![]).await?)?),
            AgentTask::DocumentPatch => Ok(serde_json::to_value(self.execute::<crate::domain::DocumentPatches>(role,project,instruction,feedback,vec![]).await?)?),
            AgentTask::InteractionSupplement => Ok(serde_json::to_value(self.execute::<crate::domain::InteractionSupplement>(role,project,instruction,feedback,vec![]).await?)?),
            AgentTask::Resources => Ok(serde_json::to_value(self.execute::<crate::domain::ResourcePlan>(role,project,instruction,feedback,vec![]).await?)?),
            AgentTask::DocumentAudit => Ok(serde_json::to_value(self.execute::<crate::domain::AuditReport>(role,project,instruction,feedback,vec![]).await?)?),
        }
    }

    async fn annotate(&self, project: &Project, path: &str, title: &str) -> Result<Vec<crate::domain::Annotation>> {
        let images=vision::image_blocks(&project.working_dir()?, &[path.into()]);
        let model=self.settings(Role::Writer)?.vision_model;
        let result=self.execute_vision::<Annotations>(project,format!("观察所附真实参考图片，选择 1–8 个对策划说明有帮助的实际可见区域。x/y 是相对原图的归一化中心坐标，不要臆测。图题：{title}"),images,&model).await?;
        if result.labels.is_empty() || result.labels.len()>8 { return Err(AppError::Tool("视觉模型未返回有效数量的标注".into())); }
        Ok(result.labels)
    }
}

fn parse_result<T:AgentResult>(text: &str) -> Result<T> {
    let trimmed=text.trim().trim_start_matches("```json").trim_start_matches("```").trim_end_matches("```").trim();
    let start=trimmed.find('{').ok_or_else(||AppError::Agent("模型未返回 JSON 对象".into()))?;
    let end=trimmed.rfind('}').ok_or_else(||AppError::Agent("模型 JSON 对象不完整".into()))?;
    serde_json::from_str(&trimmed[start..=end]).map_err(|error|AppError::Agent(format!("结构化结果校验失败：{error}")))
}

fn schema(properties: Value, required: &[&str]) -> Value { json!({"type":"object","properties":properties,"required":required}) }
fn function(name: &str, description: &str, parameters: Value) -> Value { json!({"type":"function","function":{"name":name,"description":description,"parameters":parameters}}) }
fn tool_schemas(role: Role) -> Vec<Value> {
    match role {
        Role::Research=>vec![
            function("search_web","DeepSeek 官方网页搜索",schema(json!({"query":{"type":"string"},"count":{"type":"integer","minimum":1,"maximum":10}}),&["query"])),
            function("fetch_page","抓取 DeepSeek 搜索发现的重点网页",schema(json!({"url":{"type":"string"}}),&["url"])),
        ],
        Role::Writer|Role::Interaction=>vec![
            function("image_search","豆包图片搜索，返回真实候选，不保证授权",schema(json!({"query":{"type":"string"},"count":{"type":"integer","minimum":1,"maximum":5}}),&["query"])),
            function("select_image","下载真实参考图并由视觉模型观察后标注",schema(json!({"url":{"type":"string"},"source_page":{"type":"string"},"title":{"type":"string"}}),&["url","title"])),
            function("flowchart","实际生成流程图并返回插图 Markdown",schema(json!({"title":{"type":"string"},"steps":{"type":"array","items":{"type":"string"}}}),&["title","steps"])),
            function("wireframe","实际生成编号线框图并返回插图 Markdown",schema(json!({"title":{"type":"string"},"regions":{"type":"array","items":{"type":"object"}}}),&["title","regions"])),
        ],
        _=>vec![],
    }
}
