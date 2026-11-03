## Why

AI shell suggestion, prompt suggestion, and memory note rewrite currently set the request model independently of the sampling client. A configured auxiliary model can therefore be sent to the active session endpoint, backend, and credential instead of its own provider route.

## What Changes

- Resolve each selected auxiliary model once into a complete sampler route, then derive both the client and request model from that config.
- Treat model selectors as exact catalog IDs/aliases; the catalog entry's `model` value remains the provider-facing wire name.
- Return the existing best-effort empty/failed result when a selected ID cannot be resolved; never fall back to the active session route.
- Preserve Workflow Run route freezing: the image-description selector uses its dedicated frozen sampler; other selectors use only samplers held by the Run snapshot. Do not consult a changed live catalog when the frozen selector is absent.
- Verify actual requests across distinct endpoints, backends, and credentials.
