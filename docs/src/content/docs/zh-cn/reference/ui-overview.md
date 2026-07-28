---
title: 界面术语速查
description: 本手册指称 PlotX 窗口各部分时使用的名称。
---

本手册用一套固定名称指称 PlotX 窗口的各个部分。当某页写着"打开对象检
查器"而你不确定去哪里找时，这一页就是地图。
[界面速览](/zh-cn/getting-started/quick-tour/)以走查形式介绍同样的区域。

PlotX 的界面为英文；手册中加粗的英文词即界面上的原文标签。

## 窗口区域

- **主侧栏（Primary Side Bar）**——左侧面板。**Canvas** 模式列出图形、
  页面和已保存的画板视野；**Data** 模式显示每个数据集及其派生结果。
- **画布（Canvas）/ 画板（board）**——中央区域：承载图形的无限画板，
  按网格对齐的页面组织。"页面"是画板上的一块带框区域，导出时对应一张
  图。
- **副侧栏（Secondary Side Bar）**——右侧面板，承载对象检查器和针对当前
  选择的上下文分析工具。整栏作为一列滚动，因此再长的检查器也不会把下方工具
  挤出可及范围。处理不在这里，它在画布上。
- **任务 dock（Task dock）**——画布右上角的卡片，承载多步任务：Processing、
  Regions、Curve Fit 与 Statistics。同时打开两个及以上时会出现
  **Process**、**Regions**、**Fit**、**Stats** 标签，一次显示一页。其
  Processing 页对 1D 与伪 2D 数据集显示一条管线，对真 2D 谱显示两条：
  **F2 (direct)** 与 **F1 (indirect)**。参见[数据处理](/zh-cn/guides/processing/)。
- **Ribbon**——标题栏下方的命令条，按任务页签组织（**Data**、
  **Process**、**Analyze**、**Figure**、**Arrange**、**View**）。它是
  快捷入口：其上的一切也都能在菜单或命令面板中找到。
- **上下文行**——Ribbon 下方的一行，显示当前画布、对象、数据集、任务
  和工具。
- **状态栏**——底部条带，显示提示、进度和选择详情。

## 常见元素

- **任务页（task page）**——任务 dock 中某一个任务的内容页，拖动其下边缘可
  调整高度。切换标签会保留该页的设置，只有用 ✕ 关闭才会丢弃；选中标签还会把
  该页对应的数据集设为当前数据集。
- **对象检查器（Object inspector）**——所选画板对象的属性面板：图表
  类型、样式、几何，以及其绘制内容的显示设置，例如
  [等高线层级](/zh-cn/guides/contour-levels/)。常用设置直接显示，其余收在
  **Advanced**（高级）折叠区中。顶部一排小按钮——**Layout**、**Data**、
  **Type** 以及当前适用的样式分区——可直接滚动到对应分区。标题会写明当前选中
  的内容：一个对象及其数据集，或者同时选中多个对象时，有多少个对象、来自多少
  个数据集。其中 **Contour** 与 **Line** 两个区域只在所选内容确实这样绘制时才
  出现；**Figure typography** 属于文档，始终显示。
- **设置行（setting row）**——一项设置在对象检查器、Canvas settings、处理
  步骤或 Preferences 中的呈现。同一行无论出现在哪里，行为都一致。偏离默认值的
  行会显示圆点，悬停可看到默认值，旁边的重置按钮可恢复它。按
  <kbd>Ctrl</kbd>+<kbd>K</kbd> 可按名称搜索这些设置：激活结果后，PlotX 会打开
  该行所在的面板，展开收起它的折叠区，滚动到该行并高亮。当前状态下不能编辑的
  行仍然显示，只是变灰；悬停即可看到要先改哪个开关。
  多选对象时，一次编辑会应用到全部对象；若它们原本取值不一致，该行标注
  **mixed** 并以短横线代替数值，而不是拿其中一个对象的值冒充整组结果。悬停可
  看到有多少个不一致，以及此时写入一个值会产生什么效果。
- **设置分组（settings group）**——一组同属一处的相关设置。Ribbon 为每个
  分组提供一个按钮，它只负责打开这些设置的所在处，而不重复摆一套控件：
  **Figure → Style** 中的 **Contour settings**、**Line settings** 与
  **Figure typography settings**，**Process → Processing** 中的
  **Apodization settings**。画布右键菜单会列出当前适用的同一批分组，形如
  *Contour settings…*；<kbd>Ctrl</kbd>+<kbd>K</kbd> 则能找到分组里的单项
  设置。应用偏好不属于任何对象，因此画布右键菜单不列出它们：请用 **View →
  Display** 中的 **Preferences…**、命令面板，或按 <kbd>Ctrl</kbd>+<kbd>,</kbd>
  打开。
- **数据表（Data sheet）**——数据表格的电子表格视图，双击表格打开。
- **命令面板（Command palette）**——<kbd>Ctrl</kbd>+<kbd>K</kbd> 打开
  的可搜索列表，涵盖命令、设置与数据；见
  [命令面板](/zh-cn/reference/command-palette/)。
- **尺寸标签（Size chip）**——页面左上角上方的标签，显示页面尺寸和匹
  配到的期刊预设。
- **步骤行（Step row）**——当前数据集处理管线中的一个步骤。点击即可编辑参数；
  用其 ⋯ 菜单中的 **Move earlier** / **Move later** 或
  <kbd>Alt</kbd>+<kbd>↑</kbd>/<kbd>↓</kbd> 前移或后移。列表中的每一步都是真正
  会执行的步骤，FFT 也不例外；已导入的频域谱看不到 FFT 行，是因为它的配方里
  本来就没有 FFT。参见[数据处理](/zh-cn/guides/processing/)。

## 数据术语

- **数据集（Dataset）**——任何承载数据的可导入或派生对象：一条谱、
  一段记录或一张表。
- **派生数据（Derived data）**——由其他数据集产生的结果（切片、投影、
  区域表、拟合表）；数据浏览器把它们列在来源之下。
- **伪 2D（Pseudo-2D）**——在某个参数变化下采集的一叠 1D 谱（DOSY 变
  梯度强度，T1/T2 变延迟），与 COSY、HSQC 这类**真 2D** 谱相对。
- **管线（Pipeline）**——应用于数据集原始数据的有序处理步骤列表。
- **配方 / 模板（Recipe / template）**——保存下来的管线：可分享的
  `.plotxproc` 文件，或存放在设置中的命名模板；见
  [配方与模板](/zh-cn/guides/templates/)。
