---
title: 导入数据
description: 支持的文件格式及打开方式。
---

PlotX 直接读取厂商 LC–MS、NMR、AFM 与电生理格式，无需任何转换步骤。

## 支持的格式

| 格式 | 扩展名 | 说明 |
| --- | --- | --- |
| JEOL Delta | `.jdf` | 1D、2D 及伪 2D（DOSY / T1 / T2） |
| Bruker TopSpin | `fid` / `ser` 目录 | 1D 与 2D |
| Waters MassLynx RAW | `.raw` 目录 | 已验证的低分辨率数据，包括 SQD2 数据 |
| mzML | `.mzML` | 使用 32 位或 64 位、未压缩或 zlib 压缩数组的质心或轮廓 LC–MS 谱图 |
| Bruker NanoScope AFM | `.spm` / `.pfc` | 图像、力曲线、Force Volume 与 PeakForce Capture 数据立方体 |
| JCAMP-DX | `.dx` / `.jdx` / `.jcamp` | 1D 频域 NMR 谱 |
| Axon Binary Format 2 | `.abf` | int16/float32、多通道、多 sweep，以及文件内 DAC/epoch 刺激 |
| 表格数据 | `.csv`、`.tsv`、`.txt`、`.xlsx` | 保留列类型与空单元格；每个 XLSX 工作表导入为独立数据表 |
| Origin 项目（实验性） | `.opj`、`.opju` | 经验证的 Origin 7.0552 与 Origin 9.51 OPJ 配置中的工作表；不导入图形，`.opju` 仅作识别。见[兼容性详情](/zh-cn/reference/file-formats/)。 |
| Zip 压缩包 | `.zip` | 打包的数据文件夹 |
| PlotX 项目 | `.plotx` | 完整项目：数据、处理与排版 |

## 打开文件

把文件拖到 PlotX 窗口上，或使用工具栏的打开菜单：*Open File…*、
*Open Folder…*（用于 Bruker TopSpin 与 Waters MassLynx RAW 等采集目录）、
*Open Project…* 或 *Import Table…*。每个导入的数据集会出现在主侧栏中，
并自动放置到画板上。
文件选择器可以一次选择多个 ABF。打开文件夹时会递归导入其中所有 `.abf`、
`.spm`、`.pfc` 和已识别的 `.raw` 数据包。每个 `.raw` 目录会作为一次完整采集
导入一次，其中的内部文件不会被当作独立数据集。对 ABF 文件，每个文件的直接
父目录名会成为可编辑的初始 cell ID。

## mzML

打开或拖入 `.mzML` 文件。PlotX 会将谱图导入与 Waters 数据相同的 LC–MS
数据集和图表工作流。谱图按 MS 级别和极性分组；以秒或分钟记录的扫描时间都会
换算为分钟显示。文件自带的色谱图暂不导入。

导入器支持小端 32 位和 64 位浮点 m/z 与强度数组，可不压缩或使用 zlib 压缩。
Numpress、大端数组以及缺少任一必需数组的谱图会使导入停止并显示错误。

## Waters MassLynx RAW

请打开或拖入 `.raw` 目录本身。PlotX 会导入其中受支持的 MS 功能和光学检测器
通道。温度、压力及其他可读取的辅助通道会保留在数据集中，但默认不绘图。

存在光学检测器数据时，初始页面会把 UV 通道放在活动功能的总离子流图（TIC）
上方，并共享保留时间轴。多个 UV 通道会叠加显示，图例使用文件中存储的波长，
例如 `214 nm`。若要隐藏、移动或调整该图例的排版，请选中 UV 图并使用对象检查器的
**Legend & scales**。没有光学数据时，初始页面只显示 TIC。

选中 LC–MS 数据集，然后在 **Analyze（分析）** 标签中选择 **Extract Mass
Spectrum**。PlotX 会在右侧栏打开 **Dataset tools → Mass spectrometry**，并启用
保留时间范围选择。

单击 TIC 或 UV 色谱图，会在 **Scan preview** 中显示保留时间最近的 MS 扫描。
预览会标明保留时间和原始扫描编号，但不会加入页面，也不会保存为结果。选择
**Extract current scan** 可把该扫描作为棒状谱加入页面。

若要从时间窗提取，请选择 **Method**，启用 **Select range**，然后在 TIC 或 UV
色谱图上拖出范围。选择 **Extract spectrum**，即可把峰顶扫描、最近扫描、平均谱
或求和谱加入页面。每张提取谱都会记录功能、时间范围和提取方式；移动预览游标
不会改变它，并且它会列在数据浏览器的 **Analysis** 下。

数据中含多个受支持的 MS 功能时，请使用 **Dataset tools → Mass spectrometry**
中的 **MS function**。初始活动功能是第一个非参考 MS 功能。切换功能与提取谱图
都可通过标准的 Edit 命令撤销和重做。

PlotX 支持已使用 SQD2 数据验证的低分辨率 MassLynx 编码。如果必需的 MS 功能
使用其他编码，导入会停止并指出相应功能和仪器。如果其余数据可读，不受支持的
可选功能或参考功能会产生导入警告。

PlotX 不提供 LC–MS 处理流程。导入的数据、活动功能、检测器通道、提取谱图和页面
排版都会保存在 `.plotx` 项目中。扫描预览是临时状态，重新打开项目后会被清除。

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

## Bruker NanoScope AFM

PlotX 可导入 NanoScope `.spm` 图像、力曲线与 Force Volume 网格，以及
PeakForce Capture `.pfc` 数据立方体。图像通道按文件记录的扫描尺寸和物理单位
绘制成地图，并锁定纵横比。力曲线按 approach 与 retract 两段分别绘制；文件中
记录了偏转灵敏度时，纵轴为以纳米计的偏转量，否则曲线保持文件中存储的单位。
PlotX 按采集原样显示数据——不会推断接触点、压痕或模量，也不会拟合接触力学
模型。

PeakForce Capture 文件旁通常保存着一个 AllImages `.spm` 导出文件。PlotX 会
找到这个配套文件，核对其图像网格与力网格一致后，把两者作为一个数据集导入；
打开文件夹时这样的文件对也只导入一次，不会成为两个数据集。默认画布会把通道
地图和网格中心像素的力曲线并排放置。找不到配套文件、或其网格不一致时，
`.pfc` 文件仍会导入，只是仅含力曲线。

PeakForce Capture 曲线是逐像素采集的原始信号。模量等派生 QNM 地图作为独立的
图像通道导入；PlotX 不会从曲线重新计算这些地图。
