## Design
Wrap spawn, group attachment, wait and cleanup failures with explicit stage names. Preserve ErrorKind and render the original error, including its OS code when supplied. Existing tracing of the returned error makes this visible without unconditional stderr output. No retries or permission-error suppression.
## Verification
Use a missing program and an injected Unix pre_exec PermissionDenied error to verify startup-stage attribution and error kinds. Existing timeout and descendant tests verify handling remains intact. Do not present injected errors as reproduction of the original intermittent failure.
