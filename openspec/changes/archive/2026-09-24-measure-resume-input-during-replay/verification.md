# Verification

- The ignored 512-turn PTY test with a 40 ms frame-writer delay passed. Twenty keys were echoed while the final history marker was absent; p95 77.6 ms, maximum 78.2 ms. The draft remained visible after `SessionLoaded`. The replay reached its final marker in 7.40 s.
- The existing ignored 128/512-turn baseline test also passed: p95 7.4/7.6 ms and final history visible in 2.46/6.23 s.
- Early test iterations marked only the first user update or a prefix in each stream chunk. They timed out because those markers scrolled out of the PTY screen. The final fixture marks the end of every agent text chunk so the assertion observes active replay instead of scrollback position.
