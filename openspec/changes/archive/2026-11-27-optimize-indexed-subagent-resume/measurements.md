# 委派恢复存储基准

两个 Shell 测试可执行文件分别调用改动前后的生产 `durable_resume_source_for` 和 `bootstrap_initial_context`。每次请求都恢复独立的来源身份，并实际准备继承所需的模型 Surface 与有序展示记录。采样区间从提交请求开始，到全部继承材料返回为止。每个样本在独立进程中运行，夹具构造和产物哈希核对在区间外。旧版的空索引基准仍由 `measure_concurrent_resume_source_reads` 单独提供。

三组临时 home 使用 v57 工作索引。夹具由 `init_session_with_summary` 原子发布 journal、工作索引及 manifest。准备阶段再用生产 writer 加载并确认每个 session 的工作索引覆盖全部物理记录和 canonical 字节。新旧算法复用相同的 journal 文件、inode 和展示来源。夹具以已发布的基线为主，另测活 writer 的未发布尾部。

- 父会话组包含 4 个各自完成的来源和 32 MiB 无关父记录。这些父 Observation 使用普通保留分类。每个来源有自己的 Spawn/Seed/Result/Ended 链和独立展示正文。
- 来源诊断组的父历史很小，来源 A 除约 64 KiB 有效模型与展示文本外，含 32 MiB Shell 历史诊断记录。
- 来源正文组的父历史同样很小，来源 A 含 128 条由 `init_session_with_summary` 写入的 assistant seed 正文及相应的有序历史引用，共约 32 MiB 正文。恢复时完整展开这些引用。夹具没有发起网络模型请求。

环境为 Apple M4 Pro、24 GiB 内存、macOS 27.0.1 arm64、APFS SSD，使用 Rust 1.93.1 未优化测试构建，关闭调试信息与增量编译。基准对继承的完整 Surface 和有序展示分别计算 BLAKE3，同时核对输出行数与正文长度。RSS 取 `getrusage(RUSAGE_SELF).ru_maxrss`。墙钟时间涵盖整组并发。按设备号和 inode 统计 `read/pread/readv/preadv` 成功返回的字节，分别归到父 journal、A journal、其他来源 journal 和各自的索引、summary 等附属文件。这些字节含文件缓存命中，排除了未显式读取的映射和内核元数据操作。物理读取另以 macOS `proc_pid_rusage(RUSAGE_INFO_V2)` 的差值观察。

## 已发布父历史的并发恢复

每种并发度先预跑一次，再分别启动三个独立进程，表中列出三次的中位数。每次启动前预读同一组夹具文件。每路请求指向同一父会话下不同的已完成子会话，全部材料留存到最后一个请求返回。父 journal 为 33,592,264 B，每个来源的 journal 为 1,616 B。

| 并发数 | Dense 墙钟 ms | Indexed 墙钟 ms | Dense 峰值 RSS MiB | Indexed 峰值 RSS MiB | Dense 父 journal 逻辑读 B | Indexed 父 journal 逻辑读 B | Dense / Indexed 父附属文件逻辑读 B |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 1,602 | 5 | 75.2 | 50.8 | 33,600,456 | 1,120 | 316 / 39,449 |
| 2 | 1,592 | 5 | 108.1 | 56.7 | 67,200,912 | 2,240 | 632 / 78,898 |
| 4 | 1,608 | 7 | 173.8 | 66.0 | 134,401,824 | 4,480 | 1,264 / 157,796 |

主表三次计时在全套回归开始前完成。逻辑读数取单独的第四个追踪进程。随后使用共享工作区构建的 Shell 测试程序复查五个样本的全部文件读取量、Surface 和原始展示哈希，结果逐项相同。4 路时，子会话 journal 合计从 12,928 B 降至 3,084 B，子会话附属文件从 1,524 B 升至 387,646 B。两版计时进程的初始高水位均约 23.5 MiB。父历史组的全部计时样本都报告 0 B 进程物理读，表中的差额来自文件缓存以上的读取与处理。

## 活动父会话的未发布尾部

另取三份新夹具，用实际持有 writer 的适配器加载父会话，追加一条 Observation，核对 canonical 长度增加及 manifest 字节保持一致。将同一条已验证的父 Timeline 交给 ChatState actor，4 个并发请求均通过真实 writer、actor 与 resolver 完成。按文件计数从 writer 准备完毕后开始。每个样本返回 4 组完整材料，每组含 2 条模型消息和 1 条 72 B 展示正文。

| 父 journal 已发布字节 | 未发布尾部 | 父 journal 逻辑读 B | 父附属文件逻辑读 B | 进程物理读 B |
| ---: | --- | ---: | ---: | ---: |
| 4,484 | 普通保留 Observation，260 B | 4,480 | 1,752 | 528,384 |
| 33,592,264 | 普通保留 Observation，264 B | 4,480 | 1,728 | 532,480 |
| 33,592,264 | Shell 诊断 Observation，260 B | 4,480 | 1,772 | 532,480 |

活动 writer 与 actor 的截点一致时，每个请求直接核对对应的 Spawn 和 Ended。普通尾部的两个样本增加了约 32 MiB 父历史，四路父 journal 读取字节数相同。每组单独运行一次，进程物理计数包含其他文件。本节只比较 I/O，计时与 RSS 未混入上方三轮中位数。

## A 自身的历史

此组一次恢复来源 A。父 journal 只有 4,488 B。两种分布分别要求恢复 1 条 65,544 B 的正文，以及 128 条共 33,555,456 B 的正文。每种分布都包含额外一条 System 模型消息。

| A 中的材料 | 实现 | 墙钟 ms | 峰值 RSS MiB | A journal 逻辑读 B | A 附属文件逻辑读 B |
| --- | --- | ---: | ---: | ---: | ---: |
| 32 MiB 无关诊断 | Dense | 1,602 | 75.6 | 33,662,534 | 392 |
| 32 MiB 无关诊断 | Indexed | 6 | 50.9 | 66,247 | 170,671 |
| 32 MiB 有效正文 | Dense | 1,621 | 112.8 | 33,634,861 | 389 |
| 32 MiB 有效正文 | Indexed | 611 | 129.2 | 33,556,163 | 35,945,143 |

两个 A journal 实际大小分别为 33,654,342 B 和 33,626,669 B。全部相关计时样本均为热缓存，进程物理读取计数为 0 B。展示正文从 journal 恢复，模型 Surface 从工作数据库恢复。当 A 的历史几乎全是要继承的正文，这两条路径仍各读约一份正文。该分布下 indexed 的墙钟耗时更短，应用层总读取与峰值内存高于 dense。

曾用同样的 128 条正文定位 redb 数据页的读取放大。v55 将整个执行快照存成一个值，在 `working_snapshot::hydrate → Index::execution_checkpoint → redb::ReadTransaction::open_table` 的一次 `pread` 中读取 67,108,864 B。A 附属文件总读 67,304,109 B，峰值 RSS 243.0 MiB。v56 分块后总读 33,901,238 B，峰值 159.0 MiB。v57 按 Surface 项恢复后峰值 129.2 MiB，A 附属文件总读 35,945,143 B。这三种索引格式均由对应版本 writer 发布，结果均输出 128 条正文和相同的模型 Surface。

继承材料另以 `compare_published_resume_seed_facts` 和 canonical projector 逐项比对。两版的模型 Surface、展示顺序和字节长度一致。源展示通知的临时 `grow/historyCursor` 使用不同的索引前缀哈希，旧、新通知的原始序列化哈希因此不同。新会话写入前会移除来源游标。对同一个新 seed 使用 `SeedPresentation::into_facts` 后，逐条持久化内容相同。

## 新子会话 B 写入继承材料时的界面调度

`child_seed_publication_yields_the_command_executor` 在单线程 Tokio runtime 中发布 4 MiB 继承内容，并同时安排一个已就绪的界面任务。旧实现一次观测中，占用该线程 868.665 ms 直到发布结束，界面任务错过了这次调度。新实现将写入移到阻塞线程池。三次独立进程中，首次轮询分别在 90.583、90.917、77.750 µs 返回，界面任务均可在发布完成前执行。同期发布完整完成分别耗时 1.948、1.895、1.932 s。这三次运行期间系统还在运行其他验证，首轮调度延迟与持久化总耗时分别记录。

## 复现

令 `fixture` 指向临时目录下尚未存在的子路径。创建与测量必须在不同进程运行。将 `parent` 改为 `source-diagnostic` 或 `source-content` 可生成来源组，每组使用不同夹具路径。

```sh
export RUST_MIN_STACK=16777216
export CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_TEST_INCREMENTAL=false
export GROW_HOME="$fixture" GROW_RESUME_INDEXED_FIXTURE="$fixture"
GROW_RESUME_INDEXED_SCENARIO=parent cargo +1.93.1 test --offline --locked -p shell --lib create_published_resume_bench_fixture -- --ignored --nocapture
for width in 1 2 4; do
  GROW_RESUME_BENCH_WIDTH="$width" cargo +1.93.1 test --offline --locked -p shell --lib measure_published_resume_material -- --ignored --nocapture
done
```

对照组保留改动前的生产实现，只把同一个 benchmark 驱动适配到当时的 resolver 与 bootstrap 调用签名。比较时把相同的 `GROW_RESUME_INDEXED_FIXTURE` 分别传给两个已编译的测试程序。采样启动程序直接调用测试程序，排除了 Cargo、编译和链接。

复测活动 writer 时，用同一个已编译测试程序另建一份 `small-parent` 夹具及两份 `parent` 夹具。各跑一轮 `measure_published_resume_material`，设置 `GROW_RESUME_BENCH_WIDTH=4` 和 `GROW_RESUME_BENCH_LIVE_TAIL_MARKER`（夹具以外尚未存在的临时文件路径）。普通尾部使用 `GROW_RESUME_BENCH_TAIL_SCOPE=benchmark`，诊断尾部使用 `shell`。

macOS 按文件读取追踪沿用前一轮测量的 DYLD interpose 方法，对 `read`、`pread`、`readv`、`preadv` 的成功返回值按文件的设备号和 inode 求和。这里对父和四个来源的全部现有普通文件分别分组，`timeline.jsonl` 单列。以下拦截器在每次有实际数据返回时计入该 inode 对应的组。在线实验设置 marker 后才开始计数。

<details><summary>macOS 逻辑读取复现</summary>

将下列 C 源码保存到临时目录的 `trace.c`。在该目录运行 `clang -Os -dynamiclib trace.c -o trace.dylib`，将 `DYLD_INSERT_LIBRARIES` 指向动态库的绝对路径，将 `GROW_BENCH_SCOPE_DIR` 指向夹具中的 `sessions/%2Fworkspace`，再直接运行已构建的测试程序。

```c
#include <unistd.h>
#include <sys/stat.h>
#include <sys/uio.h>
#include <stdint.h>
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <errno.h>
#include <string.h>
#include <dirent.h>
#include <limits.h>
struct tracked { dev_t device; ino_t inode; size_t group; };
static struct tracked tracked[256];
static size_t count;
static const char *gate;
static _Atomic int armed;
static _Atomic uint64_t bytes[6];
__attribute__((constructor)) static void setup(void) {
    const char *dir = getenv("GROW_BENCH_SCOPE_DIR");
    if (!dir) return;
    gate=getenv("GROW_RESUME_BENCH_LIVE_TAIL_MARKER");
    armed=(gate==NULL);
    for (int i=0; i<=4; ++i) {
        char scope[PATH_MAX];
        if (!i) snprintf(scope,sizeof scope,"%s/parent-session",dir);
        else snprintf(scope,sizeof scope,"%s/source-%d",dir,i-1);
        DIR *d=opendir(scope);
        if (!d) abort();
        struct dirent *item;
        while ((item=readdir(d))) {
            char path[PATH_MAX]; struct stat s;
            snprintf(path,sizeof path,"%s/%s",scope,item->d_name);
            if (stat(path,&s) || !S_ISREG(s.st_mode)) continue;
            if (count >= sizeof tracked / sizeof tracked[0]) abort();
            size_t group=i==0 ? 0 : i==1 ? 2 : 4;
            tracked[count++]=(struct tracked){s.st_dev,s.st_ino,group+(strcmp(item->d_name,"timeline.jsonl")!=0)};
        }
        closedir(d);
    }
}
static ssize_t account(int fd,ssize_t n) {
    int saved=errno;
    if (!armed) { if (!gate || access(gate,F_OK)!=0) { errno=saved; return n; } armed=1; }
    struct stat s;
    if (count && n>0 && !fstat(fd,&s)) {
        for (size_t i=0;i<count;i++) {
            if (s.st_dev==tracked[i].device && s.st_ino==tracked[i].inode) {
                atomic_fetch_add(&bytes[tracked[i].group],(uint64_t)n);
                break;
            }
        }
    }
    errno=saved;
    return n;
}
static ssize_t rd(int fd,void *b,size_t n){ return account(fd,read(fd,b,n)); }
static ssize_t pr(int fd,void *b,size_t n,off_t o){ return account(fd,pread(fd,b,n,o)); }
static ssize_t rv(int fd,const struct iovec *v,int n){ return account(fd,readv(fd,v,n)); }
static ssize_t pv(int fd,const struct iovec *v,int n,off_t o){ return account(fd,preadv(fd,v,n,o)); }
__attribute__((used,section("__DATA,__interpose"))) static const struct { const void *wrapper,*original; } hooks[]={{rd,read},{pr,pread},{rv,readv},{pv,preadv}};
__attribute__((destructor)) static void finish(void) {
    if (count) fprintf(stderr,"parent_journal=%llu parent_other=%llu source_a_journal=%llu source_a_other=%llu siblings_journal=%llu siblings_other=%llu\n",
            (unsigned long long)bytes[0],(unsigned long long)bytes[1],(unsigned long long)bytes[2],
            (unsigned long long)bytes[3],(unsigned long long)bytes[4],(unsigned long long)bytes[5]);
}
```

</details>
