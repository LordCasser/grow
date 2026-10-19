Plan 的 Accepted 路径已经使用 `QuestionAnnotation.notes` 传递自由文本。ChatAboutThis 与 SkipInterview 继续使用原 `partial_answers`，并增加可选的同形 `annotations` 字段；Pager 将选项标签放在 `partial_answers`，将勾选状态下的自由文本放入 `annotations[question].notes`。工具结果格式器从该字段读取 notes，在对应回答后呈现。没有自由文本时省略 annotations，保持输出简洁。

不改变选项选择、取消、问题排序或默认问答模式，也不引入新的用户问题消息结构。
