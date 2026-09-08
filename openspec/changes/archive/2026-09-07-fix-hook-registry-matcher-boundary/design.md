# Design
使用字段 deserialize_with 将 hooks map 解码后逐项恢复。共享私有 recompile_matcher 以 configured_matcher 作为唯一事实：Some 合法编译、非法 Never、None matcher=None。append/dedup 接纳规格前也调用，防止克隆或程序化 stale matcher 进入 registry。批量 recompile_matchers 复用该函数，移除 workspace wire adapter 重复步骤。

测试覆盖三入口和合法/非法/未配置模式矩阵，以及配置清除后的批量重建。单独 HookSpec 仍是可构造传输对象，只有进入 registry 才保证派生缓存一致。
