## Tasks

- [x] 核对上述入口与全部调用方，补齐固定 Rust 工具链 release 现代长会话基线，保存原始计时与 oracle。
- [x] 实现窄 wire 解码和同次 response/branch 借用复用，保留当前 schema；验证重复 typed 字段拒绝的明确 delta。
- [x] typed projection 直接转发，保留 metadata、source、工具合并、completion 与 resident 行为。
- [x] 验证 malformed/version/size/torn-tail、projection exact/conflict、repair、rewind/fork、cursor/resident 与真实 TUI 顺序/退出；完成同夹具前后测量。
- [x] 记录成本和残余热点；核对唯一 wire 拒绝边界 delta 及其余契约，执行 OpenSpec strict 校验后再归档，清理本项构建残留。
