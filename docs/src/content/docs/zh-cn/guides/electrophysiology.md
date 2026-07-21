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

## 时间窗统计

输入起止时间（秒），并选择 Positive、Negative 或 Absolute 峰值模式。
**Create statistics table** 会为每个所选 sweep 生成包含带符号峰值、平均值
和峰值时间的标准 PlotX 数据表。空窗口或非有限值会明确报错，不会伪造
`0`。结果可用现有 Data Sheet 与**导出数据…**查看和导出。

对于 recording 本身，**导出数据…**会写出当前通道中全部已选 sweep，并应用
当前滤波设置。第一列为时间，后续每列对应一个 sweep；较短 sweep 的尾部留空。

## 刺激与 IV

**From ABF** 表示命令来自文件内 DAC/epoch。若文件没有波形，PlotX 可按
协议名建议 Voltage Step、Current Step 或 Ramp；建议值只是占位，必须编辑
并明确确认模板后才能进行 IV 分析。

**Create IV table** 把刺激值与 peak/average 响应组合起来。电压刺激要求
电流响应，电流刺激要求电压响应；物理量不匹配时会停止计算并说明原因。
Ramp 协议不支持 IV 分析：刺激在每个 sweep 内连续变化，没有可以对应的
单一刺激值。在数据浏览器中，派生表始终列在其来源记录之下，刺激来源也随
数据集保存。

## Recording 元数据

Cell ID、experiment、label、seal resistance、leak current、capacitance 和
series resistance 都可编辑并保存在 `.plotx` 中。
