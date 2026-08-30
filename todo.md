# Todo

未完成需求（feature.md；fix.md 已全部完成；+DELETED 已明确不做——delete 即硬删文件；wait 已完成）。

## 1. `day_start` / `day_end` 自定义
- 新增 `[date] day_start` / `[date] day_end`，默认 `00:00:00` / `23:59:59`
- 用途：命名日期边界（`sod`/`eod`、`sow`/`eow`、`som`/`eom` 等）不再硬编码
  00:00 / 23:59:59，改用配置值
- 影响 overdue 判断（若有依赖）与 all-day 语义
- 配置校验：`HH:MM[:SS]`

## 2. `default_event_duration = ""` → 瞬间日程
- 空串时，`add`/`modify` 生成 VEVENT 只写 `DTSTART`，不写 `DTEND`
- 用途：瞬间日程（如生日、截止时刻）
- 当前默认 `"1h"`；空串目前会被 `parse_duration` 报错 → 需特判
- ICS 读写 roundtrip 支持无 DTEND 的 VEVENT

## 3. 删除 `due_date_overdue_today` 配置
- 移除 `[date] due_date_overdue_today`（当前 false/true 两档）
- 仅保留 false 行为：all-day due 过期**次日**才算 overdue
- 涉及：config.rs 字段 + 默认值、config.default.toml / config.example.toml、
  filter.rs overdue 分支、相关测试与文档

## 4. Todo → Event（"start a todo, when stop it create an event"）
**需求**：开始 todo → 停止时生成 event（dtstart=开始, dtend=停止）。

**待定方案**：
- `calman start <id>` → 记录 `started_at`（新字段）
- `calman stop <id>` → 同 source 创建 event（复制字段，`related_to`=原 todo），
  todo 标 completed
- `+STARTED` 虚拟标签（可选）

## 5. TUI（Phase 3）
- 完整左右双栏 TUI：list + detail、编辑、设置 overlay、sync、i18n
- `tui.toml` 独立配置（或沿用 `[tui]` 段）
- 见 PLAN.md Phase 3（3.1–3.5），拆多轮实现

## 6. tui.toml（随 TUI 一起做）