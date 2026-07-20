//! 行内引擎：移植自 velotype `components/markdown/`。

// 引用定义扫描、图片解析、HTML 分类等 API 为后续文档解析阶段预留。
#![allow(dead_code)]

pub mod tree;
pub mod link;
pub mod html;
pub mod footnote;
pub mod image;
