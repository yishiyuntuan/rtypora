<script setup vapor>
import { reactive, ref, computed, watch, onMounted, onBeforeUnmount } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { getPrefs, setPref } from '../utils/prefs.js';
import { isMac, mathCjkFontSpec } from '../utils/platform.js';
import { listThemes, currentThemeId, applyTheme, importThemeJson, removeCustomTheme, themeVersion, previewTheme, isSystemDark } from '../themes/index.js';

// 偏好设置页：编辑器 / 图像 / Markdown / 外观 四页。
// 两种形态：整窗页面层（默认）/ 菜单第三列内嵌（embedded）。
// 返回编辑器：右上角 × / Esc / 「← 返回」按钮
const props = defineProps({
  visible: { type: Boolean, default: false },
  page: { type: String, default: 'editor' },
  // 内嵌模式（菜单第三列）：根节点脱离整窗绝对定位，变为普通列容器
  embedded: { type: Boolean, default: false },
});

const emit = defineEmits(['close']);

// Esc 返回编辑器（与 × 等效）
function onPrefsKeydown(e) {
  if (e.key === 'Escape') {
    e.stopPropagation();
    emit('close');
  }
}
onMounted(() => window.addEventListener('keydown', onPrefsKeydown, true));
onBeforeUnmount(() => window.removeEventListener('keydown', onPrefsKeydown, true));

const activePage = ref(props.page);
// 打开时同步目标页并载入当前值
watch(() => props.visible, (v) => {
  if (v) {
    activePage.value = props.page;
    Object.assign(form, getPrefs());
  }
});
// 内嵌模式下列间切换：外部 page 变化同步到分页
watch(() => props.page, (p) => {
  activePage.value = p;
});

const form = reactive({ ...getPrefs() });

// 部分设置项仅 macOS 生效（原生标题栏红绿灯相关）

const pages = [
  { id: 'editor', label: '编辑器' },
  { id: 'image', label: '图像' },
  { id: 'markdown', label: 'Markdown' },
  { id: 'appearance', label: '外观' },
];

const imageBehaviors = [
  { value: 'off', label: '关闭（忽略粘贴的图片）' },
  { value: 'document', label: '复制到文档所在文件夹' },
  { value: 'assets', label: '复制到 assets 子文件夹' },
];

function update(key, value) {
  setPref(key, value === '' ? null : value);
}

// ---------- 外观页：主题选择 / 导入 / 删除自定义主题 / 公式中文字体 ----------
const themes = computed(() => {
  themeVersion.value;
  return listThemes();
});

// 系统字体列表（queryLocalFonts 同步；不支持/无权限时回落常见字体）
const systemFonts = ref([]);
const FALLBACK_FONTS = [
  '微软雅黑', '宋体', '等线', '黑体', '楷体', '仿宋', '苹方-简',
  'Consolas', 'Cascadia Mono', 'Cascadia Code', 'JetBrains Mono', 'Courier New', 'Georgia', 'Arial',
];
onMounted(async () => {
  try {
    if ('queryLocalFonts' in window) {
      const fonts = await window.queryLocalFonts();
      systemFonts.value = [...new Set(fonts.map((f) => f.family))].sort((a, b) =>
        a.localeCompare(b, 'zh-CN'),
      );
    }
  } catch {
    // 权限拒绝或平台不支持：回落常见字体清单
  }
  if (!systemFonts.value.length) systemFonts.value = FALLBACK_FONTS;
});
const activeThemeId = computed(() => {
  themeVersion.value;
  return currentThemeId();
});

// 主题卡片缩略图配色（解析主题继承链后的完整配色，一次预计算）
const themePreviews = computed(() => {
  themeVersion.value;
  const map = new Map();
  for (const pack of listThemes()) {
    const c = previewTheme(pack).colors || {};
    map.set(pack.id, {
      bg: c.editor_background,
      heading: c.text_h1 || c.text_default,
      text: c.text_default,
      strong: c.text_strong || c.text_default,
      link: c.text_link,
      markBg: c.mark_bg,
      markText: c.mark_text,
      inlineCodeBg: c.inline_code_bg,
      inlineCodeText: c.inline_code_text,
      quoteBorder: c.border_quote,
      quoteBg: c.quote_bg,
      quoteText: c.text_quote || c.text_default,
      marker: c.list_marker,
      codeBg: c.code_bg,
      codeKw: c.code_syntax_keyword,
      codeStr: c.code_syntax_string,
      codeNum: c.code_syntax_number,
      codeComment: c.code_syntax_comment,
      tblHeaderBg: c.table_header_bg,
      tblCellBg: c.table_cell_bg,
      accent: c.tab_indicator,
      border: c.table_border,
      statusBg: c.status_bar_background,
      statusText: c.status_bar_text_dim || c.status_bar_text,
    });
  }
  return map;
});

function onSelectTheme(id) {
  if (form.theme_follow_system) {
    // 跟随系统模式：卡片选择写入当前系统外观对应的主题槽位
    // （prefsVersion watcher 会自动应用）
    update(isSystemDark() ? 'theme_dark_id' : 'theme_light_id', id);
  } else {
    applyTheme(id);
  }
}
function onRemoveTheme(id) {
  removeCustomTheme(id);
  themeVersion.value += 1;
}
// 导入主题：Rust 端原生对话框选读主题包（WKWebView 不认隐藏 file input 的程序化点击）
async function onImportTheme() {
  const text = await invoke('pick_theme_file').catch(() => null);
  if (!text) return;
  try {
    importThemeJson(text);
  } catch (err) {
    alert(`导入主题失败：${err.message || err}`);
  }
}
</script>

<template>
  <div v-if="visible" class="prefs-page t-app" :class="{ 'prefs-page-embedded': embedded }">
    <!-- 内嵌列顶部拖拽条（设置打开时窗口顶部可拖动；整窗模式用下方头部） -->
    <div v-if="embedded" class="prefs-drag-strip" data-tauri-drag-region></div>
    <!-- 整窗模式的头部（内嵌列不显示：页面切换由菜单卡片列承担，Esc 收列） -->
    <div v-if="!embedded" class="flex h-9 shrink-0 items-center justify-between border-b border-(--t-table-border) px-4" data-tauri-drag-region>
      <span class="text-[13px] font-medium" data-tauri-drag-region>偏好设置</span>
      <span class="t-btn flex cursor-pointer items-center gap-1 rounded px-2 py-0.5 text-[13px]" title="返回编辑器（Esc）" @click="emit('close')">
        <svg viewBox="0 0 16 16" class="size-[12px]" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
          <path d="M10 3L5 8l5 5" />
        </svg>
        返回
      </span>
    </div>
    <div class="flex flex-1 overflow-hidden">
      <!-- 左侧导航（内嵌列不显示） -->
      <div v-if="!embedded" class="w-36 shrink-0 border-r border-(--t-table-border) py-2">
        <div
          v-for="p in pages"
          :key="p.id"
          class="t-btn cursor-pointer px-4 py-2 text-[12px]"
          :class="{ 'bg-(--t-status-bar-button-hover) font-medium': activePage === p.id }"
          @click="activePage = p.id"
        >
          {{ p.label }}
        </div>
      </div>

      <!-- 内容区 -->
      <div class="flex-1 overflow-y-auto text-[13px]" :class="embedded ? 'px-8 py-7' : 'p-6'">
        <div class="mx-auto max-w-160">
        <!-- 编辑器 -->
            <div v-if="activePage === 'editor'" class="space-y-4">
              <label class="block">
                <span class="t-dim mb-1 block text-[12px]">正文字号（px，留空跟随主题）</span>
                <input
                  type="number" min="10" max="32" step="0.5"
                  class="prefs-input"
                  :value="form.text_size"
                  placeholder="17"
                  @change="update('text_size', $event.target.valueAsNumber || null)"
                />
              </label>
              <label class="block">
                <span class="t-dim mb-1 block text-[12px]">行高（倍数，留空跟随主题）</span>
                <input
                  type="number" min="1" max="2.5" step="0.05"
                  class="prefs-input"
                  :value="form.text_line_height"
                  placeholder="1.6"
                  @change="update('text_line_height', $event.target.valueAsNumber || null)"
                />
              </label>
              <label class="block">
                <span class="t-dim mb-1 block text-[12px]">内容最大宽度（px，留空全宽）</span>
                <input
                  type="number" min="400" max="1600" step="10"
                  class="prefs-input"
                  :value="form.content_max_width"
                  placeholder="全宽"
                  @change="update('content_max_width', $event.target.valueAsNumber || null)"
                />
              </label>
              <label class="block">
                <span class="t-dim mb-1 block text-[12px]">编辑区内边距（px，留空跟随主题）</span>
                <input
                  type="number" min="0" max="120" step="1"
                  class="prefs-input"
                  :value="form.editor_padding"
                  placeholder="24"
                  @change="update('editor_padding', $event.target.valueAsNumber || null)"
                />
              </label>
              <label class="flex cursor-pointer items-center gap-2">
                <input
                  type="checkbox"
                  :checked="form.first_line_indent"
                  @change="update('first_line_indent', $event.target.checked)"
                />
                <span>段落首行缩进（2 字符宽）</span>
              </label>
              <label class="flex cursor-pointer items-center gap-2">
                <input
                  type="checkbox"
                  :checked="form.scrollbar_auto_hide"
                  @change="update('scrollbar_auto_hide', $event.target.checked)"
                />
                <span>滚动条自动隐藏（悬停或滚动时显示）</span>
              </label>
              <label class="flex cursor-pointer items-center gap-2">
                <input
                  type="checkbox"
                  :checked="form.sidebar_toolbar_auto_hide"
                  @change="update('sidebar_toolbar_auto_hide', $event.target.checked)"
                />
                <span>侧边栏操作栏自动隐藏（鼠标滑过侧栏时显示）</span>
              </label>
              <p class="t-dim text-[12px]">仅对普通段落生效；引用、列表、表格等容器内的段落不缩进。</p>

              <div>
                <span class="t-dim mb-2 block text-[12px]">自动保存触发方式（开关在状态栏，默认关闭）</span>
                <label class="t-btn mb-1 flex cursor-pointer items-center gap-2 rounded px-2 py-1.5">
                  <input
                    type="radio"
                    name="auto-save-trigger"
                    :checked="form.auto_save_trigger === 'blur'"
                    @change="update('auto_save_trigger', 'blur')"
                  />
                  <span>焦点切换时保存</span>
                </label>
                <label class="t-btn mb-1 flex cursor-pointer items-center gap-2 rounded px-2 py-1.5">
                  <input
                    type="radio"
                    name="auto-save-trigger"
                    :checked="form.auto_save_trigger === 'delay'"
                    @change="update('auto_save_trigger', 'delay')"
                  />
                  <span>输入停止后</span>
                  <input
                    type="number" min="1" max="60" step="1"
                    class="prefs-input"
                    style="width: 64px"
                    :value="form.auto_save_delay_seconds"
                    @change="update('auto_save_delay_seconds', Math.min(60, Math.max(1, $event.target.valueAsNumber || 3)))"
                  />
                  <span>秒自动保存</span>
                </label>
                <p class="t-dim text-[12px]">仅对已保存过的文件生效；新文档请先手动保存。</p>
              </div>
            </div>

            <!-- 图像 -->
            <div v-else-if="activePage === 'image'" class="space-y-4">
              <div>
                <span class="t-dim mb-2 block text-[12px]">粘贴图片时</span>
                <label
                  v-for="opt in imageBehaviors"
                  :key="opt.value"
                  class="t-btn mb-1 flex cursor-pointer items-center gap-2 rounded px-2 py-1.5"
                >
                  <input
                    type="radio"
                    name="image-paste"
                    :checked="form.image_paste_behavior === opt.value"
                    @change="update('image_paste_behavior', opt.value)"
                  />
                  <span>{{ opt.label }}</span>
                </label>
              </div>
              <p class="t-dim text-[12px]">相对路径的图片以当前文档所在文件夹为基准保存与解析。</p>
            </div>

            <!-- Markdown -->
            <div v-else-if="activePage === 'markdown'" class="space-y-3">
              <label class="flex cursor-pointer items-center gap-2">
                <input
                  type="checkbox"
                  :checked="form.render_code_highlight"
                  @change="update('render_code_highlight', $event.target.checked)"
                />
                <span>代码块语法高亮</span>
              </label>
              <label class="flex cursor-pointer items-center gap-2">
                <input
                  type="checkbox"
                  :checked="form.render_code_line_numbers"
                  @change="update('render_code_line_numbers', $event.target.checked)"
                />
                <span>代码块行号显示</span>
              </label>
              <label class="flex cursor-pointer items-center gap-2">
                <input
                  type="checkbox"
                  :checked="form.render_mermaid"
                  @change="update('render_mermaid', $event.target.checked)"
                />
                <span>Mermaid 图表渲染</span>
              </label>
              <label class="flex cursor-pointer items-center gap-2">
                <input
                  type="checkbox"
                  :checked="form.render_plantuml"
                  @change="update('render_plantuml', $event.target.checked)"
                />
                <span>PlantUML 图表渲染</span>
              </label>
              <label class="flex items-center justify-between gap-2">
                <span>PlantUML 渲染方式</span>
                <select
                  class="prefs-select"
                  :value="form.plantuml_renderer"
                  @change="update('plantuml_renderer', $event.target.value)"
                >
                  <option value="local">本地引擎渲染（离线，无需服务器）</option>
                  <option value="server">渲染服务器（需网络）</option>
                </select>
              </label>
              <label class="flex items-center justify-between gap-2">
                <span>PlantUML 服务器</span>
                <input
                  type="text"
                  class="prefs-select w-64"
                  :value="form.plantuml_server"
                  placeholder="https://www.plantuml.com/plantuml"
                  @change="update('plantuml_server', $event.target.value.trim() || 'https://www.plantuml.com/plantuml')"
                />
              </label>
              <label class="flex cursor-pointer items-center gap-2">
                <input
                  type="checkbox"
                  :checked="form.render_math"
                  @change="update('render_math', $event.target.checked)"
                />
                <span>数学公式渲染</span>
              </label>
              <label class="flex cursor-pointer items-center gap-2">
                <input
                  type="checkbox"
                  :checked="form.render_html_block"
                  @change="update('render_html_block', $event.target.checked)"
                />
                <span>&lt;section&gt; 图文排版渲染</span>
              </label>
              <label class="flex cursor-pointer items-center gap-2">
                <input
                  type="checkbox"
                  :checked="form.html_to_md"
                  @change="update('html_to_md', $event.target.checked)"
                />
                <span>HTML 标签转换为 Markdown 语法</span>
              </label>
              <label class="flex cursor-pointer items-center gap-2">
                <input
                  type="checkbox"
                  :checked="form.callout_unify"
                  @change="update('callout_unify', $event.target.checked)"
                />
                <span>警告框扩展语法统一为标准格式</span>
              </label>
              <p class="t-dim text-[12px]">开启后 Obsidian 别名（[!hint] 等）与 :::warning / !!! warning 容器按标准 [!TYPE] 警告框解析并在保存时统一为该格式。</p>
              <label class="flex items-center justify-between gap-2">
                <span>公式自动编号</span>
                <select
                  class="prefs-select"
                  :value="form.math_numbering"
                  @change="update('math_numbering', $event.target.value)"
                >
                  <option value="off">不启用公式的自动编号</option>
                  <option value="ams">根据 AMS 规则对公式使用自动编号</option>
                  <option value="all">对所有公式使用自动编号</option>
                </select>
              </label>
              <p class="t-dim text-[12px]">关闭后对应内容按源码显示，不影响文档内容。</p>
            </div>

            <!-- 外观 -->
            <div v-else-if="activePage === 'appearance'" class="space-y-4">
              <div>
                <span class="t-dim mb-2 block text-[12px]">主题（内置与自定义，点击卡片切换）</span>
                <div class="grid grid-cols-2 gap-3">
                  <div
                    v-for="theme in themes"
                    :key="theme.id"
                    class="theme-card"
                    :class="{ 'theme-card-active': activeThemeId === theme.id }"
                    :title="theme.name"
                    @click="onSelectTheme(theme.id)"
                  >
                    <!-- 竖版卡片（扑克牌式）：上方主题缩略图（该主题完整配色画的迷你编辑器：
                         标题/正文行内样式/引用/列表/代码语法/表格/状态条），下方名称与操作 -->
                    <div
                      class="theme-thumb"
                      :style="{ background: themePreviews.get(theme.id)?.bg, borderColor: themePreviews.get(theme.id)?.border, color: themePreviews.get(theme.id)?.text }"
                    >
                      <div class="thumb-h" :style="{ color: themePreviews.get(theme.id)?.heading }">标题 Aa</div>
                      <div class="thumb-line">
                        正文 <span :style="{ color: themePreviews.get(theme.id)?.link }">链接</span>
                        <b :style="{ color: themePreviews.get(theme.id)?.strong }">加粗</b>
                        <span class="thumb-mark" :style="{ background: themePreviews.get(theme.id)?.markBg, color: themePreviews.get(theme.id)?.markText }">标记</span>
                        <code class="thumb-icode" :style="{ background: themePreviews.get(theme.id)?.inlineCodeBg, color: themePreviews.get(theme.id)?.inlineCodeText }">code</code>
                      </div>
                      <div
                        class="thumb-quote"
                        :style="{ borderLeftColor: themePreviews.get(theme.id)?.quoteBorder, background: themePreviews.get(theme.id)?.quoteBg, color: themePreviews.get(theme.id)?.quoteText }"
                      >引用文本一行</div>
                      <div class="thumb-line">
                        <span class="thumb-li"><i :style="{ background: themePreviews.get(theme.id)?.marker }"></i>列表项一</span>
                        <span class="thumb-li"><i :style="{ background: themePreviews.get(theme.id)?.marker }"></i>列表项二</span>
                      </div>
                      <div class="thumb-code" :style="{ background: themePreviews.get(theme.id)?.codeBg }">
                        <span :style="{ color: themePreviews.get(theme.id)?.codeKw }">fn</span>
                        <span :style="{ color: themePreviews.get(theme.id)?.codeStr }"> "str"</span>
                        <span :style="{ color: themePreviews.get(theme.id)?.codeNum }"> 42</span>
                        <span :style="{ color: themePreviews.get(theme.id)?.codeComment }"> // 注释</span>
                      </div>
                      <div class="thumb-table" :style="{ borderColor: themePreviews.get(theme.id)?.border }">
                        <span class="thumb-th" :style="{ background: themePreviews.get(theme.id)?.tblHeaderBg, borderColor: themePreviews.get(theme.id)?.border }">表头</span>
                        <span class="thumb-td" :style="{ background: themePreviews.get(theme.id)?.tblCellBg, borderColor: themePreviews.get(theme.id)?.border }">单元格</span>
                      </div>
                      <div class="thumb-status" :style="{ background: themePreviews.get(theme.id)?.statusBg, color: themePreviews.get(theme.id)?.statusText }">
                        <span class="thumb-dot" :style="{ background: themePreviews.get(theme.id)?.accent }"></span>
                        <span class="thumb-stext">Ln 1, Col 1</span>
                      </div>
                    </div>
                    <div class="theme-card-foot">
                      <span class="theme-card-check">{{ activeThemeId === theme.id ? '✓' : '' }}</span>
                      <span class="min-w-0 flex-1 truncate">{{ theme.name }}</span>
                      <span
                        v-if="theme.id.startsWith('custom-')"
                        class="theme-card-del t-dim"
                        title="删除该自定义主题"
                        @click.stop="onRemoveTheme(theme.id)"
                      >删除</span>
                    </div>
                  </div>
                </div>
              </div>
              <!-- 主题跟随系统：暗色/亮色外观可分别指定主题 -->
              <div class="settings-card">
                <label class="flex cursor-pointer items-center gap-2">
                  <input
                    type="checkbox"
                    :checked="form.theme_follow_system"
                    @change="update('theme_follow_system', $event.target.checked)"
                  />
                  <span>主题跟随系统外观</span>
                </label>
                <template v-if="form.theme_follow_system">
                  <label class="mt-2 block">
                    <span class="t-dim mb-1 block text-[12px]">暗色外观主题</span>
                    <select
                      class="prefs-select w-full max-w-100"
                      :value="form.theme_dark_id"
                      @change="update('theme_dark_id', $event.target.value)"
                    >
                      <option v-for="t in themes" :key="t.id" :value="t.id">{{ t.name }}</option>
                    </select>
                  </label>
                  <label class="mt-2 block">
                    <span class="t-dim mb-1 block text-[12px]">亮色外观主题</span>
                    <select
                      class="prefs-select w-full max-w-100"
                      :value="form.theme_light_id"
                      @change="update('theme_light_id', $event.target.value)"
                    >
                      <option v-for="t in themes" :key="t.id" :value="t.id">{{ t.name }}</option>
                    </select>
                  </label>
                  <p class="t-dim mt-2 text-[12px]">系统外观切换时自动应用对应主题；点击上方主题卡片可设置当前外观使用的主题。</p>
                </template>
              </div>
              <!-- 设置项同样卡片化 -->
              <div v-if="isMac" class="settings-card">
                <label class="flex cursor-pointer items-center gap-2">
                  <input
                    type="checkbox"
                    :checked="form.traffic_light_autohide"
                    @change="update('traffic_light_autohide', $event.target.checked)"
                  />
                  <span>窗口按钮自动隐藏</span>
                </label>
                <p class="t-dim mt-1 text-[12px]">仅 macOS 生效：平时隐藏左上角的红绿灯按钮，鼠标滑过左上角时显示。</p>
              </div>
              <div class="settings-card">
                <span class="t-dim mb-2 block text-[12px]">编辑器字体（列表与系统同步，留空跟随主题）</span>
                <input
                  type="text"
                  class="prefs-input w-full max-w-100"
                  list="prefs-font-list"
                  :value="form.editor_font_family"
                  placeholder="跟随主题（如 微软雅黑）"
                  @change="update('editor_font_family', $event.target.value.trim() || null)"
                />
              </div>
              <div class="settings-card">
                <span class="t-dim mb-2 block text-[12px]">代码块字体（等宽，留空跟随主题）</span>
                <input
                  type="text"
                  class="prefs-input w-full max-w-100"
                  list="prefs-font-list"
                  :value="form.code_font_family"
                  placeholder="跟随主题（如 Consolas）"
                  @change="update('code_font_family', $event.target.value.trim() || null)"
                />
              </div>
              <datalist id="prefs-font-list">
                <option v-for="f in systemFonts" :key="f" :value="f" />
              </datalist>
              <div class="settings-card">
                <span class="t-dim mb-2 block text-[12px]">导入主题</span>
                <button type="button" class="prefs-select cursor-pointer" @click="onImportTheme">导入主题（JSON/JSONC/YAML）…</button>
                <p class="t-dim mt-2 text-[12px]">按块样式、伪元素/状态键与主题 token 的编写方式见项目内「自定义主题.md」。</p>
              </div>
              <div class="settings-card">
                <span class="t-dim mb-2 block text-[12px]">公式中文字体</span>
                <input
                  type="text"
                  class="prefs-input w-full max-w-100"
                  :value="form.math_cjk_font"
                  :placeholder="mathCjkFontSpec"
                  @change="update('math_cjk_font', $event.target.value.trim() || null)"
                />
                <p class="t-dim mt-2 text-[12px]">ratex 字体规格：路径 或 路径#字体族名；留空跟随主题，重启应用后生效。</p>
              </div>
            </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
/* 偏好设置整页层（覆盖整个窗口含侧边栏，同一窗口打开非对话框） */
.prefs-page {
  position: absolute;
  inset: 0;
  z-index: 40;
  display: flex;
  flex-direction: column;
  background: var(--t-editor-background);
  color: var(--t-text-default);
}
/* 内嵌模式（菜单第三列）：脱离整窗绝对定位，作为普通列容器撑满高度 */
.prefs-page.prefs-page-embedded {
  position: relative;
  inset: auto;
  z-index: auto;
  height: 100%;
}
/* 内嵌列顶部拖拽条（菜单/设置打开时的窗口拖动区） */
.prefs-drag-strip {
  flex-shrink: 0;
  height: 12px;
  display: flex;
  align-items: center;
  justify-content: center;
}
.prefs-drag-strip::after {
  content: '';
  width: 28px;
  height: 3px;
  border-radius: 2px;
  background: var(--t-table-border);
}
.prefs-input {
  width: 200px;
  padding: 4px 8px;
  border: 1px solid var(--t-table-border);
  border-radius: 6px;
  background: transparent;
  color: inherit;
  outline: none;
}
.prefs-select {
  max-width: 260px;
  padding: 4px 8px;
  border: 1px solid var(--t-table-border);
  border-radius: 6px;
  background: transparent;
  color: inherit;
  outline: none;
}
/* 下拉闭合与展开都跟随主题，避免原生白底闪变 */
.prefs-select option {
  background: var(--t-editor-background);
  color: var(--t-text-default);
}

/* 主题卡片（竖版扑克牌式：上方缩略图取该主题完整配色画的迷你编辑器） */
.theme-card {
  display: flex;
  flex-direction: column;
  border: 1px solid var(--t-table-border);
  border-radius: 8px;
  overflow: hidden;
  cursor: pointer;
  transition: border-color 0.1s, box-shadow 0.1s;
}
.theme-card:hover {
  border-color: color-mix(in srgb, var(--t-tab-indicator) 45%, transparent);
}
.theme-card-active {
  border-color: var(--t-tab-indicator);
  box-shadow: 0 0 0 1px var(--t-tab-indicator);
}
.theme-thumb {
  position: relative;
  height: 196px;
  flex-shrink: 0;
  padding: 10px 12px 16px;
  border-bottom: 1px solid var(--t-table-border);
  font-size: 11px;
  line-height: 1.6;
  overflow: hidden;
  user-select: none;
}
.thumb-h {
  font-size: 13px;
  font-weight: 700;
}
.thumb-line {
  margin-top: 2px;
  display: flex;
  align-items: center;
  gap: 4px;
  flex-wrap: wrap;
}
.thumb-mark {
  padding: 0 3px;
  border-radius: 3px;
}
.thumb-icode {
  padding: 0 4px;
  border-radius: 3px;
  font-family: monospace;
  font-size: 10px;
}
.thumb-quote {
  margin-top: 4px;
  padding: 1px 6px;
  border-left: 2px solid;
  border-radius: 0 3px 3px 0;
}
.thumb-li {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  margin-right: 8px;
}
.thumb-li i {
  width: 4px;
  height: 4px;
  border-radius: 50%;
}
.thumb-code {
  display: inline-block;
  margin-top: 4px;
  padding: 1px 6px;
  border-radius: 4px;
  font-family: monospace;
}
.thumb-table {
  display: inline-flex;
  margin-top: 5px;
  border: 1px solid;
  border-radius: 3px;
  overflow: hidden;
}
.thumb-th,
.thumb-td {
  padding: 0 8px;
  font-size: 10px;
}
.thumb-th {
  border-right: 1px solid;
  font-weight: 600;
}
.thumb-status {
  position: absolute;
  left: 0;
  right: 0;
  bottom: 0;
  height: 14px;
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 0 10px;
  font-size: 9px;
}
.thumb-dot {
  width: 14px;
  height: 2px;
  border-radius: 1px;
}
.thumb-stext {
  opacity: 0.9;
}
.theme-card-foot {
  display: flex;
  flex: 1;
  align-items: center;
  gap: 5px;
  padding: 6px 8px;
  font-size: 12px;
  min-width: 0;
}
.theme-card-check {
  width: 12px;
  flex-shrink: 0;
  color: var(--t-text-link);
}
.theme-card-del {
  cursor: pointer;
  flex-shrink: 0;
}
.theme-card-del:hover {
  color: #d35d2e;
}

/* 设置项卡片（与主题卡片同一容器语言） */
.settings-card {
  border: 1px solid var(--t-table-border);
  border-radius: 8px;
  padding: 12px 14px;
}
</style>
