from __future__ import annotations

from ..models import ResourcePlan


def resource_markdown(plan: ResourcePlan) -> str:
    lines = ["\n## 附录：功能与制作资源拆解\n",
             "> 本清单由初级项目经理依据当前正文整理；排期与成本需项目负责人另行评估。\n"]
    for title, key in (("功能点", "features"), ("美术资源", "art"), ("音效资源", "audio")):
        lines.extend([f"### {title}\n", "| ID | 名称 | 规格 | 来源章节 | 依赖 | 优先级 | 验收标准 |",
                      "| --- | --- | --- | --- | --- | --- | --- |"])
        items = getattr(plan, key)
        if not items:
            lines.append("| — | 当前正文未提取到此类资源 | 待确认 | — | — | — | 待补充正文后复核 |")
        for item in items:
            values = [item.id, item.name, item.specification, item.source_section,
                      item.dependencies, item.priority, item.acceptance]
            lines.append("| " + " | ".join(v.replace("|", "\\|").replace("\n", " ") for v in values) + " |")
        lines.append("")
    return "\n".join(lines)
