## Final verification
`cargo test --locked --offline -p client-support --lib clipboard --quiet`: 54 passed, 4 ignored, 0 failed (1.39s). Ignored real clipboard tests remained ignored; no real clipboard data was accessed or modified.

- Length validation accepts zero/one/exact50,000,000 and rejects limit+1/usize::MAX without allocating those buffers. Source inspection confirms native_image_read invokes it after NSData.length and before bytes pointer retrieval, Vec allocation and copy.
- Counted Cursor test proves fallback consumes only min(source length, limit+1), preserves exact bytes below/equal the limit and propagates I/O errors.
- Sparse file of production limit+1 goes through read_clipboard_image_from_class, fails with the production limit, removes the file, then releases its TempDir. This allocates up to the bounded read buffer but avoids writing a large physical fixture.
- Existing MIME/class routing, private temp directory cleanup, AppleScript argv and process-budget tests remain green.
- Native returns Result<Option<ImageData>>; both platform get_image and get_attachments use ? before fallback. The ignored native smoke was updated for the internal signature and compiles. The public clipboard APIs retain their signatures. There was no injected real NSData oversize test; rejection order and no-fallback propagation are code evidence plus pure budget tests.

No claim is made about OS allocation inside dataForType, script-created image file size before read, raw image decoding or nonmacOS clipboard memory. These remain separate backlog boundaries. No feature removal.
