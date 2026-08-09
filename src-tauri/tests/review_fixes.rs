//! 全量审查修复的回归测试：递归深度上限、任务勾选伪命中、引用式链接/图片、
/// Markdown 嗅探、相邻表格序列化、脚注模板 id、渐进打开 CJK 接缝。
use tauri_app_lib::markdown;

#[test]
fn 超深引用嵌套不栈溢出() {
    let src = format!("{}x", "> ".repeat(3000));
    let blocks = markdown::parse_blocks(&src);
    assert!(!blocks.is_empty(), "超深嵌套应降级为原文块而非崩溃");
    let rt = markdown::serialize_markdown(blocks);
    assert!(rt.contains('x'), "内容不丢: {rt}");
}

#[test]
fn 超深列表嵌套不栈溢出() {
    let mut src = String::new();
    for i in 0..3000 {
        src.push_str(&"  ".repeat(i));
        src.push_str("- x\n");
    }
    let blocks = markdown::parse_blocks(&src);
    assert!(!blocks.is_empty());
}

#[test]
fn 超深行内html嵌套不栈溢出() {
    let src = format!("{}x{}", "<u>".repeat(3000), "</u>".repeat(3000));
    let blocks = markdown::parse_blocks(&src);
    assert!(!blocks.is_empty());
    let rt = markdown::serialize_markdown(blocks);
    assert!(rt.contains('x'), "内容不丢: {rt}");
}

#[test]
fn 深度内的嵌套引用照常解析() {
    let src = format!("{}x", "> ".repeat(10));
    let blocks = markdown::parse_blocks(&src);
    // 10 层引用正常解析为嵌套 quote（不是 raw）
    let kinds: Vec<String> = blocks.iter().map(|b| format!("{:?}", b.kind)).collect();
    assert_eq!(kinds, ["Quote"]);
}

#[test]
fn 任务勾选不篡改代码内同形文本() {
    let src = "- [x] A\n\n```\n[x] in code\n```\n\n- [x] B";
    // 取消勾选 B（occurrence=1）：只改 B 的标记，代码里的 [x] 不动
    let out = markdown::toggle_task_markdown(src, false, Some(1));
    assert!(out.contains("[x] in code"), "代码内容不得篡改: {out}");
    assert!(out.contains("- [ ] B"), "B 应取消勾选: {out}");
    assert!(out.contains("- [x] A"), "A 不受影响: {out}");
    // 勾选（occurrence=0）：A 变 [ ]
    let out2 = markdown::toggle_task_markdown("- [ ] A\n\n`[ ] inline`", true, Some(0));
    assert!(out2.contains("- [x] A"));
    assert!(out2.contains("`[ ] inline`"), "行内代码不得篡改: {out2}");
}

#[test]
fn 引用式链接在生产解析中解析() {
    let blocks = markdown::parse_markdown("这是 [链接文字][r] 测试\n\n[r]: https://example.com \"标题\"");
    let para = &blocks[0];
    let json = serde_json::to_string(para).unwrap();
    assert!(json.contains("example.com"), "引用式链接应解析为目标: {json}");
}

#[test]
fn 引用式图片在生产解析中解析() {
    let blocks = markdown::parse_markdown("![alt][img]\n\n[img]: ./pic/a.png");
    let para = &blocks[0];
    let json = serde_json::to_string(para).unwrap();
    assert!(json.contains("./pic/a.png"), "引用式图片应解析出 src: {json}");
}

#[test]
fn markdown嗅探命令() {
    assert!(markdown::looks_like_markdown("# 标题"));
    assert!(markdown::looks_like_markdown("含 **粗体** 文本"));
    assert!(markdown::looks_like_markdown("- 列表项"));
    assert!(markdown::looks_like_markdown("[链接](https://a.com)"));
    assert!(markdown::looks_like_markdown("<kbd>Ctrl</kbd>"));
    assert!(!markdown::looks_like_markdown("这是一段纯文本，没有任何标记。"));
    assert!(!markdown::looks_like_markdown("价格 5 * 3 = 15 的算式"));
}

#[test]
fn 引用内相邻表格序列化不合并() {
    let src = "> | A | B |\n> |---|---|\n> | 1 | 2 |\n>\n> | C | D |\n> |---|---|\n> | 3 | 4 |";
    let blocks = markdown::parse_markdown(src);
    let json = serde_json::to_string(&blocks).unwrap();
    assert!(json.contains("\"A\""), "解析应含两张表: {json}");
    assert!(json.contains("\"C\""), "第二张表不被吞: {json}");
    // 序列化往返：两张表仍然各自独立
    let rt = markdown::serialize_markdown(blocks);
    let reparsed = markdown::parse_markdown(&rt);
    let json2 = serde_json::to_string(&reparsed).unwrap();
    assert!(json2.contains("\"A\"") && json2.contains("\"C\""), "往返后两表保持独立: {json2}");
}

#[test]
fn 脚注模板支持指定id() {
    let tpl = markdown::block_template("footnoteDef", None, None, None, Some("7".into()));
    assert_eq!(tpl.markdown, "[^7]: ");
    let tpl2 = markdown::block_template("footnoteDef", None, None, None, Some("  ".into()));
    assert_eq!(tpl2.markdown, "[^1]: ", "空白 id 回落 1");
}

#[test]
fn 渐进打开cjk接缝偏移正确() {
    // CJK 文档（UTF-16 偏移 != 字节偏移）的渐进接缝：tail_from 按 UTF-16 计
    let mut doc = String::new();
    for i in 0..3000 {
        doc.push_str(&format!("第{i}段中文内容填充每一行使其足够长长长\n\n"));
    }
    let dir = std::env::temp_dir().join(format!("tauri-app-cjk-seam-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("cjk.md");
    std::fs::write(&path, &doc).unwrap();
    let opened = tauri_app_lib::files::read_markdown_parsed(path.to_str().unwrap()).expect("应读取成功");
    let tail_from = opened.tail_from.expect("大文件应有尾部");
    let last = opened.blocks.last().unwrap();
    assert_eq!(Some(tail_from), last.start, "tail_from = 首屏末块起点（UTF-16）");
    // UTF-16 切片拼接（模拟前端 JS slice）
    let utf16: Vec<u16> = doc.encode_utf16().collect();
    let tail_text = String::from_utf16(&utf16[tail_from..]).unwrap();
    let tail = markdown::parse_blocks(&tail_text);
    assert!(!tail.is_empty(), "尾部重解析应有块");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn 有序列表序列化按位置递增序号() {
    // 源序号规范化后按兄弟位置递增（而非全部 1.），与渲染层 numberedOrdinals 同规则
    let src = "1. aaaaaaa\n\n2. bbbbbb\n\n3. cccccc\n\n4. ddddd";
    let ser = markdown::serialize_markdown(markdown::parse_blocks(src));
    assert!(ser.contains("1. aaaaaaa") && ser.contains("2. bbbbbb") && ser.contains("3. cccccc") && ser.contains("4. ddddd"), "序号应按位置递增: {ser}");
    // 空段落（松散列表）不打断编号；内容块打断后重新从 1 计
    let ser2 = markdown::serialize_markdown(markdown::parse_blocks("1. a\n\n2. b\n\n段落\n\n1. c"));
    assert!(ser2.contains("2. b"), "{ser2}");
    assert!(ser2.contains("1. c"), "打断后重新计数: {ser2}");
}

#[test]
fn 有序列表源序号与渲染一致() {
    // 自定义起始序号保留（源即所见）：3 起始的列表序列化仍为 3 起始
    let ser = markdown::serialize_markdown(markdown::parse_blocks("3. a\n\n4. b"));
    assert!(ser.contains("3. a") && ser.contains("4. b"), "自定义起始保留: {ser}");
    // 空行后遇源标记 1. 视为新列表重启（两块列表各自从 1 开始）
    let ser2 = markdown::serialize_markdown(markdown::parse_blocks("1. a\n\n1. b\n\n1. c"));
    assert!(ser2.matches("1. ").count() == 3, "空行+1. 重启编号: {ser2}");
    // 空行后源标记递增则延续编号
    let ser3 = markdown::serialize_markdown(markdown::parse_blocks("1. a\n\n2. b"));
    assert!(ser3.contains("2. b"), "延续编号: {ser3}");
    // 内容块打断后新组采用其源标记起始
    let ser4 = markdown::serialize_markdown(markdown::parse_blocks("1. a\n\n段落\n\n5. b"));
    assert!(ser4.contains("5. b"), "新组采用源标记 5: {ser4}");
}

#[test]
fn 行首img标签后同行文字拆分() {
    // <img ... /> **文字** 同行：拆分为图片块 + 段落（文字保持行内格式），而非整行原文显示
    let blocks = markdown::parse_blocks("<img src=\"./img/a.png\" alt=\"x\" style=\"zoom:50%;\" /> **说明文字**");
    assert_eq!(blocks.len(), 2, "应拆为两块: {:?}", blocks.iter().map(|b| format!("{:?}", b.kind)).collect::<Vec<_>>());
    assert!(blocks[0].image.is_some(), "首块应携带图片信息");
    assert_eq!(blocks[0].image.as_ref().unwrap().zoom, Some(0.5), "缩放保留");
    let para_json = serde_json::to_string(&blocks[1]).unwrap();
    assert!(para_json.contains("说明文字"), "剩余文字成段落: {para_json}");
    // 属性引号内的 > 不误判为标签结束
    let blocks2 = markdown::parse_blocks("<img src=\"./img/a.png\" alt=\"a>b\" /> **说明**");
    assert_eq!(blocks2.len(), 2);
    // 独立标签行行为不变
    let blocks3 = markdown::parse_blocks("<img src=\"./img/a.png\" alt=\"x\" />");
    assert_eq!(blocks3.len(), 1);
    assert!(blocks3[0].image.is_some());
}

#[test]
fn 行首img拆分块偏移不重叠() {
    // 拆分块共享整行区间会导致编辑图片块时整行被覆盖（文字被复制出一行）
    let src = "<img src=\"./img/a.png\" alt=\"x\" /> **说明**";
    let blocks = markdown::parse_blocks(src);
    assert_eq!(blocks.len(), 2);
    let (a, b) = (&blocks[0], &blocks[1]);
    assert!(a.end.is_some() && a.end == b.start, "两偏移应相接不重叠: {:?} {:?}", a.end, b.start);
    let utf16: Vec<u16> = src.encode_utf16().collect();
    let slice_a = String::from_utf16(&utf16[a.start.unwrap()..a.end.unwrap()]).unwrap();
    assert!(slice_a.starts_with("<img") && slice_a.ends_with("/>"), "首块区间应恰为标签: {slice_a}");
}
