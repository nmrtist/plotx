---
title: 导入数据
description: 支持的文件格式及打开方式。
---

PlotX 直接读取厂商 NMR 与电生理格式，无需任何转换步骤。

## 支持的格式

| 格式 | 扩展名 | 说明 |
| --- | --- | --- |
| JEOL Delta | `.jdf` | 1D、2D 及伪 2D（DOSY / T1 / T2） |
| Bruker TopSpin | `fid` / `ser` 目录 | 1D 与 2D |
| JCAMP-DX | `.dx` / `.jdx` / `.jcamp` | 1D 频域 NMR 谱 |
| Axon Binary Format 2 | `.abf` | int16/float32、多通道、多 sweep，以及文件内 DAC/epoch 刺激 |
| 表格数据 | `.csv`、`.tsv`、`.txt`、`.xlsx` | 保留列类型与空单元格；每个 XLSX 工作表导入为独立数据表 |
| Origin 项目（实验性） | `.opj`、`.opju` | 经验证的经典 OPJ 配置中的工作表；可以识别 `.opju`，但不能导入。见[兼容性详情](/zh-cn/reference/file-formats/)。 |
| Zip 压缩包 | `.zip` | 打包的数据文件夹 |
| PlotX 项目 | `.plotx` | 完整项目：数据、处理与排版 |

## 打开文件

把文件拖到 PlotX 窗口上，或使用工具栏的打开菜单：*Open File…*、
*Open Folder…*（用于 Bruker TopSpin 等采集目录）、*Open Project…* 或
*Import Table…*。每个导入的数据集会出现在主侧栏中，并自动放置到
画板上。
文件选择器可以一次选择多个 ABF。打开文件夹时会递归导入其中所有 `.abf`；
每个文件的直接父目录名会成为可编辑的初始 cell ID。

表格也可以直接从剪贴板粘贴：`Ctrl` + `Shift` + `V` 会把逗号、制表符或
分号分隔的文本变成新数据表。

无论从文件还是剪贴板导入表格，都会先打开 **Review table import** 对话框。它会
列出每列推断出的类型和单位、该列是否允许空单元格、前几行的预览，以及任何导入
诊断。选择 **Import table** 导入，或选择 **Cancel** 保持项目与最近文件列表不变。
含多个工作表的 XLSX 会额外提供 **Table** 选择器，可逐一预览工作簿中的各工作表；
一次 **Import table** 会把它们作为独立数据表全部导入。

PlotX 会区分布尔、整数、小数、文本和空单元格。混合了不同类型、或取值含糊的列会
保留为文本而不会被丢弃。除非文件自带 PlotX 的类型信息（见下），只有毫不含糊的
取值才会自动获得类型：`true`/`false`、十进制整数、`YYYY-MM-DD` 日期，以及
`YYYY-MM-DDTHH:MM:SSZ` UTC 时间戳。依赖地区习惯的日期以及数值与文本混合的列仍
保留为文本，PlotX 不会猜测地区格式。

PlotX 导出 CSV 或 TSV 时，会在旁边写入一个配套的 `.plotx-schema.json` 文件；
复制 TSV 时（Windows 上）也会把同样的信息与纯文本一起放到剪贴板。重新打开其中
任一种，都能恢复原始的列类型、单位和误差棒关系。没有该配套信息时，PlotX 会在
导入时推断类型，并在检查对话框中标出含糊之处。

在 `.xlsx` 工作簿中，每个可见工作表都导入为独立数据表，PlotX 会把类型信息保存在
一个隐藏工作表中。PlotX 读取 Excel 为每个公式缓存的结果，但不会自行重新计算公式；
没有缓存值的公式单元格会以空导入，并列入诊断。导出的 XLSX 文件只包含确定值，
因此不依赖 Excel 重新计算。

## Origin 项目导入（实验性）

Origin 的 `.opj` 与 `.opju` 文件会出现在 *Open File…* 和 *Import Table…*
两个入口的文件选择器中。这两个入口均根据文件内容与签名识别格式，
而不是只看扩展名。

受支持的 `.opj` 成功生成工作表后，PlotX 会打开现有的 **Review table
import** 预览，可先检查每个候选数据表。确认一次会导入全部候选数据表；
取消则保持当前项目和最近文件列表不变。预览尚未处理完时，若再选择第二个
表格路径，PlotX 会给出明确提示并拒绝该操作；请先完成或取消当前预览。

无需安装或启动 Origin，PlotX 也不会自动化或调用 Origin。严格且以证据为限的
兼容范围见[文件格式](/zh-cn/reference/file-formats/)。

## 伪 2D 实验

DOSY、T1、T2 实验会根据采集参数自动识别，并获得专属的分析工具——参见
[伪 2D 分析](/zh-cn/guides/pseudo-2d/)。

膜片钳 sweep、滤波、时间窗统计、刺激与 IV 分析见
[电生理](/zh-cn/guides/electrophysiology/)。
