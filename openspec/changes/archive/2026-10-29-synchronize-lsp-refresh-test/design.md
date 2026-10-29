# Design

The test supplies a unique acknowledgement-file path through the mock server's environment. The mock writes that marker only after it reads the response to its refresh request. The test waits for the marker using the existing bounded `wait_until` helper, then inspects the original diagnostics. This replaces elapsed-time guessing with evidence that the protocol exchange completed; it does not change the assertion or any timeout.
