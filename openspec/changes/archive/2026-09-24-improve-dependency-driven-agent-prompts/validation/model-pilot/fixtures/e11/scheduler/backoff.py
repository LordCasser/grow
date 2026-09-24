def retry_delay(attempt: int, base: float = 0.5, cap: float = 16.0) -> float:
    """Exponential retry delay capped at cap seconds."""
    if attempt < 0:
        raise ValueError("attempt must be nonnegative")
    return min(cap, base * (2 ** attempt))
