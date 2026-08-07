---
title: 电生理
description: 导入 ABF2 记录、检查 sweep、测量响应并生成 IV 表。
---

PlotX 把 ABF 2.x 记录作为原生电生理数据集导入。当前支持 int16 与
float32、单/多记录通道、定长或变长 sweep、ADC 缩放、通道名称和单位，
以及 DAC epoch 波形；暂不支持 ABF1 和压缩 ABF2。

## Sweep 与滤波

默认图表按时间叠加所选通道的全部 sweep。选中图形后，可在对象检查器的
**Data** 中逐条显示或隐藏 sweep。通过某个绘图系列的 **Choose trace…**，可将其
替换为同一 recording 中的另一种刺激；**Add series…** 会一次加入所选兼容
recording 的全部 sweep。可用 **Show all**、**Hide all**、每行的复选框或删除按钮，
把 stack 缩减到需要比较的电压或电流。**Stack selected data** 同样会从每个所选
recording 的全部 sweep 开始。这些操作只改变图形。在 Dataset tools 的
**Patch clamp** 中选择记录通道，以及参与区域测量、时间窗统计、IV 表和
数据导出的 sweep。零相位 Gaussian 低通默认启用，截止频率为 1 kHz。
绘图和分析使用同一处理结果；原始样本不改变，设置会随项目保存。

新导入记录的图例默认隐藏。若要在图中识别 sweep，请在 **Legend & scales**
中将 **Visibility** 设为 **Show**。如果某条 sweep 含有 ABF DAC 波形，图例会
使用命令值及其单位；否则，PlotX 会优先使用已确认的电压阶跃或电流阶跃模板，
再回退为 `Sweep n`。命令单位不受记录响应通道影响。对于多 epoch 协议，
PlotX 使用各 sweep 之间发生变化的命令 epoch，因此固定的预脉冲不会取代测试
脉冲值。**Figure typography** 中的 **Legend size** 和 **Legend text color**
统一设定文档内所有图例的样式。
启用 **Select** 工具后可把图例拖到不遮挡曲线的位置；双击图例即可恢复自动放置。

## 区域与时间窗统计

选中 recording，然后选择**分析** → **绘制区域**。在曲线上拖动，标出一个或
多个时间窗；每个时间窗都会在所有已选 sweep 中测量。选择 Height、Area、Max、
Min 或 Mean，再选择 **View extracted curves**。PlotX 会选中散点图并自动缩放，
图中每个区域对应一个同色系列。同步数据表仍列在 Data 浏览器的 recording 之下，
但不会另占一个画板框架。在 Curve Fit 任务卡片中使用 **Fit curves**、
**View data** 或 **Back to regions**。数据以只读方式打开；若要生成独立表，
可在 Regions 任务卡片中选择 **Save Snapshot**，也可在数据表中选择
**Save editable snapshot**。同步表会保留创建时选定的 sweep；之后改变当前
分析选择不会改写已有表行。

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
