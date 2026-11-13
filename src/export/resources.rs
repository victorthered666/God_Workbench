use crate::domain::ResourcePlan;

pub fn resource_markdown(plan: &ResourcePlan) -> String {
    let mut result=String::from("\n## 附录：功能与制作资源拆解\n\n> 本清单由初级项目经理依据当前正文整理；排期与成本需项目负责人另行评估。\n\n");
    for (title,items) in [("功能点",&plan.features),("美术资源",&plan.art),("音效资源",&plan.audio)] {
        result.push_str(&format!("### {title}\n\n| ID | 名称 | 规格 | 来源章节 | 依赖 | 优先级 | 验收标准 |\n| --- | --- | --- | --- | --- | --- | --- |\n"));
        if items.is_empty() { result.push_str("| — | 当前正文未提取到此类资源 | 待确认 | — | — | — | 待补充正文后复核 |\n"); }
        for item in items {
            let priority=match item.priority { crate::domain::Priority::P0=>"P0",crate::domain::Priority::P1=>"P1",crate::domain::Priority::P2=>"P2" };
            let values=[&item.id,&item.name,&item.specification,&item.source_section,&item.dependencies,&priority.to_string(),&item.acceptance];
            result.push_str(&format!("| {} |\n",values.map(|value|value.replace('|',"\\|").replace('\n'," ")).join(" | ")));
        }
        result.push('\n');
    }
    result
}
