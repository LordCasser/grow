# pager-render 文件与规范覆盖核对

本表核对已登记阅读文件的证据去向，不以文件有映射推导函数、分支或整包已验收。包仍为 pending；内部功能补齐和跨包集成继续进行。

| 文件 | 证据角色 | 对应要求或说明 |
| --- | --- | --- |
| `crates/codegen/pager-render/Cargo.toml` | 行为来源 | Pager render feature surfaces |
| `crates/codegen/pager-render/src/appearance/cache.rs` | 行为来源 | Appearance cache thread local setters<br>Appearance scroll line zero distinction<br>Scroll speed environment parsing order<br>Scroll mode environment fallback<br>Appearance cache prime split behavior |
| `crates/codegen/pager-render/src/appearance/config.rs` | 行为来源 | Appearance animation conversion bounds<br>Manual fold preference persistence<br>Manual fold config table shape<br>Appearance optional color parsing<br>Appearance optional color serialization loss<br>Appearance optional color quantization access<br>Appearance scroll conversion limits<br>Appearance execute preview conversion<br>Appearance edit optional defaults<br>Appearance thinking conversion and fixed flags<br>Appearance layout padding and compact projection<br>Appearance terminal defaults and runtime initialization<br>Appearance scrollback display optional conversion |
| `crates/codegen/pager-render/src/appearance/mod.rs` | 行为来源 | Global tab width atomic setting |
| `crates/codegen/pager-render/src/appearance/permission_cursor.rs` | 行为来源 | Permission sticky cursor lifetime<br>Permission cursor environment default sentinel<br>Permission cursor unmatched sticky fallback |
| `crates/codegen/pager-render/src/appearance/render_mermaid.rs` | 行为来源 | Mermaid preference canonical values |
| `crates/codegen/pager-render/src/appearance/scroll_mode.rs` | 行为来源 | Scroll mode canonical values |
| `crates/codegen/pager-render/src/appearance/text_selection.rs` | 行为来源 | Text selection preference predicates |
| `crates/codegen/pager-render/src/appearance/watcher.rs` | 行为来源 | Appearance watcher static snapshot |
| `crates/codegen/pager-render/src/clipboard/mod.rs` | 行为来源 | Clipboard route and OSC disable precedence<br>Tmux copy process success<br>Clipboard backup attempted after copy<br>Clipboard typed text read errors<br>Clipboard payload origin comparison<br>Clipboard attachment routing<br>Clipboard snapshot gate and baseline<br>Clipboard attachment file precedence<br>Clipboard probe prewarm lifecycle<br>Clipboard probe test hook scope<br>Clipboard write legs execute without success short circuit<br>Clipboard backup path and write boundary<br>Bracketed paste attachment probe size and line gates |
| `crates/codegen/pager-render/src/clipboard/trust.rs` | 行为来源 | Clipboard delivery classification<br>Clipboard expectation is preflight<br>Clipboard native trust boundary<br>Clipboard actual feedback precedence |
| `crates/codegen/pager-render/src/gboom/assets.rs` | 行为来源 | Gboom random float actual range |
| `crates/codegen/pager-render/src/gboom/engine.rs` | 行为来源 | Gboom procedural assets<br>Gboom rendering layer order |
| `crates/codegen/pager-render/src/gboom/game.rs` | 行为来源 | Gboom queued fire coalescing<br>Gboom held movement modes<br>Gboom crosshair target selection<br>Gboom damage and death transition<br>Gboom game victory predicate<br>Gboom enemy state transitions<br>Gboom map collision and wall visibility<br>Gboom player integration and simulation ordering |
| `crates/codegen/pager-render/src/gboom/mod.rs` | 行为来源 | Gboom phase and tick lifecycle<br>Gboom keyboard phase handling<br>Gboom PNG cache and caller size limit |
| `crates/codegen/pager-render/src/glyphs.rs` | 行为来源 | Legacy console glyph override<br>Toast control character sanitization |
| `crates/codegen/pager-render/src/host/display_refresh.rs` | 行为来源 | Display refresh probe cache and remote gate |
| `crates/codegen/pager-render/src/host/mod.rs` | 行为来源 | Host environment unicode collection<br>Display server first observation |
| `crates/codegen/pager-render/src/lib.rs` | 模块导出 | 已复核仅声明模块与 re-export，不另造行为要求；各模块实现在本表分别登记。 |
| `crates/codegen/pager-render/src/link_opener.rs` | 行为来源 | URL opening outcome boundary |
| `crates/codegen/pager-render/src/modal_window_state.rs` | 行为来源 | Modal chrome initial state |
| `crates/codegen/pager-render/src/prompt_images.rs` | 行为来源 | Deferred image viewer loading<br>Deferred image viewer failure state<br>Image preview first completion<br>Image preview preparation inputs<br>Image preview execution boundary<br>Prompt image reconciliation cleanup<br>Prompt image counter reset<br>Dropped path batch all or nothing<br>Single image extraction from mixed drops<br>Clipboard image construction dimensions<br>Image session persistence commit<br>Image send byte and dimension limits<br>Image content block order and skips<br>Image orphan budget scope<br>Image placeholder path stripping<br>Scrollback image reference extraction order<br>Media only markdown count boundary<br>Dropped path token decoding boundary<br>Dropped image and nonimage path distinction<br>Session media directory identity prerequisite |
| `crates/codegen/pager-render/src/render/color.rs` | 行为来源 | Color indexed conversion palette<br>Color blending supported representations |
| `crates/codegen/pager-render/src/render/draw.rs` | 行为来源 | Render writer drain acknowledgement<br>Render writer ownership and buffering<br>Frame draw sequencing and idle discard |
| `crates/codegen/pager-render/src/render/gboom_overlay.rs` | 行为来源 | Gboom overlay chrome and health thresholds |
| `crates/codegen/pager-render/src/render/highlight.rs` | 行为来源 | Search highlight reverse modifier<br>Search highlight viewport branch scope |
| `crates/codegen/pager-render/src/render/image_overlay/content.rs` | 行为来源 | Image overlay metadata formatting<br>Image overlay path character truncation |
| `crates/codegen/pager-render/src/render/image_overlay/geometry.rs` | 行为来源 | Image overlay readiness and displayed path<br>Image overlay minimum geometry |
| `crates/codegen/pager-render/src/render/image_overlay/tests.rs` | 测试证据 | 测试支持相邻生产模块契约；测试通过记录见 pager-render-test-results.json，不作为额外产品能力。 |
| `crates/codegen/pager-render/src/render/image_overlay.rs` | 行为来源 | Image overlay escapes result boundary |
| `crates/codegen/pager-render/src/render/line_utils.rs` | 行为来源 | Line width fitting span boundaries |
| `crates/codegen/pager-render/src/render/mod.rs` | 模块导出 | 已复核仅声明模块与 re-export，不另造行为要求；各模块实现在本表分别登记。 |
| `crates/codegen/pager-render/src/render/osc8.rs` | 行为来源 | Remote editor self resolving links<br>File link target conversion boundary<br>Local link existing file and media fallback<br>Explicit anchored file link existence<br>Link scanning priority and existence scope<br>Wrapped link overlay atomic projection |
| `crates/codegen/pager-render/src/render/preview_overlay.rs` | 行为来源 | Text preview overlay truncation and configuration |
| `crates/codegen/pager-render/src/render/renderable.rs` | 行为来源 | Renderable basic height contracts<br>Renderable owned and borrowed delegation |
| `crates/codegen/pager-render/src/render/safe_buf.rs` | 行为来源 | Safe buffer helper coordinate scope |
| `crates/codegen/pager-render/src/render/scrollbar.rs` | 行为来源 | Scrollbar hiding and layout reservation |
| `crates/codegen/pager-render/src/render/terminal_output.rs` | 行为来源 | Terminal output bounded virtual grid<br>Terminal output control interpretation |
| `crates/codegen/pager-render/src/render/tool_paths.rs` | 行为来源 | Tool path filesystem target spelling<br>Tool path expanded display<br>Tool path component shortening |
| `crates/codegen/pager-render/src/render/wrapping.rs` | 行为来源 | Styled wrapping hard and soft breaks<br>Table wrapping special case<br>Matching wrap helper scope |
| `crates/codegen/pager-render/src/syntax.rs` | 行为来源 | Embedded syntax theme selection<br>Syntax foreground and font modifiers<br>Terminal native syntax hue mapping<br>Ordinary syntax color quantization<br>Stateful syntax line fallback |
| `crates/codegen/pager-render/src/terminal/da2.rs` | 行为来源 | DA2 startup probe gate |
| `crates/codegen/pager-render/src/terminal/embedded_editor.rs` | 行为来源 | Embedded editor environment precedence |
| `crates/codegen/pager-render/src/terminal/hyperlinks.rs` | 行为来源 | Hyperlink standard scheme filter<br>Hyperlink capability declaration boundary |
| `crates/codegen/pager-render/src/terminal/image/tests.rs` | 测试证据 | 测试支持相邻生产模块契约；测试通过记录见 pager-render-test-results.json，不作为额外产品能力。 |
| `crates/codegen/pager-render/src/terminal/image.rs` | 行为来源 | Scrollback image disable scope<br>Kitty image format sniff boundary<br>Kitty image transmission chunking<br>Image fit minimum cells |
| `crates/codegen/pager-render/src/terminal/keyboard.rs` | 行为来源 | Keyboard capability host scope<br>Keyboard modifier rescue classification |
| `crates/codegen/pager-render/src/terminal/kitty_keyboard.rs` | 行为来源 | Kitty keyboard negotiated version boundary<br>Kitty keyboard recorded state predicates<br>Kitty keyboard teardown consume once |
| `crates/codegen/pager-render/src/terminal/mod.rs` | 行为来源 | Terminal context cached observation<br>Alternate screen selection priority |
| `crates/codegen/pager-render/src/terminal/overlay.rs` | 行为来源 | Overlay ownership delayed commit<br>Overlay explicit commit boundary |
| `crates/codegen/pager-render/src/terminal/probe.rs` | 行为来源 | Terminal query render fd gate<br>Terminal reply bounded buffer semantics |
| `crates/codegen/pager-render/src/terminal/term_version.rs` | 行为来源 | Terminal version source precedence<br>Terminal environment version corroboration |
| `crates/codegen/pager-render/src/terminal/test.rs` | 测试证据 | 测试支持相邻生产模块契约；测试通过记录见 pager-render-test-results.json，不作为额外产品能力。 |
| `crates/codegen/pager-render/src/terminal/tmux_probe.rs` | 行为来源 | Tmux probe process and drain deadlines<br>Tmux typed query result projection |
| `crates/codegen/pager-render/src/terminal/xtversion.rs` | 行为来源 | XTVERSION startup allowlist<br>XTVERSION first recorded outcome |
| `crates/codegen/pager-render/src/theme/cache.rs` | 行为来源 | Initial theme configuration resolution<br>Runtime auto theme resolution |
| `crates/codegen/pager-render/src/theme/color_support.rs` | 行为来源 | Color detection first observation and native cap<br>Standalone color evidence input<br>Color level explicit initialization precedence<br>Color quantization representation matrix |
| `crates/codegen/pager-render/src/theme/growday.rs` | 行为来源 | Built in theme palette and heading modifiers |
| `crates/codegen/pager-render/src/theme/grownight.rs` | 行为来源 | Built in theme palette and heading modifiers |
| `crates/codegen/pager-render/src/theme/md_style.rs` | 行为来源 | Markdown theme style conversion |
| `crates/codegen/pager-render/src/theme/mod.rs` | 行为来源 | Theme availability color capability<br>Theme name parsing whitespace boundary<br>Theme application native lock<br>Theme current palette auto fallback<br>Theme cursor sequence delivery<br>Theme diff and polarity helper predicates |
| `crates/codegen/pager-render/src/theme/osc11.rs` | 行为来源 | OSC11 reply parsing scope<br>OSC11 luminance polarity<br>OSC11 terminal mode restoration scope |
| `crates/codegen/pager-render/src/theme/oscura.rs` | 行为来源 | Built in theme palette and heading modifiers |
| `crates/codegen/pager-render/src/theme/rosepine.rs` | 行为来源 | Built in theme palette and heading modifiers |
| `crates/codegen/pager-render/src/theme/system_appearance.rs` | 行为来源 | System appearance OSC fallback scope<br>System appearance watcher lifecycle |
| `crates/codegen/pager-render/src/theme/terminal_default.rs` | 行为来源 | Built in theme palette and heading modifiers |
| `crates/codegen/pager-render/src/theme/tokyonight.rs` | 行为来源 | Built in theme palette and heading modifiers<br>Theme helper styles preserve native foreground |
| `crates/codegen/pager-render/src/util.rs` | 行为来源 | Grow home display containment<br>Relative time calendar approximation<br>Monotonic wall clock projection<br>Limited HTML entity replacement<br>Schedule interval parser scope |
| `crates/codegen/pager-render/assets/grow-day.tmTheme` | 内嵌资源 | Embedded syntax theme selection；Syntax foreground and font modifiers。资源加载和样式转换的输入，具体配色已在阅读记录核对。 |
| `crates/codegen/pager-render/assets/grow-night.tmTheme` | 内嵌资源 | Embedded syntax theme selection；Syntax foreground and font modifiers。资源加载和样式转换的输入，具体配色已在阅读记录核对。 |
| `crates/codegen/pager-render/assets/tokyo-night.tmTheme` | 内嵌资源 | Embedded syntax theme selection；Syntax foreground and font modifiers。资源加载和样式转换的输入，具体配色已在阅读记录核对。 |

## 本次核对结果

- 全清单 738 个已登记文件 SHA-256 与当前工作树一致。
- pager-render 登记 71 个文件，当前 173 条要求。
- 聚合模块、测试与主题资源已明确证据角色，没有为了增加数量虚构独立需求。
- 未运行 Cargo；之前单元测试结果不扩展为真实终端或其他平台验证。
