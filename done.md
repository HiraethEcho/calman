# Done

已完成功能清单（来自 feature.md / fix.md，截至 v0.1.0）。

## CLI / Filter
- 共享 filter 语法（CLI + report filter）：
  - `and` / `or` / `(` / `)`，相邻 atom 隐式 `and`；`and` 优先于 `or`
  - `type:todo` / `type:event` / `type:all`
  - `source:<name>` / `src:<name>` / `-source:<name>` / `-src:<name>`
  - `due:<date>`（当天）`due.before:` `due.by:` `due.after:`
  - todo 比 `due`，event 比 `dtstart`
  - 虚拟标签：`+OVERDUE +PENDING +COMPLETED +CANCELLED +IN-PROCESS +TAGGED
    +UNTAGGED +SCHEDULED +PARENT`
- rc 覆盖：`rc.report.<name>.columns/labels/filter/sort`；`columns` 支持
  `field` / `field.format`。

## Date
- `T` 紧凑格式：`20260812T090000`、`0826T0930`、`T0900`、`25`（本月第 25 天）
- ISO `YYYY-MM-DD [HH:MM]`、`HH:MM`（今天）
- 命名日期：`today/tomorrow/yesterday/sod/eod/sow/eow/soww/eoww/som/eom/soq/
  eoq/soy/eoy/sond/eond/sonw/eonw/sonww/eonww/sopw/eopw/sopww/eopww/sonm/
  eonm/sopm/eopm/sony/eony/sopy/eopy`
- `eoww` 可由 `[date] workweek_end` 配置（默认周五 17:00）
- 相对偏移：`+3d` / `-2w` / `+1m` / `+1y` / `+2h`
- date-only `due:`/`start:` → all-day（`VALUE=DATE`，`task.allday=true`）；
  all-day 仅由 `YYYYMMDD`/`YYYY-MM-DD` 语法表达
- 命名边界带时刻：`sod`/`sow`/`som`/… = `[date] day_start`；
  `eod`/`eow`/`eom`/… = `[date] day_end`（默认 00:00:00 / 23:59:59）；
  `today`/`tomorrow`/`yesterday` 仍 date-only
- overdue 规则固定：all-day due 次日才算 overdue（无配置开关）
- `[date] default_event_duration` 为空 → 瞬间日程（仅 DTSTART，无 DTEND）
- ISO 8601 时长：`alert:PT15M`、`duration:P2W`、`recur:P7D`

## Recurrence
- `recur:`（alias `repeat:`）→ RFC 5545 `RRULE`
- raw passthrough：`FREQ=…` / `RRULE:…` 原样
- friendly 语法：freq / interval / byday / count / until
- ISO 周期：`P7D` / `P2W` / `P1M` / `P1Y`
- 自然语言（default build）：`recur:"every tuesday"`（text2rrule）
- 系列模型：master = `status:recurring`、虚拟 `+PARENT`、默认隐藏；
  `done` master → `cancelled`
- `recur-expand`（默认）：展开 occurrence，纯数字 ID，`[defaults]
  recur_expand_count`（默认 1 = 最近一个）
- 单次例外（iOS 兼容）：`done/delete` → `EXDATE`；`modify` → 同 UID
  `RECURRENCE-ID` override；寻址：纯 ID / `id.n` / `on:<date>`
- `recur`/`recurrence` 报告列显示 `P7D` 式周期

## Report
- 内置 `ls` / `list` / `next`；bare `calman` → `[defaults] default_report`
- 默认只显示未来 event；todo 保持原 filter；recurring parent 默认隐藏
- STATUS 列合并（event → 日历图标，todo → 状态图标）
- DATE 列（`event_format` / `todo_format` 可配）
- 字段：`id status summary desc tags date due pri type source recur/recurrence`
- `format`: relative / countdown / iso / truncate；`width`
- `sort`: `key+`/`key-`，尾部 `/` 断行；keys 含 id/created/updated/due/pri/
  status/summary
- icons 3 级回退（列 > 全局 > 内置）；`[icons.todo]`/`[icons.event]` 含
  `recurring` 键
- `[colorscheme]` 行级颜色：priority 顺序 + 内联 rules 表
- 显示宽度对齐（unicode-width），CJK 安全截断；长 desc 单行化（␤）

## Storage / Sync
- JSONL：`tasks.jsonl` 原子写
- ICS：每项一文件，VTODO/VEVENT，TZID 本地墙钟，all-day `VALUE=DATE`，
  VALARM，EXDATE / RECURRENCE-ID 读写
- `ics-dir`：自动发现 collection；复合引用 `source:name/collection`；
  done/delete/modify 均可用（`cli::resolve_source`）
- Sync executor：`pre_hook`/`cmd`/`post_hook`，`{location}` 替换，
  `.sync.lock`

## Config
- 双层：`config.default.toml`（自含完整默认）+ `config.example.toml`
  （注解样例）；`include` 合并
- `[defaults]`（write_source / default_report / recur_expand_count）
- `[contexts]` cli/sync/tui
- `[date]` workweek_end / default_event_duration / due_date_overdue_today /
  timezone
- `[tui]`（Phase-3 预留）、`[icons]`、`[colorscheme]`、`[report.*]`
- 删除 dead：`[locale]`、`[date] week_start`、`default_start_time`、
  `[ui]`、`report.default.toml`、theme.default.toml

## 其他
- `done` recurring master → cancelled；`delete` 移除
- `src:` 别名；`desc:` capture 修复；CJK 参数安全
- Wait（Taskwarrior 式）：`wait:<date>`/`wait:-1d`/`wait:PT12H` → 相对偏移
  存储；默认报告隐藏 `-WAITING`；`+WAITING` 显式显示；周期任务按 occurrence
  逐期生效（`X-CALMAN-WAIT-OFFSET` 私有属性）
- Cargo features：`default = [storage-jsonl, storage-ics, date-ical,
  date-natural, recur-expand]`；可选 `tui`；minimal 构建可行

## fix.md 六项（全部完成）
1. 删 `default_start_time`；date-only start → all-day
2. `[ui]` → `[tui]`（default_filter 移入）
3. config.default.toml 展示默认 icons
4. 删 `report.default.toml`，并入 config.default.toml
5. `[colorscheme.rules]` 内联表格式
6. `modify start:<date>` → all-day，清除过期 dtend