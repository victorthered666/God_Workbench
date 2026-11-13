from __future__ import annotations

import html
import uuid
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

from ..tools import ToolError
from .network import MAX_PIXELS, download_image


def font(size: int, bold: bool = False) -> ImageFont.FreeTypeFont | ImageFont.ImageFont:
    candidates = [
        Path("C:/Windows/Fonts/msyhbd.ttc" if bold else "C:/Windows/Fonts/msyh.ttc"),
        Path("C:/Windows/Fonts/simhei.ttf"), Path("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"),
    ]
    for path in candidates:
        if path.exists():
            return ImageFont.truetype(str(path), size=size)
    return ImageFont.load_default()


class MediaStore:
    def __init__(self, project_dir: Path):
        self.root = project_dir.resolve()
        (self.root / "assets/images").mkdir(parents=True, exist_ok=True)
        (self.root / "assets/diagrams").mkdir(parents=True, exist_ok=True)

    def safe_path(self, relative: str) -> Path:
        path = (self.root / relative).resolve()
        if self.root not in path.parents:
            raise ToolError("素材路径越界")
        if not path.is_file():
            raise ToolError("素材不存在")
        return path

    def download(self, url: str, source_page: str = "") -> dict:
        metadata = download_image(url, self.root / "assets/images" / uuid.uuid4().hex)
        return {**metadata, "path": str(Path(metadata["path"]).relative_to(self.root)).replace("\\", "/"),
                "source_url": url, "source_page": source_page}

    def annotate(self, path: str, title: str, labels: list[dict], source_url: str = "", source_page: str = "") -> str:
        if not 1 <= len(labels) <= 8:
            raise ToolError("每张图片需要 1–8 个标注")
        source = self.safe_path(path)
        with Image.open(source) as raw:
            if raw.width * raw.height > MAX_PIXELS:
                raise ToolError("图片像素超出处理限制")
            if raw.width * raw.height > 12_000_000 or max(raw.size) > 3200:
                scale = min(1.0, (12_000_000 / (raw.width * raw.height)) ** 0.5, 3200 / max(raw.size))
                resized = raw.resize((max(1, int(raw.width * scale)), max(1, int(raw.height * scale))), Image.Resampling.LANCZOS)
                image = resized.convert("RGB")
                resized.close()
            else:
                image = raw.convert("RGB")
            width, height = image.size
            header = max(70, int(height * 0.12))
            canvas = Image.new("RGB", (width, height + header), "#F4F1E9")
            canvas.paste(image, (0, header))
            draw = ImageDraw.Draw(canvas)
            draw.text((22, 18), title[:100], fill="#152B32", font=font(max(22, width // 36), True))
            legend = []
            for index, label in enumerate(labels, 1):
                x, y = float(label.get("x", 0.5)), float(label.get("y", 0.5))
                if not 0 <= x <= 1 or not 0 <= y <= 1:
                    raise ToolError("标注坐标必须在 0–1 范围内")
                px, py = int(x * width), int(y * height) + header
                radius = max(18, min(width, height) // 36)
                draw.ellipse((px-radius, py-radius, px+radius, py+radius), fill="#D15D3B", outline="white", width=3)
                number = str(index)
                box = draw.textbbox((0, 0), number, font=font(radius, True))
                draw.text((px-(box[2]-box[0])/2, py-(box[3]-box[1])/2-2), number, fill="white", font=font(radius, True))
                legend.append(f"{index}. {str(label.get('text', ''))[:120]}")
            result = self.root / "assets/images" / f"{uuid.uuid4().hex}.png"
            canvas.save(result)
            rel = str(result.relative_to(self.root)).replace("\\", "/")
            canvas.close()
            image.close()
        caption = self.root / "assets/images" / f"{result.stem}.txt"
        caption.write_text("；".join(legend), encoding="utf-8")
        caption = f"![{title}]({rel})\n\n*标注：" + "；".join(legend) + "*"
        if source_url:
            caption += f"\n\n原图：[检索来源]({source_url})"
            if source_page:
                caption += f" · [来源网页]({source_page})"
            caption += "。可检索不代表取得使用授权。"
        return caption

    def flowchart(self, title: str, steps: list[str]) -> str:
        if not 2 <= len(steps) <= 8:
            raise ToolError("流程图需要 2–8 个步骤")
        width, row_height = 1200, 144
        height = 160 + row_height * len(steps) + 100
        image = Image.new("RGB", (width, height), "#F5F2EA")
        draw = ImageDraw.Draw(image)
        draw.text((64, 45), title[:100], fill="#17363B", font=font(38, True))
        for index, step in enumerate(steps):
            top = 145 + index * row_height
            fill = "#E1ECE6" if index % 2 == 0 else "#E5E9EF"
            draw.rounded_rectangle((130, top, 1070, top+92), radius=14, fill=fill, outline="#77928C", width=2)
            draw.ellipse((64, top+23, 110, top+69), fill="#CB6040")
            draw.text((79, top+28), str(index+1), fill="white", font=font(23, True))
            text = str(step)[:55]
            draw.text((170, top+28), text, fill="#183238", font=font(28))
            if index < len(steps)-1:
                draw.line((600, top+94, 600, top+134), fill="#CB6040", width=4)
                draw.polygon([(590, top+120), (610, top+120), (600, top+136)], fill="#CB6040")
        filename = f"{uuid.uuid4().hex}.png"
        image.save(self.root / "assets/diagrams" / filename)
        source = f"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\"><title>{html.escape(title)}</title><desc>{html.escape(' → '.join(steps))}</desc></svg>"
        (self.root / "assets/diagrams" / filename.replace(".png", ".svg")).write_text(source, encoding="utf-8")
        return f"![{title}](assets/diagrams/{filename})\n\n*自绘流程图：" + " → ".join(steps) + "*"

    def wireframe(self, title: str, regions: list[dict]) -> str:
        if not 2 <= len(regions) <= 8:
            raise ToolError("线框图需要 2–8 个界面区域")
        image = Image.new("RGB", (1400, 960), "#F5F2EA")
        draw = ImageDraw.Draw(image)
        draw.text((50, 36), title[:100], fill="#17363B", font=font(36, True))
        draw.rounded_rectangle((45, 110, 1355, 905), radius=20, fill="white", outline="#284B51", width=3)
        legend = []
        for index, region in enumerate(regions, 1):
            x, y = float(region.get("x", 0.1)), float(region.get("y", 0.1))
            w, h = float(region.get("w", 0.3)), float(region.get("h", 0.2))
            if not (0 <= x <= 1 and 0 <= y <= 1 and 0 < w <= 1-x and 0 < h <= 1-y):
                raise ToolError("线框图区域必须位于画面内")
            box = (75+int(x*1250), 145+int(y*710), 75+int((x+w)*1250), 145+int((y+h)*710))
            draw.rectangle(box, fill="#E6ECE9", outline="#678983", width=2)
            draw.text((box[0]+16, box[1]+14), f"{index}  {str(region.get('name', '界面区域'))[:38]}", fill="#1D3A40", font=font(25, True))
            legend.append(f"{index}. {str(region.get('interaction', region.get('name', '')))[:160]}")
        filename = f"{uuid.uuid4().hex}.png"
        image.save(self.root / "assets/diagrams" / filename)
        return f"![{title}](assets/diagrams/{filename})\n\n*交互标注：" + "；".join(legend) + "*"
