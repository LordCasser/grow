# Verification

The real v2.6.7 CLI ran under the PTY with an isolated workspace and loopback provider. The command stayed in the foreground for at least 3.2 seconds before Ctrl+B. Ctrl+D twice suspended the session and stopped the child process. The cold --continue process showed an empty running Tasks pane and one failed history row with elapsed 3.6s. Its CommandsSettled outcome was interrupted and its stored execution interval was 3.589823s. The original settlement and single command start remained after resume.

The exact test is `ctrl_d_suspend_and_continue::ctrl_d_resume_restores_interrupted_command_as_terminal_in_tasks` in the `pty_e2e_persistence` binary. The release workflow runs it against its own exact-commit build.

Repeat on the same candidate showed the same terminal row at 3.6s for a recorded 3.600871s execution interval.
