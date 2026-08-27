# Calman

## 1. 项目定位
**calman** 是一个**极简的、键盘驱动的任务管理前端**。它不内置任何同步逻辑，而是将同步操作完全委托给用户配置的外部命令（`pimsync`、`git`、`rclone` 等）。

它遵循 Unix 哲学：**只做一件事，并把它做好**——提供一流的 CLI 和 TUI 体验来管理本地存储的任务数据，同时将“如何同步”的决定权完全交给用户。

## 2. 核心设计原则
1.  **存储即文件**：任务数据以 `jsonl`（默认，高性能）或 `ics`（兼容 CalDAV/VDir 格式）存储在本地。
2.  **同步即命令**：同步就是执行一串用户自定义的 shell 命令（`pre_hook` → `cmd` → `post_hook`）。
3.  **界面即效率**：CLI 完整复刻 Taskwarrior 的操作习惯（自然语言日期、短 ID、筛选器）；TUI 采用“列表 + 详情”左右双面板布局，最大限度减少界面噪音。
4.  **模态交互**：TUI 严格区分浏览模式与编辑模式，防止误操作。
5.  **配置即代码**：所有 TUI 设置（显示源、过滤器、键位）持久化写入 `config.toml`。

## 3. 技术栈

| 模块 | 选型 | 说明 |
| :--- | :--- | :--- |
| **语言** | Rust (Edition 2024) | 无 GC、零运行时、极致性能与低内存占用 |
| **TUI** | `ratatui` + `crossterm` | Rust 生态最成熟的 TUI 库，跨平台支持 |
| **CLI 解析** | `clap` (Derive 风格) | 支持复杂子命令和参数校验 |
| **HTTP (可选)** | `reqwest` | (仅当用户自定义命令中需要时) |
| **iCalendar** | `ical` / `vobject` crate | 读写 `.ics` 文件（VTODO 和 VEVENT） |
| **JSONL** | `serde_json` 流式解析 | 逐行读写，内存友好，Git diff 友好 |
| **日期解析** | `chrono` + `pest`/`nom` 自研解析器 | 支持 Taskwarrior 风格自然语言 |
| **配置** | `toml` + `serde`；写入时使用 `toml_edit` | 保留格式与注释 |
| **命令执行** | `std::process::Command` | 调用系统 `sh` 执行用户自定义同步命令 |
| **日志** | `tracing` / `env_logger` | 分级日志，便于调试 |

## 4. 主要功能亮点
-   **多源管理**：支持同时管理多个数据源（如 `work`, `personal`），并针对 CLI、Sync、TUI 三种场景分别配置默认源列表。
-   **双存储格式**：支持 `jsonl`（默认，极快）和 `ics`（兼容 vdirsyncer/pimsync 生态）。
-   **原子写入**：所有数据写入均采用“临时文件 + rename”机制，杜绝文件损坏。
-   **外部同步**：通过 `pre_hook`, `cmd`, `post_hook` 调用外部工具（如 `git`, `pimsync`, `rclone`），并支持 `{location}` 占位符。
-   **CLI 体验**：短 ID 系统、自然语言日期（`due:today`, `eow`, `eond` 等）、`+OVERDUE` 预定义筛选器、`count` 子命令。
-   **TUI 体验**：左右双面板布局（列表 + 预览/编辑），支持 Vim 风格及方向键两种键位模式，设置面板实时写入配置。


# calman — 详细功能设计文档 (DESIGN.md)

本文档相当于 **calman 的完整手册页（Man Page）**。它详细描述了配置格式、数据模型、CLI 命令语法、TUI 交互逻辑以及所有内部机制。第三方开发者可根据本文档重写一个功能完全一致的应用。

---

## 1. 配置文件设计 (`~/.config/calman/config.toml`)

### 1.1 完整结构
```toml
[defaults]
write_source = "work"           # 执行 `add` 时的默认写入目标（单一字符串）

[contexts]
cli = ["work", "personal"]      # `list`, `count`, `done` 等命令默认查询的源列表
sync = ["work"]                 # `sync` 命令默认同步的源列表
tui = ["work", "personal"]      # `tui` 启动时默认显示的源列表

[date]
workweek_end = "17:00"          # 用于计算 `eoww` 的时间点
week_start = "monday"           # 周起始日

[ui]
theme = "default"               # default | dark | light
vim_keys = true                 # true=Vim 风格 (j/k/gg/G), false=方向键
default_filter = "all"          # "todo" | "event" | "all"

[locale]
language = "en"                 # en | zh-CN

[[source]]
name = "work"                   # 唯一名称
type = "ics"                    # "jsonl" 或 "ics"
location = "~/.local/share/calman/work/"

[source.sync]                   # 同步配置块（可选）
pre_hook = "git pull --rebase origin main"
cmd = "git add . && git commit -m 'calman sync' && git push"
post_hook = "echo 'done'"
```

### 1.2 字段详细说明
-   **`defaults.write_source`**: 必须为单一有效 `source.name`。
-   **`contexts`**:
    -   若某项未配置，回退逻辑为 **遍历所有 `[[source]]`**（对于 `sync`，额外过滤掉未配置 `sync.cmd` 的源）。
-   **`source.sync`**:
    -   `cmd` 不存在时，该 Source 被视为“不支持同步”，在 `sync` 上下文回退时会被自动忽略。
    -   支持占位符 `{location}` 和 `{name}`，执行时会替换为实际值。
    -   执行时，**工作目录 (current_dir)** 会被设置为 `source.location`。

---

## 2. 数据模型与存储

### 2.1 统一数据模型 (内部表示)
```rust
pub struct Task {
    // --- 标识与归属 ---
    pub uid: String,                // UUID v4
    pub source: String,             // 对应 source.name

    // --- 核心字段 ---
    pub summary: String,
    pub description: Option<String>,
    pub status: TaskStatus,         // Pending, InProgress, Completed, Cancelled
    pub priority: Option<u8>,       // 0-9, 0 最低
    pub tags: Vec<String>,

    // --- VTODO (任务) ---
    pub due: Option<DateTime<Utc>>,
    pub percent_complete: Option<u8>,
    pub completed_at: Option<DateTime<Utc>>,

    // --- VEVENT (日程) ---
    pub dtstart: Option<DateTime<Utc>>,
    pub dtend: Option<DateTime<Utc>>,
    pub rrule: Option<String>,      // "FREQ=WEEKLY;COUNT=10"
    pub location: Option<String>,

    // --- 关联 ---
    pub related_to: Option<String>, // 父任务 UID

    // --- 时间戳 ---
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub enum TaskStatus { Pending, InProgress, Completed, Cancelled }
```
**类型判定规则**：
-   若 `dtstart` 不为 `None`，视为 **Event (日程)**。
-   否则视为 **Todo (任务)**。

### 2.2 存储格式细节
#### A. `type = "jsonl"` (推荐)
-   **文件**：`<location>/tasks.jsonl`。
-   **格式**：每行一个完整的 JSON 对象（包含上述 `Task` 所有字段）。
-   **读取**：逐行流式读取，构建索引（UID -> 行偏移量）以支持快速随机写入。
-   **写入**：修改时**全量写入临时文件** `tasks.jsonl.tmp`，成功后 `rename` 覆盖原文件（原子操作）。
-   **新行追加**：`add` 时直接在末尾追加一行，无需重写整个文件（除非需要修改）。

#### B. `type = "ics"` (兼容模式)
-   **目录**：`<location>/`。
-   **文件**：每个任务一个 `<UID>.ics` 文件。
-   **内容**：标准 iCalendar 格式，包含 `BEGIN:VCALENDAR` ... `END:VCALENDAR`。
-   **写入**：同样采用“先写临时文件，再 rename”的策略。

### 2.3 元数据文件
-   **文件**：`<location>/.calman-state.json`
-   **内容**：
    ```json
    {
      "version": 1,
      "uid_counter": 1234,
      "last_modified": "2026-08-24T10:00:00Z"
    }
    ```

---

## 3. 同步机制 (Sync)

### 3.1 执行流程
1.  解析目标 Source 列表（若命令行指定则用指定的，否则用 `contexts.sync` 或回退）。
2.  对每个 Source：
    a. 检查 `location/.sync.lock` 是否存在，存在则跳过并警告。
    b. 创建 `.sync.lock`。
    c. 顺序执行 `pre_hook` → `cmd` → `post_hook`。
    d. 若 `pre_hook` 或 `cmd` 退出码非 0，**立即终止**并删除锁文件，报错退出。
    e. 删除锁文件。
    f. 更新 `.calman-state.json` 中的 `last_modified`。

### 3.2 冲突处理策略
-   **calman 不处理冲突**。
-   用户需在自定义命令中处理（如 `git pull --rebase` 或 `pimsync` 的冲突标记）。
-   若外部命令导致数据文件格式错误，calman 会拒绝读取并提示用户手动修复。

---

## 4. CLI 模式 (命令行接口)

### 4.1 通用参数
```bash
calman [--source <source1,source2,...>] <COMMAND> [ARGS]
```
-   `--source`：**接受逗号分隔的列表**（例如 `--source work,personal`）。
-   **优先级**：命令行指定的 `--source` > `[contexts].{context}` > 所有 Sources（sync 时排除无 cmd 的源）。

### 4.2 子命令详细规范

#### A. `add` — 添加任务/日程
-   **语法**：`calman add <SUMMARY> [OPTIONS]`
-   **写入目标**：命令行 `--source` 若提供（必须为单一值），则覆盖 `defaults.write_source`。若 `--source` 提供了多个，报错。
-   **选项**：
    -   `--due <DATE>`: 自然语言日期 (Todo)
    -   `--priority <LEVEL>`: `high` (9), `medium` (5), `low` (1) 或数字 0-9
    -   `--tags <TAGS>`: 逗号分隔
    -   `--description <TEXT>`: 长描述
    -   `--start <DATE>`: 开始时间 (Event)
    -   `--end <DATE>`: 结束时间 (Event)
    -   `--location <TEXT>`: 地点 (Event)
    -   `--repeat <RRULE>`: 重复规则 (Event)

#### B. `list` — 列出任务
-   **语法**：`calman list [FILTERS...] [--format table|json|csv]`
-   **数据源**：读取 `contexts.cli` 或 `--source` 指定的源列表。
-   **输出**：第一列为动态短 ID（跨所有 Source 统一编号）。
-   **筛选器 (Filters)**：
    -   `due:<date>`: 截止日期
    -   `status:<status>`: `pending`, `in-progress`, `completed`
    -   `priority:<level>`: `high`, `medium`, `low` 或数字范围 `priority:5-9`
    -   `tags:<tag>`: 包含标签
    -   `+OVERDUE`: 过期且未完成
    -   `+PENDING`: 待办或进行中
    -   `+COMPLETED`: 已完成

#### C. `done` — 完成任务
-   **语法**：`calman done <ID> [ID...]` 或 `calman done --uid <UID>`
-   **作用域**：基于当前 `--source` 或 `contexts.cli` 合并列表中的短 ID。
-   **行为**：将 `status` 设为 `Completed`，记录 `completed_at`。

#### D. `delete` — 删除任务
-   **语法**：`calman delete <ID> [ID...]` 或 `calman delete --uid <UID>`
-   **行为**：从存储中永久移除。

#### E. `modify` — 修改任务
-   **语法**：`calman modify <ID> <FIELD>=<VALUE> ...`
-   **示例**：`calman modify 1 due=tomorrow priority=high`
-   **支持字段**：`summary`, `description`, `due`, `priority`, `tags`, `status`, `percent` (Todo)；`dtstart`, `dtend`, `location`, `rrule` (Event)；`related_to` (通用)。

#### F. `count` — 计数
-   **语法**：`calman count [FILTERS...]`
-   **输出**：纯数字（无额外文本）。若 `--format json` 或 `csv` 则输出对应格式。
-   **用途**：脚本组合，如 `alert $(calman count +OVERDUE) overdue tasks.`

#### G. `sync` — 执行同步
-   **语法**：`calman sync [--source <source1,source2,...>]`
-   **默认源**：`contexts.sync`。若未指定或未配置，则同步所有配置了 `cmd` 的源。
-   **输出**：实时显示外部命令的 stdout/stderr。

#### H. `tui` — 启动终端界面
-   **语法**：`calman tui [--source <source1,source2,...>]`
-   **默认源**：`contexts.tui`。若未指定或未配置，则显示所有源。

---

## 5. TUI 模式 (终端用户界面)

### 5.1 布局规范
-   **左右比例**：列表占 40%，详情占 60%。
-   **顶部状态栏**：显示当前激活的 Source 列表、同步状态、快捷键提示。
-   **底部状态栏**：显示当前模式（列表/详情/编辑）和上下文快捷键。

### 5.2 交互模式 (状态机)

#### A. 列表模式 (默认)
-   **焦点**：左侧列表。
-   **导航**：
    -   Vim 模式 (`vim_keys=true`)：`j/k` 上下，`gg` 首，`G` 尾。
    -   方向键模式 (`vim_keys=false`)：`↑/↓` 上下，`Home` 首，`End` 尾。
-   **右侧预览**：实时显示选中项的完整信息（只读）。
-   **进入详情**：按 `Enter`。

#### B. 详情模式 (浏览)
-   **焦点**：右侧详情面板。
-   **滚动**：`j/k` 或 `↑/↓` 滚动长文本（描述/元数据）。
-   **进入编辑**：
    -   Vim 模式：按 `i`。
    -   方向键模式：**自动进入编辑模式**。
-   **返回**：按 `Esc` 返回列表模式。

#### C. 编辑模式
-   **焦点**：右侧详情面板，高亮当前编辑字段。
-   **字段导航**：`Tab` 切换下一个字段，`Shift+Tab` 切换上一个。
-   **保存**：`Ctrl+Enter` 保存所有修改，返回列表模式。
-   **放弃**：`Esc` 放弃所有修改，返回列表模式。

### 5.3 详情面板内容结构
```
📋 Todo / 📅 Event                [Source: work]
─────────────────────────────────────────────────
<Summary (大字号)>

● Pending   ████████░░░ (7/9)    #shopping #errand
─────────────────────────────────────────────────
── 时间 ──
Due:  2026-08-24 17:00  ⚠️ (逾期)
─────────────────────────────────────────────────
── 描述 ──
<description 自动换行>
─────────────────────────────────────────────────
── 元数据 ──
Source:     work
UID:        abc-123
Related:    parent-uid (若有)
创建:       2026-08-20 14:30
更新:       2026-08-22 09:15
```
**可编辑字段 (Tab 顺序)**：
1.  `source` (下拉选择框，可移动任务到其他 Source)
2.  `summary` (文本输入)
3.  `status` (下拉选择)
4.  `priority` (数字/滑块)
5.  `tags` (文本输入，逗号分隔)
6.  `due` (Todo) / `dtstart` (Event) (日期输入)
7.  `percent_complete` (Todo) / `dtend` (Event) (数字/日期输入)
8.  `location` (Event) (文本输入)
9.  `rrule` (Event) (文本输入)
10. `related_to` (文本输入)
11. `description` (多行文本输入框)

### 5.4 设置面板 (Settings Overlay)
-   **呼出**：`Ctrl+p`。
-   **功能**：
    -   **显示源**：复选框，修改 `contexts.tui` 并立即刷新列表。
    -   **写入源**：下拉选择，修改 `defaults.write_source`。
    -   **过滤器**：单选按钮 (`Todo`, `Event`, `Both`)，修改 `ui.default_filter`。
    -   **键位**：单选框 (`Vim` / `Arrow`)，修改 `ui.vim_keys`。
-   **持久化**：点击 **`Apply`** 后，使用 `toml_edit` 原子写入 `config.toml`。点击 **`Cancel`** 放弃修改。

### 5.5 全局快捷键表
| 按键 | 列表模式 | 详情模式 | 编辑模式 |
| :--- | :--- | :--- | :--- |
| `j` / `↓` | 下移 | 滚动 (Vim) | 当前字段输入 |
| `k` / `↑` | 上移 | 滚动 (Vim) | 当前字段输入 |
| `Enter` | 进入详情 | — | 确认字段值 |
| `Tab` | — | — | 切换下一字段 |
| `i` (仅Vim) | — | 进入编辑 | — |
| `Ctrl+Enter` | — | — | **保存并返回列表** |
| `Esc` | — | 返回列表 | **放弃并返回列表** |
| `a` | 添加对话框 | — | — |
| `d` | 标记完成 | — | — |
| `D` | 删除 (确认) | — | — |
| `Ctrl+p` | 打开设置 | 打开设置 | 打开设置 |
| `q` | 退出 | 退出 | 需先 Esc 退出编辑 |

---

## 6. 日期解析引擎

### 6.1 支持的输入格式
-   **绝对时间**：`2026-08-25`, `08/25/2026`, `2026-08-25 14:30`。
-   **相对偏移**：`+3d` (3天后), `-2w` (2周前), `+1m` (1月), `+1y` (1年)。
-   **关键词**：`today`, `tomorrow`, `yesterday`, `now`, `eow` (周日 23:59), `eoww` (周五 17:00), `eond` (明天 23:59), `sow` (周一 00:00), `eom` (月末 23:59)。
-   **星期**：`monday`/`mon`, `friday`/`fri`, 支持 `fri+1` (下周五)。
-   **时间**：`2pm`, `14:30` (若单独出现则附加到今日)。

### 6.2 时区处理
1.  解析时**假定为系统本地时区** (Local)。
2.  存储时通过 `chrono` 转换为 **UTC** (`DateTime<Utc>`)。
3.  显示时再转换回本地时区。

# 文件三：PLAN.md（分阶段开发计划）

```markdown
# calman — 分阶段开发计划 (PLAN.md)

## 阶段一：核心引擎与 CLI 基础 (预估 3-4 周)
**目标**：实现可用的 CLI 工具，能增删改查任务，并能调用外部命令同步。

### 1.1 基础框架 (Day 1-3)
- [ ] 初始化 Rust 项目 (`cargo init`)。
- [ ] 添加依赖：`clap` (derive), `serde`, `serde_json`, `toml`, `toml_edit`, `chrono`, `anyhow`, `thiserror`。
- [ ] 实现配置加载器 (`config.rs`)：
    - 读取 `~/.config/calman/config.toml`。
    - 实现默认配置生成逻辑。
    - 支持 `contexts` 回退逻辑。

### 1.2 数据模型与存储 (Day 4-10)
- [ ] 定义 `Task` 结构体及 `TaskStatus` 枚举 (`model.rs`)。
- [ ] 实现 `Storage` Trait：
    - `JsonlStorage`：实现 `tasks.jsonl` 的读取、追加、原子写入、按 UID 修改/删除。
    - `IcsStorage`：实现 `*.ics` 文件的读写（使用 `ical` crate）。
- [ ] 实现状态管理 (`state.rs`)：读写 `.calman-state.json`，管理 `uid_counter`。

### 1.3 同步执行器 (Day 11-14)
- [ ] 实现 `sync/executor.rs`：
    - 解析 `pre_hook`, `cmd`, `post_hook` 中的 `{location}` 占位符。
    - 调用 `std::process::Command` 执行命令。
    - 实现 `.sync.lock` 锁机制。
    - 实现短路退出逻辑 (pre_hook 失败则终止)。

### 1.4 CLI 命令骨架 (Day 15-21)
- [ ] 在 `main.rs` 中使用 `clap` 定义子命令结构。
- [ ] 实现 `add` 命令 (包含写入源逻辑)。
- [ ] 实现 `list` 命令 (包含跨 Source 合并、短 ID 生成、表格/JSON/CSV 输出)。
- [ ] 实现 `done` / `delete` / `modify` 命令 (支持短 ID 和 `--uid`)。
- [ ] 实现 `count` 命令 (支持 `+OVERDUE` 等筛选器)。

---

## 阶段二：CLI 高级特性与集成 (预估 2-3 周)
**目标**：完善 CLI 体验，使其达到 Taskwarrior 的可用度。

### 2.1 日期解析引擎 (Day 22-28)
- [ ] 使用 `pest` 或 `nom` 定义日期语法。
- [ ] 实现 `DateParser`：
    - 处理 `eow`, `eoww`, `eond` 等关键词。
    - 处理偏移量 (`+3d`)。
    - 处理星期几 (`fri+1`)。
    - 集成 `chrono` 进行 UTC 转换。

### 2.2 筛选器引擎 (Day 29-33)
- [ ] 解析 `list` 和 `count` 的命令行参数。
- [ ] 实现 `Filter` 结构体，支持 `due`, `status`, `priority`, `tags` 过滤。
- [ ] 实现预定义筛选器映射 (`+OVERDUE` 等)。

### 2.3 集成测试与 CLI 文档 (Day 34-35)
- [ ] 编写集成测试 (`tests/`) 覆盖核心命令。
- [ ] 完善 `--help` 输出。

---

## 阶段三：TUI 完整实现 (预估 4-5 周)
**目标**：实现左右双面板 TUI，支持浏览、编辑、设置持久化。

### 3.1 TUI 骨架与布局 (Day 36-40)
- [ ] 引入 `ratatui`, `crossterm`, `tui-react` (或自定义事件循环)。
- [ ] 实现应用状态机 (`AppState`)：List, Detail, Edit, Settings。
- [ ] 实现左右布局渲染：列表占 40%，详情占 60%。

### 3.2 列表面板与详情预览 (Day 41-47)
- [ ] 列表渲染：支持高亮、滚动、筛选 (结合 `contexts.tui`)。
- [ ] 详情渲染 (`DetailPane`)：
    - 渲染 Header (Type, Source)。
    - 渲染 Status Bar (状态, 优先级, 标签)。
    - 渲染动态时间区 (Todo/Event 切换)。
    - 渲染元数据 (包含 Source 字段)。

### 3.3 模态交互与编辑功能 (Day 48-55)
- [ ] 实现列表模式 → 详情模式 → 编辑模式的完整状态流转。
- [ ] 实现字段编辑器 (文本输入、下拉选择、日期输入)。
- [ ] 实现 `Ctrl+Enter` 保存与 `Esc` 放弃逻辑。
- [ ] 实现 `source` 字段编辑（移动任务到其他 Source）。

### 3.4 设置面板与持久化 (Day 56-60)
- [ ] 实现 `SettingsOverlay` (浮动窗口)。
- [ ] 绑定 `Ctrl+p` 呼出。
- [ ] 使用 `toml_edit` 实现配置的原子写入。
- [ ] 实现键位模式切换（Vim / 方向键）并即时生效。

### 3.5 同步指示与杂项 (Day 61-65)
- [ ] TUI 中触发 `sync` (按 `s`) 并显示外部命令输出日志。
- [ ] 实现添加对话框 (`AddDialog`)。
- [ ] 实现多语言框架 (i18n) 初始化。
- [ ] 性能优化：大文件 (1000+ 任务) 下的滚动与渲染优化。

---

## 4. 里程碑与发布
- **v0.1.0 (Alpha)**：阶段一完成，可用 CLI 进行基础任务管理。
- **v0.2.0 (Beta)**：阶段二完成，CLI 功能追上 Taskwarrior 核心特性。
- **v1.0.0 (Stable)**：阶段三完成，TUI 稳定，文档齐全，正式发布。

## 5. 风险与应对
| 风险 | 应对策略 |
| :--- | :--- |
| **ICS 格式兼容性** | 使用成熟的 `ical` crate，并编写针对 Radicale 的兼容性测试。 |
| **并发写入冲突** | 依赖 `.sync.lock` 和外部命令的事务性（如 git 的原子性）。 |
| **TUI 性能** | JSONL 采用流式读取，详情渲染仅针对当前选中项，避免全量遍历。 |
| **配置写入冲突** | 使用 `toml_edit` 保留注释，若文件被外部修改则提示用户重启。 |

