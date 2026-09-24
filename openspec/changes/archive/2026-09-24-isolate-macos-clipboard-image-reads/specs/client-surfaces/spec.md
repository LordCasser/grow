## MODIFIED Requirements

### Requirement: macOS clipboard encoded images have a read budget
macOS 剪贴板图片 SHALL 经由 `osascript` 子进程和私有临时文件传输。Grow 对编码数据的实际读取 SHALL 限制为最多 50,000,001 字节，并拒绝超过 50,000,000 字节的结果；空结果保持无图片语义。该边界隔离 Grow 进程免受 AppKit `dataForType:` 图片物化分配影响，但不限制 `osascript`/AppKit 子进程内存、图片写入临时文件前的磁盘用量或图片解码内存。元数据探测仍可通过原生 AppKit 读取 `changeCount` 与 `types`，不得读取图片数据。

#### Scenario: Native image exceeds the budget
- **WHEN** the pasteboard advertises an image whose encoded data exceeds 50,000,000 bytes
- **THEN** Grow SHALL NOT request image data through `dataForType:` and SHALL route the explicit image read through `osascript`; the helper-process allocation is outside the Grow-process memory boundary.

#### Scenario: Fallback image file exceeds the budget
- **WHEN** AppleScript writes an encoded image larger than 50,000,000 bytes to its transfer file
- **THEN** Grow reads at most 50,000,001 bytes, returns an error, and cleans up the file through the owned private temporary directory; the oversized result is not treated as an empty image.

#### Scenario: Image fits the budget
- **WHEN** encoded image data is non-empty and no larger than 50,000,000 bytes
- **THEN** Grow returns the full encoded data with the existing MIME/type priority through the AppleScript transfer path.

#### Scenario: Image data is acquired
- **WHEN** `get_image` or `get_attachments` reads an explicit image paste
- **THEN** image bytes are acquired by the `osascript` subprocess and Grow does not call `NSPasteboard dataForType:`; native metadata probes remain data-free.
