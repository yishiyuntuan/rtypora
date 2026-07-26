<script setup vapor>
import { ref, watch, nextTick } from 'vue';
import { SLASH_TEXT_GROUPS, CALLOUT_TYPES, FONT_COLORS, FONT_SIZES } from '../utils/wysiwyg.js';

// 斜杠命令菜单面板：只负责展示与事件转发。
// 「文本」行（四行徽章：标题 H1-H6 / 行内格式 B/I/U/S/== / 列表·引用·链接 / 代码·公式，
// ←/→ 移列、↑/↓ 移行）、表格行（行×列数量，↑/↓ 增减、←/→ 切换行列）、
// 警告框行（[!TYPE] 类型徽章两行，←/→ 切换、Enter 应用、点选直用）、
// 字体颜色行（色板 + RGB 输入内联一行，←/→ 切换色板、Enter 应用）；
// 样式走 --t-* 主题变量，blocks.slashMenu* 可定制。
const props = defineProps({
  items: { type: Array, default: () => [] },
  index: { type: Number, default: 0 },
  // 文本行当前徽章行列（←/→ 移列、↑/↓ 移行、悬停徽章同步）
  textRow: { type: Number, default: 0 },
  textCol: { type: Number, default: 0 },
  // 表格行行列数量与当前调节字段（'rows' | 'cols'）
  tableRows: { type: Number, default: 2 },
  tableCols: { type: Number, default: 2 },
  tableField: { type: String, default: 'rows' },
  // 警告框当前类型（CALLOUT_TYPES 下标）
  calloutType: { type: Number, default: 0 },
  // 字体行当前行列（row 0=颜色、1=字号）与 RGB 非法值标记
  fontRow: { type: Number, default: 0 },
  fontColorIndex: { type: Number, default: 0 },
  fontSizeIndex: { type: Number, default: 0 },
  rgbError: { type: Boolean, default: false },
  left: { type: Number, default: 0 },
  top: { type: Number, default: 0 },
});
const emit = defineEmits([
  'pick',
  'hover',
  'text-cell',
  'table-field',
  'callout-type',
  'font-cell',
  'rgb-apply',
  'rgb-focus',
  'rgb-cancel',
  'rgb-nav',
]);

const groups = SLASH_TEXT_GROUPS;
const calloutTypes = CALLOUT_TYPES;
// 警告框类型分两行展示（与「文本」项同布局：前导行 + 徽章行）
const calloutRows = [CALLOUT_TYPES.slice(0, 3), CALLOUT_TYPES.slice(3)];
const fontColors = FONT_COLORS;
const fontSizes = FONT_SIZES;
// 字体行的 RGB 输入（本地未受控文本，Enter/Esc 经事件上交）
const rgbText = ref('');
// 行导航进入 RGB 输入框行（row=2 且字体项激活）时聚焦输入框。
// 注意：ref 位于 v-for 内，Vue 3 会收集为数组，需取首元素
const rgbInputEl = ref(null);
function focusRgbInput() {
  const el = Array.isArray(rgbInputEl.value) ? rgbInputEl.value[0] : rgbInputEl.value;
  el?.focus();
}
watch(
  () => [props.fontRow, props.index],
  () => {
    if (props.fontRow !== 2) return;
    if (props.items[props.index]?.id !== 'fontColor') return;
    nextTick(focusRgbInput);
  },
);

// 键盘导航时让选中项保持可见：只调整菜单自身滚动位置
//（不用 scrollIntoView——它会连带滚动编辑器/页面等外层滚动容器；
// flush: 'post' 等 active 类落到新选中项后再测量，否则滚动慢半拍、选项卡在下沿）
const menuEl = ref(null);
watch(
  () => props.index,
  () => {
    const menu = menuEl.value;
    const active = menu?.querySelector('.md-slash-item-active');
    if (!menu || !active) return;
    if (active.offsetTop < menu.scrollTop) {
      menu.scrollTop = active.offsetTop - 4;
    } else if (active.offsetTop + active.offsetHeight > menu.scrollTop + menu.clientHeight) {
      menu.scrollTop = active.offsetTop + active.offsetHeight - menu.clientHeight + 4;
    }
  },
  { flush: 'post' },
);
</script>

<template>
  <div ref="menuEl" class="md-slash-menu" :style="{ left: `${left}px`, top: `${top}px` }">
    <template v-if="items.length">
      <template v-for="(item, i) in items" :key="item.id">
        <!-- 文本：前导图标独占一行 + 三行多彩徽章（标题 / 行内格式 / 列表） -->
        <div
          v-if="item.id === 'text'"
          class="md-slash-item md-slash-text"
          :class="{ 'md-slash-item-active': i === index }"
          @mousedown.prevent="emit('pick', item)"
          @mouseenter="emit('hover', i)"
        >
          <span class="md-slash-text-lead">
            <span class="md-slash-icon" :style="{ color: item.iconColor }" v-html="item.icon"></span>
            <span>{{ item.label }}</span>
          </span>
          <span class="md-slash-groups">
            <span v-for="(group, gi) in groups" :key="gi" class="md-slash-levels">
              <span
                v-for="(b, bi) in group"
                :key="b.id"
                class="md-slash-level"
                :style="{ color: b.color }"
                :class="{ 'md-slash-level-icon': !!b.icon, 'md-slash-level-active': i === index && gi === textRow && bi === textCol }"
                :title="b.label"
                @mousedown.prevent.stop="emit('pick', item, { row: gi, col: bi })"
                @mouseenter="emit('text-cell', { row: gi, col: bi })"
              ><span v-if="b.icon" v-html="b.icon"></span><template v-else>{{ b.text }}</template></span>
            </span>
          </span>
        </div>
        <!-- 表格：行×列数量徽章（点击徽章切换调节字段，行点击应用） -->
        <div
          v-else-if="item.id === 'table'"
          class="md-slash-item"
          :class="{ 'md-slash-item-active': i === index }"
          @mousedown.prevent="emit('pick', item)"
          @mouseenter="emit('hover', i)"
        >
          <span class="md-slash-icon" :style="{ color: item.iconColor }" v-html="item.icon"></span>
          <span>{{ item.label }}</span>
          <span class="md-slash-levels">
            <span
              class="md-slash-dim"
              :class="{ 'md-slash-level-active': i === index && tableField === 'rows' }"
              title="数据行数（←/→ 进入字段，↑/↓ 增减）"
              @mousedown.prevent.stop="emit('table-field', 'rows')"
            >行 {{ tableRows }}</span>
            <span class="md-slash-dim-x">×</span>
            <span
              class="md-slash-dim"
              :class="{ 'md-slash-level-active': i === index && tableField === 'cols' }"
              title="列数（←/→ 进入字段，↑/↓ 增减）"
              @mousedown.prevent.stop="emit('table-field', 'cols')"
            >列 {{ tableCols }}</span>
          </span>
        </div>
        <!-- 警告框：前导行 + 两行 [!TYPE] 类型徽章（与「文本」同布局；点击直用，←/→ 切换、Enter 应用当前类型） -->
        <div
          v-else-if="item.id === 'callout'"
          class="md-slash-item md-slash-text"
          :class="{ 'md-slash-item-active': i === index }"
          @mousedown.prevent="emit('pick', item)"
          @mouseenter="emit('hover', i)"
        >
          <span class="md-slash-text-lead">
            <span class="md-slash-icon" :style="{ color: item.iconColor }" v-html="item.icon"></span>
            <span>{{ item.label }}</span>
          </span>
          <span class="md-slash-groups">
            <span v-for="(row, ri) in calloutRows" :key="ri" class="md-slash-levels">
              <span
                v-for="t in row"
                :key="t.id"
                class="md-slash-dim"
                :style="{ color: t.color }"
                :class="{ 'md-slash-level-active': i === index && calloutTypes.indexOf(t) === calloutType }"
                :title="t.label"
                @mousedown.prevent.stop="emit('pick', item, { calloutVariant: t.id })"
                @mouseenter="emit('callout-type', calloutTypes.indexOf(t))"
              >{{ t.id }}</span>
            </span>
          </span>
        </div>
        <!-- 字体：前导行 + 色板（6×2）+ 字号（5×2）+ RGB 输入（点选直用；←/→ 移列，↑/↓ 网格内上下/换区，Enter 应用） -->
        <div
          v-else-if="item.id === 'fontColor'"
          class="md-slash-item md-slash-text"
          :class="{ 'md-slash-item-active': i === index }"
          @mousedown.prevent="emit('pick', item)"
          @mouseenter="emit('hover', i)"
        >
          <span class="md-slash-text-lead">
            <span class="md-slash-icon" :style="{ color: item.iconColor }" v-html="item.icon"></span>
            <span>{{ item.label }}</span>
          </span>
          <span class="md-slash-groups">
            <span class="md-color-swatches">
              <button
                v-for="(c, ci) in fontColors"
                :key="c.label"
                type="button"
                class="md-color-swatch"
                :class="{ 'md-color-swatch-active': i === index && fontRow === 0 && ci === fontColorIndex }"
                :title="c.label"
                :style="{ background: c.css }"
                @mousedown.prevent.stop="emit('pick', item, { fontColor: c.color })"
                @mouseenter="emit('font-cell', { row: 0, col: ci })"
              ></button>
            </span>
            <span class="md-slash-levels md-slash-sizes">
              <span
                v-for="(s, si) in fontSizes"
                :key="s.label"
                class="md-slash-level"
                :class="{ 'md-slash-level-active': i === index && fontRow === 1 && si === fontSizeIndex }"
                :title="`字号 ${s.label}px`"
                @mousedown.prevent.stop="emit('pick', item, { fontSize: s.fontSize })"
                @mouseenter="emit('font-cell', { row: 1, col: si })"
              >{{ s.label }}</span>
            </span>
            <span class="md-color-rgb">
              <input
                ref="rgbInputEl"
                v-model="rgbText"
                class="md-color-input"
                :class="{ error: rgbError, 'md-color-input-active': i === index && fontRow === 2 }"
                placeholder="207,34,46 / #cf222e / red"
                @mousedown.stop
                @click.stop="focusRgbInput"
                @focus="emit('rgb-focus')"
                @keydown.enter.prevent="emit('rgb-apply', rgbText)"
                @keydown.esc.prevent="emit('rgb-cancel')"
                @keydown.up.prevent="emit('rgb-nav', -1)"
                @keydown.down.prevent="emit('rgb-nav', 1)"
              />
            </span>
          </span>
        </div>
        <div
          v-else
          class="md-slash-item"
          :class="{ 'md-slash-item-active': i === index }"
          @mousedown.prevent="emit('pick', item)"
          @mouseenter="emit('hover', i)"
        >
          <span class="md-slash-icon" :style="{ color: item.iconColor }" v-html="item.icon"></span>
          <span>{{ item.label }}</span>
        </div>
      </template>
    </template>
    <div v-else class="md-slash-empty">无匹配语法</div>
  </div>
</template>
