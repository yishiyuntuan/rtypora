//! 解析器：pulldown-cmark 事件流 → 块模型。
//! 输出块的 `start`/`end` 统一换算为 UTF-16 码元偏移，供前端按区间编辑源码。

use std::ops::Range;

use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use uuid::Uuid;

use super::model::{Block, BlockKind, Inline, ListItem};

/// 解析 Markdown 全文为块树。
pub fn parse(markdown: &str) -> Vec<Block> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    let events: Vec<(Event, Range<usize>)> = Parser::new_ext(markdown, options)
        .into_offset_iter()
        .collect();
    let mut parser = BlockParser {
        text: markdown,
        events: &events,
        pos: 0,
    };
    parser.parse_blocks()
}

struct BlockParser<'t, 'e> {
    text: &'t str,
    events: &'e [(Event<'t>, Range<usize>)],
    pos: usize,
}

impl<'t, 'e> BlockParser<'t, 'e> {
    /// 当前事件（返回的引用绑定在事件切片上，与 &self 无关，避免借用冲突）。
    fn peek(&self) -> Option<&'e Event<'t>> {
        self.events.get(self.pos).map(|(ev, _)| ev)
    }

    fn range(&self) -> Range<usize> {
        self.events[self.pos].1.clone()
    }

    /// 字节偏移 → UTF-16 码元偏移。
    fn u16(&self, byte_offset: usize) -> usize {
        self.text[..byte_offset].encode_utf16().count()
    }

    fn new_block(&self, range: Range<usize>, kind: BlockKind) -> Block {
        Block {
            id: Uuid::new_v4().to_string(),
            start: self.u16(range.start),
            end: self.u16(range.end),
            kind,
        }
    }

    /// 解析连续的块，遇到 `End` 事件（不消费）或事件流结束时返回，由调用方消费 `End`。
    fn parse_blocks(&mut self) -> Vec<Block> {
        let mut blocks = Vec::new();
        while let Some(ev) = self.peek() {
            match ev {
                Event::Start(Tag::Paragraph) => blocks.push(self.paragraph()),
                Event::Start(Tag::Heading { .. }) => blocks.push(self.heading()),
                Event::Start(Tag::CodeBlock(_)) => blocks.push(self.code_block()),
                Event::Start(Tag::BlockQuote) => blocks.push(self.block_quote()),
                Event::Start(Tag::List(_)) => blocks.push(self.list()),
                Event::Start(Tag::Table(_)) => blocks.push(self.table()),
                Event::Start(Tag::HtmlBlock) => blocks.push(self.html_block()),
                Event::Rule => {
                    let range = self.range();
                    blocks.push(self.new_block(range, BlockKind::ThematicBreak));
                    self.pos += 1;
                }
                Event::End(_) => break,
                // 紧凑列表项等场景下内容无 Paragraph 包裹，包一层段落块
                _ => blocks.push(self.bare_paragraph()),
            }
        }
        blocks
    }

    /// 解析裸行内内容（如紧凑列表项）为一个段落块。
    fn bare_paragraph(&mut self) -> Block {
        let start = self.range().start;
        let inlines = self.parse_bare_inlines();
        // 区间终点取最后一个被消费事件的终点
        let end = self.events[self.pos - 1].1.end;
        self.new_block(start..end, BlockKind::Paragraph { inlines })
    }

    /// 进入某块容器：记录 Start 区间起点并跳过 Start 事件。
    fn enter(&mut self) -> Range<usize> {
        let range = self.range();
        self.pos += 1;
        range
    }

    /// 离开某块容器：块终点取 Start/End 事件区间终点的较大者，并跳过 End 事件。
    fn leave(&mut self, start_range: Range<usize>) -> Range<usize> {
        let end = self.range().end.max(start_range.end);
        self.pos += 1;
        start_range.start..end
    }

    fn paragraph(&mut self) -> Block {
        let start_range = self.enter();
        let inlines = self.parse_inlines();
        let range = self.leave(start_range);
        self.new_block(range, BlockKind::Paragraph { inlines })
    }

    fn heading(&mut self) -> Block {
        let level = match self.peek() {
            Some(Event::Start(Tag::Heading { level, .. })) => *level,
            _ => unreachable!("heading() 仅在 Heading 事件处调用"),
        };
        let start_range = self.enter();
        let inlines = self.parse_inlines();
        let range = self.leave(start_range);
        self.new_block(
            range,
            BlockKind::Heading {
                level: heading_level_u8(level),
                inlines,
            },
        )
    }

    fn code_block(&mut self) -> Block {
        let language = match self.peek() {
            Some(Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(lang)))) if !lang.is_empty() => {
                Some(lang.to_string())
            }
            _ => None,
        };
        let start_range = self.enter();
        let mut code = String::new();
        while let Some(ev) = self.peek() {
            match ev {
                Event::Text(text) => {
                    code.push_str(text);
                    self.pos += 1;
                }
                Event::End(_) => break,
                _ => self.pos += 1,
            }
        }
        let range = self.leave(start_range);
        self.new_block(range, BlockKind::CodeBlock { language, code })
    }

    fn block_quote(&mut self) -> Block {
        let start_range = self.enter();
        let children = self.parse_blocks();
        let range = self.leave(start_range);
        self.new_block(range, BlockKind::BlockQuote { children })
    }

    fn list(&mut self) -> Block {
        let (ordered, start) = match self.peek() {
            Some(Event::Start(Tag::List(first))) => (first.is_some(), *first),
            _ => unreachable!("list() 仅在 List 事件处调用"),
        };
        let start_range = self.enter();
        let mut items = Vec::new();
        while let Some(Event::Start(Tag::Item)) = self.peek() {
            items.push(self.list_item());
        }
        let range = self.leave(start_range);
        self.new_block(
            range,
            BlockKind::List {
                ordered,
                start,
                items,
            },
        )
    }

    fn list_item(&mut self) -> ListItem {
        self.pos += 1; // 跳过 Start(Item)
        let (checked, marker_offset) = self.scan_task_marker();
        let children = self.parse_blocks();
        self.pos += 1; // 跳过 End(Item)
        ListItem {
            checked,
            marker_offset,
            children,
        }
    }

    /// 在当前 Item 的事件区间内（不深入嵌套 Item）找第一个 TaskListMarker，
    /// 返回勾选状态与 `[` 的 UTF-16 偏移（事件区间可能包含 `- ` 前缀，故定位到括号）。
    fn scan_task_marker(&self) -> (Option<bool>, Option<usize>) {
        let mut depth = 0usize;
        let mut idx = self.pos;
        while idx < self.events.len() {
            match &self.events[idx].0 {
                Event::Start(Tag::Item) => depth += 1,
                Event::End(TagEnd::Item) => {
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                }
                Event::TaskListMarker(checked) if depth == 0 => {
                    let range = &self.events[idx].1;
                    let bracket = self.text[range.clone()].find('[').unwrap_or(0);
                    return (Some(*checked), Some(self.u16(range.start + bracket)));
                }
                _ => {}
            }
            idx += 1;
        }
        (None, None)
    }

    fn table(&mut self) -> Block {
        let alignments = match self.peek() {
            Some(Event::Start(Tag::Table(aligns))) => aligns
                .iter()
                .map(|align| {
                    match align {
                        Alignment::None => "none",
                        Alignment::Left => "left",
                        Alignment::Center => "center",
                        Alignment::Right => "right",
                    }
                    .to_string()
                })
                .collect(),
            _ => unreachable!("table() 仅在 Table 事件处调用"),
        };
        let start_range = self.enter();
        let mut head = Vec::new();
        let mut rows = Vec::new();
        while let Some(ev) = self.peek() {
            match ev {
                Event::Start(Tag::TableHead) => {
                    self.pos += 1;
                    head = self.table_cells();
                }
                Event::Start(Tag::TableRow) => {
                    self.pos += 1;
                    rows.push(self.table_cells());
                }
                Event::End(TagEnd::Table) => break,
                _ => self.pos += 1,
            }
        }
        let range = self.leave(start_range);
        self.new_block(range, BlockKind::Table {
            alignments,
            head,
            rows,
        })
    }

    /// 解析连续的 TableCell，直到并消费 End(TableHead)/End(TableRow)。
    fn table_cells(&mut self) -> Vec<Vec<Inline>> {
        let mut cells = Vec::new();
        while let Some(ev) = self.peek() {
            match ev {
                Event::Start(Tag::TableCell) => {
                    self.pos += 1;
                    cells.push(self.parse_inlines());
                    self.pos += 1; // 跳过 End(TableCell)
                }
                Event::End(_) => break,
                _ => self.pos += 1,
            }
        }
        self.pos += 1; // 消费 End(TableHead)/End(TableRow)
        cells
    }

    fn html_block(&mut self) -> Block {
        let start_range = self.enter();
        let mut html = String::new();
        while let Some(ev) = self.peek() {
            match ev {
                Event::Html(text) => {
                    html.push_str(text);
                    self.pos += 1;
                }
                Event::End(_) => break,
                _ => self.pos += 1,
            }
        }
        let range = self.leave(start_range);
        self.new_block(range, BlockKind::Html { html })
    }

    /// 解析行内事件，遇到 `End` 事件（不消费）或事件流结束时返回，由调用方消费 `End`。
    fn parse_inlines(&mut self) -> Vec<Inline> {
        self.parse_inlines_until(false)
    }

    /// 解析裸行内内容（紧凑列表项），遇到块级事件开始也停止。
    fn parse_bare_inlines(&mut self) -> Vec<Inline> {
        self.parse_inlines_until(true)
    }

    fn parse_inlines_until(&mut self, stop_at_block: bool) -> Vec<Inline> {
        let mut inlines = Vec::new();
        while let Some(ev) = self.peek() {
            match ev {
                Event::End(_) => break,
                Event::Start(tag) if stop_at_block && is_block_tag(tag) => break,
                Event::Start(Tag::Strong) => {
                    self.pos += 1;
                    let children = self.parse_inlines();
                    self.pos += 1; // 跳过 End(Strong)
                    inlines.push(Inline::Bold { children });
                }
                Event::Start(Tag::Emphasis) => {
                    self.pos += 1;
                    let children = self.parse_inlines();
                    self.pos += 1;
                    inlines.push(Inline::Italic { children });
                }
                Event::Start(Tag::Strikethrough) => {
                    self.pos += 1;
                    let children = self.parse_inlines();
                    self.pos += 1;
                    inlines.push(Inline::Strikethrough { children });
                }
                Event::Start(Tag::Link {
                    dest_url, title, ..
                }) => {
                    let dest = dest_url.to_string();
                    let title = title.to_string();
                    self.pos += 1;
                    let children = self.parse_inlines();
                    self.pos += 1;
                    inlines.push(Inline::Link {
                        dest,
                        title,
                        children,
                    });
                }
                Event::Start(Tag::Image {
                    dest_url, title, ..
                }) => {
                    let src = dest_url.to_string();
                    let title = title.to_string();
                    self.pos += 1;
                    let children = self.parse_inlines();
                    self.pos += 1;
                    inlines.push(Inline::Image {
                        src,
                        title,
                        alt: plain_text(&children),
                    });
                }
                Event::Text(text) => {
                    inlines.push(Inline::Text {
                        text: text.to_string(),
                    });
                    self.pos += 1;
                }
                Event::Code(code) => {
                    inlines.push(Inline::Code {
                        code: code.to_string(),
                    });
                    self.pos += 1;
                }
                Event::SoftBreak => {
                    inlines.push(Inline::SoftBreak);
                    self.pos += 1;
                }
                Event::HardBreak => {
                    inlines.push(Inline::HardBreak);
                    self.pos += 1;
                }
                // 行内 HTML 按原文文本展示，避免注入
                Event::InlineHtml(text) => {
                    inlines.push(Inline::Text {
                        text: text.to_string(),
                    });
                    self.pos += 1;
                }
                _ => self.pos += 1,
            }
        }
        inlines
    }
}

fn heading_level_u8(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// 判断 Tag 是否为块级容器（用于裸行内内容遇到块级边界时停止）。
fn is_block_tag(tag: &Tag) -> bool {
    matches!(
        tag,
        Tag::Paragraph
            | Tag::Heading { .. }
            | Tag::BlockQuote
            | Tag::CodeBlock(_)
            | Tag::HtmlBlock
            | Tag::List(_)
            | Tag::Item
            | Tag::Table(_)
            | Tag::TableHead
            | Tag::TableRow
            | Tag::TableCell
    )
}

/// 把行内节点压平为纯文本（用于图片 alt）。
fn plain_text(inlines: &[Inline]) -> String {
    let mut text = String::new();
    for inline in inlines {
        match inline {
            Inline::Text { text: t } => text.push_str(t),
            Inline::Code { code } => text.push_str(code),
            Inline::Bold { children }
            | Inline::Italic { children }
            | Inline::Strikethrough { children }
            | Inline::Link { children, .. } => text.push_str(&plain_text(children)),
            Inline::Image { alt, .. } => text.push_str(alt),
            Inline::SoftBreak | Inline::HardBreak => text.push('\n'),
        }
    }
    text
}

