# Sync vendor maintenance notes

## Why

Backlog的vendor文档项已获本轮非延期处理授权。third_party/README.md和NOTICE把sqlite-vec描述为只补字节相同的四个C文件，与Cargo.toml维护头及实际语义修复不符；汇总表未列nix-ohos。dependency-workarounds.md已有sqlite语义修复说明，但需把验证门槛与实际锁定graph统一。

## What Changes

仅校正文档：列出nix-ohos0.26.4/pprof0.15的路径与MIT许可证；将sqlite-vec描述为包装修复及持续维护的边界、错误所有权和析构修复；明确移除前逐项复核维护补丁并运行native_boundaries普通和sanitizer验证。依赖版本、源码、构建及行为不变，skip_specs:true，不虚构delta。

验证Tier0：对照Cargo.lock、根patch表、各vendor Cargo头、许可证路径和现Core命令，diff检查与OpenSpec严格/归档校验。没有删除维护补丁或升级依赖，不重复执行本轮未改动的C测试，也不宣称新上游版本已包含修复。
