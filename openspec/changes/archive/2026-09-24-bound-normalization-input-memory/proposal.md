## Why

The image normalizer admits only one compute worker, but one accepted image may decode up to the provider's 178,956,970-pixel ceiling, and any number of encoded batches can wait outside that worker. A 20.16 MP camera fixture already reaches 203 MB peak RSS in the shell test process. The provider ceiling is not a useful client memory policy because images are downscaled to 2.4 MP before send.

## What Changes

Limit normalizer decode to 50 million source pixels, retain a 48 MP camera path, and cap each batch at 25 images and 80 MB of encoded image data. Across concurrent normalization calls, admit at most 160 MB of encoded data; reject excess promptly with indexed dropped-image notices instead of retaining unbounded waiters. A canceled caller's running worker retains its byte reservation until the worker actually exits.

This bounds the normalizer's owned staging. Clipboard acquisition, inline-media rendering, and external terminal decode remain separate resource domains with their own existing limits.
