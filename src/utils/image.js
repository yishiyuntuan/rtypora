// 图片源解析：远程 URL 直出（浏览器自有 HTTP 缓存）；
// 本地路径经 Rust 解析为存在的绝对路径后走 asset 协议（convertFileSrc）——
// 浏览器原生缓存 + 流式读取，虚拟滚动重挂载不再重复 base64 解码（滚动不再闪烁）。
// 解析结果按 文档目录|源 缓存（asset URL 恒定，重挂载零成本）。
import { invoke, convertFileSrc } from '@tauri-apps/api/core';

const cache = new Map();
// asset URL → 本地源信息（导出 HTML 时把 asset URL 换回 data URL，保证导出文件自包含）
const assetUrlToLocal = new Map();

// 解析图片源为可用于 <img src> 的 URL；失败返回 null（显示占位）
export async function resolveImageSrc(source, baseDir) {
  if (!source) return null;
  if (/^https?:\/\//.test(source)) return source;
  const key = `${baseDir ?? ''}|${source}`;
  if (cache.has(key)) return cache.get(key);
  const promise = invoke('resolve_image_path', { source, baseDir: baseDir ?? null })
    .then((abs) => {
      if (!abs) return null;
      const url = convertFileSrc(abs);
      assetUrlToLocal.set(url, { source, baseDir: baseDir ?? null });
      return url;
    })
    .catch(() => null);
  cache.set(key, promise);
  const result = await promise;
  if (!result) cache.delete(key); // 失败不缓存（文件可能稍后生成）
  return result;
}

// 导出 HTML 用：asset URL → data URL（非本应用生成的 asset URL 返回 null，保持原样）
export async function assetUrlToDataUrl(url) {
  const hit = assetUrlToLocal.get(url);
  if (!hit) return null;
  return invoke('read_image_data_url', { source: hit.source, baseDir: hit.baseDir }).catch(() => null);
}
