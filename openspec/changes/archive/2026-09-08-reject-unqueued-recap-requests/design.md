# Evidence
extensions/recap.rs drops cmd_tx.send result. extensions/interject.rs maps the same channel failure to ACP internal_error. Pager SendRecap maps ACP errors to RecapRequested.error; task_result.rs clears manual recap feedback by SessionId. Reuse that path without new request or state entities.

Use existing MvpAgent and SessionHandle fixtures to call the actual extension with closed and live command receivers, covering auto/manual flags and accepted command content. No provider call or actual session actor execution is needed for enqueue semantics. Isolate GROW_HOME in a temp directory and explicitly enable recap under the existing serial environment-test lock.

# Adjacent boundaries
Disabled response is still success-with-disabled; the pager currently ignores its body. A feature change after initialization could leave a manual spinner. Record that separately rather than expanding this queue-admission fix. Successful enqueue does not prove completion after actor shutdown, and no recap request identity protocol is introduced here.
