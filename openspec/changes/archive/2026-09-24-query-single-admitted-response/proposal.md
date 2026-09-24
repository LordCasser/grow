# Query one admitted response for live projection

## Why

After a response is admitted, the Shell requests and clones the entire Timeline, reconstructs it, and clones every admitted response merely to project the one response it just submitted. Long sessions therefore amplify a single attempt's transient memory peak with unrelated history.

## What Changes

Add a ChatState query for one identity-bearing response on the active branch. It uses the existing branch provenance fold, returns only the matching response, and keeps projection and durable ordering unchanged. This is an internal allocation refactor with no contract delta.
