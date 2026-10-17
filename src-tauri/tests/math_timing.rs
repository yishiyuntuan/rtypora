//! 临时计时探针（验证后删除）：ratex 渲染耗时按内容对比
use std::time::Instant;
use tauri_app_lib::latex::{render_display_math, set_math_unicode_font};

fn bench(raw: &str, n: usize) -> f64 {
    let start = Instant::now();
    for _ in 0..n {
        let _ = render_display_math(raw, Some("#111111"), Some(16.0));
    }
    start.elapsed().as_secs_f64() * 1000.0 / n as f64
}

#[test]
fn 渲染耗时对比() {
    let ascii = "$$E=mc^2$$";
    let cjk = "$$\\text{质能方程与中文注释} E=mc^2$$";
    let complex = "$$\\int_{-\\infty}^{\\infty} e^{-x^2} \\, dx = \\sqrt{\\pi}, \\quad \\sum_{n=1}^{\\infty} \\frac{1}{n^2} = \\frac{\\pi^2}{6}$$";
    println!("ASCII 单次: {:.2}ms", bench(ascii, 10));
    println!("CJK 单次: {:.2}ms", bench(cjk, 10));
    println!("复杂公式 单次: {:.2}ms", bench(complex, 10));
    // 设置中文回落字体后再测
    set_math_unicode_font("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf");
    println!("CJK+字体回落 单次: {:.2}ms", bench(cjk, 10));
}
