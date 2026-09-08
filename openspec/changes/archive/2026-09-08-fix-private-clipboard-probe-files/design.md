## Design
既有 paths 工厂返回目录所有者和三个路径。get_image/get_attachments 每次各创建一次，并保持目录到返回。run_attachments_osascript 接收调用者路径，不能自行生成第二套路径。临时目录 Unix 0700 隔离其内部文件，即使 AppleScript 使用默认文件 mode，其他用户也不能遍历访问。TempDir 自动删除整个目录，保留现有已读文件清理但不再依赖分支覆盖完整性。

## Validation
先验证两次工厂返回同一路径的旧缺陷。再验证目录和三种路径不同、0700、同名文件内容互不影响、读出一份不删除另一份、成功与错误返回均清理全部残留。测试不调用系统剪贴板。

## Boundaries
不在本次加入 osascript deadline、输出预算或改变真实读取策略。AppleScript 路径字符串转义单独核对；目录随机后缀仅为隔离，并不宣称脚本语言转义已处理。
