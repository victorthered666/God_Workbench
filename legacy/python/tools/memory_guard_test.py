"""Exercise memory-sensitive paths under a Windows Job Object memory cap."""
from __future__ import annotations

import ctypes
import json
import sys
from ctypes import wintypes
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))


class JobMemoryLimit:
    def __init__(self, limit: int):
        self.limit = limit

    def __enter__(self):
        if sys.platform != "win32":
            import resource
            resource.setrlimit(resource.RLIMIT_AS, (self.limit, self.limit))
            return
        self.kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        self.kernel.CreateJobObjectW.restype = wintypes.HANDLE
        self.job = self.kernel.CreateJobObjectW(None, None)
        if not self.job:
            raise ctypes.WinError(ctypes.get_last_error())

        class Basic(ctypes.Structure):
            _fields_ = [("PerProcessUserTimeLimit", ctypes.c_int64), ("PerJobUserTimeLimit", ctypes.c_int64),
                        ("LimitFlags", wintypes.DWORD), ("MinimumWorkingSetSize", ctypes.c_size_t),
                        ("MaximumWorkingSetSize", ctypes.c_size_t), ("ActiveProcessLimit", wintypes.DWORD),
                        ("Affinity", ctypes.c_size_t), ("PriorityClass", wintypes.DWORD), ("SchedulingClass", wintypes.DWORD)]

        class Counters(ctypes.Structure):
            _fields_ = [(name, ctypes.c_uint64) for name in ("ReadOperationCount", "WriteOperationCount", "OtherOperationCount", "ReadTransferCount", "WriteTransferCount", "OtherTransferCount")]

        class Extended(ctypes.Structure):
            _fields_ = [("BasicLimitInformation", Basic), ("IoInfo", Counters),
                        ("ProcessMemoryLimit", ctypes.c_size_t), ("JobMemoryLimit", ctypes.c_size_t),
                        ("PeakProcessMemoryUsed", ctypes.c_size_t), ("PeakJobMemoryUsed", ctypes.c_size_t)]

        info = Extended()
        info.BasicLimitInformation.LimitFlags = 0x2000  # JOB_OBJECT_LIMIT_PROCESS_MEMORY
        info.ProcessMemoryLimit = self.limit
        self.kernel.SetInformationJobObject.argtypes = [wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p, wintypes.DWORD]
        self.kernel.SetInformationJobObject.restype = wintypes.BOOL
        if not self.kernel.SetInformationJobObject(self.job, 9, ctypes.byref(info), ctypes.sizeof(info)):
            raise ctypes.WinError(ctypes.get_last_error())
        self.kernel.GetCurrentProcess.restype = wintypes.HANDLE
        self.kernel.AssignProcessToJobObject.argtypes = [wintypes.HANDLE, wintypes.HANDLE]
        self.kernel.AssignProcessToJobObject.restype = wintypes.BOOL
        if not self.kernel.AssignProcessToJobObject(self.job, self.kernel.GetCurrentProcess()):
            raise ctypes.WinError(ctypes.get_last_error())

    def __exit__(self, *_):
        if sys.platform == "win32":
            self.kernel.CloseHandle(self.job)


def main():
    with JobMemoryLimit(256 * 1024 * 1024):
        from app.export.cover import calc_title_layout
        for _ in range(1000):
            for width in (1, 10, 100, 9906):
                for title in ("a" * 120, "星屿采集玩法设计（离线演示）", " " * 120):
                    try:
                        calc_title_layout(title, width)
                    except ValueError:
                        pass
        from app.agents.vision import image_blocks
        from PIL import Image
        target = ROOT / "data/memory-guard"
        target.mkdir(parents=True, exist_ok=True)
        for _ in range(30):
            path = target / "large.png"
            Image.new("RGB", (2400, 1800), "#7e958d").save(path)
            blocks = image_blocks(target, ["large.png"] * 20, limit=20)
            assert len(blocks) <= 6
            assert len(json.dumps(blocks)) < 12_000_000
    print("PASS: bounded title and vision workloads within 256 MiB process memory limit")


if __name__ == "__main__":
    main()
