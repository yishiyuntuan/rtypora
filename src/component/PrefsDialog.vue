<script setup vapor>
import { reactive, ref, watch } from 'vue';
import { getPrefs, setPref } from '../utils/prefs.js';

// 偏好设置对话框：编辑器 / 图像 / Markdown / 外观 四页（Typora 一体化外观风格）。
const props = defineProps({
  visible: { type: Boolean, default: false },
  page: { type: String, default: 'editor' },
});

const emit = defineEmits(['close']);

const activePage = ref(props.page);
// 打开时同步目标页并载入当前值
watch(() => props.visible, (v) => {
  if (v) {
    activePage.value = props.page;
    Object.assign(form, getPrefs());
  }
});

const form = reactive({ ...getPrefs() });

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
</script>

<template>
  <Teleport to="body">
    <div v-if="visible" class="prefs-backdrop" @click.self="emit('close')">
      <div class="prefs-panel t-app" @click.stop>
        <div class="flex h-9 items-center justify-between border-b border-(--t-table-border) px-4">
          <span class="text-[13px] font-medium">偏好设置</span>
          <span class="t-btn cursor-pointer rounded px-1.5 text-[15px]" @click="emit('close')">×</span>
        </div>
        <div class="flex flex-1 overflow-hidden">
          <!-- 左侧导航 -->
          <div class="w-28 border-r border-(--t-table-border) py-2">
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
          <div class="flex-1 overflow-y-auto p-4 text-[13px]">
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
            <div v-else-if="activePage === 'appearance'" class="space-y-3">
              <p class="t-dim text-[12px]">主题与字体</p>
              <p class="text-[13px]">主题切换、自定义主题导入、按块样式与公式中文字体请使用菜单「主题」或状态栏的主题下拉。</p>
            </div>
          </div>
        </div>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.prefs-backdrop {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.3);
  z-index: 95;
  display: flex;
  align-items: center;
  justify-content: center;
}
.prefs-panel {
  width: 560px;
  height: 380px;
  border: 1px solid var(--t-table-border);
  border-radius: 10px;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  box-shadow: 0 12px 40px rgba(0, 0, 0, 0.28);
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
</style>
