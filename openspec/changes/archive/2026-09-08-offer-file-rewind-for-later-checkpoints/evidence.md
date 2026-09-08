# Evidence

Shell handle_rewind collects file checkpoints with prompt_index >= target. Pager dispatch_rewind_picker_select and dispatch_rewind_back_to_mode_select instead used only the target point's has_file_changes. The view gates the FilesOnly row/key on both offer_files_only and has_file_changes, so later changes were insufficient to expose that mode.

Red rewind_mode_eligibility_includes_later_checkpoints: 0 passed / 1 failed (0.06s). With unordered points [(2,true),(0,false)] selecting target 0 produced has_file_changes=false instead of true. The red run stopped on initial selection; remaining cases run on fixed source.

Both dispatch paths now inspect any changed point >= the resolved target. Back target fallback is unchanged. Inline editing still determines offer_files_only independently. No row counts or server contract fields changed.

Adjacent metadata audit: MapEntryCount ignores snapshot values, and shell builds picker checkpoints from prompt indices rather than metadata presence. Metadata schema tightening alone would not fix this target-range bug. Unknown metadata/error signaling remains separate.
