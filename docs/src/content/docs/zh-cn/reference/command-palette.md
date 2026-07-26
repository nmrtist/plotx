---
title: 命令面板
description: 用键盘搜索命令、设置与数据。
---

命令面板提供命令的键盘入口——从打开文件到切换工具——同时也涵盖设置与数据，
无需在菜单和面板中逐层查找。

## 打开与关闭

按 `Ctrl` + `K` 或 `Ctrl` + `Shift` + `P`（macOS 上为 `Cmd`）打开面板，
搜索框自动获得焦点。再按一次快捷键、按 `Esc`，或点击面板外部即可关闭。

也可以点击 Ribbon 任务页签右侧的 **Search commands**；在 Windows 和 Linux 上还可以使用
**Help → Command Palette…**。

## 搜索与执行

输入即过滤列表。匹配不区分大小写；多个词以空格分隔，全部命中才算匹配。

- `↑` / `↓` 移动选中项，自动跳过不可用的行。
- `Enter` 或鼠标点击激活所选行并关闭面板。

列表每行左侧是名称，右侧以灰色显示提示：命令显示其快捷键，设置显示其所在面板，
数据显示其类型或所属页面。

设置的匹配范围不止你看到的标签：还包括它的别名，以及它在[工作流](/zh-cn/guides/automation/)
中使用的 id（整体或逐词拆开）。`contour threshold`、`sigma` 与
`series.contour.count` 都能命中等高线相关的行。

激活一个设置不会改变任何内容。它会打开该设置所在的面板、展开其分节、滚动到该行
并短暂高亮，让你先看清当前值再编辑。

若某个设置属于某一个处理步骤，激活它还会打开处理面板，并展开第一个真正带有该
设置的步骤——例如窗函数没有 **GB** 的步骤会被跳过，而不会展开到一行并不存在
的控件上。

## 可用性

在当前上下文中不适用的命令会置灰——例如没有活动画布时的导出命令，或所选
对象不足时的对齐与分布。

当前上下文无法承载的设置同样会置灰——没有选中任何图，选中的谱线画的不是等高线，
或数据集里没有切趾步骤。它们仍留在列表中，以便你按名称找到；悬停即可看到不可用
的原因。

适用于当前选择、但在那里改不了的设置也会置灰。被锁定的图给出的原因是
*Unlock this plot to change its settings; it can still be read while locked.*

## 收录范围

命令、设置与数据三类行共用同一个搜索。

命令：

- 打开、导入与保存；按模板新建画布。
- 导出（SVG、PDF、PNG、JPEG、TIFF）与复制图像。
- 撤销、重做、全选与编组。
- 侧栏与视图切换，以及首选项。
- Ribbon 中的视图、数据、处理、分析、拟合与峰命令。
- 排列：网格、对齐、分布、层序与 *Tidy up frames*（一键整理）。
- 应用主题与堆叠数据。
- 切换到任意工具。
- *Contour settings*（等高线设置）、*Line settings*（线条设置）、
  *Figure typography settings*（图形排印设置）、*Apodization settings*
  （切趾设置）、*Raise lowest level*（提高最低层）与
  *Lower lowest level*（降低最低层）。

设置：

- 对象检查器 **Contour** 区域的各行——最低层、锚定、层数、层间比值、负等高线、
  颜色与线宽。参见[等高线层级](/zh-cn/guides/contour-levels/)。
- 对象检查器 **Line** 区域的 **Stroke width**，即线条序列的线宽。`line width`、
  `stroke width`、`trace thickness` 与 `line thickness` 都能命中它。
- 对象检查器 **Figure typography** 区域的 **Tick-label size**。`font size`、
  `tick size`、`points` 与 `figure typography` 都能命中它。参见
  [版面与导出](/zh-cn/guides/layout-and-export/)。
- 处理面板中切趾步骤的 **Window**、**LB** 与 **GB**。`apodization`、
  `window function`、`exponential`、`gaussian`、`LB`、`line broadening`、
  `GB` 与 `gaussian broadening` 都能命中它们。参见
  [数据处理](/zh-cn/guides/processing/)。

数据：

- 项目中的每个数据集、页面，以及页面上的每个对象。激活即打开并选中它。

需要在画布上点选目标的参数化操作——如某个具体的积分或相位调整——不在面板
中；请改为切换到对应工具完成。某次操作的一次性输入（例如单次导出的分辨率）属于
该操作的对话框，而不是可搜索的设置。
