use std::{fs, sync::Arc};
use serde_json::{json, Value};
use crate::{agent::runtime::{AgentExecutor, AgentTask}, config::{Config, MAX_REVIEW_ROUNDS}, domain::{GateRequest, Project, ProjectCreate, RevisionRequest, Role}, export::resources::resource_markdown, paths::safe_name, repository::Repository, AppError, Result};

#[derive(Clone)]
pub struct Workflow {
    pub repo: Repository,
    pub agents: Arc<dyn AgentExecutor>,
    pub config: Config,
}

impl Workflow {
    const MAX_DOCUMENT_BYTES: usize = 2 * 1024 * 1024;

    pub fn new(repo: Repository, agents: Arc<dyn AgentExecutor>, config: Config) -> Self { Self { repo, agents, config } }

    pub fn create(&self, data: &ProjectCreate) -> Result<Project> {
        let mut project=self.repo.create_project(data)?;
        let root=self.config.output_dir.join(format!("{}-{}",safe_name(&data.name),&project.id[..8]));
        for path in ["assets/images","assets/diagrams","research","reviews","versions"] { fs::create_dir_all(root.join(path))?; }
        project.artifacts=json!({"working_dir":root});
        self.repo.update(&project.id,None,Some(project.artifacts.clone()),None,None,None)?;
        self.repo.event(&project.id,"stage","已创建项目；等待调研策划")?;
        Ok(project)
    }

    pub async fn execute(&self, project_id: &str, stage: &str, feedback: &str) -> Result<Project> {
        let project=self.repo.require(project_id)?;
        if project.running() { return Err(AppError::Workflow("已有阶段正在执行，请等待完成".into())); }
        let allowed=match stage {
            "research"=>matches!(project.status.as_str(),"research_review"|"interrupted"),
            "framework"=>matches!(project.status.as_str(),"framework_review"|"interrupted"),
            "revision_plan"=>matches!(project.status.as_str(),"revision_review"|"revision_planning"|"interrupted"),
            "document"=>matches!(project.status.as_str(),"framework_approved"|"interrupted"),
            "interaction"=>matches!(project.status.as_str(),"interaction"|"interrupted"),
            "resources"=>matches!(project.status.as_str(),"resources"|"interrupted"),
            "review"=>matches!(project.status.as_str(),"review"|"interrupted"),
            "revise"=>matches!(project.status.as_str(),"review"|"needs_human"),
            _=>false,
        };
        if stage=="framework" && project.status=="research_review" { return Err(AppError::Workflow("调研意见尚未由用户确认".into())); }
        if stage=="document" && project.status=="framework_review" { return Err(AppError::Workflow("文档框架尚未由用户确认".into())); }
        if !allowed { return Err(AppError::Workflow(format!("当前状态 {} 不能执行 {}；请先完成或确认前置阶段",project.status,stage))); }
        self.repo.update(project_id,Some(&format!("running:{stage}")),None,Some(None),None,None)?;
        self.repo.event(project_id,"stage",&format!("开始执行 {stage}"))?;
        let run_id=self.repo.create_run(project_id,stage,json!({}))?;
        if !feedback.is_empty() { self.repo.save_artifact(project_id,"feedback",json!({"stage":stage,"feedback":feedback}),feedback)?; }
        let result=self.execute_stage(&project,stage,feedback).await;
        match result {
            Ok(())=>{ self.repo.finish_run(&run_id,None)?; self.repo.event(project_id,"stage",&format!("完成 {stage}"))?; self.repo.require(project_id) },
            Err(error)=>{ self.repo.finish_run(&run_id,Some(&error.to_string()))?; self.repo.update(project_id,Some("interrupted"),None,Some(Some(error.to_string())),None,None)?; self.repo.event(project_id,"error",&error.to_string())?; Err(error) },
        }
    }

    async fn execute_stage(&self, project: &Project, stage: &str, feedback: &str) -> Result<()> {
        match stage {
            "research"=>self.research(project,feedback).await,
            "revision_plan"=>self.revision_plan(project,feedback).await,
            "framework"=>self.framework(project,feedback).await,
            "document"=>self.document(project,feedback).await,
            "interaction"=>self.interaction(project,feedback).await,
            "resources"=>self.resources(project,feedback).await,
            "review"=>self.review(project,feedback).await,
            "revise"=>self.revise(project,feedback).await,
            _=>Err(AppError::Workflow("未知阶段".into())),
        }
    }

    fn save(&self, project: &Project, stage: &str, content: Value, status: &str, feedback: &str) -> Result<()> {
        self.repo.save_artifact(&project.id,stage,content.clone(),feedback)?;
        self.repo.update(&project.id,Some(status),None,Some(None),None,None)?;
        let root=project.working_dir()?;
        if let Some(markdown)=content.get("markdown").and_then(Value::as_str) { fs::write(root.join(format!("{stage}.md")),markdown)?; }
        else { fs::write(root.join(format!("{stage}.json")),serde_json::to_string_pretty(&content)?)?; }
        Ok(())
    }

    fn bounded_document(&self, project: &Project) -> Result<String> {
        let markdown=project.artifact("document").and_then(|v|v.get("markdown")).and_then(Value::as_str).unwrap_or("").to_string();
        if markdown.len()>2*1024*1024 { return Err(AppError::Workflow("正文超过 2 MiB，请先拆分为独立章节项目".into())); }
        Ok(markdown)
    }

    fn ensure_size(&self, markdown: &str) -> Result<()> {
        if markdown.len()>Self::MAX_DOCUMENT_BYTES { return Err(AppError::Workflow("合并后的正文超过 2 MiB，请拆分为独立章节项目".into())); }
        Ok(())
    }

    async fn research(&self, project: &Project, feedback: &str) -> Result<()> {
        let instruction=format!("请针对以下设计目的与参考对象完成有证据的调研。先搜索再抓取重点来源，给出可执行的撰写意见。\n设计目的：{}\n参考对象：{}\n平台：{}\n受众：{}\n最后输出 ResearchReport JSON。来源 ID 必须真实来自工具。",project.purpose,project.reference_text,if project.platform.is_empty(){"未指定"}else{&project.platform},if project.audience.is_empty(){"未指定"}else{&project.audience});
        let result=self.agents.run(AgentTask::Research,project,instruction,feedback).await?;
        self.save(project,"research",result,"research_review",feedback)
    }

    async fn revision_plan(&self, project: &Project, feedback: &str) -> Result<()> {
        let instruction=format!("用户希望修改已完成的策划文档。根据修改目的、范围、约束和当前文档判断影响范围，不要重写文档。research_needed 表示是否必须重新联网调研；列出受影响与可保留章节。\n修改请求：{}\n当前文档：{}",project.artifacts.get("revision_request").unwrap_or(&json!({})),project.artifact("document").unwrap_or(&json!({})));
        let result=self.agents.run(AgentTask::RevisionPlan,project,instruction,feedback).await?;
        self.save(project,"revision_plan",result,"revision_review",feedback)
    }

    async fn framework(&self, project: &Project, feedback: &str) -> Result<()> {
        if !project.approved("research") { return Err(AppError::Workflow("调研意见尚未由用户确认".into())); }
        let instruction=format!("根据用户已确认的调研意见形成可执行 GDD 框架。\n{}\n修改影响计划（如有）：{}\n章节 id 使用稳定英文短横线格式，输出 Framework JSON。",project.artifact("research").unwrap_or(&json!({})),project.artifact("revision_plan").unwrap_or(&json!({})));
        let result=self.agents.run(AgentTask::Framework,project,instruction,feedback).await?;
        self.save(project,"framework",result,"framework_review",feedback)
    }

    async fn document(&self, project: &Project, feedback: &str) -> Result<()> {
        if !project.approved("framework") { return Err(AppError::Workflow("文档框架尚未由用户确认".into())); }
        let evidence=self.repo.evidence_list(&project.id,100)?;
        let evidence_json=serde_json::to_string(&evidence)?;
        let evidence_text=crate::paths::truncate_utf8(&evidence_json,45000);
        let old=self.bounded_document(project)?;
        if old.is_empty() {
            let instruction=format!("依据已确认框架撰写完整 GDD，不做提纲式占位。\n{}\n已登记检索证据（优先使用其 URL，引用格式 [标题](URL)）：{}\n关键参考对象至少提供一张真实参考截图，用 image_search 和 select_image；核心循环至少一张自绘流程图。每张图紧邻解释它的正文，写图号、标题与来源。输出 DocumentDraft JSON，markdown 为完整文档。",project.artifact("framework").unwrap_or(&json!({})),evidence_text);
            let result=self.agents.run(AgentTask::Document,project,instruction,feedback).await?;
            return self.save(project,"document",result,"interaction",feedback);
        }
        // Revision of an existing document: patch mode — output only changed
        // sections so output size never scales with document length.
        let headings=extract_headings(&old);
        let empty_request=json!({});
        let revision_request=project.artifacts.get("revision_request").unwrap_or(&empty_request);
        let instruction=format!(
            "这是对既有 GDD 的修订。只输出 DocumentPatches JSON：patches 数组，每项 {{heading, markdown}}。\nheading 必须逐字取自下方章节清单；markdown 为该章节的完整替换内容（以该标题行开头，到下一同级或更高级标题为止）。只输出需要修改的章节，未受影响章节不要出现；确需新增章节时，heading 使用新标题并把补丁插入位置说明写进 markdown 首行注释。\n修改要求：{}\n受影响章节：{}\n章节清单：{}\n现有全文（仅供理解上下文，禁止整篇输出）：{old}",
            serde_json::to_string(project.artifacts.get("revision_request").unwrap_or(&empty_request))?,
            serde_json::to_string(revision_request.get("affected_sections").unwrap_or(&json!([])))?,
            serde_json::to_string(&headings)?,
        );
        let result=self.agents.run(AgentTask::DocumentPatch,project,instruction,feedback).await?;
        let plan:crate::domain::DocumentPatches=serde_json::from_value(result.clone())?;
        let merged=apply_patches(&old,&plan.patches)?;
        self.ensure_size(&merged)?;
        self.save(project,"document",json!({"markdown":merged}),"interaction",feedback)
    }

    async fn interaction(&self, project: &Project, feedback: &str) -> Result<()> {
        let document=self.bounded_document(project)?;
        let instruction=format!(
            "审阅以下 GDD 全文，撰写「交互设计补充」章节：覆盖交互入口、输入方式与平台差异、界面状态（默认/选中/执行中/成功/失败/不可用/空状态）、操作路径、反馈与防误触；至少为最关键的 1–2 个界面使用 wireframe 生成编号线框图，并在正文中解释每个编号。\n原文较长，切勿在输出中复述或改写原文的任何其他内容——只输出你新增的补充章节。\n只输出 InteractionSupplement JSON：markdown 为该章节的完整 Markdown（以 ## 标题开头）。\n全文如下：\n{document}"
        );
        let result=self.agents.run(AgentTask::InteractionSupplement,project,instruction,feedback).await?;
        let supplement:crate::domain::InteractionSupplement=serde_json::from_value(result.clone())?;
        let merged=merge_supplement(&document,&supplement.markdown);
        self.ensure_size(&merged)?;
        self.save(project,"document",json!({"markdown":merged}),"resources",feedback)?;
        self.repo.save_artifact(&project.id,"interaction",json!({"completed":true}),"")?;
        Ok(())
    }

    async fn resources(&self, project: &Project, feedback: &str) -> Result<()> {
        let document=self.bounded_document(project)?;
        // 长文档一次性穷举资源会顶爆输出上限；按章节分批提取后合并。
        // 单批仍被截断时自适应对半拆分重试，直到每批输出都能装下为止。
        let mut pending:std::collections::VecDeque<String>=split_document_chunks(&document,6000).into_iter().collect();
        let mut merged=crate::domain::ResourcePlan::default();
        let mut batch=0usize;
        while let Some(chunk)=pending.pop_front() {
            batch+=1;
            let chars=chunk.chars().count();
            let instruction=format!(
                "仅针对以下章节片段（约 {chars} 字）提取功能、美术、音效资源，输出 ResourcePlan JSON。\n注意：本文档所描述的玩法机制、规则、数值、AI 行为、输入方案与界面规格本身就是本项目的功能点，必须提取；《巫师3》《黑神话：悟空》等既有游戏才是参考对象，其原版内容不得提取。\n要求：规格与验收标准各一行以内；同一系统多个子项合并为一条资源；宁缺毋滥；\n片段：\n{chunk}"
            );
            self.repo.event(&project.id,"stage",&format!("资源拆解：开始第 {batch} 批提取（约 {chars} 字，剩余 {} 批）",pending.len()))?;
            match self.agents.run(AgentTask::Resources,project,instruction,feedback).await {
                Ok(result)=>{
                    let plan:crate::domain::ResourcePlan=serde_json::from_value(result.clone())?;
                    merged.features.extend(plan.features);
                    merged.art.extend(plan.art);
                    merged.audio.extend(plan.audio);
                    self.repo.event(&project.id,"stage",&format!("资源拆解：完成第 {batch} 批提取（剩余 {} 批）",pending.len()))?;
                },
                Err(AppError::OutputTruncated)=>{
                    match split_chunk_in_half(&chunk) {
                        Some((first,second))=>{
                            pending.push_front(second);
                            pending.push_front(first);
                            batch-=1;
                            self.repo.event(&project.id,"stage","该批内容过密导致输出截断，已对半拆分重试")?;
                        },
                        None=>return Err(AppError::OutputTruncated),
                    }
                },
                Err(error)=>return Err(error),
            }
        }
        dedupe_and_renumber(&mut merged);
        let result=serde_json::to_value(&merged)?;
        let base=document.split("\n## 附录：功能与制作资源拆解").next().unwrap_or(&document);
        self.repo.save_artifact(&project.id,"resources",result,"")?;
        self.save(project,"document",json!({"markdown":format!("{base}{}",resource_markdown(&merged))}),"review",feedback)
    }

    async fn review(&self, project: &Project, feedback: &str) -> Result<()> {
        let document=self.bounded_document(project)?;
        let evidence_json=serde_json::to_string(&self.repo.evidence_list(&project.id,100)?)?;
        let evidence=crate::paths::truncate_utf8(&evidence_json,20000);
        let instruction=format!("独立审核完整 GDD。检查目标一致性、框架覆盖、规则闭环、交互状态、图文引用、来源可追溯与资源清单。只报告有正文证据的问题；若可通过，approved=true 且 findings 为空。输出 Review JSON。\n已确认框架：{}\n文档：{}\n证据索引：{}",project.artifact("framework").unwrap_or(&json!({})),document,evidence);
        let result=self.agents.run(AgentTask::Review,project,instruction,feedback).await?;
        let review:crate::domain::Review=serde_json::from_value(result.clone())?;
        if !review.approved && review.findings.is_empty() { return Err(AppError::Workflow("审核拒绝但未给出可修订问题，未自动通过".into())); }
        self.save(project,"review",result,"review",feedback)?;
        if review.approved { self.repo.update(&project.id,Some("approved"),None,Some(None),None,None)?; }
        else if project.review_round>=MAX_REVIEW_ROUNDS { self.repo.update(&project.id,Some("needs_human"),None,Some(Some("自动修订已达 3 轮，审核仍未通过；请人工处理或明确继续。".into())),None,None)?; }
        Ok(())
    }

    async fn revise(&self, project: &Project, feedback: &str) -> Result<()> {
        let review:crate::domain::Review=serde_json::from_value(project.artifact("review").cloned().unwrap_or(json!({})))?;
        if review.approved { return Err(AppError::Workflow("审核已通过，不需要自动修订".into())); }
        let acknowledged=project.artifacts.get("review_acknowledged").and_then(Value::as_bool).unwrap_or(false);
        // 人工知悉架构类问题后，允许再进行一轮针对性自动修订（每次知悉 = 一次明确授权）。
        if !acknowledged && project.review_round>=MAX_REVIEW_ROUNDS { return Err(AppError::Workflow("自动修订已达 3 轮，请人工处理".into())); }
        let architecture_findings=review.findings.iter().any(|f|matches!(f.owner,Role::Research|Role::Architect));
        if architecture_findings && !acknowledged {
            return Err(AppError::Workflow("问题涉及调研或架构，请人工调整已确认意见/框架；若决定带病继续，可点击「架构类问题已人工知悉」跳过此类问题修订其余项".into()));
        }
        let mut document=self.bounded_document(project)?;
        for role in [Role::Writer,Role::Interaction,Role::Producer] {
            let findings:Vec<_>=review.findings.iter().filter(|finding|finding.owner==role).collect();
            if findings.is_empty() { continue; }
            let findings_json=serde_json::to_string(&findings)?;
            if role==Role::Producer {
                let instruction=format!("依据正文和以下审核问题更新资源清单，输出 ResourcePlan JSON。\n问题：{findings_json}\n正文：{document}");
                let result=self.agents.run(AgentTask::Resources,project,instruction,feedback).await?;
                let plan:crate::domain::ResourcePlan=serde_json::from_value(result.clone())?;
                self.repo.save_artifact(&project.id,"resources",result,"")?;
                document=format!("{}{}",document.split("\n## 附录：功能与制作资源拆解").next().unwrap_or(&document),resource_markdown(&plan));
            } else if role==Role::Writer {
                // Patch mode: only output replaced sections, never the whole document.
                let headings=extract_headings(&document);
                let instruction=format!(
                    "仅修复以下审核问题。只输出 DocumentPatches JSON：patches 数组，每项 {{heading, markdown}}；heading 逐字取自下方章节清单，markdown 为该章节完整替换内容。未受影响章节不要输出。\n问题：{findings_json}\n章节清单：{}\n现有全文：{document}",
                    serde_json::to_string(&headings)?
                );
                let result=self.agents.run(AgentTask::DocumentPatch,project,instruction,feedback).await?;
                let plan:crate::domain::DocumentPatches=serde_json::from_value(result.clone())?;
                document=apply_patches(&document,&plan.patches)?;
            } else {
                let instruction=format!(
                    "仅修复以下审核问题，输出 InteractionSupplement JSON：markdown 为更新后的交互补充章节完整 Markdown（## 开头），不要复述原文。\n问题：{findings_json}\n现有全文：{document}"
                );
                let result=self.agents.run(AgentTask::InteractionSupplement,project,instruction,feedback).await?;
                let supplement:crate::domain::InteractionSupplement=serde_json::from_value(result.clone())?;
                document=replace_supplement(&document,&supplement.markdown);
            }
            self.ensure_size(&document)?;
        }
        self.repo.save_artifact(&project.id,"document",json!({"markdown":document}),"")?;
        fs::write(project.working_dir()?.join("document.md"),&document)?;
        self.repo.update(&project.id,None,None,None,None,Some(project.review_round+1))?;
        self.review(&self.repo.require(&project.id)?,feedback).await
    }

    pub async fn feedback(&self, project_id: &str, stage: &str, request: &GateRequest) -> Result<Project> {
        let project=self.repo.require(project_id)?;
        let expected=match stage { "research"=>"research_review","framework"=>"framework_review","revision_plan"=>"revision_review",_=>return Err(AppError::Workflow("该阶段不支持反馈修改".into())) };
        if project.status!=expected { return Err(AppError::Workflow("当前状态不接受此阶段反馈".into())); }
        if let Some(content)=&request.content {
            let value:Value=serde_json::from_str(content)?;
            self.save(&project,stage,value,expected,&request.feedback)?;
            return self.repo.require(project_id);
        }
        self.execute(project_id,stage,&request.feedback).await
    }

    pub fn approve(&self, project_id: &str, stage: &str) -> Result<String> {
        let project=self.repo.require(project_id)?;
        let expected=match stage { "research"=>"research_review","framework"=>"framework_review","revision_plan"=>"revision_review",_=>return Err(AppError::Workflow("当前阶段不可确认或前置条件尚未满足".into())) };
        if project.status!=expected { return Err(AppError::Workflow("当前阶段不可确认或前置条件尚未满足".into())); }
        self.repo.approve(project_id,stage)?;
        self.repo.event(project_id,"gate",&format!("用户确认 {stage}"))?;
        let next=match stage {
            "research"=>"framework",
            "framework"=>"document",
            "revision_plan"=>if project.artifact("revision_plan").and_then(|v|v.get("research_needed")).and_then(Value::as_bool).unwrap_or(false) { "research" } else { "framework" },
            _=>return Err(AppError::Workflow("未知阶段".into())),
        };
        if stage=="revision_plan" && next=="framework" { self.repo.approve(project_id,"research")?; }
        self.repo.update(project_id,Some(if next=="document"{"framework_approved"}else{"interrupted"}),None,Some(None),None,None)?;
        Ok(next.into())
    }

    pub fn start_revision(&self, project_id: &str, request: &RevisionRequest) -> Result<()> {
        let project=self.repo.require(project_id)?;
        if project.running() { return Err(AppError::Workflow("任务运行时不能发起修改".into())); }
        if project.artifact("document").is_none() { return Err(AppError::Workflow("尚无可修改的正文".into())); }
        let mut artifacts=project.artifacts.clone();
        artifacts["revision_request"]=serde_json::to_value(request)?;
        artifacts["base_document"]=project.artifact("document").cloned().unwrap_or_else(||json!({}));
        for stage in ["research","framework","document","interaction","resources","review","revision_plan"] {
            if let Some(artifact)=artifacts.get_mut(stage) { artifact["approved"]=json!(false); }
        }
        artifacts["review_acknowledged"]=json!(false);
        self.repo.update(project_id,Some("revision_planning"),Some(artifacts),Some(None),Some(project.revision+1),Some(0))?;
        self.repo.save_artifact(project_id,"revision_request",serde_json::to_value(request)?,"")?;
        Ok(())
    }

    pub fn manual_document(&self, project_id: &str, content: &str) -> Result<()> {
        let project=self.repo.require(project_id)?;
        if project.running() { return Err(AppError::Workflow("任务运行时不能手动编辑".into())); }
        if content.trim().is_empty() { return Err(AppError::Workflow("文档不能为空".into())); }
        if content.len()>2*1024*1024 { return Err(AppError::Workflow("正文超过 2 MiB，请先拆分".into())); }
        self.save(&project,"document",json!({"markdown":content}),if project.artifact("document").is_some(){"resources"}else{&project.status},"")?;
        self.repo.event(project_id,"edit","用户手动保存正文；需重新整理资源并审核")
    }
}

/// 章节标题清单（1–4 级），供补丁模型逐字引用。
fn extract_headings(markdown: &str) -> Vec<String> {
    markdown.lines().filter_map(|line| {
        let trimmed=line.trim();
        if !trimmed.starts_with('#') { return None; }
        let hashes=trimmed.chars().take_while(|&c|c=='#').count();
        if hashes>4 { return None; }
        let title=trimmed[hashes..].trim();
        if title.is_empty() { return None; }
        Some(title.to_string())
    }).collect()
}

/// 在正文中定位章节标题：返回（标题行起点，内容起点，标题级别）。
fn find_section(markdown: &str, heading: &str) -> Option<(usize,usize,usize)> {
    let target=heading.trim();
    let mut offset=0;
    for line in markdown.split_inclusive('\n') {
        let trimmed=line.trim_end();
        if trimmed.starts_with('#') {
            let hashes=trimmed.chars().take_while(|&c|c=='#').count();
            let title=trimmed[hashes..].trim();
            if title==target { return Some((offset,offset+line.len(),hashes)); }
        }
        offset+=line.len();
    }
    None
}

/// 从内容起点向后扫描，返回第一个同级或更高级标题的起点；无则到文末。
fn section_end(markdown: &str, scan_from: usize, level: usize) -> usize {
    let mut offset=scan_from;
    for line in markdown[scan_from..].split_inclusive('\n') {
        let trimmed=line.trim_end();
        if trimmed.starts_with('#') {
            let hashes=trimmed.chars().take_while(|&c|c=='#').count();
            if hashes<=level { return offset; }
        }
        offset+=line.len();
    }
    markdown.len()
}

/// 把交互补充章节合并进正文：优先插入到资源附录之前，否则追加到文末。
fn merge_supplement(document: &str, supplement: &str) -> String {
    let supplement=supplement.trim();
    if let Some(idx)=document.find("\n## 附录：功能与制作资源拆解") {
        format!("{}\n\n{}\n{}", &document[..idx], supplement, &document[idx+1..])
    } else {
        format!("{}\n\n{}", document.trim_end(), supplement)
    }
}

/// 修订交互补充：若正文已存在同名补充章节则整段替换，否则按合并规则插入。
fn replace_supplement(document: &str, supplement: &str) -> String {
    let title=supplement.trim().lines().find_map(|line| {
        let trimmed=line.trim();
        trimmed.strip_prefix("## ").map(str::trim)
    });
    if let Some(title)=title {
        if let Some((start,_,level))=find_section(document,title) {
            let heading_line_len=document[start..].find('\n').map(|i|i+1).unwrap_or(document.len()-start);
            let end=section_end(document,start+heading_line_len,level);
            let mut out=String::with_capacity(document.len()+supplement.len());
            out.push_str(&document[..start]);
            out.push_str(supplement.trim());
            out.push('\n');
            out.push_str(&document[end..]);
            return out;
        }
    }
    merge_supplement(document,supplement)
}

/// 按二级章节把正文切成片段（前言并入第一批），再贪心合并至不超过 max_chars。
fn split_document_chunks(document: &str, max_chars: usize) -> Vec<String> {
    let mut sections:Vec<String>=Vec::new();
    for line in document.split_inclusive('\n') {
        if sections.is_empty() || line.starts_with("## ") {
            sections.push(line.to_string());
        } else if let Some(last)=sections.last_mut() {
            last.push_str(line);
        }
    }
    if sections.is_empty() { sections.push(document.to_string()); }
    let mut chunks:Vec<String>=Vec::new();
    for section in &sections {
        match chunks.last_mut() {
            Some(last) if last.chars().count()+section.chars().count()<=max_chars => last.push_str(section),
            _=>chunks.push(section.clone()),
        }
    }
    if chunks.is_empty() { chunks.push(document.to_string()); }
    chunks
}

/// 把被截断的批次沿最近的 ###/#### 边界对半拆开；过短或无法拆分时放弃。
fn split_chunk_in_half(chunk: &str) -> Option<(String,String)> {
    if chunk.len()<800 { return None; }
    let mid=chunk.len()/2;
    let mut boundary=None;
    let mut best_distance=usize::MAX;
    let candidates=chunk.match_indices("\n### ").chain(chunk.match_indices("\n#### ")).map(|(index,_)|index);
    for index in candidates {
        let distance=index.abs_diff(mid);
        if distance<best_distance && index>200 && chunk.len()-index>200 {
            best_distance=distance; boundary=Some(index+1);
        }
    }
    if boundary.is_none() {
        for index in chunk.match_indices("\n\n").map(|(index,_)|index) {
            let distance=index.abs_diff(mid);
            if distance<best_distance && index>200 && chunk.len()-index>200 {
                best_distance=distance; boundary=Some(index+2);
            }
        }
    }
    let index=boundary?;
    Some((chunk[..index].to_string(),chunk[index..].to_string()))
}

/// 合并各批提取结果：按（名称+来源章节）去重，再按类别重新编号保证 ID 唯一。
fn dedupe_and_renumber(plan: &mut crate::domain::ResourcePlan) {
    fn dedupe(items: Vec<crate::domain::ResourceItem>, prefix: &str) -> Vec<crate::domain::ResourceItem> {
        let mut seen=std::collections::HashSet::new();
        let mut result=Vec::new();
        for mut item in items {
            let key=(item.name.trim().to_string(),item.source_section.trim().to_string());
            if !seen.insert(key) { continue; }
            item.id=String::new();
            result.push(item);
        }
        for (index,item) in result.iter_mut().enumerate() { item.id=format!("{prefix}-{:03}",index+1); }
        result
    }
    plan.features=dedupe(std::mem::take(&mut plan.features),"F");
    plan.art=dedupe(std::mem::take(&mut plan.art),"A");
    plan.audio=dedupe(std::mem::take(&mut plan.audio),"S");
}

/// 应用章节补丁：每个补丁按标题定位并整段替换；找不到标题直接报错，绝不静默丢失。
fn apply_patches(document: &str, patches: &[crate::domain::DocumentPatch]) -> Result<String> {
    if patches.is_empty() { return Err(AppError::Workflow("模型未返回任何章节补丁".into())); }
    let mut md=document.to_string();
    for patch in patches {
        let heading=patch.heading.trim();
        if heading.is_empty() { return Err(AppError::Workflow("补丁缺少章节标题".into())); }
        if let Some((start,content_start,level))=find_section(&md,heading) {
            let end=section_end(&md,content_start,level);
            let mut replacement=patch.markdown.trim().to_string();
            replacement.push('\n');
            md.replace_range(start..end,&replacement);
        } else {
            return Err(AppError::Workflow(format!("未在正文中找到待修改章节「{}」，未应用任何补丁",heading)));
        }
    }
    Ok(md)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patches_replace_targeted_sections_only() {
        let doc="# T\n\n## A\n\nold a\n\n### A1\n\ndeep\n\n## B\n\nold b\n";
        let patches=vec![crate::domain::DocumentPatch{heading:"A".into(),markdown:"## A\n\nnew a\n".into()}];
        let out=apply_patches(doc,&patches).unwrap();
        assert!(out.contains("new a") && out.contains("old b") && out.contains("## B"));
        assert!(!out.contains("old a") && !out.contains("deep"));
        assert!(apply_patches(doc,&[]).is_err());
    }

    #[test]
    fn unknown_patch_heading_fails_loudly() {
        let doc="# T\n\n## A\n\nold\n";
        let patches=vec![crate::domain::DocumentPatch{heading:"不存在".into(),markdown:"x".into()}];
        assert!(apply_patches(doc,&patches).is_err());
    }

    #[test]
    fn supplement_inserts_before_appendix() {
        let doc="# T\n\nbody\n\n## 附录：功能与制作资源拆解\n\n| x |\n";
        let out=merge_supplement(doc,"## 交互设计补充\n\nwireframe");
        assert!(out.find("交互设计补充").unwrap()<out.find("附录：功能").unwrap());
        assert!(out.ends_with("| x |\n"));
    }

    #[test]
    fn replace_supplement_updates_existing_block() {
        let doc="# T\n\n## 交互设计补充\n\nold ix\n\n## 附录\n\nrows\n";
        let out=replace_supplement(doc,"## 交互设计补充\n\nnew ix");
        assert!(out.contains("new ix") && !out.contains("old ix") && out.contains("rows"));
    }

    #[test]
    fn heading_extraction_and_section_bounds() {
        let heads=extract_headings("# T\n## A\n### A1\n#### deep\n");
        assert_eq!(heads,vec!["T","A","A1","deep"]);
        let doc="# T\n\n## A\n\na\n\n### A1\n\ndeep\n\n## B\n\nb\n";
        let (_,content_start,level)=find_section(doc,"A").unwrap();
        assert_eq!(section_end(doc,content_start,level),doc.find("## B").unwrap());
    }

    #[test]
    fn chunking_groups_small_sections_and_respects_limit() {
        let doc="# 标题\n\n前言。\n\n## 一\n\n内容一。\n\n## 二\n\n内容二。\n\n## 三\n\n内容三比较长一些。\n";
        let chunks=split_document_chunks(doc,40);
        assert!(chunks.len()>=2);
        let joined=chunks.join("");
        for key in ["前言。","内容一。","内容二。","内容三比较长一些。"] {
            assert!(joined.contains(key),"chunking lost content: {key}");
        }
        for chunk in &chunks {
            assert!(chunk.chars().count()<=60,"chunk too large: {}",chunk.chars().count());
        }
        assert_eq!(split_document_chunks("short",6000).len(),1);
    }

    #[test]
    fn split_chunk_in_half_finds_nearest_subsection_boundary() {
        let filler="填充内容以超过最小拆分长度。".repeat(30);
        let doc=format!("# T\n\n## 一\n\n{filler}\n\n### 一甲\n\n{filler}\n\n## 二\n\n{filler}\n\n### 二甲\n\n{filler}\n");
        let (first,second)=split_chunk_in_half(&doc).unwrap();
        assert!(first.contains("一甲") || first.contains("## 一"));
        assert!(second.contains("二甲") || second.contains("## 二"));
        assert_eq!(format!("{first}{second}"),doc);
        // 过短内容直接放弃，交由上层报错
        assert!(split_chunk_in_half("短文本").is_none());
    }

    #[test]
    fn dedupe_renames_and_keeps_first() {
        use crate::domain::{ResourceItem,ResourcePlan};
        let item=|id:&str,name:&str,section:&str| ResourceItem{id:id.into(),name:name.into(),specification:"s".into(),source_section:section.into(),dependencies:"".into(),priority:crate::domain::Priority::P1,acceptance:"a".into()};
        let mut plan=ResourcePlan{
            features:vec![item("F-001","采集","核心循环"),item("F-002","采集","核心循环"),item("F-003","制作","核心循环")],
            art:vec![item("A-009","采集界面","交互")],
            audio:vec![],
        };
        dedupe_and_renumber(&mut plan);
        assert_eq!(plan.features.iter().map(|i|i.id.as_str()).collect::<Vec<_>>(),vec!["F-001","F-002"]);
        assert_eq!(plan.art[0].id,"A-001");
    }
}
