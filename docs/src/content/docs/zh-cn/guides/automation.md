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
哪些目标、哪些会被跳过。**Confirm and execute** 执行，整批操作合并为
一次 **Undo automation** 撤销。

### 图的参数

有三个工具能改到对象检查器（**Object inspector**）里的等高线参数，于是一个
层级、一种颜色或一个线宽可以经一次可预检的批处理，应用到项目中的所有二维图：

| 工具 | 作用 |
| --- | --- |
| **Inspect a property** | 读取当前值、默认值，以及该参数允许的范围 |
| **Set a property** | 写入一个值 |
| **Reset a property** | 按当前数据重新推导该值，与面板上的重置按钮一致 |

勾选要处理的图对象——不是页面，也不是数据集——然后在 **Parameters (JSON)**
中用 id 指定参数：**Set a property** 用
`{"key": "series.contour.count", "value": 12}`，另外两个用
`{"key": "series.contour.count"}`。

| 对象检查器中的设置 | id | 取值 |
| --- | --- | --- |
| **Lowest level** | `series.contour.base.magnitude` | 大于 0 的数；究竟以什么计量取决于 **Anchor** |
| **Anchor** | `series.contour.base.policy` | `absolute`、`noise_floor`、`background_scale`、`fraction_of_range` |
| **Levels** | `series.contour.count` | 1 到 256 |
| **Level ratio** | `series.contour.ratio` | 大于 1，最大 10 |
| **Negative contours** | `series.contour.negative.enabled` | `true` 或 `false` |
| **Positive colour** | `series.contour.positive_color` | `"#rrggbb"` |
| **Negative colour** | `series.contour.negative_color` | `"#rrggbb"` |
| **Line width** | `series.contour.line_width` | 0.05 到 10 |

各参数的含义、以及每种锚定方式需要什么样的数据，见
[等高线层级](/zh-cn/guides/contour-levels/)。

一个图可以包含多条序列，因此这些工具按序列处理：预检和结果列表为每条序列
各列一行，并指明是哪个图对象里的哪条序列。参数够不到的序列——例如等高线
下方的热图——会标为 **Skipped** 并给出原因，其余序列照常应用；没有可寻址
内容的对象（如文本框）同样被跳过。

超出范围的值在 **Preflight** 阶段、也就是确认之前就会被拒绝，并同时给出你
填的值和拒绝它的边界。过了这一步，列出的序列要么全部生效，要么全部不变，
写入的改动合并为一次 **Undo automation** 撤销。

**Inspect a property** 把读数返回给调用方：作为工作流的一步运行时，读数会
进入运行记录，[命令行](/zh-cn/reference/cli/)写出的清单文件里也是同一份
内容。在窗口里，数值显示在逐序列结果下方的 **Result value (JSON)** 区域：
每条序列一份读数，包含当前值、默认值和它接受的范围。

## External Inputs

运行一个从磁盘文件开始的已保存工作流——例如：导入文件夹里的每个实验、
应用一个处理配方、逐个导出图形。点击 **Open workflow…** 载入，
**Validate** 校验，再点 **Confirm and run workflow**。进度会逐步显示，
较长的运行可以取消。

## 什么会被记录下来

一次工作流运行会留下一份记录：工作流本身及其哈希、PlotX 版本，以及每一步
作用的目标、参数和结果。在这里运行，记录保存在项目中；用
[命令行](/zh-cn/reference/cli/)运行，同样的记录写成文件。**Current
Project** 里的批处理不会留下这种记录——它就是一次普通编辑，在文档里，也在
撤销栈上。

任何向项目之外写文件的操作（例如导出图形）还会列在 **Help → Operation and
Diagnostic History** 中，附带每个文件的路径、大小和 SHA-256 校验和。中途
失败的导出也会列出此前已经写出的文件，落到磁盘上的内容不会无从追查。

工作流文件是纯 JSON，也可以脱离桌面应用运行——见
[命令行](/zh-cn/reference/cli/)，它以无界面方式执行同样的工作流。工作流
与运行记录文件本身的说明见[文件格式](/zh-cn/reference/file-formats/)。
