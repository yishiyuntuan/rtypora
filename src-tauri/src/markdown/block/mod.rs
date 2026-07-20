//! 块模型与文档树：移植自 velotype `components/block/state.rs`、`editor/document.rs`、`editor/tree.rs`。

// 供后续文档序列化等阶段使用的 API 暂未被调用。
#![allow(dead_code)]

pub mod state;
pub mod math;
pub mod mermaid;
pub mod document;
pub mod tree;
