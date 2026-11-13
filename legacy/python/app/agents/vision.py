from __future__ import annotations

import base64
import io
from pathlib import Path

from PIL import Image

MAX_IMAGE_EDGE = 1920
MAX_IMAGE_PIXELS = 12_000_000
MAX_ENCODED_BYTES = 2 * 1024 * 1024
MAX_VISION_PAYLOAD_BYTES = 8 * 1024 * 1024


def image_blocks(project_dir: Path, paths: list[str], limit: int = 6) -> list[dict]:
    """Encode bounded thumbnails, not unbounded originals; never persist these blocks."""
    blocks, encoded_bytes = [], 0
    limit = min(max(1, limit), 6)
    for relative in paths[:limit]:
        candidate = (project_dir / relative).resolve()
        if project_dir.resolve() not in candidate.parents or not candidate.is_file():
            continue
        try:
            with Image.open(candidate) as source:
                width, height = source.size
                if width * height > MAX_IMAGE_PIXELS:
                    continue
                image = source.convert("RGB")
                image.thumbnail((MAX_IMAGE_EDGE, MAX_IMAGE_EDGE), Image.Resampling.LANCZOS)
                output = io.BytesIO()
                quality = 85
                while True:
                    output.seek(0)
                    output.truncate()
                    image.save(output, format="JPEG", quality=quality, optimize=True)
                    if output.tell() <= MAX_ENCODED_BYTES:
                        break
                    if image.width > 640 and image.height > 640:
                        image.thumbnail((image.width * 3 // 4, image.height * 3 // 4), Image.Resampling.LANCZOS)
                    elif quality > 55:
                        quality -= 10
                    else:
                        output.close()
                        break
                encoded = output.getvalue()
                output.close()
                image.close()
        except (OSError, Image.DecompressionBombError):
            continue
        if not encoded or encoded_bytes + len(encoded) > MAX_VISION_PAYLOAD_BYTES:
            break
        encoded_bytes += len(encoded)
        blocks.append({"type": "image_url", "image_url": {
            "url": "data:image/jpeg;base64," + base64.b64encode(encoded).decode("ascii"),
        }})
    return blocks
