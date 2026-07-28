//! 行内引擎：移植自 velotype `components/markdown/`。

// 引用定义扫描、图片解析、HTML 分类等 API 为后续文档解析阶段预留。
#![allow(dead_code)]

pub mod tree;
pub mod link;
pub mod html;
pub mod footnote;
pub mod image;

use std::cell::RefCell;

use image::ImageReferenceDefinitions;
use link::LinkReferenceDefinitions;

thread_local! {
    /// 全文解析期间的引用定义（[label]: url）上下文：线程本地（async 命令各自独立），
    /// 每次全文/片段解析前重置——片段解析只见片段内定义（文档级定义在片段外时，
    /// 引用式语法按字面量保留，内容不丢，下次全文重解析恢复）。
    static PARSE_LINK_REFS: RefCell<LinkReferenceDefinitions> =
        RefCell::new(LinkReferenceDefinitions::new());
    static PARSE_IMAGE_REFS: RefCell<ImageReferenceDefinitions> =
        RefCell::new(ImageReferenceDefinitions::new());
}

/// 在给定引用定义的作用域内执行块树解析（parse_to_dtos 入口先扫描定义再进入）。
pub(crate) fn with_parse_reference_definitions<T>(
    link_defs: &LinkReferenceDefinitions,
    image_defs: &ImageReferenceDefinitions,
    f: impl FnOnce() -> T,
) -> T {
    PARSE_LINK_REFS.with(|c| *c.borrow_mut() = link_defs.clone());
    PARSE_IMAGE_REFS.with(|c| *c.borrow_mut() = image_defs.clone());
    f()
}

/// 当前解析作用域的链接引用定义（块/单元格行内解析经此取，不克隆）。
pub(crate) fn with_current_link_refs<T>(f: impl FnOnce(&LinkReferenceDefinitions) -> T) -> T {
    PARSE_LINK_REFS.with(|c| f(&c.borrow()))
}

/// 当前解析作用域的图片引用定义。
pub(crate) fn with_current_image_refs<T>(f: impl FnOnce(&ImageReferenceDefinitions) -> T) -> T {
    PARSE_IMAGE_REFS.with(|c| f(&c.borrow()))
}
