from __future__ import annotations

from docx.enum.table import WD_CELL_VERTICAL_ALIGNMENT, WD_ROW_HEIGHT_RULE
from docx.enum.text import WD_ALIGN_PARAGRAPH
from docx.oxml import OxmlElement
from docx.oxml.ns import qn
from docx.shared import Pt, Twips


MAX_TITLE_CHARS = 120


def calc_title_layout(title: str, max_width_twips: int, preferred: int = 40, minimum: int = 26) -> tuple[int, list[str]]:
    """R4 title sizing with strict, finite line breaking for arbitrary input."""
    if not 0 < max_width_twips <= 100_000 or not title.strip() or len(title) > MAX_TITLE_CHARS:
        raise ValueError(f"封面标题必须为 1–{MAX_TITLE_CHARS} 个字符，且页面宽度有效")
    if not 1 <= minimum <= preferred <= 100:
        raise ValueError("封面字号范围不合法")
    for size in range(preferred, minimum - 1, -2):
        per_line = max_width_twips // (size * 20)
        if per_line < 2:
            continue
        lines = split_title(title, per_line)
        if len(lines) <= 3:
            return size, lines
    return minimum, split_title(title, max(1, max_width_twips // (minimum * 20)))


def split_title(title: str, per_line: int) -> list[str]:
    if not title or per_line <= 0:
        raise ValueError("标题和标题行宽不能为空")
    per_line = max(2, min(per_line, MAX_TITLE_CHARS))
    if len(title) <= per_line:
        return [title]
    break_after = set("，。、；：！？的与和及之在于为-_—–·/ \t")
    lines, remaining = [], title
    # Each iteration must consume characters; whitespace-only fragments are dropped.
    while len(remaining) > per_line:
        index = -1
        for i in range(per_line, max(1, int(per_line * .6)) - 1, -1):
            if i < len(remaining) and remaining[i - 1] in break_after:
                index = i
                break
        if index < 0:
            index = per_line
        if not lines and index == per_line and len(title) > per_line:
            index = min(index + 1, len(remaining))
        fragment, rest = remaining[:index].strip(), remaining[index:].strip()
        if len(fragment) + len(rest) >= len(remaining) and not fragment:
            raise ValueError("标题换行未能推进")
        if fragment:
            lines.append(fragment)
        if len(rest) >= len(remaining):
            raise ValueError("标题换行未能推进")
        remaining = rest
    if remaining:
        lines.append(remaining)
    if len(lines) >= 2 and len(lines[-1]) <= 2 and len(lines[-2]) + len(lines[-1]) <= per_line:
        lines[-2] += lines.pop()
    return lines


def build_cover_r4(document, title: str, version: str, date: str, text) -> None:
    """R4: 45% dark title block, accent divider, lower metadata; 16838 wrapper."""
    size, lines = calc_title_layout(title, 9906)
    upper = max(7500, len(lines) * (size * 23 + 200) + 707 + 476 + 2300)
    top_space = max(upper - len(lines) * (size * 23 + 200) - 707 - 476 - 280 - 800, 400)
    wrapper = document.add_table(rows=1, cols=1)
    wrapper.autofit = False
    wrapper.rows[0].height = Twips(16838)
    wrapper.rows[0].height_rule = WD_ROW_HEIGHT_RULE.EXACTLY
    cell = wrapper.cell(0, 0)
    cell.vertical_alignment = WD_CELL_VERTICAL_ALIGNMENT.TOP
    cell.width = Twips(11906)
    cell.text = ""
    upper_table = cell.add_table(rows=1, cols=1)
    upper_table.autofit = False
    row = upper_table.rows[0]
    row.height = Twips(upper)
    row.height_rule = WD_ROW_HEIGHT_RULE.EXACTLY
    block = row.cells[0]
    props = block._tc.get_or_add_tcPr()
    fill = OxmlElement("w:shd"); fill.set(qn("w:fill"), "153C42"); props.append(fill)
    mar = OxmlElement("w:tcMar")
    for side, width in (("left", "1200"), ("right", "800")):
        node = OxmlElement(f"w:{side}"); node.set(qn("w:w"), width); node.set(qn("w:type"), "dxa"); mar.append(node)
    props.append(mar)
    p = block.paragraphs[0]; p.paragraph_format.space_before = Pt(top_space / 20)
    p = block.add_paragraph(); p.paragraph_format.space_after = Pt(25)
    text(p, "G A M E   D E S I G N   D O C U M E N T", color="C3D5C7", size=9)
    for i, line in enumerate(lines):
        p = block.add_paragraph()
        p.paragraph_format.line_spacing = Pt(size * 23 / 20)
        p.paragraph_format.space_after = Pt(5 if i < len(lines)-1 else 10)
        text(p, line, bold=True, color="FFFFFF", size=size)
    p = block.add_paragraph(); text(p, "系统 · 规则 · 体验 · 制作", color="D6E5DB", size=12)
    divider = cell.add_table(rows=1, cols=1)
    divider.rows[0].height = Twips(60)
    divider.rows[0].height_rule = WD_ROW_HEIGHT_RULE.EXACTLY
    shade = OxmlElement("w:shd"); shade.set(qn("w:fill"), "BF593B")
    divider.cell(0, 0)._tc.get_or_add_tcPr().append(shade)
    for _ in range(2):
        p = cell.add_paragraph(); p.paragraph_format.left_indent = Twips(1200)
    for line in ("游戏策划工作文档", f"版本  {version}    |    生成日期  {date}", "参考图仅用于设计研究，不代表取得再发布授权。"):
        p = cell.add_paragraph(); p.paragraph_format.left_indent = Twips(1200)
        text(p, line, bold=line == "游戏策划工作文档", color="BF593B" if line == "游戏策划工作文档" else "667A79", size=14 if line == "游戏策划工作文档" else 10)
