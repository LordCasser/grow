# Design

AgentView owns one optional (UUID, session ID) for the currently pending rewind read. Each points/preview effect carries that UUID and echoes it in both success and failure results. The dispatcher checks UUID, current session and expected phase before consuming ownership. New reads replace the token; dismissal clears it. UUID avoids reused-agent/recreated-view collisions. No wire protocol or backend behavior changes. Execute responses represent committed side effects and do not use this dismissible read guard.

Points failure restores the stashed composer draft before clearing state; an obsolete failure must neither clear a newer overlay nor show a toast. View switching alone must not invalidate an existing agent/session read.
