---
title: 用 CRAFT 处理 1D NMR
description: 将一维复数 FID 拟合为共振频率、振幅、线宽和相位。
---

CRAFT 直接拟合一维 NMR 采集中的原始复数 FID，并报告共振分量。信号重叠，或
需要在不依赖完整处理谱图的情况下检查 FID 时，可以使用它。

## 运行 CRAFT

1. 导入包含原始复数 FID 的一维 NMR 采集数据。没有 FID 的已处理谱或非一维数据
   不能运行 CRAFT。
2. 如果需要校准化学位移轴，先在 **Processing** 中设置 **Reference**，再选择
   **Process → CRAFT…**。参考位移会应用于所选信号区间和输出的化学位移。
3. 选择分析目标：
   - **Explore full bandwidth** 查看整个采集带宽，结果仅供探索。
   - **Measure selected signals** 只报告你在谱图上圈出的一个或多个信号组内的
     分量。
   - **Compare two signals** 要求恰好两个互不重叠的信号组，并报告考虑相位的
     相干振幅比。
4. 对后两个目标，选择 **Select on spectrum**。PlotX 会打开或聚焦该数据集的
   频域谱图。在每个峰或多重峰上拖动即可建立信号组。
5. 普通采集保留 **Conventional FID**；只有采集确实使用该序列时才选择
   **SSFP / interrupted FID**。
6. 检查就绪摘要。**Ready** 表示输入通过检查；**Ready with warnings** 表示
   可以运行但需要复核。若显示 **Cannot run**，请按其下方的操作建议处理。
7. 选择 **Run CRAFT**。计算会在后台运行；需要停止时选择 **Cancel CRAFT**。

设置页会显示采集载频、应用的参考位移、采集时长和可用点数。FID 样本或采集元数据
无效、参考信息无效、信号组重叠或超出带宽，或可用点数过少时，运行会被阻止。记录
过短、没有清晰信号或信号组过于拥挤时只会产生警告；请先检查结果再使用。

谱图上的刻线标出清晰信号。悬停刻线可查看 ppm 值。双击刻线会建立一个 90 Hz 宽的
信号组。拖动组的主体可移动，拖动任一边缘可调整宽度。边缘会吸附到附近的清晰信号；
拖动时按住 <kbd>Alt</kbd> 可关闭吸附。按 <kbd>Esc</kbd> 取消拖动，按
<kbd>Delete</kbd> 删除所选组，或用方向键每次移动一个谱点（按住 <kbd>Shift</kbd>
时每次移动十个）。需要精确边界时，可直接编辑 **Signal groups** 下的数值字段。

一个信号组可以包含多个拟合分量。分量是拟合得到的共振贡献，不是化合物鉴定，也不
保证对应一个肉眼可见的多重峰。较宽的信号组可能被分成多个计算窗口，但分量仍会
归在你选择的信号组下。

## 高级拟合设置

常规分析无需展开 **Advanced fit settings**。Conventional FID 的默认值为：

- **Minimum A/N**：3.3。较低值会保留更弱的候选分量，但也更容易拟合噪声；低于
  3.3 时会标记为需要复核。
- **Max components / fit window**：15（允许范围 1–64）。达到上限会在诊断中给出
  警告。
- **Linewidth range (Hz)**：0.05–10 Hz。
- **Fit window width (Hz)**：500 Hz。它只决定宽信号组如何分段计算，不会增加
  信号组。

编辑后的值可用旁边的 **Reset** 恢复为所选运行或配置的默认值。切换配置会保留已选
信号组，并载入新配置的设置。Conventional FID 始终是默认配置；PlotX 不会根据波形
猜测 SSFP。

**SSFP / interrupted FID** 配置的 **Skip initial** 默认值为 0.5 ms，并默认启用
时长 1.2 s 的 **Extend reconstructed FID**。跳过开头的数据点可以排除快速衰减的
背景；重建功能会为该配置延长模型 FID。除非实验已经完成定量验证，否则 SSFP 结果
应仅用于筛查或相对比较；拟合完成并不代表自动得到绝对 qNMR 结果。

## 查看运行结果

完成的运行会随 PlotX 项目保存，并附在源数据集上。打开 **Results**，选择一次运行，
再选择 **Open result canvas**，即可在普通 PlotX 画布中查看。画布联动显示实验与重建
谱、信号组比较和复数残差，三者共享横向 ppm 范围。默认通道为 **Magnitude**；也可
切换到 **Real** 或 **Imaginary**。**Normalize rows** 只适合比较形状，归一化后的
各行不表示相对定量振幅。

结果标签的用途如下：

- **Overview** 显示每个信号组的相干振幅；恰好两个组时还显示它们的比值。该比值
  使用考虑相位的相干振幅，不是把各分量模值相加。
- **Signals** 列出拟合分量。可按信号组筛选，按化学位移或振幅噪声比排序，并展开
  分量查看数值和可用的不确定度。
- **Diagnostics** 显示警告和拟合质量信息。可在警告旁选择 **Adjust setup** 修改
  运行设置。

使用 **Adjust & rerun…** 可把一次运行作为起点，修改设置后生成新运行；使用
**Rerun unchanged** 可直接重复。源 FID 或已启用的 **Reference** 步骤发生变化时，
运行会标记为 **Stale**；请重新运行后再解读。即使计算完成，只要有警告或拟合不完整，
运行也会标记为 **Needs review**。

选择 **Export components…** 可打开标准 CSV、TSV、XLSX 或剪贴板导出对话框。在
**Signals** 中选择 **Create data table**，可在 CRAFT 卡片内创建可排序的 PlotX 表格。
创建后选择 **View data table** 查看、绘图或导出；只有需要把表格放到 board 的 sheet
上时，才选择 **Add to board**。

## 解释结果

CRAFT 分量描述的是所选 FID，不会鉴定化合物，也不能替代浓度校准。进行定量指纹分析
时，应先在信号组内相干合并复数分量振幅，再取模。若运行带有警告、达到限制或使用
SSFP，请用独立采集或其他方法验证结果。
