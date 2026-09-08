## Why
Pager treats every ACP recap response as accepted. Shell can return a successful extension envelope with result.disabled=true when the feature changes after connection initialization; no recap notification will follow and manual progress remains visible.

## What Changes
Decode the extension envelope and require an accepted result. Disabled, failed or malformed admission becomes the existing session-bound RecapRequested error. Accepted responses retain progress until the asynchronous recap arrives; auto failures remain silent. Do not globally revoke advertised availability from one response.
