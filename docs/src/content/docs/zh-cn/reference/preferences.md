---
title: 偏好设置
description: 偏好设置窗口中的每一项设置，按类别列出。
---

用 `Ctrl` + `,`（macOS 为 `Cmd` + `,`）或从菜单打开偏好设置。修改立即
生效并自动保存；**Reset to Defaults** 恢复全部默认值，但保留最近文件
列表。

## General（通用）

- **Object snapping**——拖动时把图形和形状吸附到参考线（也可在工具栏
  中切换）。
- **Project backup copies**——在每个项目旁以隐藏文件保留指定数量的完整
  历史保存。每份副本可能与项目一样大；选 Off 关闭。
- **Automatic updates** 与 **Update channel**——见
  [更新](/zh-cn/reference/updates/)。此区域还显示已安装版本、
  **Check now** 按钮，以及更新就绪后的 **Restart now**。

## Appearance（外观）

- **Chrome theme**——浅色、深色或跟随系统。它设置的是应用窗口的外观；
  图形本身的外观由各画布的画布主题决定。
- **UI scale**——界面文字和控件的大小，按显示器分别设置。自动模式根据
  显示器报告的像素密度选择物理上可读的尺寸；手动选项和
  `Ctrl` + `+` / `Ctrl` + `-` 快捷键只覆盖当前显示器。
- **Graphics processor**——PlotX 启动时申请的 GPU 类别；重启后生效。
  仅在多 GPU 机器上出现渲染问题时才需要改动。

## Export（导出）

- **Embed view snapshots**——把每个图的屏幕视图保存进 `.plotx` 文件。
- **Raster resolution**——位图导出的默认像素密度（72–1200 dpi）。

## Recent（最近）

最近打开的文件、文件夹和项目——与 **File → Open Recent** 及欢迎页相同
的列表——并提供 **Clear recent files** 按钮。
