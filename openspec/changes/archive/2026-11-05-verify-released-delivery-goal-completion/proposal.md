## Why

用户要求保持正常运行不受干扰，测试使用独立GROW_HOME。LocalPlan修复已有纯状态和生产库验证，但需要在新构建的真实Goal/Task/Planning入口证明：释放交付后仍须处置执行，迟到canonical结果可清除未应用更新并允许Goal完成。

## What Changes

只新增一个真实CLI/mock-provider隔离回归及记录，不改变产品契约。测试限定自己创建的进程、workspace、GROW_HOME和loopback provider，不连接用户服务/现有会话、不读写正常运行存储，不安装或重启当前Grow。`.openspec.yaml`使用skip_specs，无delta。

## Impact

测试写面为shell/tests中的单个新集成文件。默认普通套件保持ignored，显式使用本次GROW_BINARY运行。现有task-planning契约已定义行为，无新工具/状态/权限。
