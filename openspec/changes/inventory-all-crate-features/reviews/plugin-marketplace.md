# plugin-marketplace 逐包审阅（进行中）

已完整读取Cargo.toml、lib.rs、types.rs、error.rs、config.rs（含测试）；catalog/index/install_resolve/installer/git/matcher/scanner待完成。包保持pending。

- 无Cargo features；依赖agent/config/extension-types/tty-utils以及Git等待、fs2等基础库，实际扫描/安装执行尚待读，不从依赖名称推导能力。
- canonical_github_owner_repo先trim，依次剥一个末尾/和精确小写.git，再整体lowercase；接受https/http/ssh前缀、git@、www.以及github.com/或冒号。剩余非空即返回，不要求恰好owner/repo两段；大写.GIT在lowercase前不被剥掉。后续调用方需核对其身份比较范围。
- MarketplaceRelativePath parse剥一次./，拒绝空、absolute、任意分隔段空/点/父目录/冒号；同时识别slash/backslash，归一为slash；不trim普通段，可包含空白。
- join_under canonicalize root，然后从candidate向上寻找exists祖先，canonicalize该祖先要求在root内，再拼回缺失后缀；不是文件创建或FD授权。dangling symlink因exists false被当缺失后缀，实际使用需后续核对；存在symlink指向root外被拒绝。
- SourceKind serde内部tag type、snake_case；Local path，Git url/branch。MarketplaceEntry保留name/version/description/category/author、tags/keywords/domains、homepage、relative_path和remote_url/ref/sha/subdir/components。组件Option不意味着这里执行扫描；catalog_loaded只是scan诊断字段。
- load_sources bootstrap先解析并featured=true，普通sources后追加且false；bootstrap错误warn后继续，普通数组任何元素反序列化失败使整个普通数组丢弃，仍保留bootstrap。没有source名称/URL去重或非空校验。git优先path，branch仅Git使用；无两者warn skip。
- 本地path以任意首字符~触发展开，~other也当HOME/other，不实现其他用户HOME查找；home缺失保留原串。展开非canonical，不要求absolute/存在。
- load_require_sha为env_bool与TOML bool OR，默认false，只能收紧；env解析复用config。测试修改GROW_MARKETPLACE_REQUIRE_SHA最后remove，没有保存调用前值，局部锁不保护其他模块环境读取；若运行测试须注意范围与进程环境隔离。
- 已读tests覆盖relative path输入/静态symlink祖先逃逸、entry serde roundtrip、local/git/bootstrap排序/mixed/empty/missing字段/require_sha OR。本阶段未运行测试。

## index/catalog/scanner 完成

完整读取catalog.rs140行、index.rs200行、scanner.rs356行及全部测试；git、matcher、install_resolve、installer尚待读。

- index唯一位置.grow-plugin/marketplace.json，schema version必须1；顶层、owner、entry、author、tagged source均deny_unknown_fields。缺失/IO/JSON/version错误整体Err；不搜索.claude-plugin，也不回退目录扫描。name与plugins必需，但未验证名称非空/重复、URL/SHA语法。
- IndexSource Local path，Git url/ref/sha/path，ref映射git_ref；local path按MarketplaceRelativePath解析，remote字段只透传。
- catalog唯一.grow-plugin/plugin-index.json，可选，缺失静默None，IO/JSON/未知字段/非version1 warn后None。plugins默认空map，entry components必需、sha可选；load成功逐项components.sanitize。components_for按index_name查询，expected SHA存在才精确相等检查；不验证SHA长度或hex。
- scanner必须先有效index，再可选catalog；按index顺序处理，没有全局name去重。remote只用index元数据，不clone或读远程manifest，relative_path设置entry.name；仅index带SHA且catalog同名精确SHA匹配才有components，未pin即None。
- local解析相对路径并join_under、必须is_dir、agent manifest加载成功，否则warn skip。name/version从manifest，不校验与index.name一致、也不使用index.version补缺；description/author仅None时从index补（空字符串仍保留）；category/tags/keywords/domains/homepage直接index覆盖。
- local components先实际scan，后如catalog按index.name有entry则整体替换，无SHA验证；因此组件展示不一定反映当前本地文件，catalog仅展示信息不能作为安装授权事实。
- skills通过agent registry skill_md_paths、名称取父目录名；commands/agents对manifest dirs只read_dir一级，精确.md扩展，不检查entry是普通文件；坏dir/entry静默跳过，不读Markdown内容。
- hooks从文件JSON和inline追加，要求hooks对象下每event为groups数组；每group的hooks数组长度决定重复ComponentItem数量，name=event、description=matcher字符串。没有验证handler内容、没有hooks去重/排序。
- MCP文件取mcpServers对象keys、inline先normalize；LSP文件和inline直接顶层keys。read_json读/parse错误静默None。skills/commands/agents/MCP/LSP按(name,description)去重排序，hooks保留重复计数和遍历顺序。
- tests已读：catalog canonical/SHA mismatch/alternate位置/unknown字段；index local+git及missing/version/string-source/alternate；scanner local技能、remote字段、缺失坏index、无manifest跳过。未新运行测试，未将agent依赖内部行为算作本包已读。

## matcher与Git生产实现

matcher.rs260行全部读完；git.rs已读1–455（生产全部，余456–681测试待补）。

- matcher草稿少于3个Unicode字符不匹配；draft和关键词仅ASCII lowercase。effective顺序为显式keywords（trim非空）、domains、name；不去重，按关键词UTF8字节长度稳定降序，首匹配胜，同长度保留候选插入顺序，不按文本出现位置排序。
- domain去任意://前缀、截断/ ? #、lowercase再去www.；未做URL语法、端口或userinfo校验。keyword边界比较两侧ASCII alnum/_类别是否变化，不是Unicode分词；标点开头/结尾关键词边界有特殊效果，不应描述为简单两侧非字母数字。
- Git cache hash仅validate后的URL，DefaultHasher输出16hex，不含branch；同URL不同branch共享目录与lock。UseTtl在.git存在且FETCH_HEAD mtime年龄<5min直接返回，不核对当前branch；Force绕过TTL。futuremtime/无FETCH_HEAD不fresh。
- sync_with_mode持SourceCacheLease锁直到Drop，便捷sync/force只返回path且立即释放lease；扫描调用方是否保留锁需install_resolve等核对。
- lock read/write/create非truncate，try exclusive每100ms、超30秒错误；URL/ref在创建cache根前验证，复用agent helper，不重复声称该helper内部已审阅。
- 已有repo fetch depth1 origin branch或HEAD，再checkout --detach FETCH_HEAD、reset --hard FETCH_HEAD；每步15秒。任一步失败尝试reclone；没有git clean或origin URL一致性检查。
- clone depth1、可选--branch，--分隔URL/dest；失败best-effort remove_dir_all(dest)。reclone先到pid+nanos临时目录，成功后旧dest rename backup，再temp rename dest；失败尝试恢复，恢复也失败时错误明确backup位置。旧缓存移走失败时temp可能遗留，删除backup也best-effort。
- git_command detach/null stdin、pager_env、--no-optional-locks，设置GIT_TERMINAL_PROMPT=0/GIT_ASKPASS空/GIT_LFS_SKIP_SMUDGE=1/GIT_SSH_COMMAND ssh BatchMode；不额外清空所有Git环境或配置。probe ls-remote -- URL HEAD只检查exit，不检查HEAD输出非空。
- run_git_timed先wait_timeout再读stderr，stdout null；stderr满可能使child阻塞直到超时，后代持stderr时退出后read_to_end也可能无界。timeout或wait错误只kill/reap直接child，stderr读取错误忽略。
- failure message三种精确auth字符串变为统一认证或非repo提示；否则只收集行首fatal:/error:，没有则trim全文。debug仍记录完整stderr。
- matcher11 tests全部读，Git前段tests含hash/defaultroot、--分隔、非法operand在目录创建前失败；未执行测试。

## install_resolve与Git剩余测试完成

完整读取install_resolve.rs673行及git.rs456–681；现在仅installer.rs1502行待读。

- parse_marketplace_ref排除含://、起始git@、绝对slash/backslash、点/tilde前缀、ASCII盘符冒号、任何#；按第一个@拆name与qualifier。name非空且无slash/backslash，qualifier trim非空，但存储原值不trim，name空白可接受，多@保留在qualifier。
- slugify仅ASCII小写+Unicode whitespace分段再hyphen连接，不过滤标点或slash。addressable GitHub用共享owner_repo归一；非GitHub git/<source slug>，local/<source slug>。
- qualifier匹配是owner_repo、local/git前缀slug、source原name相等或两者slug相等的并集；所有匹配source index收集，唯一成功、零Unknown、多Ambiguous。local/git前缀大小写敏感且slug比较无进一步lowercase；同一source多条件匹配只算一次，不同来源name/owner碰撞不隐式偏好。
- bare-name用ASCII case-insensitive但不trim；单匹配直接选；多个时恰好一个featured entry赢并other_count=总数-1，否则Ambiguous。数的是匹配entry不是独立source，featured source内重复同名仍可能多featured歧义。
- resolver全部tests已读，覆盖路径/URL排除、首@拆分、空qualifier、GitHub多URL形式及.git、local/git slug、git-owner碰撞、source name与owner交叉歧义、唯一source多条件、bare唯一/featured/多featured/未找到。
- Git剩余tests用temp本地repo检验TTL sentinel、Force拿新commit、probe有效与非repo、200ms sleep kill、lease阻塞和release、损坏cache重clone及失败保留旧目录。git_available缺失会early return，不能把测试计数自动等同于全部Git分支执行。
- Git测试fixture尊重GIT_BIN_PATH但生产git_command固定git；夹具commit继承其余Git环境和用户配置，可能影响执行。测试未开启本阶段，待installer读完统一验证。

## installer生产实现完成

installer.rs已读1–820；生产实现1–701全部覆盖，测试821–1502待补。

- local install解析相对path并join_under，调用agent install_from_source(Local,false)，成功build repo附provenance、insert/save。AlreadyInstalled分支best-effort删除旧install目录/链接、remove registry并save后重试；不是先staging验证再保留旧安装的事务。
- remote install先规范subdir和clone_operands，再按provenance source_url_or_path+plugin_subdir精确查已安装；命中在pin gate前AlreadyInstalled，但畸形URL/ref/SHA先拒绝。实际新装委托agent install_from_source_with_label(require_sha,name)。底层AlreadyInstalled也删旧后重试，有失败丢失旧安装的边界。
- update首先entry.relative_path与provenance.plugin_subdir分别parse并比归一值，随后精确匹配registry来源字符串+路径，不做GitHub URL归一；远程hoist ref pin、clone_operands、ensure_pinned在创建staging前。local从synced marketplace复制不受remote pin gate。
- staging/backup在registry install_dir用repo_key+pid/nanos，final使用old_repo.path。stage remote clone或local递归复制，失败清理；discover必须有至少一个manifest插件，之后才移动旧final。无本层并发registry锁/来源cache lease。
- changed仅比较两个HashMap values().next()插件version；不比较commit、字节、组件列表，多插件first非稳定顺序。reinstalled成功恒true，installed_at保留，updated_at新时间，新plugins整体替换。
- final先rename backup，再stage rename final，失败尝试还原并恢复内存registry。insert/save失败先尝试删除新final+rename backup；FS回滚失败保留新内存记录并报不一致/backup位置；FS成功再恢复原registry并save，失败报registry rollback错误。未实现进程崩溃后恢复协议，backup清理best-effort。
- find_installed按provenance精确字符串，返回首repo及首plugin version，缺version空串；不校验实际安装目录存在。
- remote InstallKind.git_ref优先保存sha否则ref；未pinclone depth1可branch，使用tty_utils::git_command的output而非本包15秒run_git_timed。SHA clone init/add/fetch/checkout后read HEAD并case-insensitive比完整SHA，不匹配清理。所有这些output无本层deadline，不能沿用cache timeout声明。
- discover remote_subdir只lexical检查absolute/parent/root/prefix+is_dir，不canonical containment；远程symlink subdir可能跟随。根有效manifest优先，否则仅一层真实目录（file_type.is_dir不跟随目录entry symlink）；重复manifest name转HashMap会覆盖。
- copy_dir_recursive按Path::is_dir跟随symlink并递归，文件fs::copy也跟随；没有循环/深度/大小/子树containment限制，不保留symlink实体。此为需独立复现与调用方核对的债务，本迁移不修实现。
- 已读前段tests包含SHA argv边界、非法SHA在target创建前拒绝、require_sha未pin拒绝及已安装畸形operand开头；不预报测试通过。

## 全包阅读完成与验证启动

installer剩余821–1502已读完，全包11个Rust文件4423行及manifest全部覆盖；尚待结构化能力映射，不提前标reviewed。

- installer测试显式InstallRegistry::empty(temp installed-plugins)，在模块锁下重建temp安装目录，不调用环境grow_home定位安装目录。测试使用本地file://仓库，没有执行真实marketplace install命令。
- 已核对测试覆盖require_sha已安装跳过gate而畸形operand仍拒绝、未pin更新保留旧安装、local path traversal、空registry、local版本与marker更新、normalized SHA作为git_ref及下游Pinned状态、installed_at保持/updated_at更新、save注入失败后文件与磁盘registry恢复、stage无有效manifest保持旧版本、remote subdir安装更新、路径逃逸拒绝和provenance跨repo key变化幂等。
- 当前测试没有有效完整SHA的远程update checkout动态回归，也未覆盖rollback的FS失败、symlink复制、重复plugin名称或版本相同字节变化；不据测试标题声称这些通过。
- 启动cargo test --locked -p plugin-marketplace -- --test-threads=1，串行避免本包测试环境变量相互影响。日志/tmp/grow-plugin-marketplace-inventory-tests.log，待同一进程终态。

## 能力映射完成

# plugin-marketplace 逐包核查

包路径：`crates/codegen/plugin-marketplace`。全部 Rust 模块、Cargo.toml 和可用 build.rs 已阅读；测试作为证据阅读，本包串行默认测试已运行，终态见verification。

## 模块与开关

- `crates/codegen/plugin-marketplace/Cargo.toml`
- `crates/codegen/plugin-marketplace/src/catalog.rs`
- `crates/codegen/plugin-marketplace/src/config.rs`
- `crates/codegen/plugin-marketplace/src/error.rs`
- `crates/codegen/plugin-marketplace/src/git.rs`
- `crates/codegen/plugin-marketplace/src/index.rs`
- `crates/codegen/plugin-marketplace/src/install_resolve.rs`
- `crates/codegen/plugin-marketplace/src/installer.rs`
- `crates/codegen/plugin-marketplace/src/lib.rs`
- `crates/codegen/plugin-marketplace/src/matcher.rs`
- `crates/codegen/plugin-marketplace/src/scanner.rs`
- `crates/codegen/plugin-marketplace/src/types.rs`

Cargo feature：`{}`。

## 功能与规范映射

- [Marketplace source configuration and pin policy](../specs/extension-runtime/spec.md#requirement-marketplace-source-configuration-and-pin-policy)：市场来源 SHALL 先加载bootstrap且featured=true，再加载普通sources；git优先path，无两者跳过。require_sha采用环境bool与配置bool OR。
- [Marketplace relative paths and existing ancestor checks](../specs/extension-runtime/spec.md#requirement-marketplace-relative-paths-and-existing-ancestor-checks)：MarketplaceRelativePath SHALL 剥一次./并拒绝空段、点、父目录、absolute和冒号前缀，归一反斜杠为slash。
- [Canonical marketplace index schema](../specs/extension-runtime/spec.md#requirement-canonical-marketplace-index-schema)：市场 SHALL 仅从.grow-plugin/marketplace.json加载version1严格schema，包含name/plugins及typed local或git source。
- [Optional marketplace component catalog](../specs/extension-runtime/spec.md#requirement-optional-marketplace-component-catalog)：组件catalog SHALL 只从.grow-plugin/plugin-index.json加载version1，未知字段或读取解析错误降级None；成功后sanitize组件。
- [Marketplace indexed discovery and enrichment](../specs/extension-runtime/spec.md#requirement-marketplace-indexed-discovery-and-enrichment)：扫描 SHALL 按index顺序发现，remote直接透传元数据不clone，local要求受限路径有效目录与可加载manifest。
- [Marketplace component inventory extraction](../specs/extension-runtime/spec.md#requirement-marketplace-component-inventory-extraction)：本地组件 SHALL 扫描skill路径父目录名、一级.md commands/agents、hooks事件与handler数量、MCP/LSP对象keys。
- [Marketplace keyword matching precedence](../specs/extension-runtime/spec.md#requirement-marketplace-keyword-matching-precedence)：关键词匹配 SHALL 在草稿至少3个Unicode字符时，对ASCII lowercase的keywords、规范domain及name按UTF8字节长度稳定降序寻找首匹配。
- [Marketplace install reference parsing](../specs/extension-runtime/spec.md#requirement-marketplace-install-reference-parsing)：安装ref SHALL 接受name及首@拆出的qualifier，把URL、Git shorthand、slash路径、点/tilde起始、盘符和含#输入留给其他解析。
- [Marketplace source qualifier resolution](../specs/extension-runtime/spec.md#requirement-marketplace-source-qualifier-resolution)：qualifier SHALL 以GitHub owner/repo、local/git slug、原source name或slug name匹配并集，唯一匹配成功，多来源歧义返回所有index。
- [Marketplace bare name featured selection](../specs/extension-runtime/spec.md#requirement-marketplace-bare-name-featured-selection)：bare name SHALL ASCII不区分大小写匹配entry，唯一直接选，多项时仅恰好一个featured匹配获选并返回other_count。
- [Marketplace cache identity TTL and leases](../specs/extension-runtime/spec.md#requirement-marketplace-cache-identity-ttl-and-leases)：Git来源cache SHALL 以URL的16hex DefaultHasher值定位目录与锁，UseTtl按FETCH_HEAD年龄小于5分钟跳过更新，Force强制刷新。
- [Marketplace cache fetch and replacement](../specs/extension-runtime/spec.md#requirement-marketplace-cache-fetch-and-replacement)：cache SHALL depth1 clone或fetch origin指定branch/HEAD，detach及hard reset FETCH_HEAD，失败尝试临时reclone后backup替换。
- [Marketplace Git command interaction and timeout](../specs/extension-runtime/spec.md#requirement-marketplace-git-command-interaction-and-timeout)：cache Git命令 SHALL 禁交互认证/LFS smudge、null stdin、detach并使用--分隔operand；每clone/fetch/checkout/reset/ls-remote设置15秒等待。
- [Marketplace local install provenance and reinstall](../specs/extension-runtime/spec.md#requirement-marketplace-local-install-provenance-and-reinstall)：本地安装 SHALL 校验市场相对路径并委托Local安装，附provenance后insert/save，不施加remote pin gate。
- [Marketplace remote install idempotence and pins](../specs/extension-runtime/spec.md#requirement-marketplace-remote-install-idempotence-and-pins)：远程安装 SHALL 先校验subdir/URL/ref/SHA，再按provenance来源字符串与plugin_subdir精确检查已有安装；新装委托带label的pin gate。
- [Marketplace transactional update staging and identity](../specs/extension-runtime/spec.md#requirement-marketplace-transactional-update-staging-and-identity)：update SHALL 要求entry与provenance归一相对路径相同，按精确来源字符串与路径找旧repo，在staging clone或copy且发现有效插件后才替换final。
- [Marketplace update rollback and change reporting](../specs/extension-runtime/spec.md#requirement-marketplace-update-rollback-and-change-reporting)：update SHALL 先final rename backup再staging rename final，registry保存失败尝试文件回滚后恢复原registry并保存。
- [Marketplace remote staging clone verification](../specs/extension-runtime/spec.md#requirement-marketplace-remote-staging-clone-verification)：远程staging SHALL 有SHA时init/add/fetch/checkout并验证HEAD与完整SHA不区分大小写相等，保存git_ref优先SHA，否则ref。
- [Marketplace staged plugin discovery and local copying](../specs/extension-runtime/spec.md#requirement-marketplace-staged-plugin-discovery-and-local-copying)：staged发现 SHALL 根manifest优先，否则只检查一层真实目录，生成按manifest name的RepoPlugin map；local copy递归复制。

## 边界

- 普通数组整体丢弃但保留bootstrap；任意首~以HOME展开，不验证来源名称非空或重复；pin默认false，仅任一来源true即可收紧。
- canonicalize最近exists祖先并检查在canonical root内，再拼缺失后缀；不提供文件句柄绑定，dangling symlink可作为缺失后缀。
- 整体返回错误，不回退目录扫描；entry名称重复、URL/SHA具体格式不在load_index校验。
- 只有index携带SHA且同名catalog SHA精确匹配才展示；local使用None预期SHA可覆盖实际扫描组件，不构成安装身份授权。
- name/version来自manifest，description/author仅None补index，其余分类匹配元数据来自index；不校验同名或去重entries。
- 坏读取静默忽略；除hooks外按name/description去重排序，hooks保留数量；.md条目未额外检查普通文件。
- 同长度保留候选插入顺序；边界是ASCII alnum/_类别变化，非Unicode分词；domain剥scheme/www/path而非完整URL验证。
- name仅非空且无slash，不trim；qualifier trim非空但保存原值，多余@保留。
- 返回Ambiguous，不隐式优先任何解释；slug仅ASCII lowercase和空白hyphen化，GitHub辅助不保证owner/repo恰好两段。
- 按entry计数，多个featured仍Ambiguous；不按独立source去重、不trimname。
- branch不参与key，fresh时不核对branch；with_mode返回持锁lease，便捷返回path时释放锁；lock每100ms尝试，30秒超时。
- 旧cache尽量保留，安装rename失败尝试restore，restore失败报backup位置；不执行git clean或origin一致性检查，cleanup best-effort。
- wait后才读stderr，可能因pipe满超时或读后代pipe超时界限失效；kill仅直接child。probe只检查exit成功，不确认HEAD输出。
- 先best-effort删除旧安装、remove/save registry再重试，失败不保证旧安装恢复；与transactional update不同。
- 返回AlreadyInstalled并跳过新fetch gate，畸形operand仍先拒绝；不检查实际目录存在，底层冲突可走删旧重试。
- 远程hoist pin、校验并ensure_pinned先于staging；local来自同步来源不受pin gate。installed_at保留、updated_at刷新，新plugins整体替换。
- 明确报告不一致及backup/重装需求，不宣称崩溃原子性；changed只比较HashMap首插件version，成功reinstalled恒true，不比较代码或commit。
- 未pindepth1可branch；此路径使用tty_utils git output，无cache的15秒deadline；HEAD读取失败在非pin元数据路径可为空。
- subdir仅lexical检查后is_dir，未canonical containment；copy跟随symlink且无深度/容量限制；重复name覆盖，first version来自不稳定HashMap遍历。

本包已枚举入口的功能均已映射至上述 delta；调用方的更大流程仍在其所属 crate 核查，不由本包推导额外保证。

## plugin-marketplace 收口验证

测试会话12623退出0，串行默认测试100 passed、0 failed、0 ignored，doc-tests 0，日志/tmp/grow-plugin-marketplace-inventory-tests.log。新增19项要求，来源符号和哈希一致；严格校验15项及git diff --check通过。累计31/61包、317项要求、25个delta能力，剩30包，整个change仍不归档。
