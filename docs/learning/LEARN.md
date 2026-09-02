# Learn calman — 从这里开始

This folder turns the calman source code into a **Rust learning project**.
You do not need any coding experience. Each file teaches one thing.

本目录把 calman 源码变成 **Rust 学习项目**。无需编程经验。

## 怎么读 (How to read)

1. Read `rust-basics.md` first — the Rust vocabulary you will meet everywhere
   (`Option`, `Result`, `struct`, `enum`, traits, `serde`).
   先读 `rust-basics.md`，学会 Rust 常用词汇。
2. Read `code-workflow.md` — what the whole program does from `main()` to the
   screen, and how the pieces connect.
   再读 `code-workflow.md`，理解整个程序从 `main()` 到输出的流程。
3. Follow `module-map.md` in the given order — it tells you which file to open
   next and what to look for.
   按 `module-map.md` 的顺序打开文件，对照学习。
4. Every `.rs` file has bilingual comments: `// 中文说明 + English term`.
   Code itself is English; the comments teach you both.
   每个 `.rs` 文件都有中英双语注释。

## 三个文件 (The three guides)

| File 文件 | What it teaches 教什么 |
|---|---|
| `rust-basics.md` | Rust fundamentals used in this codebase (Rust 基础) |
| `code-workflow.md` | Program flow from start to finish (程序流程) |
| `module-map.md` | Per-module guide & reading order (模块导览) |

## Golden rules 学习要点

- Run `cargo build` after editing anything — the compiler is your first teacher.
  改完就 `cargo build`，编译错误是最好的老师。
- Run `cargo test` to see what the code is supposed to do.
  `cargo test` 看代码应有的行为。
- If a comment confuses you, open the file in an editor and read the code next
  to it. Comments explain *why*, code shows *how*.
  注释解释「为什么」，代码展示「怎么做」。
