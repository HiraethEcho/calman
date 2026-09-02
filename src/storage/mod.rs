//! 存储层：定义统一的 `Storage` trait，并提供 JSONL / ICS 两个后端与状态管理器。
//! Storage layer: defines the common `Storage` trait, plus JSONL/ICS backends and the state manager.
//!
//! 核心概念 / Key concepts:
//! - `Storage` trait：所有后端共享的“读写任务”接口（list / add / update / remove）。
//! - 原子写入 `atomic_write`：先写临时文件，再 `rename` 替换目标文件，避免半截文件。
//! - 数据格式：JSONL 每行一个 JSON 任务；元数据存于 `.calman-state.json`。

pub mod state;

// 条件编译：只有启用 `storage-ics` feature 时，`ics` 模块才会被编译。
#[cfg(feature = "storage-ics")]
pub mod ics;
// 条件编译：`storage-jsonl` 默认启用（见 Cargo.toml），关闭后此模块不参与编译。
#[cfg(feature = "storage-jsonl")]
pub mod jsonl;

// 两个后端都没有启用时，编译直接失败并给出友好提示。
#[cfg(not(any(feature = "storage-jsonl", feature = "storage-ics")))]
compile_error!("at least one of `storage-jsonl` or `storage-ics` features is required");

use crate::model::Task;
use anyhow::Result;
use std::fs;
use std::io::Write;
use std::path::Path;

/// 所有存储后端都必须实现的公共接口。
/// Common interface that every storage backend must implement.
///
/// Rust 概念：`trait` 类似“接口/契约”；`impl Storage for X` 表示给类型 `X` 实现该接口。
/// `&self` 是只读借用（不能修改内部数据），`&mut self` 是可变借用（可以修改内部数据）。
pub trait Storage {
    /// 返回当前全部任务的只读引用 `&[Task]`（借用了内部数据，不复制）。
    /// Returns a read-only reference to all tasks currently held.
    fn list(&self) -> &[Task];

    /// 追加一个新任务。
    /// Append a new task.
    ///
    /// Rust 错误处理：返回 `Result<()>`，`Ok(())` 表示成功，`Err` 携带失败原因。
    fn add(&mut self, task: Task) -> Result<()>;

    /// 按 UID 修改任务；返回更新后的任务副本，找不到则返回 `None`。
    /// Mutate a task by UID. Returns the updated task, or `None` if missing.
    ///
    /// Rust 概念：`F: FnOnce(&mut Task) -> Result<()>` 表示参数是一个闭包（closure），
    /// 它接收 `&mut Task`（可修改任务）并返回 `Result`。CLI 传入的闭包负责改字段；
    /// 若闭包返回 `Err`（例如校验失败），本次修改整体中止，不会写盘。
    fn update<F>(&mut self, uid: &str, f: F) -> Result<Option<Task>>
    where
        F: FnOnce(&mut Task) -> Result<()>;

    /// 按 UID 删除任务；返回被删除的任务，找不到则返回 `None`。
    /// Delete a task by UID. Returns the removed task, or `None` if missing.
    fn remove(&mut self, uid: &str) -> Result<Option<Task>>;
}

/// 原子写入：先把内容写到临时文件，成功后再用 `rename` 替换目标文件。
/// Atomic write: write to a temp file first, then `rename` over the target.
///
/// 为什么原子：`rename` 在同一文件系统内是“要么整体成功、要么整体失败”的操作，
/// 读者永远不会看到写了一半的文件。Why atomic: rename is all-or-nothing on one filesystem.
pub fn atomic_write(path: &Path, contents: &[u8]) -> Result<()> {
    let tmp = path.with_extension("tmp"); // 临时文件与目标同目录，保证 rename 在同一文件系统内
    let mut file = fs::File::create(&tmp)?; // `?` 遇到 Err 会立刻把错误返回给调用者
    file.write_all(contents)?;
    file.sync_all()?; // 强制刷盘，避免系统崩溃时数据还停留在内存缓存里
    fs::rename(&tmp, path)?; // 原子替换：要么新文件生效，要么保持旧文件
    Ok(())
}

/// 运行时可选的存储后端：`enum` 表示一个值只能是其中一种变体（variant）。
/// A concrete storage backend chosen at runtime by source type.
///
/// 变体由 `#[cfg(feature = ...)]` 条件编译决定：未启用的后端不会出现在类型里。
pub enum Store {
    #[cfg(feature = "storage-jsonl")] // 条件编译：只有启用 storage-jsonl 时才编译这一变体
    Jsonl(jsonl::JsonlStorage),
    #[cfg(feature = "storage-ics")] // 条件编译：只有启用 storage-ics 时才编译这一变体
    Ics(ics::IcsStorage),
}

impl Storage for Store {
    fn list(&self) -> &[Task] {
        match self {
            #[cfg(feature = "storage-jsonl")]
            Store::Jsonl(s) => s.list(),
            #[cfg(feature = "storage-ics")]
            Store::Ics(s) => s.list(),
        }
    }

    fn add(&mut self, task: Task) -> Result<()> {
        match self {
            #[cfg(feature = "storage-jsonl")]
            Store::Jsonl(s) => s.add(task),
            #[cfg(feature = "storage-ics")]
            Store::Ics(s) => s.add(task),
        }
    }

    fn update<F>(&mut self, uid: &str, f: F) -> Result<Option<Task>>
    where
        F: FnOnce(&mut Task) -> Result<()>,
    {
        match self {
            #[cfg(feature = "storage-jsonl")]
            Store::Jsonl(s) => s.update(uid, f),
            #[cfg(feature = "storage-ics")]
            Store::Ics(s) => s.update(uid, f),
        }
    }

    fn remove(&mut self, uid: &str) -> Result<Option<Task>> {
        match self {
            #[cfg(feature = "storage-jsonl")]
            Store::Jsonl(s) => s.remove(uid),
            #[cfg(feature = "storage-ics")]
            Store::Ics(s) => s.remove(uid),
        }
    }
}
