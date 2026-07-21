---
title: 安装
description: 在 Windows、macOS 和 Linux 上安装并首次启动 PlotX。
---

PlotX 是原生桌面应用，支持 Windows、macOS 和 Linux。

## 下载与安装

预编译安装包发布在
[GitHub releases 页面](https://github.com/nmrtist/plotx/releases)。
下载对应平台的压缩包，解压到任意位置，运行其中的 `plotx` 可执行文件即可。
无需安装程序，也不需要管理员权限。

## 首次启动

由于安装包从网络下载，操作系统可能在首次运行前给出警告：

- **Windows**——若 SmartScreen 提示"无法识别的应用"，选择
  **更多信息 → 仍要运行**。
- **macOS**——若 Gatekeeper 阻止运行，右键点击应用选择**打开**并确认；
  macOS 会记住这一选择。

首次启动后 PlotX 显示欢迎页。把数据文件拖进窗口，或使用
**File → Open File…** 开始——[界面速览](/zh-cn/getting-started/quick-tour/)
会带你认识界面。

## PlotX 的设置存放在哪

偏好设置、已保存的处理模板和自定义拟合模型存放在一个较小的按用户配置
文件夹中：

| 平台 | 位置 |
| --- | --- |
| Windows | `%APPDATA%\plotx\config` |
| macOS | `~/Library/Application Support/plotx` |
| Linux | `~/.config/plotx` |

你的数据和项目从不存放在这里——它们保存在你选择的位置。删除该文件夹会
把 PlotX 恢复为默认设置。

## 更新与卸载

PlotX 会在后台检查并自动更新——见[更新](/zh-cn/reference/updates/)。
卸载时删除应用文件夹即可；若想彻底清理，可一并删除上述配置文件夹。

## 从源码构建

开发者可从代码仓库构建 PlotX，步骤见
[仓库 README](https://github.com/nmrtist/plotx#build-from-source)。使用
发行版安装包的用户无需这样做。
