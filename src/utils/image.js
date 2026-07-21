// 图片源解析：远程 URL 直出，本地路径经 Rust 命令读取为 data URL（结果缓存）。
import { invoke } from '@tauri-apps/api/core';

const cache = new Map();

// 解析图片源为可用于 <img src> 的 URL；失败返回 null（显示占位）
export async function resolveImageSrc(source, baseDir) {
  if (!source) return null;
  const key = `${baseDir ?? ''}|${source}`;
  if (cache.has(key)) return cache.get(key);
  const promise = invoke('read_image_data_url', { source, baseDir: baseDir ?? null })
    .catch(() => null);
  cache.set(key, promise);
  const result = await promise;
  if (!result) cache.delete(key); // 失败不缓存（文件可能稍后生成）
  return result;
}
