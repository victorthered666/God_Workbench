from __future__ import annotations

import threading
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass, field

from .workflow import Workflow


@dataclass
class CancelToken:
    cancelled: threading.Event = field(default_factory=threading.Event)

    def check(self) -> None:
        if self.cancelled.is_set():
            raise RuntimeError("任务已由用户取消；外部响应可能已经返回，不会自动重发")


class Runner:
    def __init__(self, workflow: Workflow):
        self.workflow = workflow
        self.executor = ThreadPoolExecutor(max_workers=2, thread_name_prefix="gdd-stage")
        self.running: dict[str, str] = {}
        self.tokens: dict[str, CancelToken] = {}
        self.lock = threading.Lock()

    def start(self, project_id: str, stage: str, feedback: str = "") -> None:
        with self.lock:
            if project_id in self.running:
                raise ValueError("该项目已有任务运行")
            self.tokens[project_id] = CancelToken()
            future = self.executor.submit(self._run, project_id, stage, feedback, self.tokens[project_id])
            self.running[project_id] = future
            future.add_done_callback(lambda _: self._done(project_id))

    def _run(self, project_id: str, stage: str, feedback: str, token: CancelToken) -> None:
        token.check()
        self.workflow.execute(project_id, stage, feedback)

    def _done(self, project_id: str) -> None:
        with self.lock:
            self.running.pop(project_id, None)
            self.tokens.pop(project_id, None)

    def cancel(self, project_id: str) -> None:
        with self.lock:
            token = self.tokens.get(project_id)
            future = self.running.get(project_id)
        if not future:
            raise ValueError("没有运行中的任务")
        token.cancel()
        if future.cancel():
            self.workflow.repo.update(project_id, status="interrupted", error="任务已取消")
        else:
            raise ValueError("模型或工具请求已发出，不能可靠中断；请等待返回。不会自动重发")

    def stop(self) -> None:
        self.executor.shutdown(wait=False, cancel_futures=True)
