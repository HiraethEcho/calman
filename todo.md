# Todo

未完成需求。已完成项见 done.md。
（+DELETED 已明确不做——delete 即硬删文件。）

## 1. Todo → Event（"start a todo, when stop it create an event"）
**需求**：开始 todo → 停止时生成 event（dtstart=开始, dtend=停止）。

**待定方案**：
- `calman start <id>` → 记录 `started_at`（新字段）
- `calman stop <id>` → 同 source 创建 event（复制字段，`related_to`=原 todo），
  todo 标 completed
- `+STARTED` 虚拟标签（可选）

## 2. TUI（Phase 3）
- 完整左右双栏 TUI：list + detail、编辑、设置 overlay、sync、i18n
- `tui.toml` 独立配置（或沿用 `[tui]` 段）
- 见 PLAN.md Phase 3（3.1–3.5），拆多轮实现

## 3. tui.toml（随 TUI 一起做）