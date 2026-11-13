from __future__ import annotations

import hashlib
import ipaddress
import socket
from pathlib import Path
from urllib.parse import urlparse

import httpx
from PIL import Image, UnidentifiedImageError

from ..tools import ToolError

MAX_BYTES = 12 * 1024 * 1024
MAX_PIXELS = 40_000_000


def public_https_url(url: str) -> None:
    parsed = urlparse(url)
    if parsed.scheme != "https" or not parsed.hostname or parsed.username or parsed.password:
        raise ToolError("图片仅允许不带凭据的 HTTPS 地址")
    try:
        addresses = socket.getaddrinfo(parsed.hostname, parsed.port or 443, type=socket.SOCK_STREAM)
        ips = {entry[4][0] for entry in addresses}
    except socket.gaierror as exc:
        raise ToolError("图片域名无法解析") from exc
    for value in ips:
        ip = ipaddress.ip_address(value)
        if not ip.is_global:
            raise ToolError("图片地址指向本机、内网或保留网络，已拒绝")


def download_image(url: str, destination: Path, client: httpx.Client | None = None) -> dict:
    """Check every redirect hop, stream with a byte cap, and decode the actual image."""
    public_https_url(url)
    own = client is None
    client = client or httpx.Client(timeout=httpx.Timeout(25, connect=8), follow_redirects=False)
    digest = hashlib.sha256()
    size = 0
    suffix = ".img"
    try:
        with client.stream("GET", url, headers={"User-Agent": "GDDHarness/0.1 (local research)"}) as response:
            if response.status_code in (301, 302, 303, 307, 308):
                location = response.headers.get("location", "")
                target = str(response.url.join(location))
                public_https_url(target)
                response.close()
                with client.stream("GET", target, headers={"User-Agent": "GDDHarness/0.1 (local research)"}) as final:
                    return _write(final, destination, digest)
            return _write(response, destination, digest)
    except httpx.RequestError as exc:
        raise ToolError(f"图片下载失败：{type(exc).__name__}") from exc
    finally:
        if own:
            client.close()


def _write(response: httpx.Response, destination: Path, digest) -> dict:
    if response.status_code != 200:
        raise ToolError(f"图片下载失败 HTTP {response.status_code}")
    content_type = response.headers.get("content-type", "").split(";")[0].lower()
    formats = {"image/jpeg": ".jpg", "image/png": ".png", "image/webp": ".webp"}
    if content_type not in formats:
        raise ToolError(f"不支持的图片类型：{content_type or '未知'}")
    temp = destination.with_suffix(".download")
    temp.parent.mkdir(parents=True, exist_ok=True)
    size = 0
    try:
        with temp.open("wb") as file:
            for chunk in response.iter_bytes():
                size += len(chunk)
                if size > MAX_BYTES:
                    raise ToolError("图片超过 12 MB 限制")
                digest.update(chunk)
                file.write(chunk)
        Image.MAX_IMAGE_PIXELS = MAX_PIXELS
        with Image.open(temp) as image:
            image.verify()
        with Image.open(temp) as image:
            width, height = image.size
            fmt = image.format
            if fmt not in ("JPEG", "PNG", "WEBP"):
                raise ToolError("图片内容格式不受支持")
        final = destination.with_suffix(formats[content_type])
        temp.replace(final)
        return {"path": str(final), "sha256": digest.hexdigest(), "bytes": size,
                "width": width, "height": height, "format": fmt}
    except (UnidentifiedImageError, Image.DecompressionBombError) as exc:
        temp.unlink(missing_ok=True)
        raise ToolError("图片内容无法安全解析") from exc
    except Exception:
        temp.unlink(missing_ok=True)
        raise
