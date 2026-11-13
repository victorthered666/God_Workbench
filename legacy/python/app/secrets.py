from __future__ import annotations

import json
import os
import re
from pathlib import Path

from cryptography.fernet import Fernet, InvalidToken

from .config import DATA_DIR


class SecretStore:
    """Locally encrypted settings override inherited environment variables when explicitly saved."""

    def __init__(self, directory: Path = DATA_DIR):
        self.directory = directory
        directory.mkdir(parents=True, exist_ok=True)
        self.key_file = directory / ".secret-key"
        self.file = directory / "secrets.json"
        if not self.key_file.exists():
            self.key_file.write_bytes(Fernet.generate_key())
            if os.name != "nt":
                self.key_file.chmod(0o600)
        if os.name == "nt":
            self._restrict_windows_file(self.key_file)
        self.fernet = Fernet(self.key_file.read_bytes())

    @staticmethod
    def _restrict_windows_file(path: Path) -> None:
        import subprocess
        subprocess.run(["icacls", str(path), "/inheritance:r", "/grant:r", f"{os.getlogin()}:(R,W)"],
                       capture_output=True, check=False)

    def _all(self) -> dict[str, str]:
        if not self.file.exists():
            return {}
        try:
            return json.loads(self.file.read_text(encoding="utf-8"))
        except (json.JSONDecodeError, OSError):
            return {}

    def set(self, name: str, value: str) -> None:
        if not re.fullmatch(r"[A-Z][A-Z0-9_]{1,80}", name):
            raise ValueError("密钥变量名不合法")
        secrets = self._all()
        secrets[name] = self.fernet.encrypt(value.encode()).decode()
        self.file.write_text(json.dumps(secrets), encoding="utf-8")
        if os.name == "nt":
            self._restrict_windows_file(self.file)
        elif os.name != "nt":
            self.file.chmod(0o600)

    def get(self, name: str) -> str | None:
        encrypted = self._all().get(name)
        if encrypted:
            try:
                return self.fernet.decrypt(encrypted.encode()).decode()
            except InvalidToken:
                raise ValueError("本地密钥无法解密，请重新配置") from None
        return os.getenv(name)

    def configured(self, name: str) -> bool:
        return bool(os.getenv(name) or name in self._all())

    def redact(self, text: str) -> str:
        result = re.sub(r"Bearer\s+[A-Za-z0-9._~+/=-]+", "Bearer [已隐藏]", text)
        result = re.sub(r"(?i)(api[_ ]?key|key)\s*[:=]\s*[^\s,;]+", r"\1: [已隐藏]", result)
        for name in set(self._all()) | {"DEEPSEEK_API_KEY", "WEB_SEARCH_API_KEY"}:
            value = self.get(name)
            if value and len(value) >= 6:
                result = result.replace(value, "[已隐藏]")
        return result
