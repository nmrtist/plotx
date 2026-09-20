---
title: 命令行
description: 不打开应用即可运行导入、处理、导出和已保存的工作流。
---

`plotx-cli` 在终端或脚本中运行 PlotX 操作，不需要窗口，也不需要显示
服务器——适合在服务器上处理一晚上的实验数据，或把 PlotX 接入更大的流
水线。它执行的操作与应用内的[自动化](/zh-cn/guides/automation/)窗口相同。

:::note[获取方式]
命令行工具目前不随发行版安装包分发。需要时可从代码仓库构建——见
[仓库 README](https://github.com/nmrtist/plotx#build-from-source)——或
使用应用内的自动化窗口，它运行同样的工作流。
:::

## 检查与处理数据

```sh
plotx-cli inspect <input> [--json] [--sampling-declaration <file.json>]
plotx-cli craft <input> --output <result.json> [--region <start-ppm:end-ppm>]... [--expected-ratio <value>]...
plotx-cli process <input> --scheme <recipe.plotxproc> --output <path> [--format svg|pdf|png|tiff|jpeg] [--sampling-declaration <file.json>]
```

`inspect` 检测、加载并描述一个受支持的数据集；`--json` 输出稳定的机器
可读报告，便于脚本使用。对 ABF2 记录还会报告 ABF 版本、通道名称与单位、
采样率、扫描数和协议名。
对 XPS 还会报告测量位置数、谱区数、总点数、谱区名称，以及具有结合能轴或仅有
动能轴的谱区数量。

对 NMR，`inspect` 描述源数据，不执行处理。Bruker 实验目录优先选择原始数据；
要检查已处理谱，请明确指定谱文件。目录有歧义时，请指定具体文件或处理目录。
NUS 数据报告的形状包含未采样网格点，不等于实际观测数。时间、频率或参数轴混合时，
报告中的 `domain` 为 `"mixed"`。

处理前请检查报告中的警告。缺失的校准或数字滤波延迟保持未知，详见
[NMR 格式限制](/zh-cn/reference/file-formats/#nmr-数据与项目)。处理失败会指出
具体步骤，并返回非零退出码。

`process` 是"一次导入、一个[处理配方](/zh-cn/guides/templates/)、一次
图形导出"的便捷路径。省略 `--format` 时按输出文件扩展名推断格式。

`craft` 对一个一维复数 FID 运行 CRAFT。如果 `<input>` 本身是原始采集目录，
即使其中包含已处理数据子目录，也只会把它作为一个输入。批目录只分析可识别为
原始采集的直接子目录；没有原始采集时会拒绝该目录。重复使用
`--region <start-ppm:end-ppm>` 可把拟合限制在指定的 ppm 区间；省略时使用完整
采集带宽。输出采用 `plotx.craft.batch.v1` JSON 格式，包含每个输入的拟合分量、
各区域相干振幅、恰好两个区域时的振幅比、诊断和质量检查。即使选择范围较宽，
结果也仍按你给出的区域汇总。

CRAFT 要求已知谱宽、观测频率、化学位移参考和数字滤波延迟。信息缺失时，
报告会为该输入记录失败。ppm 参考频率与观测频率分别保留；报告中的
`chemical_shift_reference.reference_frequency_mhz` 决定 Hz 到 ppm 的换算。
FFT 交叉检查幅度受建模区间、指数窗和零填充影响，用于评估拟合；
比较区域振幅比时，应使用报告中的相干振幅。

恰好指定两个区域时，可按输入顺序重复 `--expected-ratio`，将测得的振幅比与参考值
比较。报告会记录相对误差，误差不超过 5% 时通过检查。`all_succeeded` 表示所有
计算是否完成；`all_quality_checks_passed` 的要求更严格：输入或拟合出现警告、
选定区域没有分量，或达到诊断限制时都会为 `false`。即使命令完成，质量结果为
`false` 也表示数据需要进一步的科学复核。

## 采样声明

`inspect` 和 `process` 可使用 `--sampling-declaration sampling.json`，为支持范围内的
二维 Bruker NUS 或仅采集部分间接网格点的 JEOL 数据补录采样表。JSON 文件上限为 8 MiB，
每个字段都必须明确提供，例如：

```json
{
  "assertion_id": "my-sampling-table-1",
  "source": "user-provided sampling table from experiment notes",
  "grid_shape": [4],
  "coordinates": [[4], [2]],
  "index_base": "one",
  "component_counts": [2]
}
```

请根据原始采集记录填写各字段：

| 字段 | 填写内容 |
| --- | --- |
| `assertion_id` | 此采样声明的标识。 |
| `source` | 采样表来源，例如采集日志。 |
| `grid_shape` | 间接轴完整网格点数（包括未采样点），写成单元素数组。 |
| `coordinates` | 按采集顺序填写每次观测的间接索引，每个索引各占一个数组。 |
| `index_base` | 索引从 0 开始填 `"zero"`，从 1 开始填 `"one"`。 |
| `component_counts` | 每次观测的分量记录数（lane 数），写成单元素数组。 |

示例表示完整网格有 4 点，每次观测有 2 个分量，按从 1 开始的索引依次采集第 4、2 点。
每行坐标代表该次观测的全部分量。必须保留采集顺序和重复观测；重复观测可以导入，
但目前不能进行 NUS 重建。

PlotX 会将声明与采集数据核对。缺少网格或校准信息、分量或观测数量不符、与已有采样表
冲突，都会报错。此选项不适用于 Varian 数据、已处理谱或支持范围以外的二维采集形式。

例如，将声明保存为 `sampling.json`，再检查采集数据：

```sh
plotx-cli inspect experiment/ser --sampling-declaration sampling.json --json
```

工作流工具 `data.import` 的可选参数 `sampling_declaration` 接受同一个 JSON 对象。
声明必须适用于该导入节点中的每个路径；不同采样表应使用不同节点。保存项目会保留
声明，不会修改厂商文件。

## 运行工作流

```sh
plotx-cli batch --workflow <workflow.json> --manifest <run-manifest.json>
```

`batch` 运行一个工作流文件——与应用内自动化窗口使用同一种 JSON，因此
最简单的做法是先在应用里构建并校验工作流，再把文件交给命令行做无人值守
运行。

每次运行都会写出一份运行记录：工作流及其哈希、PlotX 版本、每一步作用的
目标，以及每一步的参数、结果、警告和错误。同一份 JSON 同时写入
`--manifest` 文件和标准输出，脚本既可以归档也可以据此做出反应。

值得了解的安全性质：

- 工作流中的相对路径相对于工作流文件解析。
- 未知参数、循环依赖和指向不存在步骤的引用会在任何步骤运行前被拒绝。
- 已存在的输出文件不会被覆盖，除非该步骤把 `overwrite` 设为 `true`。
- `failure_policy` 决定某步失败后的行为：`strict`（默认）终止运行，
  `continue_compatible` 跳过失败的步骤继续执行。

## 退出码

供脚本判断：成功为 `0`；用法或工作流无效为 `2`，工作流或输入文件不可读
为 `3`，处理失败为 `4`，图形构建失败为 `5`，输出写入失败为 `6`，完成但
包含失败的运行为 `7`。其他非零值属于内部错误，欢迎报告。

## 工作流文件结构

工作流（`plotx.workflow.v1`）是无环的 JSON 步骤图。你很少需要从零手写
——应用会生成它——但格式是纯文本、可编辑的。一个最小的导入 → 处理 →
导出工作流：

```json
{
  "schema": "plotx.workflow.v1",
  "inputs": {
    "files": { "kind": "external_files", "paths": ["data/sample.dx"] }
  },
  "nodes": [
    {
      "id": "import",
      "tool_id": "data.import",
      "parameters": {},
      "targets": { "kind": "explicit", "ids": [] },
      "bindings": [
        { "parameter": "paths", "source": { "kind": "workflow_input", "name": "files" } }
      ]
    },
    {
      "id": "process",
      "tool_id": "processing.apply_scheme",
      "parameters": { "path": "routine.plotxproc", "compatible_only": true },
      "targets": { "kind": "node_output", "node": "import", "port": "resources" },
      "dependencies": ["import"]
    },
    {
      "id": "export",
      "tool_id": "figure.export",
      "parameters": { "directory": "results", "format": "svg", "overwrite": false },
      "targets": { "kind": "node_output", "node": "import", "port": "resources" },
      "dependencies": ["process"]
    }
  ],
  "failure_policy": "strict"
}
```

每个节点声明一个 `tool_id`（要运行的操作）、作用的 `targets`，以及必须
先完成的 `dependencies`。目标可以是显式列表、查询、`inputs` 中声明的文
件，或前一节点的输出。`data.transform` 节点以与工作表列菜单和
**Combine** 菜单相同的操作重塑数据表；把 `plan` 设为要应用的变换、
`name` 设为输出表名，并把目标指向要变换的表。
