## Decision

沿用release.yml/build-one.yml全部门禁，不更改平台、警告或验证标准。新的v2.6.15 tag固定最终干净提交；前次失败对象已撤销，原提交历史保留。若未公开候选失败，核对release仍不存在或只有未公开draft，先撤销失败tag再修复重试，公开版本以v2.6.14→v2.6.15递增。

本地固定Rust1.93.1。已验收产品源码证据复用，最后Behavior修复具备heldACK旧红/13项定向绿与原真实协调脚本10类绿，默认CLI/release-dist-Dwarnings/all-targets通过；metadata变化只核对33个本地包和发行说明。最终新SHA在官方全量Core、三OS原生、Windows storage、OpenSpec及十平台上重新验证。

## Review Plan

父核对最终diff与版本边界，提交后clean-tree validate-release.sh。正式workflow必须全成功且公开前十资产attestation和checksum验证成功；公开后精确11资产、非draft/非prerelease、tag SHA、native archive结构/签名/version/help/source-digest attestation通过。最终归档记录与文档提交不移动正式tag，确认main同步且工作树干净。
