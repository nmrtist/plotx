---
title: 同核 2D 对称审核
description: 在 COSY、TOCSY、NOESY 与 ROESY 谱中比较对角线两侧的对应峰、成对标峰并审核伪峰建议。
---

同核 2D 谱中，位于 `(a, b)` 的交叉峰通常在 `(b, a)` 附近存在相关证据。
**Symmetry review** 会同时显示两个位置，免去反复读取并交换坐标。镜像位置只是
证据，并非真峰判据：真实峰可能不对称或缺失，伪峰也可能看起来对称。

当真 2D COSY、TOCSY、NOESY 或 ROESY 谱的两个频率轴核种相同且化学位移范围
重叠时，此工具可用。异核、伪 2D 与堆叠数据不提供此工具。

## 检查一个信号

1. 选中谱，然后选择 **Analyze → Review → Symmetry review**。也可以在光标
   工具族之外连续按三次 `C`：**Inspect**、**Delta**、**Symmetry review**。
2. 把光标移到交叉峰上。实线标记是光标位置，虚线标记是关于对角线的镜像位置。
3. 按住 `Shift` 可临时吸附到附近候选峰；需要持续吸附时启用
   **Snap automatically**。
4. 单击以固定这次比较。对应位置若在当前视口之外，会在图边缘提示；PlotX
   不会自动移动视口。

读数会显示两个位置的坐标和强度。对称审核完成后，还会显示已检测配对的
信噪比，或说明比较状态：

- **partner found**——镜像位置关联到一个明确候选峰。
- **ambiguous**——附近存在多个合理候选峰。
- **no counterpart detected**——镜像位置在采集范围内，但没有候选峰满足关联条件。
- **partner outside acquired range**——此谱的采集范围不足以评估镜像坐标。
- **on diagonal**——两个位置重合，不能构成独立的对角线两侧配对。

## 审核整张谱

启用 **Symmetry review** 后，PlotX 会审核整张谱，并在 **Symmetry review**
面板中列出候选配对。如果审核没有自动开始，请选择 **Run symmetry audit**。
审核结果只供复核，不会替你判定峰是否真实。

通过 **Show** 可只看 **Paired**、**Unpaired**、**Ambiguous** 或
**Suggestions**。选择一行会在图上固定该比较。

## 记录判断

- **Pick both peaks**：把固定的峰及检测到的对应峰保存为互为对应峰的一对。
- **Pick paired**：保存审核中的全部配对结果。
- **Mark possible artifact**：把固定位置标为待复核的伪峰。
- **Mark suggestions**：标记审核给出的复核建议。

可将已保存标记设为 **Confirmed**、**Uncertain** 或 **Possible artifact**，
也可从列表中删除。这些编辑都支持撤销与重做。交叉峰坐标、互相配对关系和
审核状态会保存在 `.plotx` 项目中；固定的光标位置与审核结果不会保存。
