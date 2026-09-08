## Why
Concurrent first events can open several debug writers for one sink. Appender parks every guard before RoutingLayer selects a winner, leaking redundant worker lifetimes until process exit.

## What Changes
Keep guards local until routing selection; park only the retained writer guard and drop losing guards outside the routing lock. Keep file open outside lock to preserve reentrancy boundary.
