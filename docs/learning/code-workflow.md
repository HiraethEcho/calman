# Code Workflow — calman 程序流程

This doc traces what happens when you type a command. Read it after
`rust-basics.md`.

本文档追踪你输入一条命令后发生了什么。

## Big picture 总览

```
你输入命令 (you type)
   │
   ▼
main()           入口，拿到原始参数 (raw args)
   │
   ▼
config::load()   读取 ~/.config/calman/config.toml (无则生成默认)
   │
   ▼
args::parse()    把自由格式参数解析成 ParsedArgs (命令 + 过滤器 + 字段)
   │
   ▼
match cmd        根据命令分发到不同 handler
   │
   ├─ list/count/info  → 读存储 → 过滤 → 渲染
   ├─ add/done/delete/modify/start/stop → 改存储
   └─ sync             → 运行外部命令
```

## Step by step 一步步

### 1. `main()` — `src/main.rs`
- `Cli::parse()` 用 `clap` 收集命令行参数到一个 `Vec<String>`（`args` 字段）。
- `config::Config::load()?` 读配置。`?` 表示出错就退出。
- `date::set_day_bounds(...)` 设置日界（如 `eod` 边界）。
- `args::parse(&cli.args)?` 把参数解析成 `ParsedArgs`。
- `match q.cmd { ... }` 分发：`None` → 默认 report；`Some(Command::X)` → 对应 handler。

### 2. 配置 — `src/config.rs`
- `Config` 是根结构，包含 `defaults`/`contexts`/`date`/`sources`/`reports` 等。
- `serde` 把 TOML 映射到结构体；`include` 文件可合并。
- `Config::context_sources()` 决定「本次用哪些 source」。

### 3. 参数解析 — `src/args.rs`
- 把 Taskwarrior 风格输入（`add buy milk due:tomorrow pri:H +home`）解析成结构化字段。
- 产出 `ParsedArgs`：`cmd`、`ids`、`text`、`due`、`tags`、`sources`、过滤器 token 等。

### 4. 数据层 — `src/storage/`
- `Storage` trait 定义统一接口：`list`/`add`/`update`/`remove`。
- `jsonl.rs` / `ics.rs` 两种后端都实现它。`Store` 枚举在运行时选择具体后端。
- `cli::open_storage()` 按 source 类型打开对应后端。

### 5. 读取与合并 — `src/cli/mod.rs`
- `load_merged()` 把各 source 的任务读进来，按 `created_at` 排序，赋连续 ID（最旧的 = 1）。
- `load_merged_expanded()` 额外把循环任务展开成虚拟 occurrence 行（`recur-expand` 特性）。

### 6. 过滤 — `src/filter.rs`
- 过滤器表达式（`status:pending`、`+OVERDUE`、`and/or`、括号）解析成表达式树。
- 每个 `Task` 用 `matches()` 判断是否命中。

### 7. 渲染 — `src/report.rs`
- `Report::resolve()` 选 report（内置 `ls`/`list`/`next` 或用户自定义）。
- 排序行 → 渲染成表格文本（列、图标、颜色）→ `print!` 到终端。

### 8. 写操作 handler — `src/cli/*.rs`
- 例如 `add`：构造 `Task::new()`，设字段，`storage.add(task)`。
- `done`：按 ID 解析任务 → `storage.update()` 改状态。
- 修改通过 `Storage::update(uid, closure)` —— 闭包改字段，出错则中止写。

## 数据流小结 (data flow)

```
config.toml ──serde──▶ Config
参数 ──args::parse──▶ ParsedArgs
ParsedArgs + Config ──▶ 选 source ──▶ open_storage ──▶ Store(list/add/...)
Task 列表 ──filter──▶ 命中行 ──sort──▶ report::render ──▶ stdout
```

## Key concepts 关键概念

- **`?`** — 出错即返回错误，逐层向上传播到 `main`。
- **Trait 多态** — CLI 不知道也无需知道数据是 jsonl 还是 ics；只看 `Storage` 接口。
- **`Store` 枚举** — 运行时按配置选后端，把 trait 对象变成具体类型。
- **闭包 (closure)** — `update(uid, |t| { t.status = ... })` 允许在写入时批量改字段。

Next: open `module-map.md` for the per-file reading order.
