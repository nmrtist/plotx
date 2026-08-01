---
title: 电生理
description: 导入 ABF2 记录、检查 sweep、测量响应并生成 IV 表。
---

PlotX 把 ABF 2.x 记录作为原生电生理数据集导入。当前支持 int16 与
float32、单/多记录通道、定长或变长 sweep、ADC 缩放、通道名称和单位，
以及 DAC epoch 波形；暂不支持 ABF1 和压缩 ABF2。

## Sweep 与滤波

默认图表按时间叠加所选通道的全部 sweep。在 Dataset tools 的
**Patch clamp** 中可以全选、清空或单独启用 sweep，并切换记录通道。
零相位 Gaussian 低通默认启用，截止频率为 1 kHz。绘图和分析使用同一
处理结果；原始样本不改变，设置会随项目保存。

各 sweep 的名称共用图内图例。若要释放数据区空间，请选中该图，并在对象
检查器的 **Legend & scales** 中把 **Visibility** 设为 **Hide**。
**Figure typography** 中的
**Legend size** 和 **Legend text color** 统一设定文档内所有图例的样式。
启用 **Select** 工具后可把图例拖到不遮挡曲线的位置；双击图例即可恢复
自动放置。

## 区域与时间窗统计

选中 recording，然后选择**分析** → **绘制区域**。在曲线上拖动，标出一个或
多个时间窗；每个时间窗都会在所有已选 sweep 中测量。选择 Height、Area、Max、
Min 或 Mean，再选择**继续到系列表**，即可为每个区域创建联动表格和颜色对应的
点系列。

若要得到峰值、平均值和峰值时间，请打开 **Patch clamp**。PlotX 使用当前选中的
区域；若未选中区域，则使用列表中的第一个区域。在 **Peak mode** 下选择
Positive、Negative 或 Absolute，然后选择 **Create statistics table**。画出
区域前，该按钮保持禁用。如果时间窗与某个 sweep 不重叠，或其中含有非有限值，
PlotX 会报告错误，而不会填入 `0`。可在 Data Sheet 中查看结果，或通过
**导出数据…**导出。

**Show regions on figure and export**（在图形与导出中显示区域）默认开启。
开启时，图形导出会包含每个区域的彩色带、边界和标签。

对于 recording 本身，**导出数据…**会写出当前通道中全部已选 sweep，并应用
当前滤波设置。第一列为时间，后续每列对应一个 sweep；较短 sweep 的尾部留空。

## 刺激与 IV

**From ABF** 表示命令来自文件内 DAC/epoch。若文件没有波形，PlotX 可按
协议名建议 Voltage Step、Current Step 或 Ramp；建议值只是占位，必须编辑
并明确确认模板后才能进行 IV 分析。

**Create IV table** 使用同一个已选区域，把刺激值与峰值和平均响应组合起来。
电压刺激要求电流响应，电流刺激要求电压响应；物理量不匹配时会停止计算并
说明原因。Ramp 协议不支持 IV 分析：刺激在每个 sweep 内连续变化，没有
可以对应的单一刺激值。在数据浏览器中，派生表始终列在其来源记录之下，
刺激来源也随数据集保存。

## Recording 元数据

Cell ID、experiment、label、seal resistance、leak current、capacitance 和
series resistance 都可编辑并保存在 `.plotx` 中。
