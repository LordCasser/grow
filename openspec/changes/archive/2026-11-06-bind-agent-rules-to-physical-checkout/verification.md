# Verification

父核对实际实现：规则owner先解析physical cwd，再用同physical身份的checkout root建立root→cwd祖先链。canonicalize失败只probe原cwd，无法越界沿lexical祖先上溯；项目file_path显示physical来源，userroot插入和canonicalcandidate去重保持。没有改调用方sessioncwd或另一个RepoDirChain的home-is-dotfiles政策。

`cargo +1.93.1 test --locked --offline -p agent --lib -- prompt::agents_md::tests discovery::tests --test-threads=1` **82 passed / 0 failed、actual exit0**，日志 `/tmp/grow-backlog-ac04-agent.log`。包括24项rules/prompt测试与58项发现回归。新增三例调用真实生产discovery/parser：externalalias直接指向repo/sub（lexical祖先完全没有repo）、alias→repo root、alias→linkedworktree，保留root/sub顺序和physicalpath，排除alias外部AGENTS/rules及maincheckout。已有workspaceuser与home/projectroot排序/去重、noGitcontrols随套件通过。

父最初review拒绝了不充分的fixture：原alias指向repo再拼sub不覆盖核心反例；原expected少了正文的#；原worktreeadd对unbornHEAD必失败。最终改为直接链接内部sub、精确原文断言，并在自有TempDir里用git2空树合法initialcommit准备linkedworktree（不是项目提交），命令屏蔽用户/系统Gitconfig且无网络后再实际运行。没有改规则parser来匹配错误夹具。

新增链接场景仅在Unix运行；未声称Windows实测。home-as-repo未新增独立子进程case，现规则owner仍直接discover且没有homefilter，该政策由前后源码检查确认保持；现有home/projectroot测试验证另一项来源去重，不能把它们说成home-as-repo实测。共享RepoDirChain及其其他caller仍另属原owner，不借此宣称全仓alias链均修复。
