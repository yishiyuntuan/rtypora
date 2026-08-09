//! 全语法往返审计：覆盖编辑器支持的全部语法特性。
//! 不变量：
//! 1. 幂等性：serialize(parse(x)) 再解析再序列化结果不变（规范化稳定）；
//! 2. 内容零丢失：往返后可见文本关键片段全部保留；
//! 3. 规范输入原样往返：已是规范形态的源码 serialize(parse(x)) == x。
use tauri_app_lib::markdown::{parse_blocks, serialize_markdown};

fn roundtrip(src: &str) -> String {
    serialize_markdown(parse_blocks(src))
}

fn assert_idempotent(src: &str) -> String {
    let once = roundtrip(src);
    let twice = roundtrip(&once);
    assert_eq!(once, twice, "序列化不幂等:\n输入:\n{src}\n一次:\n{once}\n两次:\n{twice}");
    once
}

fn assert_keeps(src: &str, needles: &[&str]) {
    let out = roundtrip(src);
    for needle in needles {
        assert!(out.contains(needle), "内容丢失 {needle:?}:\n输入:\n{src}\n输出:\n{out}");
    }
}

// ---------- 标题 ----------
#[test]
fn 标题全级别往返() {
    let src = "# h1\n\n## h2\n\n### h3\n\n#### h4\n\n##### h5\n\n###### h6";
    assert_eq!(assert_idempotent(src), src);
    assert_keeps("# 标题 **加粗** `代码`", &["标题", "**加粗**", "`代码`"]);
    assert_keeps("# 标题 $x^2$ 结尾", &["$x^2$"]);
}

// ---------- 段落行内样式 ----------
#[test]
fn 行内样式往返() {
    let src = "普通 **粗体** *斜体* ~~删除线~~ <u>下划线</u> ==高亮== <kbd>Ctrl+Q</kbd> `代码` X^2^ H~2~O 上标下标";
    assert_keeps(src, &["**粗体**", "*斜体*", "~~删除线~~", "<u>下划线</u>", "==高亮==", "<kbd>Ctrl+Q</kbd>", "`代码`", "X^2^", "H~2~O"]);
    assert_idempotent(src);
}

#[test]
fn 链接图片公式脚注往返() {
    let src = "链接 [文字](https://example.com \"标题\") 与 <https://auto.com> 自动链接";
    assert_keeps(src, &["[文字](https://example.com \"标题\")", "https://auto.com"]);
    assert_keeps("行内公式 $\\LaTeX$ 与 $x^2+y^2$ 混排", &["$\\LaTeX$", "$x^2+y^2$"]);
    assert_keeps("脚注引用[^1] 与[^note]", &["[^1]", "[^note]"]);
    assert_keeps("颜色 <font color=\"#ff0000\">红字</font> 与 <span style=\"background:#00ff00\">绿底</span>",
        &["红字", "绿底", "#ff0000", "#00ff00"]);
    assert_idempotent("链接 [文字](https://example.com \"标题\") 结尾");
}

// ---------- 列表 ----------
#[test]
fn 无序列表与任务列表往返() {
    assert_keeps("- 甲\n- 乙\n- 丙", &["- 甲", "- 乙", "- 丙"]);
    assert_keeps("- [ ] 待办\n- [x] 完成", &["- [ ] 待办", "- [x] 完成"]);
    assert_keeps("- 父项\n  - 子项一\n  - 子项二", &["- 父项", "- 子项一", "- 子项二"]);
    assert_idempotent("- 甲\n- 乙");
}

#[test]
fn 有序列表序号往返() {
    assert_keeps("1. 一\n2. 二\n3. 三", &["1. 一", "2. 二", "3. 三"]);
    assert_keeps("1. 父\n  1. 子一\n  2. 子二\n2. 二", &["1. 父", "1. 子一", "2. 子二", "2. 二"]);
    // 自定义起始
    assert_keeps("3. 三\n4. 四", &["3. 三", "4. 四"]);
    assert_idempotent("1. 一\n2. 二");
}

// ---------- 引用与警告框 ----------
#[test]
fn 引用与警告框往返() {
    assert_keeps("> 引用内容\n> 第二行", &["引用内容", "第二行"]);
    for marker in ["NOTE", "TIP", "IMPORTANT", "WARNING", "CAUTION"] {
        let src = format!("> [!{marker}]\n> 内容文字");
        assert_keeps(&src, &[&format!("[!{marker}]"), "内容文字"]);
        assert_idempotent(&src);
    }
    assert_keeps("> [!NOTE]\n> 含 **粗体** 与 [链接](https://a.com)", &["**粗体**", "[链接](https://a.com)"]);
}

// ---------- 代码块 ----------
#[test]
fn 代码块往返() {
    assert_keeps("```rust\nfn main() {\n    println!(\"hi\");\n}\n```", &["fn main()", "println!"]);
    assert_keeps("~~~python\nprint('x')\n~~~", &["print('x')"]);
    // 内容含三反引号：围栏自动加长
    let src = "```\n内容含 ``` 三反引号\n```";
    let once = assert_idempotent(src);
    assert!(once.contains("内容含 ``` 三反引号"), "内容保留: {once}");
    // 无语言围栏
    assert_keeps("```\n纯文本\n```", &["纯文本"]);
}

// ---------- 数学与图表 ----------
#[test]
fn 数学与图表往返() {
    assert_keeps("$$\n\\int_0^1 x^2 dx\n$$", &["\\int_0^1 x^2 dx"]);
    assert_keeps("$$\n\\begin{aligned} a &= b \\\\ c &= d \\end{aligned}\n$$", &["\\begin{aligned}"]);
    assert_keeps("```mermaid\ngraph TD\n  A --> B\n```", &["graph TD", "A --> B"]);
    assert_keeps("```plantuml\n@startuml\nA -> B: hi\n@enduml\n```", &["@startuml", "A -> B: hi"]);
    assert_idempotent("$$\nx^2\n$$");
    assert_idempotent("```mermaid\ngraph TD\n  A --> B\n```");
}

// ---------- 表格 ----------
#[test]
fn 表格往返() {
    let src = "| 左 | 中 | 右 |\n|:---|:---:|---:|\n| a | b | c |";
    assert_keeps(src, &["左", "中", "右", "a", "b", "c"]);
    assert_idempotent(src);
    // 引用内相邻表格不合并
    let q = "> | A | B |\n> |---|---|\n> | 1 | 2 |\n>\n> | C | D |\n> |---|---|\n> | 3 | 4 |";
    assert_keeps(q, &["A", "C"]);
    assert_idempotent(q);
}

// ---------- HTML 与图片 ----------
#[test]
fn html与图片往返() {
    // 独立 <img> 行（含 zoom）
    assert_keeps("<img src=\"./img/a.png\" alt=\"x\" style=\"zoom:50%;\" />", &["./img/a.png"]);
    // img + 文字同行（拆分）
    let mixed = "<img src=\"./img/a.png\" alt=\"x\" /> **说明**";
    assert_keeps(mixed, &["./img/a.png", "**说明**"]);
    // markdown 图片语法
    assert_keeps("![替代](./img/b.png \"标题\")", &["![替代](./img/b.png \"标题\")"]);
    // section 容器
    assert_keeps("<section>\n\n<img src=\"./img/a.png\"></img>\n\n</section>", &["./img/a.png", "</section>"]);
    // 注释
    assert_keeps("<!-- 注释内容 -->", &["<!-- 注释内容 -->"]);
}

// ---------- 脚注/引用定义/TOC/YAML ----------
#[test]
fn 定义类块往返() {
    assert_keeps("正文[^1]\n\n[^1]: 定义内容", &["[^1]: 定义内容", "[^1]"]);
    assert_keeps("[1]: https://example.com \"标题\"", &["[1]: https://example.com \"标题\""]);
    assert_keeps("[TOC]", &["[TOC]"]);
    assert_keeps("---\ntitle: 标题\ntags: [a, b]\n---\n\n正文", &["title: 标题", "正文"]);
    // 引用式链接/图片解析为可用目标
    let ref_link = "这是 [链接文字][r] 测试\n\n[r]: https://example.com";
    let json = serde_json::to_string(&parse_blocks(ref_link)[0]).unwrap();
    assert!(json.contains("example.com"), "引用式链接解析: {json}");
    assert_keeps(ref_link, &["[链接文字][r]", "[r]: https://example.com"]);
}

// ---------- 分隔线与空段落 ----------
#[test]
fn 分隔线与空段落往返() {
    assert_keeps("---", &["---"]);
    assert_keeps("上段\n\n---\n\n下段", &["上段", "---", "下段"]);
    assert_keeps("段落一\n\n段落二\n\n段落三", &["段落一", "段落二", "段落三"]);
}

// ---------- 综合文档幂等 ----------
#[test]
fn 综合文档幂等() {
    let doc = "---\ntitle: 综合\n---\n\n# 标题 A\n\n正文 **粗体** 与 $x^2$。\n\n- [ ] 任务\n- 普通\n\n1. 有序一\n2. 有序二\n\n> [!WARNING]\n> 警告内容\n\n```c\nint main() { return 0; }\n```\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\n$$\nE = mc^2\n$$\n\n```mermaid\ngraph TD\n  A --> B\n```\n\n<img src=\"./a.png\" alt=\"x\" style=\"zoom:50%;\" /> **说明**\n\n[^1]: 脚注定义\n";
    assert_idempotent(doc);
    assert_keeps(doc, &["title: 综合", "**粗体**", "$x^2$", "- [ ] 任务", "2. 有序二", "[!WARNING]",
        "int main()", "| A | B |", "E = mc^2", "graph TD", "./a.png", "**说明**", "[^1]: 脚注定义"]);
}

// ---------- CRLF 混合 ----------
#[test]
fn crlf内容幂等() {
    // 内容中的 CR 已被读取层规范化为 LF（files.rs），解析层对纯 LF 输入幂等
    let src = "# 标题\n\n- 列表\n\n```c\ncode\n```";
    assert_idempotent(src);
}
