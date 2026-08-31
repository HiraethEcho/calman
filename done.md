# Done

已完成功能清单（持续更新；feature.md/fix.md 已归档删除）。

## CLI / Filter
- 共享 filter 语法（CLI + report filter）：
  - `and` / `or` / `(` / `)`，相邻 atom 隐式 `and`；`and` 优先于 `or`
  - `type:todo` / `type:event` / `type:all`
  - `source:<name>` / `src:<name>` / `-source:<name>` / `-src:<name>`
  - `due:<date>`（当天）`due.before:` `due.by:` `due.after:`
  - `date:<date>` / `date.before:` / `date.by:` / `date.after:`（统一日期：
    todo→due，event→dtstart）
  - `from:<date>` / `from.before:` / `from.by:` / `from.after:`（仅 VEVENT
    dtstart，todo 不匹配）
  - todo 比 `due`，event 比 `dtstart`
  - 虚拟标签：`+OVERDUE +PENDING +COMPLETED +CANCELLED +IN-PROCESS +TAGGED
    +UNTAGGED +SCHEDULED +PARENT`
- rc 覆盖：`rc.report.<name>.columns/labels/filter/sort`；`columns` 支持
  `field` / `field.format`。

## Date
- `T` 紧凑格式：`20260812T090000`、`0826T0930`、`T0900`、`25`（本月第 25 天）
- ISO `YYYY-MM-DD [HH:MM]`、`HH:MM`（今天）
- 命名日期：`today/tomorrow/yesterday/sod/eod/sopd/eopd/sow/eow/soww/eoww/
  som/eom/soq/eoq/soy/eoy/sond/eond/sonw/eonw/sonww/eonww/sopw/eopw/
  sopww/eopww/sonm/eonm/sopm/eopm/sony/eony/sopy/eopy`
- `MM-DD` 补当年（`08-26`、`9-30` → all-day）
- 相对偏移：`+3d` / `-2w` / `+1m` / `+1y` / `+2h`
- date-only `due:`/`from:` → all-day（`VALUE=DATE`，`task.allday=true`）；
  all-day 仅由 `YYYYMMDD`/`YYYY-MM-DD` 语法表达
- 命名边界带时刻：`sod`/`sow`/`som`/… = `[date] day_start`；
  `eod`/`eow`/`eom`/… = `[date] day_end`（默认 00:00:00 / 23:59:59）；
  `today`/`tomorrow`/`yesterday` 仍 date-only
- overdue 规则固定：all-day due 次日才算 overdue（无配置开关）
- `[date] default_event_duration` 为空 → 瞬间日程（仅 DTSTART，无 DTEND）
- ISO 8601 时长：`alert:PT15M`、`for:P2W`、`recur:P7D`

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
- 单次例外（iOS 兼容）：`done` occurrence → Completed override 记录；
  `delete` occurrence → `EXDATE`；`modify` occurrence → 同 UID
  `RECURRENCE-ID` override
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
- `[date]` day_start / day_end / default_event_duration / timezone
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
6. `modify from:<date>` → all-day，清除过期 dtend
## more feature（info / all-future split-truncate）
- `info` 命令：`calman info <id>` / `calman <id> info`；显示全字段（status、
  日期、recur、wait、priority、tags、location、desc、alert、related、
  created/updated/completed、uid）；支持 plain ID、`id.n` occurrence、UID
- `modify` occurrence 交互询问（TTY）是否应用到所有未来；`all-future` 关键字
  跳过询问：
  - yes → split series：旧 master RRULE 改 `COUNT=abs_index-1`（保留该 occ 之前
    所有实例），新 series = 旧内容+修改，从该 occ 起（新 UID，独立身份）
  - 旧 COUNT 继承剩余数；`from:`/`due:` 显式给出则作为新 series 首日
  - occ 是第 1 个 → 旧 master 整体删除
- `delete` occurrence 交互询问是否删除该 occ 及所有未来；`all-future` 跳过：
  - yes → truncate：旧 master `COUNT=abs_index-1`，清掉该 occ 起的
    override；第 1 个 → 删整个 master
- 绝对序号计算：幸存前驱数（expand 窗口 [DTSTART, occ)）+ EXDATE 前驱数 + 1
  （COUNT 按算法序号，避免 EXDATE 位移）；`rrule::before` 是 inclusive → 窗口
  终点用 `occ - 1s`
- 非 TTY 默认单 occurrence（不询问）；`all-future` 供脚本强制

## Todo → Event（start / stop）
- `calman start <id>`：记录 `Task.started_at` + status → InProgress；event /
  completed / recurring master 不可 start（报错）
- `calman stop <id>`：复制 todo 为 timed event（同 source，dtstart=started_at、
  dtend=now，复制 summary/desc/tags/pri/location/alert，无 related_to 回链），
  询问是否标记 todo done（TTY；非 TTY 默认否）——是 → Completed +
  completed_at；否 → 回 Pending；started_at 均清空
- `Task.started_at`：jsonl serde；ICS `X-CALMAN-STARTED` 私有属性 roundtrip
  （秒精度）
- `+STARTED` 虚拟标签 = `started_at.is_some()`；`status:started` 仍是
  in-progress 别名（status 判定）
- info 显示 started 字段；`calman st stop` 场景：重复 stop 报错未 start
