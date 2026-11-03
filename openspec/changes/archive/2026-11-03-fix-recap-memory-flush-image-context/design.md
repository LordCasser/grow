## 现有边界

`freeze_sideband_context` 已为协调问答和 `/btw` 等待图片投影 ACK、选择已有描述、再生成 portable history。recap 需要保留 backend 相关 reasoning，因此不能直接复用已经 portable 的结果。Memory Flush 会在压缩前捕获 Surface/IDs/revision，随后异步取近期完整 User turns；不能在执行时重新冻结当前 Surface 替换该来源。

## 处理方式

将现有共享冻结过程拆成两个已有职责：冻结并在 Surface 副本选择所有可用描述；按准备路由验证实际请求中的未描述图片。前者返回已有 TimelineMaterialization，保留 input_ref、Surface IDs 与 revision，不引入新状态类型；协调和 `/btw` 的入口继续包装成 portable history。

recap 使用冻结的 Surface 副本，沿原 budget/reasoning builder 处理后再验证实际发送的图片。Memory Flush 在 snapshot 创建时使用同一冻结过程；传入旧 snapshot 的执行不重新抓取新历史。先选择描述再计算图片字节/输入预算；仅对选中的最终窗口检查负能力，窗口外旧图片不应阻止 flush。

Memory Flush 使用同一份 prepared config 固定 client、model 与图片 key；既有 flush_model 仍按当前约定覆盖该 prepared config 的 model，key 反映实际请求。该 override 的跨 provider 解析留给独立事项。

## 失败与验证

咨询不启动 vision/OCR、不改主 Surface 或负能力。等待遵循 session Sideband cancellation；锁在 provider 工作前释放。recap 失败继续发既有 unavailable，flush 失败释放 flush lock、不写入本次记忆。未知路由未转换图片仍可能首次 400，不新增重试。

Tier 2：真实 HTTP 覆盖三 backend、用户/工具图片、缓存有/无、主 ACK 等待、取消/失败、原图保全；flush 额外验证压缩后仍用原 snapshot、最终窗口预算与实际 override key。复用原 helper、recap、协调与侧问回归作对抗检查。

## 必要前置修正

实际文件回归发现，ImageProjection 对已描述工具调用分配的 Surface 坐标不被 `owns_surface_id` 完整认可，导致合法 flush Sideband 恢复失败。该既有缺陷单独由 `fix-image-projection-sideband-source-coordinates` 修正：只让所有权范围覆盖现有 live/replay 已分配的 shadow、tool-call、carrier slots。本 change 不修改坐标分配、恢复协议或 Timeline 格式，并继续保留真实落盘回归验证来源。
