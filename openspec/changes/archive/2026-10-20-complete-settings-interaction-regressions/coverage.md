# Settings 交互覆盖对账

## 真实执行范围

| 类型 | 数量 | 新增执行证据 |
| --- | --- | --- |
| 顶层 Bool | 18 | 每 key Space、Enter 与鼠标值列切换，独立匹配该 key 的 typed Action 和反转值 |
| Group Bool children | 7 | 从父 Group 通过键盘/鼠标进入；逐个导航/渲染 hit rect 点击并核对该 child setter |
| Enum | 11 | 每个候选通过键盘导航、渲染选项鼠标点击；Enter 确认、Esc 取消；theme 三项验证鼠标 preview 与取消恢复，非 preview 项取消不发 setter |
| DynamicEnum | 2 | default_model、fork_secondary_model 各执行两个模型选择与 no-override 清空，键鼠选择后 Enter 提交、Esc 取消 |
| Int | 3 | 每 key 上下微调、左右粗调、上下限 clamp、Enter 提交、Esc 取消；鼠标点击渲染出的加/减控件；编辑态 d 发出该 key 的 reset 请求 |
| Group 父节点 | 1 | 键盘和鼠标进入 child sheet；无标量 value 和 scalar reset 是预期 |

计数为 25 Bool + 11 Enum + 2 DynamicEnum + 3 Int + 1 Group = 42；不是新增 42 个 test 函数。表驱动用例遍历当前 catalog 并实际调用 handlers，每类计数 guard 与已有注册集合检查一起阻止新增 key 静默失去覆盖。无当前 String-kind 条目；新增 kind 需更新已有 kind guard 和相应输入用例。

现有交互契约中，Enum/DynamicEnum 鼠标点击选择或 preview，Enter 才提交；新测试按此真实路径运行，没有虚构鼠标提交能力。Group child 不在顶层列表，没有 d-reset 菜单入口；41 scalar 的 reset 调度契约全部测试，34 个顶层 scalar 使用实际 d/y/n，7 child 仅按内部 reset dispatch 契约验证，不把它当成可见入口。鼠标确认框的重置和取消以真实 render hitbox 在 compact_mode 测试共享入口。

## 保存生命周期

41 scalar 逐项：修改并断言实际 PersistSetting → TaskComplete 失败 → 有效值回滚且重开菜单回显 → 同目标重新保存成功 → reset 保存失败 → 回到最近成功值 → 再次 reset 成功。Bool/Enum/Int/DynamicEnum 代表及 show_tips/auto_update 覆盖首个保存成功/失败、较新 reset 排队、reset 再失败的基线选择。旧专用权限与跨 key/ABA 顺序测试保留。

结果注入真实 TaskComplete 分支，覆盖队列和 UI 状态，不在本套声称重新验证所有操作系统的磁盘故障。底层原子写文件、原始 TOML 保留等使用已有 Shell writer 契约与测试。本轮未改变磁盘 writer。

## 对抗性核对

独立只读审查核对实际 input 分支和 mapper。每个 Bool/Enum/Int/模型的期望 Action 由测试直接匹配，没有调用生产 action mapper 来生成 expected。鼠标 picker 与 stepper 使用生产 renderer 生成的 hit rect；原有未命中和无几何 no-op 测试保留。确认并修正旧 theme 鼠标测试的过期注释，未删除其断言。

新测试先揭示测试夹具缓存/快照不一致及旧权限 helper 入口错误，修正夹具后又实际复现 show_tips 默认漂移导致的失败，见独立 align-preference-rollback-defaults change；没有跳过失败 key 或放宽回滚断言。
