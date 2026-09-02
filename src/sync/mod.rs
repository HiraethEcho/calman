//! 同步功能：通过用户配置的外部命令链执行同步（例如 `git` 推送/拉取）。
//! Synchronisation via user-configured external command chains.
//!
//! 模块组织：`executor` 子模块负责真正的执行与锁管理；本文件只负责声明子模块。
//! Layout: the `executor` submodule does the real work; this file just declares it.
//! Rust 概念：`pub mod` 使外部代码可以通过 `sync::executor::...` 访问其中的公开项。

pub mod executor;
