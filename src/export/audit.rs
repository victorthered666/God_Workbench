use crate::domain::AuditReport;

/// 把结构审核 + 一致性审核结果写成中文 Markdown 报告。
pub fn audit_report_markdown(title: &str, report: &AuditReport) -> String {
    let mut out=String::new();
    out.push_str(&format!("# 《{title}》文档审核报告\n\n"));
    out.push_str("> 审核对象：用户导入的已撰写文档。本报告只评价结构完整性与内部一致性，不代表对设计创意本身的评价。\n\n");

    out.push_str("## 一、结构完整性（是否从设计目的开始形成完整结构）\n\n");
    if report.structure.design_purpose.trim().is_empty() {
        out.push_str("**未能识别出明确的设计目的。** 文档缺少统领全篇的目标陈述，后续内容无法对照目的校验。\n\n");
    } else {
        out.push_str(&format!("**识别到的设计目的：** {}\n\n", report.structure.design_purpose.trim()));
    }
    if report.structure.missing_sections.is_empty() {
        out.push_str("结构缺失：未发现明显缺失的必要章节。\n\n");
    } else {
        out.push_str("**结构缺失章节：**\n\n");
        for section in &report.structure.missing_sections {
            out.push_str(&format!("- {section}\n"));
        }
        out.push('\n');
    }
    if !report.structure.comment.trim().is_empty() {
        out.push_str(&format!("**结构评述：** {}\n\n", report.structure.comment.trim()));
    }

    out.push_str("## 二、前后一致性（含与设计目的的不一致）\n\n");
    if report.consistency_findings.is_empty() {
        out.push_str("未发现明确的前后矛盾或与设计目的不一致之处。\n\n");
    } else {
        out.push_str("| # | 严重度 | 所在章节 | 问题 | 修改建议 |\n| --- | --- | --- | --- | --- |\n");
        for (index, finding) in report.consistency_findings.iter().enumerate() {
            let severity=match finding.severity { crate::domain::Severity::Low=>"低",crate::domain::Severity::Medium=>"中",crate::domain::Severity::High=>"高" };
            let cell=|text:&str| text.replace('|',"\\|").replace('\n'," ");
            out.push_str(&format!("| {} | {} | {} | {} | {} |\n", index+1, severity,
                cell(&finding.section), cell(&finding.problem), cell(&finding.recommendation)));
        }
        out.push('\n');
        out.push_str("### 问题证据（原文引用）\n\n");
        for (index, finding) in report.consistency_findings.iter().enumerate() {
            if finding.evidence.trim().is_empty() { continue; }
            out.push_str(&format!("**问题 {} 证据（{}）：**\n\n> {}\n\n", index+1,
                if finding.section.is_empty() { "未标明章节" } else { finding.section.as_str() },
                finding.evidence.trim()));
        }
    }

    out.push_str("## 三、总体结论\n\n");
    if report.summary.trim().is_empty() {
        out.push_str("（模型未给出总结；请以上述逐项发现为准。）\n");
    } else {
        out.push_str(report.summary.trim());
        out.push('\n');
    }
    out
}
