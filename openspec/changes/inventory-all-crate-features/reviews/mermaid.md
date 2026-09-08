# mermaid 逐包核查

包路径：`crates/codegen/mermaid`。全部 Rust 模块、Cargo.toml 和可用 build.rs 已阅读；已运行 `cargo test --locked -p mermaid`，退出 0；具体计数见本 change 的 verification.md。

## 模块与开关

- `crates/codegen/mermaid/Cargo.toml`
- `crates/codegen/mermaid/src/engine.rs`
- `crates/codegen/mermaid/src/lib.rs`
- `crates/codegen/mermaid/src/mmdc.rs`
- `crates/codegen/mermaid/src/pure.rs`
- `crates/codegen/mermaid/src/raster.rs`
- `crates/codegen/mermaid/src/subprocess.rs`
- `crates/codegen/mermaid/tests/pure_engine.rs`

Cargo feature：`{}`。

## 功能与规范映射

- [Default pure Rust diagram engine](../specs/diagram-rendering/spec.md#requirement-default-pure-rust-diagram-engine)：default_engine SHALL 返回共享的 Send + Sync PureRustEngine，调用 vendored mermaid-to-svg 生成 SVG，再使用公共 rasterize 输出 PNG 与实际像素尺寸；不自动探测或切换 mmdc。
- [Checked source limit and unwind boundary](../specs/diagram-rendering/spec.md#requirement-checked-source-limit-and-unwind-boundary)：render_checked SHALL 先按 UTF-8 字节长度检查 RenderLimits.max_source_bytes（默认 64 KiB），再通过 catch_unwind 调用 engine；超过上限返回 Parse，普通成功和错误原样传递。
- [Diagram error classification](../specs/diagram-rendering/spec.md#requirement-diagram-error-classification)：纯 Rust 适配层 SHALL 将 vendor ParseError、InvalidDirection、InvalidNodeShape 映射为 Parse，DotGeneration/RenderError 映射为 Layout，UnsupportedDiagramType 映射为 Unsupported，并保留错误文本。
- [Diagram themes and viewer parameters](../specs/diagram-rendering/spec.md#requirement-diagram-themes-and-viewer-parameters)：RenderParams SHALL 默认 Light、目标宽度 1024、最大高度 4096、scale 1、最小宽度 0、background None；Light/Dark surface 分别为 #FAFAFA/#18181B，纯 Rust SVG 主题使用对应 surface。
- [Raster size and allocation bounds](../specs/diagram-rendering/spec.md#requirement-raster-size-and-allocation-bounds)：栅格化 SHALL 先按非零目标宽度缩放，否则使用 scale；非有限或非正 scale 回退 1，再提高到最小宽度、按非零最大高度和 32 百万像素面积缩小。
- [Bundled font shaping and Unicode fallback](../specs/diagram-rendering/spec.md#requirement-bundled-font-shaping-and-unicode-fallback)：栅格化 SHALL 嵌入 Roboto-Regular 并将 generic font family 指向该字体，固定首选 shaping face；ASCII SVG 只用缓存的 bundled 数据库，含非 ASCII 时使用另一个加载系统字体的缓存数据库以补足 glyph。
- [SVG image resource resolution](../specs/diagram-rendering/spec.md#requirement-svg-image-resource-resolution)：rasterize SHALL 将 usvg 的字符串 image href resolver 替换为始终 None，拒绝通过该入口读取外部文件或网络图片，保留内存 data URL 解析。
- [Explicit mmdc rendering](../specs/diagram-rendering/spec.md#requirement-explicit-mmdc-rendering)：MmdcEngine SHALL 由 caller 显式传入 binary 或通过 PATH detect 创建，默认进程等待预算 1500ms，可通过 with_timeout 修改；在临时目录写源文本，调用 mmdc 输出 SVG 后使用公共 rasterize。
- [Render subprocess input and spawn retry](../specs/diagram-rendering/spec.md#requirement-render-subprocess-input-and-spawn-retry)：run_with_timeout SHALL 使用 caller 已配置的 Command，ExecutableFileBusy 时最多尝试启动 5 次，中间依次等待 20/40/60/80ms；成功 spawn 后用 scoped thread 写可选 stdin payload，并等待子进程。
- [Render subprocess exit cleanup](../specs/diagram-rendering/spec.md#requirement-render-subprocess-exit-cleanup)：run_with_timeout SHALL 对正常/非零退出均尝试清理 Unix detached 进程组，对 timeout/wait 错误尝试 group kill、direct child kill 并 wait；仅零退出返回 Ok，其他情况保持错误分类。

## 边界

- 产生可解码 PNG；sequence 覆盖激活、自调用、note 与 Unicode，class 覆盖泛型与关系，xychart 覆盖分类轴与多折线。完整语法范围继续由 vendored 包核查，不能推导所有 Mermaid 语法均支持。
- SVG 保留节点标签和箭头，长标识保持单个 tspan；同进程重复渲染相同输入和参数具有确定性。
- 允许进入 engine；超限在调用 engine 前拒绝。
- 返回 Panic，提取字符串 payload，否则使用默认消息；warn 只记消息长度，debug 可含消息。本封装不捕获 abort/OOM/栈溢出，也不实施运行时间上限，直接 engine.render 不经过此大小检查。
- 由 raster 层返回 Rasterize；枚举另有 Timeout/Panic 供进程与 checked 层使用。
- 目标宽度为 0，scale 为 2，采用调用方最小宽度/最大高度并填入主题背景。
- None 不预填背景；Some 保留 RGBA 的 alpha。SVG 自己仍可绘制不透明背景，None 不保证最终 PNG 透明；Rgba.to_hex 仅输出 RGB。
- 每轴限定 1..16384，必要时再次缩小较长轴，确保最终 width*height 不超过 32000000。最小宽度服从这些上限。
- 分别缩放两个轴以填满 pixmap，极端比例或上限处理可改变宽高比，不承诺严格保形；返回的尺寸与 PNG 一致。
- 首选仍为 bundled face，避免已有 Latin glyph 被系统字体替换。
- 由系统字体 fallback 决定能否显示；主机无覆盖字体时不保证有中文字形，也不承诺不同主机输出字节一致。字体文件及其许可告知保留在 assets。
- 不通过该 resolver 读取目标；此限制不等同于通用沙箱，Unicode 系统字体仍可加载，也不约束外部 mmdc/Chromium 的执行。
- 使用 input/output/outputFormat svg/theme 参数，Light 映射 default、Dark 映射 dark；stdin/stdout/stderr 为 null，应用 pager_env 与 detach。Unix 源文件以 create_new 和 0600 创建。
- 分别返回 Unsupported、Timeout、Layout、Layout；等待错误和临时文件写入失败为 Rasterize。检测不验证浏览器安装，测试 fake mmdc 不证明真实 Chromium 可用。
- writer 与等待并行，忽略写入错误并关闭 stdin；其他 spawn 错误直接返回 Spawn。
- 记录警告，debug 构建在 spawn 后触发断言，release 丢弃 payload；此误用路径没有通用回收 guard，caller 必须遵守前置条件。
- Unix 按 child PID 调用 killpg(SIGKILL)，仅覆盖该组且忽略清理错误；非 Unix 没有 group kill，超时/错误只终止直接子进程，不能保证 Chromium 后代全部结束。
- 预算覆盖 wait_timeout，不包括 spawn 退避，scoped writer 仍须 join；持有 stdin 的逃逸后代可延迟返回，因此不承诺任意 Command 的整个调用严格在预算内结束。

本包已枚举入口的功能均已映射至上述 delta；调用方的更大流程仍在其所属 crate 核查，不由本包推导额外保证。

## 资源与验证限制

已核对 `assets/Roboto-LICENSE.txt` 的 bundled 字体说明；二进制 `assets/Roboto-Regular.ttf` 登记哈希，并由字体解析与 glyph 测试验证。没有将字体二进制当作文本阅读。两份资源也登记到 inventory。

本包没有 feature gate，vendored 引擎始终编译。测试使用 fake mmdc，不代表真实浏览器安装或其外部访问行为。CJK 用例在主机没有覆盖字体时可提前返回，因此测试通过不证明每个平台都有中文字形。测试日志为 `/tmp/grow-mermaid-tests.log`。

render_checked 只实施 source 字节上限及 unwind 捕获；SVG 解析内存和整个渲染过程的时间/崩溃隔离需在 pager host 核查。vendor grammar 和布局细节仍在 third_party 包待核查，不能据此将这些包标记完成。
