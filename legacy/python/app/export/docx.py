from __future__ import annotations

import re
from datetime import datetime
from pathlib import Path

from docx import Document
from docx.enum.section import WD_SECTION
from docx.enum.style import WD_STYLE_TYPE
from docx.enum.table import WD_CELL_VERTICAL_ALIGNMENT
from docx.enum.text import WD_ALIGN_PARAGRAPH
from docx.oxml import OxmlElement
from docx.oxml.ns import qn
from docx.shared import Cm, Pt, RGBColor
from PIL import Image

from .cover import build_cover_r4


def shade(cell, color: str) -> None:
    props = cell._tc.get_or_add_tcPr()
    fill = OxmlElement("w:shd")
    fill.set(qn("w:fill"), color)
    props.append(fill)


def margins(table) -> None:
    props = table._tbl.tblPr
    node = OxmlElement("w:tblCellMar")
    for side in ("top", "left", "bottom", "right"):
        child = OxmlElement(f"w:{side}")
        child.set(qn("w:w"), "100")
        child.set(qn("w:type"), "dxa")
        node.append(child)
    props.append(node)


def no_row_split(row, header: bool = False) -> None:
    props = row._tr.get_or_add_trPr()
    props.append(OxmlElement("w:cantSplit"))
    if header:
        props.append(OxmlElement("w:tblHeader"))


def text(paragraph, content: str, bold: bool = False, color: str = "24373B", size: float = 10.5) -> None:
    for index, part in enumerate(re.split(r"(\*\*.*?\*\*|\*[^*]+\*)", content)):
        if not part:
            continue
        run = paragraph.add_run(part.strip("*"))
        run.bold = bold or part.startswith("**")
        run.italic = part.startswith("*") and not part.startswith("**")
        run.font.name = "Microsoft YaHei"
        run._element.rPr.rFonts.set(qn("w:eastAsia"), "Microsoft YaHei")
        run.font.size = Pt(size)
        run.font.color.rgb = RGBColor.from_string(color)


def toc(document: Document) -> None:
    paragraph = document.add_paragraph()
    run = paragraph.add_run()
    begin = OxmlElement("w:fldChar"); begin.set(qn("w:fldCharType"), "begin")
    instr = OxmlElement("w:instrText"); instr.set(qn("xml:space"), "preserve"); instr.text = 'TOC \\o "1-3" \\h \\z \\u'
    separate = OxmlElement("w:fldChar"); separate.set(qn("w:fldCharType"), "separate")
    end = OxmlElement("w:fldChar"); end.set(qn("w:fldCharType"), "end")
    for node in (begin, instr, separate, end):
        run._r.append(node)


def render_docx(title: str, markdown: str, root: Path, output: Path) -> None:
    document = Document()
    settings = document.settings._element
    update = OxmlElement("w:updateFields")
    update.set(qn("w:val"), "true")
    settings.append(update)
    section = document.sections[0]
    section.page_height, section.page_width = Cm(29.7), Cm(21)
    section.top_margin, section.bottom_margin = Cm(0), Cm(0)
    section.left_margin, section.right_margin = Cm(0), Cm(0)
    styles = document.styles
    normal = styles["Normal"]
    normal.font.name = "Microsoft YaHei"
    normal._element.rPr.rFonts.set(qn("w:eastAsia"), "Microsoft YaHei")
    normal.font.size = Pt(10.5)
    normal.font.color.rgb = RGBColor.from_string("24373B")
    normal.paragraph_format.line_spacing = 1.3
    normal.paragraph_format.space_after = Pt(7)
    for level, size in ((1, 22), (2, 16), (3, 12)):
        style = styles[f"Heading {level}"]
        style.font.name = "SimHei"
        style._element.rPr.rFonts.set(qn("w:eastAsia"), "SimHei")
        style.font.size = Pt(16 if level == 1 else 15 if level == 2 else 14)
        style.font.bold = True
        style.font.color.rgb = RGBColor.from_string("153C42")
        style.paragraph_format.space_before = Pt(18 if level == 1 else 12)
        style.paragraph_format.space_after = Pt(8)
        style.paragraph_format.keep_with_next = True
    if "Figure Caption" not in styles:
        style = styles.add_style("Figure Caption", WD_STYLE_TYPE.PARAGRAPH)
        style.font.name = "Microsoft YaHei"
        style._element.rPr.rFonts.set(qn("w:eastAsia"), "Microsoft YaHei")
        style.font.size = Pt(9)
        style.font.color.rgb = RGBColor.from_string("5F7474")
        style.paragraph_format.space_after = Pt(12)
    footer = section.footer.paragraphs[0]
    footer.alignment = WD_ALIGN_PARAGRAPH.CENTER
    text(footer, f"{title}  ·  ", color="6E8080", size=8)
    run = footer.add_run()
    field = OxmlElement("w:fldSimple"); field.set(qn("w:instr"), "PAGE")
    run._r.addnext(field)

    build_cover_r4(document, title, "1.0", f"{datetime.now():%Y-%m-%d}", text)
    body_section = document.add_section(WD_SECTION.NEW_PAGE)
    body_section.top_margin, body_section.bottom_margin = Cm(2.2), Cm(2)
    body_section.left_margin, body_section.right_margin = Cm(2.4), Cm(2.4)
    p = document.add_paragraph(); text(p, "目录", bold=True, color="153C42", size=24)
    toc(document)
    p = document.add_paragraph(); text(p, "打开 Word / WPS 后，请右键目录选择“更新域”以刷新页码。", color="6E8080", size=9)
    p = document.add_paragraph()
    p.paragraph_format.page_break_before = True
    text(p, "正文", color="FFFFFF", size=1)

    lines = markdown.replace("\r\n", "\n").split("\n")
    i = 0
    figure = 0
    while i < len(lines):
        line = lines[i].strip()
        if not line:
            i += 1
            continue
        if line.startswith("|") and i+1 < len(lines) and re.match(r"^\|[\s:|-]+\|$", lines[i+1].strip()):
            headers = [c.strip() for c in line.strip("|").split("|")]
            i += 2
            rows = []
            while i < len(lines) and lines[i].strip().startswith("|"):
                rows.append([c.strip() for c in lines[i].strip().strip("|").split("|")])
                i += 1
            table = document.add_table(rows=1, cols=len(headers))
            table.style = "Table Grid"
            margins(table)
            for j, value in enumerate(headers):
                shade(table.rows[0].cells[j], "153C42")
                p = table.rows[0].cells[j].paragraphs[0]
                text(p, value, bold=True, color="FFFFFF", size=8.5)
            no_row_split(table.rows[0], True)
            for row_index, values in enumerate(rows):
                cells = table.add_row().cells
                for j, cell in enumerate(cells):
                    if row_index % 2 == 0:
                        shade(cell, "F0F4EF")
                    text(cell.paragraphs[0], values[j] if j < len(values) else "", size=8.5)
                no_row_split(table.rows[-1])
            document.add_paragraph()
            continue
        image = re.match(r"^!\[([^\]]*)\]\(([^)]+)\)$", line)
        if image:
            figure += 1
            path = (root / image.group(2)).resolve()
            if root.resolve() not in path.parents or not path.is_file():
                p = document.add_paragraph(); text(p, f"[缺失图片：{image.group(2)}]", color="A34430")
            else:
                with Image.open(path) as im:
                    width, height = im.size
                usable = 16.0
                shown_height = usable * height / width
                if shown_height > 18:
                    usable = 18 * width / height
                document.add_picture(str(path), width=Cm(usable))
                document.paragraphs[-1].alignment = WD_ALIGN_PARAGRAPH.CENTER
                p = document.add_paragraph(style="Figure Caption")
                p.alignment = WD_ALIGN_PARAGRAPH.CENTER
                text(p, f"图 {figure}  {image.group(1)}", color="5F7474", size=9)
            i += 1
            continue
        heading = re.match(r"^(#{1,3})\s+(.+)", line)
        if heading:
            document.add_heading(heading.group(2), level=len(heading.group(1)))
        elif re.match(r"^[-*]\s+", line):
            p = document.add_paragraph(style="List Bullet")
            text(p, re.sub(r"^[-*]\s+", "", line))
        elif re.match(r"^\d+[.)]\s+", line):
            p = document.add_paragraph(style="List Number")
            text(p, re.sub(r"^\d+[.)]\s+", "", line))
        elif line.startswith(">"):
            p = document.add_paragraph()
            p.paragraph_format.left_indent = Cm(0.6)
            text(p, line.lstrip("> "), color="58716C", size=9)
        elif re.match(r"^https?://\S+$", line):
            p = document.add_paragraph(); p.add_run(line)
        else:
            p = document.add_paragraph()
            p.alignment = WD_ALIGN_PARAGRAPH.JUSTIFY
            p.paragraph_format.first_line_indent = Pt(24)
            p.paragraph_format.line_spacing = Pt(15.6)
            text(p, line, color="000000", size=12)
        i += 1
    output.parent.mkdir(parents=True, exist_ok=True)
    document.save(output)
