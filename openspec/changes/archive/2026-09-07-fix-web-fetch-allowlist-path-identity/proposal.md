# Why
允许列表把 host/path 整体调用 normalize_domain，造成路径大小写与末尾点被折叠，从而将未配置的不同路径自动放行。主机的 www 前缀去除也在转小写之前，导致大写 WWW 配置失效。

# What Changes
仅规范化主机，路径保留大小写和点；保留现有按路径段匹配及尾斜杠语义。

# Impact
共用 DomainMatcher 与域名规范化；不修改权限决策顺序或重定向策略。
