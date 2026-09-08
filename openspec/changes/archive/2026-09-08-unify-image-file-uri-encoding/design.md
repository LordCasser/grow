## Evidence
canonical_from_file_uri对strip_prefix后的文本调用urlencoding::decode；不存在“先literal路径”的实际分支，注释与实现不符。producer为placeholder恢复、真实append_prompt_images及旧prompt_images builder，均用format!(file://path)。url2已由workspace使用。
## Design
file_uri_from_path返回Option<String>，用Url::from_file_path保证绝对路径和逐字节转义。canonical_from_file_uri用Url解析并to_file_path，拒绝query/fragment以免把带附加成分的URI冒充本地图片身份，然后canonicalize或保留解析路径。相对源路径无法形成file URI时不填可选uri。不进行双重percent-decode或存在性优先猜测。
## Verification
同时存在空格与字面%20的文件、含%2F/#/?/Unicode路径往返与去重，验证字面%20附件不会抑制空格文件恢复。覆盖非file、query/fragment拒绝；回归生产loader与旧builder测试，所有改动仅统一图片URI编码。
