from .backoff import retry_delay

def next_attempt_time(now, attempt):
    return now + retry_delay(attempt)
