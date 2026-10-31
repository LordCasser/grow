# 验证记录

## 证据与失败回归

首轮正式 run 41165859929、job 124433240524 的 RISC-V GCC 构建在 vec_f32:1360、vec_quantize_int8:1698 报两条 -Wmaybe-uninitialized，原始日志 `/tmp/grow-release-linux-riscv64.log`。本机和其他已核对平台未给出同样诊断，因此本次以正式平台日志为编译证据。

fvec_from_value 的 JSON 路径在 array_init 失败后直接返回 SQLITE_NOMEM，未写 pzErr；两个 scalar 调用方无条件使用并释放它。新回归在真实 SQLite TEXT value 上只拒绝 vendored 源码的 vector malloc，旧实现于 `rc == SQLITE_NOMEM && error == NULL` 断言失败（exit 134，`/tmp/grow-native-red.log`）。夹具最初混入系统 sqlite3.h 导致声明冲突，改用原 vendored header 后才取得上述运行失败证据。

## 修复与检查

生产修改只有共享入口 `*pzErr = NULL` 一行。回归覆盖 parser 失败无 vector/cleanup 转移、两种真实 SQL scalar 失败、相同连接继续可用、普通非法文本的具体诊断及有效 float/int8 输出长度。测试只替换 vendored 源码的 malloc，SQLite 引擎仍用真实 allocator；不声称模拟整机内存耗尽。

- macOS `cc -O2 -Wall -Wextra -Werror -DSQLITE_CORE ... -lsqlite3 -lm` 编译并执行完整 native_boundaries：通过。
- `cc -O1 -g -Wall -Wextra -Werror -DSQLITE_CORE -fsanitize=address,undefined -fno-omit-frame-pointer ...` 与 `UBSAN_OPTIONS=halt_on_error=1`：通过。未声称 macOS LeakSanitizer 验证。
- GCC 16 `-O2 -Wall -Wextra -Werror -Wmaybe-uninitialized -DSQLITE_CORE -c sqlite-vec.c`：通过；确切 RISC-V GCC 再验由下一次正式矩阵执行。
- Rust 1.93.1 `cargo check --locked -p sqlite-vec`：通过，无 warning。
- `git diff --check`：通过。

对抗性复核保留成功时的 vector ownership、现有错误文本和 SQLite cleanup。没有给无结果路径填造诊断、放松 lint、改依赖版本或扩展其他解析问题。

## 发布边界

首轮 run 已确认 completed/cancelled，`gh release view v2.5.5` 返回 release not found。旧资产不作为新源的发布资产；release-2-5-5 负责重建未发布 tag 和全部门禁。

归档前全量 strict 18/18、归档后全量 strict 17/17、归档 strict 761/761 通过。主规范已按新场景归档。
