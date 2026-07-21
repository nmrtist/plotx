---
title: 自动化
description: 把同一个操作一次应用到多个数据集或图，或运行已保存的工作流。
---

当同一个操作需要作用于一整个系列的实验时，自动化用一次可预检的批处理
取代逐个数据集的点击。

选择**文件 → Automation…**（命令面板中也可调用）。窗口分两个标签页：

## Current Project

作用于已打开的内容。搜索并勾选需要的数据集或图——或点击 **Current
selection** 载入当前选择——再选择一个工具，点击 **Preflight** 预览会影响
哪些目标、是否有不兼容项。**Confirm and execute** 执行，整批操作合并为
一次 **Undo automation** 撤销。

## External Inputs

运行一个从磁盘文件开始的已保存工作流——例如：导入文件夹里的每个实验、
应用一个处理配方、逐个导出图形。点击 **Open workflow…** 载入，
**Validate** 校验，再点 **Confirm and run workflow**。进度会逐步显示，
较长的运行可以取消。每次完成的运行都会记录在项目中。

工作流文件是纯 JSON，也可以脱离桌面应用运行——见
[命令行](/zh-cn/reference/cli/)，它以无界面方式执行同样的工作流。工作流
与运行记录文件本身的说明见[文件格式](/zh-cn/reference/file-formats/)。
