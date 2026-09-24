Use the same semaphore as skill reload for both filesystem passes. The summary takes owned cwd and skill list; a worker holds its permit until it actually exits, including after the request times out. Generalize the existing worker helper over its return value rather than add a parallel executor. A blocked summary must not make the async runtime poll it synchronously or allow repeated requests to create unlimited workers.

The two passes each have a five-second deadline. A summary timeout reports failure; it does not claim the underlying file operation was interrupted.
