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

### 参数设置

有三个工具能改到对象检查器（**Object inspector**）和处理面板里的设置，于是
一个层级、一种颜色、一个线宽、一个字号或一种窗函数，都可以经一次可预检的批
处理应用到整个项目：

| 工具 | 作用 |
| --- | --- |
| **Inspect a property** | 读取当前值、默认值，以及该参数允许的范围 |
| **Set a property** | 写入一个值 |
| **Reset a property** | 按当前数据重新推导该值，与面板上的重置按钮一致 |

在 **Parameters (JSON)** 中用 id 指定参数：**Set a property** 用
`{"key": "series.contour.count", "value": 12}`，另外两个用
`{"key": "series.contour.count"}`。

这三个工具够得到各面板能编辑的一切设置：对象与序列的样式、处理步骤的参数、
文档与画布的设置，以及应用偏好。该指定哪个资源由参数本身决定——等高线和线条
参数在图对象上，切趾参数在数据集上，图形排印在文档上（列为
**PlotX document**），页面尺寸在画布上，偏好设置在应用上（列为
**PlotX application**）。

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
| **Stroke width**（**Line** 区域） | `series.line.stroke_width` | 0.05 到 10 |
| **Tick-label size**（**Figure typography** 区域） | `document.figure.typography.tick_pt` | 1 到 72 |

应用偏好设置同样用这三个工具。对于
`settings.appearance.accent.color`，**Set a property** 接受 `"#rrggbb"`，把画布
强调色固定为该颜色；**Reset a property** 清除它，强调色重新跟随主题。

| 切趾步骤上的设置 | id | 取值 |
| --- | --- | --- |
| **Window** | `dataset.processing.apodization.kind` | `none`、`cosine_bell`、`exponential`、`gaussian` |
| **LB** | `dataset.processing.apodization.lb_hz` | −10000 到 10000 |
| **GB** | `dataset.processing.apodization.gb_hz` | 大于 0，最大 10000 |

各等高线参数的含义、以及每种锚定方式需要什么样的数据，见
[等高线层级](/zh-cn/guides/contour-levels/)；切趾各行的说明见
[数据处理](/zh-cn/guides/processing/)。

#### 勾选的目标会展开成什么

被勾选的资源往往并不是真正被写入的对象——展开成什么由参数决定：图对象展开
为它的各条序列，数据集展开为它的各个处理步骤，文档就是它自己。预检和结果列
表按展开后的部件逐行列出，因此一个含三条序列的图会给出三行，并指明是哪个图
对象里的哪条序列。

参数够不到的部件会标为 **Skipped** 并给出原因，其余部件照常应用：等高线下方
的热图、参数属于切趾时列表里的零填充步骤，或对一个窗函数为 *Exponential* 的
步骤索取 **GB**，都是这种情况。完全没有可寻址内容的资源（文本框、没有管线的
数据集）同样被跳过。

结果行和运行记录会在资源旁写出部件：

```json
{
  "resource": { "id": "…", "kind": "plotx.dataset" },
  "component": { "kind": "processing_step", "id": 3 }
}
```

序列部件写作 `{"kind": "series", "id": 2}`。两种 id 都只在它所属的资源内有
意义，换一个数据集，同一个步骤 id 就什么也不代表。

#### 为什么某个目标被跳过

每一条被跳过的行都带有一句供人阅读的说明。由写入本身跳过的行还带有
`skip_reason`——一个稳定的标记，工作流据此分支即可，不必去匹配文字：

| `skip_reason` | 含义 |
| --- | --- |
| `already_at_value` | 目标本来就是这个值，没有写入任何内容 |
| `not_applicable` | 该参数不适用于这个目标 |
| `target_missing` | 这个地址在文档里已经指不到任何东西 |

在预检阶段就被排除的行只有那句说明。它们绝不会是「值本来就一样」这种情况，
因此没有这个标记本身就是区分。

如果 **Set a property** 的所有目标本来就是这个值，那么它什么也不会写入：每个
目标都报告为跳过，文档版本号不变，撤销栈上也不会多出一步。

超出范围的值在 **Preflight** 阶段、也就是确认之前就会被拒绝，并同时给出你
填的值和拒绝它的边界。过了这一步，列出的部件要么全部生效，要么全部不变，
写入的改动合并为一次 **Undo automation** 撤销。

**Inspect a property** 把读数返回给调用方：作为工作流的一步运行时，读数会
进入运行记录，[命令行](/zh-cn/reference/cli/)写出的清单文件里也是同一份
内容。在窗口里，数值显示在逐部件结果下方的 **Result value (JSON)** 区域：
每个部件一份读数，包含当前值、默认值和它接受的范围。

#### 一条读数包含什么

每条读数写出自己的 `target`，并带上当前值 `value`、参数有默认值时的
`default_value`、表示当前值是否偏离默认值的 `modified`、可用状态
`availability`，以及约束取值的 `schema`。

当前状态下不允许写入的参数照样会被读出：`availability` 为 `"disabled"`，
`disabled_reason` 说明要先改什么——例如设置 φ0 之前，先把相位模式切到
*Manual*。

schema 以 `type` 标记，取值为 `bool`、`text`、`int`、`stepped_int`、`float`、
`enum` 或 `color`。

- `int` 与 `stepped_int` 带 `min` 与 `max`，参数有单位时还带 `unit`。
  `stepped_int` 另有取值必须落在的格点 `step`——例如 Savitzky-Golay 窗口为
  3 到 201，步长为 2。
- `float` 带 `min`、`max` 与 `exclusive_min`；若参数拒绝某些取值，还带
  `excluded`（单个取值）或 `excluded_magnitude`（绝对值不超过该阈值的全部
  取值）。其 `display`（`linear`、`degrees` 或 `log10`）说明面板如何显示这个
  数；旁边的 `unit` 与 `log` 只是同一件事的另一种写法。
- `enum` 列出各变体，每个都带稳定 id 和标签。

无论 `display` 是什么，边界和取值始终使用参数自身的单位：`display` 为
`degrees` 的相位值，在线格式中仍是弧度。

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
