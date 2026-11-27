# 同父会话并发恢复的资源开销

同一个 32.04 MiB 父会话同时恢复 4 个不同来源时，Shell 校验线程共读取父 journal 128.18 MiB，进程峰值 RSS 为 165.45 MiB。只恢复一个来源时，父 journal 逻辑读取量为 32.04 MiB，峰值 RSS 为 66.73 MiB。热文件缓存下各组读盘为 0 B。单独把父文件准备为冷缓存后，各并发宽度的进程物理读取都为 32.04 MiB。

| 父 journal | 并发来源 | 父 journal 逻辑读取 B | 成功读取调用 | 峰值 RSS MiB | 相对开始时高水位增加 MiB | 全组耗时 ms |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 4,496 B | 1 | 8,992 | 2 | 34.48 | 17.20 | 2 |
| 4,496 B | 2 | 17,984 | 4 | 34.77 | 17.47 | 2 |
| 4,496 B | 4 | 35,968 | 8 | 35.41 | 18.13 | 2 |
| 33,592,916 B | 1 | 33,601,108 | 4,102 | 66.73 | 49.47 | 1,584 |
| 33,592,916 B | 2 | 67,202,216 | 8,204 | 99.75 | 82.47 | 1,591 |
| 33,592,916 B | 4 | 134,404,432 | 16,408 | 165.45 | 148.19 | 1,603 |

每个请求均调用 `durable_resume_source_for`。它独立打开同一父会话，通过 `validated_timeline` 校验 history，定位自己的 Spawn/Ended，再验证各自 child 的 Seed/Result。父 journal 每次完整读取前，`find_previous_newline` 还读取末尾最多 8 KiB 确认提交边界。小文件每次返回 4,496 B × 2，大文件每次返回 33,592,916 B + 8,192 B。与同并发数的小父会话对比，增加父记录带来的峰值 RSS 增量分别为 32.25、64.98、130.05 MiB。这组数据度量 Shell 来源校验，排除了后续子会话初始化、历史展示和模型请求。

## 父 journal 首次冷读

对 32.04 MiB 组使用同一个已编译的 Shell 基准二进制。每次测量前，独立准备进程把生成的合法夹具复制到一份新临时 home，仅父 journal 的新写入 fd 设置 macOS `F_NOCACHE`，写完 `fsync` 并关闭。新文件的内容由普通用户态复制写入，没有使用 APFS 克隆。每个并发宽度、每轮都重新建文件，并启动独立测量进程。子来源保持热缓存。进入 resolver 前后，通过只读私有映射和 `mincore` 的 `MINCORE_INCORE` 位计驻留页；检查本身没有访问映射的内容。未清理系统缓存，生产 reader 正常进行带缓存的读取。

| 并发来源 | 父文件驻留页（读前 → 读后） | 父 journal 逻辑读取 B | 进程物理读取 B | 峰值 RSS MiB | 全组耗时 ms |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 0 → 2,051 | 33,601,108 | 33,595,392 | 67.16 | 1,546 |
| 2 | 0 → 2,051 | 67,202,216 | 33,595,392 | 100.09 | 1,600 |
| 4 | 0 → 2,051 | 134,404,432 | 33,595,392 | 165.94 | 1,640 |

每个并发宽度三轮独立运行，再用 inode 插桩多跑一轮，12 次均观察到全部 2,051 页由冷变热。四个校验任务分别解码同一份历史，重复消耗内存；它们共享操作系统文件缓存。本机同一批次的物理读取约为一份文件。磁盘计数来自内核归属于测量进程的 I/O，不含 SSD 控制器内部行为。不同批次之间的缓存回收、其他负载和存储设备会影响结果。

复测时用基准生成的合成 home 作模板，给每个宽度各建一个临时目录，普通复制各子会话及 summary。对父 journal 使用下面的写入方式，再在新进程中将 `GROW_HOME` 指向该临时目录并设置 `GROW_RESUME_BENCH_WIDTH=1`、`2` 或 `4`，运行同一个忽略测试。该方式只影响创建父文件的打开描述符。

```python
with template_journal.open("rb") as src, new_journal.open("xb", buffering=0) as dst:
    fcntl.fcntl(dst, 48, 1)  # F_NOCACHE in the macOS SDK sys/fcntl.h
    shutil.copyfileobj(src, dst, 256 * 1024)
    os.fsync(dst.fileno())
```

## 复现

测试在独立临时 Grow home 中通过生产 journal 编码器写入 4 条可验证的 Spawn/Ended 与 Seed/Result 链。对照组仅保留委派事实。大父会话在四条链后追加 128 条各含 256 KiB 文本的 Observation，子会话的内容和链接结构相同。每个并发数组合都启动新进程，通过 Tokio 同时提交真实 resolver；所有调用成功且返回各自请求的来源身份。夹具构造进程的内存占用不进入测量。

环境为 macOS 27.0.1 arm64，Mac16,8，12 核，24 GiB 内存，APFS SSD，Rust 1.93.1，未优化的测试构建。峰值 RSS 使用 `getrusage(RUSAGE_SELF).ru_maxrss`，物理读取使用 `proc_pid_rusage(RUSAGE_INFO_V2).ri_diskio_bytesread` 在解析前后的进程计数差值。两表的耗时及峰值 RSS 均为三次独立运行的中位数。热缓存组每次生成文件后直接执行；冷缓存组只改变父 journal 的准备方式。耗时来自测试构建。

```sh
RUST_MIN_STACK=16777216 CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_TEST_INCREMENTAL=false \
  cargo +1.93.1 test --locked --offline -p shell --lib measure_concurrent_resume_source_reads -- --ignored --nocapture
```

另跑一轮 macOS 逻辑读跟踪，按当前夹具父文件的 `(st_dev, st_ino)` 累加实际 `read` 和 `pread` 成功返回的字节数。此轮耗时和 RSS 单独记录，未用于上表的中位数。将下方代码存为临时 `trace-journal.c`，编译后把所得库的绝对路径传给同一个测试。

```sh
/usr/bin/clang -Wall -Wextra -Os -dynamiclib trace-journal.c -o trace-journal.dylib
GROW_RESUME_BENCH_READ_TRACER="$PWD/trace-journal.dylib" RUST_MIN_STACK=16777216 \
  CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_TEST_INCREMENTAL=false \
  cargo +1.93.1 test --locked --offline -p shell --lib measure_concurrent_resume_source_reads -- --ignored --nocapture
```

```c
#include <unistd.h>
#include <sys/stat.h>
#include <stdint.h>
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <errno.h>

static struct stat parent;
static int enabled;
static _Atomic uint64_t bytes;
static _Atomic uint64_t calls;

__attribute__((constructor)) static void start(void) {
    const char *p = getenv("GROW_RESUME_BENCH_PARENT_JOURNAL");
    enabled = p && stat(p, &parent) == 0;
}
static ssize_t account(int fd, ssize_t got) {
    int saved = errno;
    struct stat entry;
    if (enabled && got > 0 && fstat(fd, &entry) == 0 &&
        entry.st_dev == parent.st_dev && entry.st_ino == parent.st_ino) {
        atomic_fetch_add(&bytes, (uint64_t)got);
        atomic_fetch_add(&calls, 1);
    }
    errno = saved;
    return got;
}
static ssize_t counted_read(int fd, void *buf, size_t count) {
    return account(fd, read(fd, buf, count));
}
static ssize_t counted_pread(int fd, void *buf, size_t count, off_t offset) {
    return account(fd, pread(fd, buf, count, offset));
}
__attribute__((used, section("__DATA,__interpose"))) static const struct {
    const void *replace, *original;
} hooks[] = {{counted_read, read}, {counted_pread, pread}};
__attribute__((destructor)) static void finish(void) {
    if (enabled) fprintf(stderr, "parent_logical_read_bytes=%llu successful_calls=%llu\n",
        (unsigned long long)bytes, (unsigned long long)calls);
}
```
