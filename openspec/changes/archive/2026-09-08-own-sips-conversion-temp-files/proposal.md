## Why
convert_via_sips creates PID/timestamp-named source and destination files directly in the shared temporary root. Write, sync, spawn and read failures can return before manual cleanup. Source image data can remain on disk.
## What Changes
Use a randomly named owned temporary directory, explicitly mode 0700 on Unix. Keep both files inside it and hold ownership through conversion/read completion. Normal return and error exits release the directory through RAII.
