## Design
复用现有 Settings handlers、测试 snapshot、合成鼠标几何与 dispatch fixture。测试明确逐项 expected action/value；不以字符串集合或测试函数存在作为交互证据。Group 父项只负责进入子菜单，七个 Bool 子项逐个驱动。Int 用方向键及提交验证；DynamicEnum 填充已知模型并验证选择和清空；Enum 验证具体候选、预览取消与 commit。

持久化测试沿 Action → dispatch → PersistSetting → TaskComplete 成功/失败，核对重建 modal snapshot 的当前值、相关缓存和最近已确认基线。reset 沿确认入口，而非仅直接调用 action_for_reset；无标量 Group 单独验证子项行为。保留旧回归及现有整组 CI 入口。测试期间统一构建，运行实际产物避免重复编译，完成后清理本轮 Rust 编译残留。
