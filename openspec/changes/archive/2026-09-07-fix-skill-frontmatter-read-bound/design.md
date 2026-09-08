## Evidence
BufReader<File>::read_line(&mut String) 可能读入整条超长行，然后 total_bytes > MAX 才退出。

## Decision
File::take(MAX+1) 放在 BufReader 内层，保证预取也受限。read_until 换行到 Vec<u8>，总字节超过 MAX 时先退出，未超限部分再 from_utf8（真正非法 UTF-8 仍报 InvalidData）。不把被截断的长行作为完整 frontmatter。返回 total_bytes 表示实际有界读取数量。

## Limits
parse_skill_files 后续描述回退及显式 body 加载仍可能完整读取文件；本次不得声称整个技能加载链路都被限流。该边界单独记录后续审计。
