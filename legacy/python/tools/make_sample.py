"""Create a clearly marked offline sample; no network or model calls."""
from __future__ import annotations

import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))

from app.assets.media import MediaStore
from app.export.docx import render_docx
from app.export.resources import resource_markdown
from app.models import ResourceItem, ResourcePlan


def main():
    target = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else ROOT / "data/qa-sample"
    if target.exists():
        raise SystemExit(f"Refusing to overwrite {target}; choose an empty QA directory")
    (target / "assets/images").mkdir(parents=True)
    store = MediaStore(target)
    source = target / "assets/images/mock-source.png"
    from PIL import Image, ImageDraw
    image = Image.new("RGB", (1200, 700), "#DDE6DE")
    draw = ImageDraw.Draw(image)
    draw.rectangle((40, 40, 1160, 130), fill="#193B3D")
    draw.rectangle((70, 170, 400, 610), fill="#B6C9B9")
    draw.rectangle((450, 170, 1130, 610), fill="#FAFAF4")
    image.save(source)
    reference = store.annotate("assets/images/mock-source.png", "模拟参考图（非真实游戏截图）", [
        {"x": .5, "y": .12, "text": "状态信息区域（模拟）"},
        {"x": .65, "y": .55, "text": "主要操作区域（模拟）"},
    ])
    flow = store.flowchart("核心循环（模拟图示）", ["探索场景", "收集资源", "制作工具", "扩大探索范围"])
    wire = store.wireframe("采集交互线框（模拟图示）", [
        {"x": .04, "y": .05, "w": .92, "h": .12, "name": "状态栏", "interaction": "显示体力与背包容量，状态变化即时反馈"},
        {"x": .04, "y": .23, "w": .6, "h": .64, "name": "场景操作区", "interaction": "选中资源后显示采集入口，不可用时说明原因"},
        {"x": .69, "y": .23, "w": .27, "h": .64, "name": "资源面板", "interaction": "展示采集结果，背包已满时禁止新增并提示整理"},
    ])
    body = """# 星屿采集玩法设计

> **离线模拟样例：** 本文仅用于验证 Harness 的排版与导出，不是调研 Agent 生成的实际策划，不构成游戏设计建议。

## 1. 设计目标与范围

本样例以轻量采集循环演示功能规则、图文对应与资源拆解。玩家在短时间内完成“探索—收集—制作—再探索”的完整行为闭环，所有数值均为模拟示例，未经过游戏平衡验证。

### 1.1 体验原则

- 每次操作都有明确、及时、可感知的结果。
- 不可用操作同时说明原因与解决方向。
- 玩家可以返回上一步，不因误触丢失关键资源。

## 2. 核心循环

玩家探索场景并选择资源点；满足体力与背包条件后可采集。资源进入背包后用于制作工具，新工具扩大后续探索范围。

""" + flow + """

### 2.1 采集规则

| 规则项 | 模拟规则 | 异常处理 | 验收标准 |
| --- | --- | --- | --- |
| 体力消耗 | 每次采集消耗 5 点 | 不足时禁止执行并提示恢复方式 | 连续点击不重复扣除 |
| 背包容量 | 初始 20 格 | 已满时不产出资源 | 提示原因与整理入口 |
| 采集结果 | 产出对应资源 1 份 | 取消时不改变资源数量 | 前后状态一致 |

## 3. 参考图标注

下图是完全由程序绘制的模拟界面，**不是来自网络的真实游戏截图**，只用于验证编号、图例与图片导出。

""" + reference + """

正文引用图 2：标注 1 指向状态信息区域，标注 2 指向主要操作区域。实际项目应使用真实检索图并保存原图地址、来源页和授权核查结果。

## 4. 交互设计

""" + wire + """

### 4.1 操作与状态

1. 默认状态展示可采集资源与剩余体力。
2. 选中资源后进入待确认状态，显示消耗和预计产出。
3. 执行期间锁定重复提交，完成后刷新资源数量与场景状态。
4. 失败时保持当前场景，说明体力不足或背包已满，并提供返回操作。
5. 空状态解释附近没有资源的原因，不以不可用的空白面板代替反馈。

## 5. 验收关注

重点检查采集闭环、重复点击、失败回滚、图片路径与图例是否一致，以及附录是否覆盖功能、界面美术和反馈音效。

""" + resource_markdown(ResourcePlan(
        features=[ResourceItem(id="F-001", name="资源采集", specification="选择资源、确认消耗、产出与状态刷新", source_section="2.1 采集规则", dependencies="背包、体力", priority="P0", acceptance="所有异常路径均有明确结果")],
        art=[ResourceItem(id="A-001", name="采集界面", specification="状态栏、场景操作区、资源面板与不可用状态", source_section="4 交互设计", dependencies="F-001", priority="P0", acceptance="各区域与线框编号一致")],
        audio=[ResourceItem(id="S-001", name="采集反馈音", specification="短促非循环反馈；失败音与成功音可区分", source_section="4.1 操作与状态", dependencies="F-001", priority="P1", acceptance="播放时机与状态变化同步")],
    ))
    output = target / "星屿采集玩法设计（离线模拟样例）.docx"
    render_docx("星屿采集玩法设计", body, target, output)
    (target / "星屿采集玩法设计（离线模拟样例）.md").write_text(body, encoding="utf-8")
    print(output)


if __name__ == "__main__":
    main()
