## Why
The 60-second refresh margin also gates the request-time bearer resolver. Newly minted tokens valid for less than 60 seconds are therefore omitted from outgoing requests.

## What Changes
Separate proactive refresh eligibility from request-time validity. Preserve fail-closed behavior after a failed refresh.
