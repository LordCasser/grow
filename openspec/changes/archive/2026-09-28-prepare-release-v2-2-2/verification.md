## 本地校验

- `cargo metadata --locked --no-deps --format-version 1`：通过，CLI 版本为 `2.2.2`。
- `openspec validate --all --strict --no-interactive`：通过。
- `bash scripts/validate-release.sh v2.2.2`：通过，tag 指向发布准备提交 `afbe3e48209d698e2539b31ce78d00bb28d3e777`。

## 官方发布

- GitHub Actions run [36391214014](https://github.com/LordCasser/grow/actions/runs/36391214014)：成功。
- 所有 10 个平台构建、smoke test、attestation 和发布校验步骤均成功。
- GitHub Release [v2.2.2](https://github.com/LordCasser/grow/releases/tag/v2.2.2) 已公开，包含 10 个平台归档及 `SHA256SUMS`，不是 draft 或 prerelease。
