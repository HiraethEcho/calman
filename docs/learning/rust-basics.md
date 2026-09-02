# Rust Basics — 用 calman 的代码学 Rust

This guide teaches the Rust concepts you will actually see in calman's source.
Each section names the concept, shows a tiny example, then points at a real
calman file so you can read it in context.

本指南讲解 calman 源码里会用到的 Rust 概念，每个概念给出小例子，并指向真实文件。

## 1. `fn` 函数 (Functions)

```rust
// 输入: &str (字符串引用) 输出: Option<u8> (可能有数字)
pub fn priority_from_str(s: &str) -> Option<u8> { ... }
```

Real file: `src/model.rs`.

- `-> Type` means the function returns that type. `-> 类型` 表示返回值。
- `pub` = public, usable outside this file. `pub` 公开。
- `fn name(args) -> ret { body }` is the shape of every function.

## 2. `Option` — 值可能不存在 (value may be absent)

```rust
pub due: Option<DateTime<Utc>>   // 有到期时间，或没有 (None)
```

`Option<T>` is either `Some(value)` or `None`. It forces you to handle
"missing" explicitly — a todo may have no due date.

`Option<T>` 只有两种：`Some(值)` 或 `None`。它强迫你显式处理「没有」的情况。

## 3. `Result` — 操作可能失败 (may fail)

```rust
pub fn load() -> Result<Config>   // 可能出错 (read file, parse)
```

`Result<T, E>` is either `Ok(value)` or `Err(error)`. calman uses the `anyhow`
crate's `Result` so errors carry a readable message.

`Result<T, E>` 两种：`Ok(值)` 或 `Err(错误)`。calman 用 `anyhow` 让错误带可读信息。

- `?` operator: "if this errors, return the error now". `?` 出错就立刻返回错误。
- `anyhow::Context` adds "what was I doing" to an error.

## 4. `struct` — 打包相关数据 (bundle related data)

```rust
pub struct Task {
    pub uid: String,
    pub summary: String,
    pub due: Option<DateTime<Utc>>,
    // ...
}
```

A `struct` groups named fields. Real file: `src/model.rs`.

`struct` 把多个字段打包在一起。

## 5. `enum` — 一组固定选项 (fixed set of choices)

```rust
pub enum TaskStatus { Pending, InProgress, Recurring, Completed, Cancelled }
```

An `enum` is one-of-many named variants. A task is in exactly one status.

`enum` 表示「多选一」。

## 6. `impl` — 给类型加方法 (methods on a type)

```rust
impl Task {
    pub fn is_event(&self) -> bool { ... }
}
```

`impl` attaches functions to a type. `&self` = "operate on the instance".
`&self` 表示「操作这个实例本身」。

## 7. 所有权与借用 (Ownership & borrowing)

Rust tracks who owns each value. One owner at a time.

- `String` owns its text.
- `&str` borrows text it does not own.
- `&T` = read-only borrow (借用只读). `&mut T` = mutable borrow (可变借用).

Rules: one `&mut` **or** many `&`, never both at once. This is why calman
passes `&conf` around — to borrow the config without copying it.

规则：同一时间只能有一个 `&mut`，或任意多个 `&`。

## 8. `derive` — 自动实现 (auto-implement traits)

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
```

The compiler writes common code for you:
- `Debug` → printable with `{:?}`
- `Clone` → copyable
- `PartialEq` → comparable with `==`
- `Serialize`/`Deserialize` → JSON/TOML in and out

编译器自动生成这些能力。

## 9. `serde` — 数据序列化 (data in/out of files)

calman stores tasks as JSONL and config as TOML. `serde` maps Rust structs to
those formats. `#[serde(rename_all = "kebab-case")]` changes field naming;
`#[serde(default)]` supplies a default when the field is missing.

calman 用 `serde` 把结构体写到 JSON/TOML 文件，也能读回来。

## 10. Traits — 共享行为 (shared behaviour)

```rust
pub trait Storage {           // src/storage/mod.rs
    fn load(&self, ...) -> ...;
    fn save(&self, ...) -> ...;
}
```

A trait is a contract: "anything that is a `Storage` can do these things".
jsonl and ics both implement it, so the rest of the code does not care which
format is used.

trait 是「契约」：任何实现它的类型都具备这些方法。

## 11. `match` — 模式匹配 (pattern matching)

```rust
match status {
    TaskStatus::Completed => "done",
    TaskStatus::Pending   => "waiting",
    _ => "other",
}
```

`match` checks a value against patterns. It must cover all cases.

`match` 必须覆盖所有分支。

## 12. 模块 (Modules) & `use`

```rust
mod storage;          // src/main.rs — 声明模块
use crate::model::Task;   // 引入名字
```

Files are modules. `src/main.rs` declares them with `mod`; `use` imports names.

## 13. 测试 (Tests)

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn new_task_defaults() { assert_eq!(t.summary, "buy milk"); }
}
```

`#[test]` functions run with `cargo test`. They assert expected behaviour.

## 14. `clap` — 命令行解析 (CLI parsing)

`clap` turns command-line arguments into typed structs. calman wraps it with a
custom parser in `src/args.rs` for Taskwarrior-style free-form input.

`clap` 把命令行参数转成结构化数据。calman 在 `src/args.rs` 用自定义解析器支持自由格式输入。

---

### Suggested next step

Open `src/model.rs`. It uses almost every idea above: `enum TaskStatus`,
`struct Task`, `impl Task`, `Option`, `#[derive(...)]`, tests. Read the
comments top to bottom, then move to `module-map.md`.
