---
title: 报告问题
description: 报告 PlotX 崩溃或运行问题时，找到需要附上的诊断文件。
---

## 崩溃之后

PlotX 捕获到内部错误时，会保存纯文本崩溃报告并显示完整路径。下次启动时，
恢复对话框或状态栏会再次显示该路径。请把报告附到
[GitHub issue](https://github.com/nmrtist/plotx/issues)；其中包含 PlotX
版本、平台、panic 位置、回溯以及会话日志末尾。

崩溃报告位于 PlotX 应用数据目录的 `crashes` 子目录中。PlotX 保留最近
10 份报告：

- Windows：`%LOCALAPPDATA%\plotx\data\crashes`
- macOS：`~/Library/Application Support/plotx/crashes`
- Linux：`$XDG_DATA_HOME/plotx/crashes`，未设置时为
  `~/.local/share/plotx/crashes`

## 其他问题的日志

每次启动都会在相邻的 `logs` 子目录中新建日志。PlotX 保留最近五次会话
日志。若问题没有导致崩溃，请附上发生问题那次会话的日志，并说明当时的
操作。只有在项目或输入文件不含敏感数据时才附上它们。

PlotX 不会自动上传报告或日志。
