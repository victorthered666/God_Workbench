from __future__ import annotations

import os
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DATA_DIR = Path(os.getenv("GDD_DATA_DIR") or ROOT / "data").resolve()
OUTPUT_DIR = Path(os.getenv("GDD_OUTPUT_DIR") or ROOT / "outputs").resolve()
DB_PATH = DATA_DIR / "gdd.db"
HOST = "127.0.0.1"
PORT = int(os.getenv("GDD_PORT", "8765"))
MAX_SEARCH_CALLS = 24
MAX_REVIEW_ROUNDS = 3
DATA_DIR.mkdir(parents=True, exist_ok=True)
OUTPUT_DIR.mkdir(parents=True, exist_ok=True)
