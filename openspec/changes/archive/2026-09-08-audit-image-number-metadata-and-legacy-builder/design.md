## Scope and findings
- 全仓检索build_content_blocks_with_workspace、build_content_blocks_with_prefixes、build_content_blocks_with_prefixes_and_caps、load_for_send和resolve_orphan_placeholders。pager-render中的builder互相调用；外部命中在pager/views/prompt_widget/tests.rs，其他命中位于同模块测试。未发现生产调用。
- 当前发送走pager/app/root/effects/helpers.rs::append_prompt_images，由effects生产调用。它先检查附件数量、文件句柄类型/长度和累计Base64预算，再take(length+1)读取并检查长度未变。必须保留。
- 旧load_for_send仍有fs::read和事后50MB检查，旧TUI orphan resolver仍事后累计总预算。但这些属于未接入的旧链，不把修补它们当作已修复线上缺陷。
- imageDisplayNumber生产写入点：client-support占位恢复、pager-render旧builder、pager实际append_prompt_images。display_number_from_meta只在测试读取。全仓Rust检索AttachedImages无定义，只有注释。image_normalize对_meta泛化复制，input_inbox测试保留_meta，这些都是载荷保存，不是编号解释消费者。
- 不推导“图片编号整体没用”：PastedImage.display_number、文本[Image #N]、匹配/恢复编号和持久化身份仍有实际消费者。仅专用ACP元数据协议是候选，外部ACP消费者不在仓库证据范围。

## Decision
R18记录闲置旧builder链，R19记录没有内部行为读者的imageDisplayNumber元数据。全部等待用户确认。删除时必须重新核对生产引用和相关契约，尤其保留泛化_meta透传、有效图片编号逻辑和实际附件发送。没有将不同编号同路径引用自动合并。
