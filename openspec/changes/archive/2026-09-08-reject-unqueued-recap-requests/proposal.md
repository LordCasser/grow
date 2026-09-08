## Why
The recap extension ignores SessionCommand send failure and returns ok even if the session actor cannot receive it. Manual recap then waits for a notification that cannot arrive from that request. The existing pager error path already clears recap feedback when ACP returns an error.

## What Changes
Return an internal ACP error if recap command enqueue fails. Preserve disabled-feature and successful queue responses. Correct the outdated client connection comment claiming recap defaults off; the actual config resolver defaults on.
