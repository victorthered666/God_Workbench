use std::{fs, sync::Arc};
use serde_json::{json, Value};
use crate::{agent::runtime::AgentExecutor, config::{Config, MAX_REVIEW_ROUNDS}, domain::{GateRequest, Project, ProjectCreate, RevisionRequest, Role}, export::resources::resource_markdown, paths::safe_name, repository::Repository, AppError, Result};

#[derive(Clone)]
pub struct Workflow {
    pub repo: Repository,
    pub agents: Arc<dyn AgentExecutor>,
    pub config: Config,
}

impl Workflow {
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

    async fn research(&self, project: &Project, feedback: &str) -> Result<()> {
        let instruction=format!("请针对以下设计目的与参考对象完成有证据的调研。先搜索再抓取重点来源，给出可执行的撰写意见。\n设计目的：{}\n参考对象：{}\n平台：{}\n受众：{}\n最后输出 ResearchReport JSON。来源 ID 必须真实来自工具。",project.purpose,project.reference_text,if project.platform.is_empty(){"未指定"}else{&project.platform},if project.audience.is_empty(){"未指定"}else{&project.audience});
        let result=self.agents.run(Role::Research,project,instruction,feedback).await?;
        self.save(project,"research",result,"research_review",feedback)
    }

    async fn revision_plan(&self, project: &Project, feedback: &str) -> Result<()> {
        let instruction=format!("用户希望修改已完成的策划文档。根据修改目的、范围、约束和当前文档判断影响范围，不要重写文档。research_needed 表示是否必须重新联网调研；列出受影响与可保留章节。\n修改请求：{}\n当前文档：{}",project.artifacts.get("revision_request").unwrap_or(&json!({})),project.artifact("document").unwrap_or(&json!({})));
        let result=self.agents.run(Role::Architect,project,instruction,feedback).await?;
        self.save(project,"revision_plan",result,"revision_review",feedback)
    }

    async fn framework(&self, project: &Project, feedback: &str) -> Result<()> {
        if !project.approved("research") { return Err(AppError::Workflow("调研意见尚未由用户确认".into())); }
        let instruction=format!("根据用户已确认的调研意见形成可执行 GDD 框架。\n{}\n修改影响计划（如有）：{}\n章节 id 使用稳定英文短横线格式，输出 Framework JSON。",project.artifact("research").unwrap_or(&json!({})),project.artifact("revision_plan").unwrap_or(&json!({})));
        let result=self.agents.run(Role::Architect,project,instruction,feedback).await?;
        self.save(project,"framework",result,"framework_review",feedback)
    }

    async fn document(&self, project: &Project, feedback: &str) -> Result<()> {
        if !project.approved("framework") { return Err(AppError::Workflow("文档框架尚未由用户确认".into())); }
        let evidence=self.repo.evidence_list(&project.id,100)?;
        let evidence_json=serde_json::to_string(&evidence)?;
        let evidence_text=crate::paths::truncate_utf8(&evidence_json,45000);
        let old=self.bounded_document(project)?;
        let instruction=format!("依据已确认框架撰写完整 GDD，不做提纲式占位。\n{}\n已登记检索证据（优先使用其 URL，引用格式 [标题](URL)）：{}\n关键参考对象至少提供一张真实参考截图，用 image_search 和 select_image；核心循环至少一张自绘流程图。每张图紧邻解释它的正文，写图号、标题与来源。输出 DocumentDraft JSON，markdown 为完整文档。\n保留仍有效的旧版正文，仅修改受影响内容：{}",project.artifact("framework").unwrap_or(&json!({})),evidence_text,old);
        let result=self.agents.run(Role::Writer,project,instruction,feedback).await?;
        self.save(project,"document",result,"interaction",feedback)
    }

    async fn interaction(&self, project: &Project, feedback: &str) -> Result<()> {
        let document=self.bounded_document(project)?;
        let instruction=format!("审阅以下 GDD，补全交互入口、操作、状态、反馈、异常与空状态；至少为关键界面使用 wireframe 生成一张编号线框图，并用正文解释编号。保留所有原有图片及资源附录占位。输出完整 DocumentDraft JSON。\n{document}");
        let result=self.agents.run(Role::Interaction,project,instruction,feedback).await?;
        self.save(project,"document",result,"resources",feedback)?;
        self.repo.save_artifact(&project.id,"interaction",json!({"completed":true}),"")?;
        Ok(())
    }

    async fn resources(&self, project: &Project, feedback: &str) -> Result<()> {
        let document=self.bounded_document(project)?;
        let instruction=format!("依据以下实际正文，提取全部功能、美术、音效资源，输出 ResourcePlan JSON。不得把参考对象中的资源误记为本项目资源。\n{document}");
        let result=self.agents.run(Role::Producer,project,instruction,feedback).await?;
        let plan:crate::domain::ResourcePlan=serde_json::from_value(result.clone())?;
        let base=document.split("\n## 附录：功能与制作资源拆解").next().unwrap_or(&document);
        self.repo.save_artifact(&project.id,"resources",result,"")?;
        self.save(project,"document",json!({"markdown":format!("{base}{}",resource_markdown(&plan))}),"review",feedback)
    }

    async fn review(&self, project: &Project, feedback: &str) -> Result<()> {
        let document=self.bounded_document(project)?;
        let evidence_json=serde_json::to_string(&self.repo.evidence_list(&project.id,100)?)?;
        let evidence=crate::paths::truncate_utf8(&evidence_json,20000);
        let instruction=format!("独立审核完整 GDD。检查目标一致性、框架覆盖、规则闭环、交互状态、图文引用、来源可追溯与资源清单。只报告有正文证据的问题；若可通过，approved=true 且 findings 为空。输出 Review JSON。\n已确认框架：{}\n文档：{}\n证据索引：{}",project.artifact("framework").unwrap_or(&json!({})),document,evidence);
        let result=self.agents.run(Role::Architect,project,instruction,feedback).await?;
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
        if project.review_round>=MAX_REVIEW_ROUNDS { return Err(AppError::Workflow("自动修订已达 3 轮，请人工处理".into())); }
        if review.findings.iter().any(|f|matches!(f.owner,Role::Research|Role::Architect)) { return Err(AppError::Workflow("问题涉及调研或架构，请人工调整已确认意见/框架，不绕过人工门禁自动改写".into())); }
        let mut document=self.bounded_document(project)?;
        for role in [Role::Writer,Role::Interaction,Role::Producer] {
            let findings:Vec<_>=review.findings.iter().filter(|finding|finding.owner==role).collect();
            if findings.is_empty() { continue; }
            let instruction=format!("仅修复以下审核问题，保留其他已验证内容、真实图片与来源，输出符合角色要求的完整结果。\n问题：{}\n正文：{document}",serde_json::to_string(&findings)?);
            let result=self.agents.run(role,project,instruction,feedback).await?;
            if role==Role::Producer {
                let plan:crate::domain::ResourcePlan=serde_json::from_value(result.clone())?;
                self.repo.save_artifact(&project.id,"resources",result,"")?;
                document=format!("{}{}",document.split("\n## 附录：功能与制作资源拆解").next().unwrap_or(&document),resource_markdown(&plan));
            } else { document=result.get("markdown").and_then(Value::as_str).unwrap_or("").to_string(); }
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
