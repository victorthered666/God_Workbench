"""Launch the local workbench without opening it to the LAN."""
import os
import subprocess
import sys
import threading
import time
import webbrowser
from pathlib import Path

ROOT = Path(__file__).resolve().parent
os.chdir(ROOT)
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from app.config import HOST, PORT


def open_when_ready():
    import httpx
    for _ in range(60):
        try:
            if httpx.get(f"http://{HOST}:{PORT}/api/health", timeout=1).status_code == 200:
                webbrowser.open(f"http://{HOST}:{PORT}")
                return
        except httpx.RequestError:
            time.sleep(0.5)


if __name__ == "__main__":
    threading.Thread(target=open_when_ready, daemon=True).start()
    import uvicorn
    uvicorn.run("app.main:app", host=HOST, port=PORT, reload=False)
