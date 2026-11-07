## ADDED Requirements

### Requirement: envrc bash fallback never treats caller paths as code

`.envrc` 的 bash fallback 加载 SHALL 以不含调用方路径字节的固定脚本执行：脚本 SHALL 通过 `current_dir` 进入目标目录并以固定相对名加载 `.envrc`，SHALL NOT 将调用方目录或文件路径插值进 shell 源码。`$PWD` 相对展开 SHALL 继续解析到调用方传入的目标目录。direnv 成功路径与 folder-trust 门控位置不受影响。

#### Scenario: 命令替换目录名不被求值
- **WHEN** 目录名含 `$(...)` 或反引号且 `.envrc` 合法导出唯一环境变量
- **THEN** 变量被正确加载，目录名从未被当作 shell 代码求值（探测标记文件不出现）。

#### Scenario: 引号、空格与换行目录名正常加载
- **WHEN** 目录名含单引号、双引号、空格或换行且 `.envrc` 合法导出唯一环境变量
- **THEN** bash fallback 加载成功，`$PWD` 相对展开解析到目标目录，不出现加倍路径。

#### Scenario: 非 UTF-8 Unix 路径保持原字节
- **WHEN** Unix 文件系统支持该非 UTF-8 目录名，目录实际存在且 `.envrc` 合法导出唯一环境变量
- **THEN** 路径仅经 OS 字符串进入子进程，变量正确加载，不经显示层有损替换。

#### Scenario: 既有 fallback 行为保持
- **WHEN** `.envrc` 正常导出变量、不存在或仅使用 direnv stub 辅助函数
- **THEN** 两层加载结构、`set -e`、stubs、`env -0` 输出与基线过滤行为保持不变。
