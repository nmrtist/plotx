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

## 检查与处理单个文件

```sh
plotx-cli inspect <input> [--json]
plotx-cli process <input> --scheme <recipe.plotxproc> --output <path> [--format svg|pdf|png|tiff|jpeg]
```

`inspect` 检测、加载并描述一个受支持的数据集；`--json` 输出稳定的机器
可读报告，便于脚本使用。对 ABF2 记录还会报告 ABF 版本、通道名称与单位、
采样率、扫描数和协议名。

`process` 是"一次导入、一个[处理配方](/zh-cn/guides/templates/)、一次
图形导出"的便捷路径。省略 `--format` 时按输出文件扩展名推断格式。

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
