// 口齿不清：口腔含物时按比例把气泡字符替换成含糊拟声词字符，模拟嘴里有东西说不清。
#![allow(non_snake_case)]

use crate::behavior::rand01;

/// 每字符被替换为含糊字符的概率。
pub const GARBLE_RATIO: f32 = 0.4;

/// 含糊拟声词字符集（随机取一个替换）。
const MUFFLED: &[char] = &['唔', '嗯', '呜', '咕', '哔', '咝', '嘟', '呣'];

/// 按 GARBLE_RATIO 随机替换 text 中的字符为含糊字符。空白字符保留（不糊掉空格/换行）。
/// 每次调用结果不同（rand01 驱动）。
pub fn garble(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if ch.is_whitespace() {
            out.push(ch);
            continue;
        }
        if rand01() < GARBLE_RATIO {
            let idx = (rand01() * MUFFLED.len() as f32) as usize;
            let idx = idx.min(MUFFLED.len() - 1);
            out.push(MUFFLED[idx]);
        } else {
            out.push(ch);
        }
    }
    out
}
