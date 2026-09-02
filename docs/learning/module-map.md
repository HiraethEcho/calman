# Module Map — calman 模块导览

Each row: file, what it does, the key types/functions, and what to look for.
Read in the numbered order — it follows dependencies (leaf first).

每行：文件、功能、关键类型/函数、看什么。按编号顺序读（从底层到上层）。

## Reading order 阅读顺序

### 1. `src/model.rs` — 核心数据模型
- `Task` struct（一条任务/事件的所有字段），`TaskStatus` enum。
- `Task::new()` 创建，`is_event()`/`is_parent()` 判断类型。
- 看：所有权、`Option`、`#[derive]`、测试。← 最好的起点

### 2. `src/date/mod.rs`, `src/date/natural.rs`, `src/date/ical.rs` — 日期解析
- 把 `tomorrow`、`eod`、`20260925` 转成 `DateTime<Utc>`。
- 看：`chrono` 时间库，字符串解析，时区。

### 3. `src/config.rs`, `src/source.rs` — 配置与数据源
- `Config` 结构、`Config::load()`、`context_sources()`。
- `source.rs` 解析 source 名字（含 `ics-dir` 集合展开）。
- 看：`serde` + TOML，`PathBuf`，错误处理 `Context`。

### 4. `src/storage/mod.rs`, `src/storage/jsonl.rs`, `src/storage/state.rs`, `src/storage/ics.rs` — 数据存取
- `Storage` trait + `Store` 枚举（选后端）。
- jsonl 一行一条 JSON；ics 是 RFC 5545 日历文件。
- 看：**trait 多态**、文件读写、原子写（tmp+rename）。

### 5. `src/sync/executor.rs` — 外部同步
- 运行 `pre_hook` → `cmd` → `post_hook`，用锁文件防止并发。
- 看：进程调用，锁，shell 命令。

### 6. `src/args.rs` — 命令行参数解析
- 自由格式 → `ParsedArgs`（命令、ID、字段、过滤器 token）。
- 看：命令分发（`Command` enum），Token 解析。

### 7. `src/filter.rs` — 过滤器引擎
- 表达式（`status:pending and +OVERDUE`、括号）→ 表达式树 → 匹配 Task。
- 看：递归解析，`and/or` 优先级。

### 8. `src/recurrence.rs`, `src/recur_expand.rs` — 循环任务
- `recur:daily` → RFC 5545 `RRULE` 规范化。
- `recur_expand`（可选特性）把 series 展开成 occurrence。
- 看：字符串解析、可选特性 `#[cfg]`。

### 9. `src/report.rs` — 报告渲染
- 选 report、排序、渲染成表格文本（列/图标/颜色）。
- 看：文本格式化、`unicode-width` 对齐。

### 10. `src/cli/mod.rs` + 子命令 (`add.rs` `list.rs` `done.rs` `delete.rs` `modify.rs` `count.rs` `info.rs` `start.rs` `stop.rs` `series.rs` `sync.rs`)
- 每个命令一个 handler。共享 helper：`resolve_sources`、`load_merged(_expanded)`、`resolve_targets`。
- 看：命令如何读数据、改数据、打印。

### 11. `src/main.rs` — 入口（最后再读一遍）
- 把所有模块接起来：解析 → 配置 → 分发。
- 看：`mod` 声明、`match cmd` 分发、`?` 错误传播。

## 依赖简图 (dependency sketch)

```
model ──▶ date ──▶ config/source ──▶ storage ──▶ sync
                └──▶ args ──▶ filter ──▶ recurrence ──▶ report
                                  └────────────────────▶ cli ──▶ main
```

## 可选特性 (optional features, `Cargo.toml`)
- `recur-expand` — 展开循环 occurrence（默认开）。
- `storage-ics` / `storage-jsonl` — 存储后端（默认都开）。
- `tui` — Phase-3 图形界面（未实现，只有占位）。

## 测试位置 (where tests live)
- 单元测试：每个文件底部 `#[cfg(test)] mod tests`。
- 集成测试：`tests/cli.rs`（端到端命令行行为）。
- 跑法：`cargo test`。
