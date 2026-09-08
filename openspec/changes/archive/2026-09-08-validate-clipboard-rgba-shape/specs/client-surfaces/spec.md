## ADDED Requirements

### Requirement: Clipboard RGBA encoding validates input shape
剪贴板RGBA编码 SHALL 在编码和输出分配前验证非零宽高、u32尺寸可表示性、宽高乘4可表示性及精确字节长度；无效输入返回错误，不因尺寸截断、整数溢出或编码器长度断言panic。

#### Scenario: Invalid dimensions or byte count
- **WHEN** 宽高为零、尺寸或长度计算不可表示，或buffer过短/过长
- **THEN** 返回明确错误，不调用PNG编码器。

#### Scenario: Valid RGBA input
- **WHEN** 尺寸有效且buffer字节数恰好为宽乘高乘4
- **THEN** 保持PNG编码并保留像素内容。
