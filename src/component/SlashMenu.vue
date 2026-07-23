<script setup>
import { ref, watch } from 'vue';
import { SLASH_TEXT_BADGES } from '../utils/wysiwyg.js';

// 斜杠命令菜单面板：只负责展示与事件转发。
// 「文本」行（标题级别 + 三种列表 + 行内代码合并徽章）、表格行（行×列数量，
// ↑/↓ 增减、←/→ 切换行列）；样式走 --t-* 主题变量，blocks.slashMenu* 可定制。
const props = defineProps({
  items: { type: Array, default: () => [] },
  index: { type: Number, default: 0 },
  // 文本行当前徽章下标（0-9，←/→ 或悬停徽章调整）
  textIndex: { type: Number, default: 0 },
  // 表格行行列数量与当前调节字段（'rows' | 'cols'）
  tableRows: { type: Number, default: 2 },
  tableCols: { type: Number, default: 2 },
  tableField: { type: String, default: 'rows' },
  left: { type: Number, default: 0 },
  top: { type: Number, default: 0 },
});
const emit = defineEmits(['pick', 'hover', 'text-index', 'table-field']);

const textBadges = SLASH_TEXT_BADGES;

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
        <!-- 文本：一行合并徽章（H1-H6 + 无序/有序/任务列表 + 行内代码） -->
        <div
          v-if="item.id === 'text'"
          class="md-slash-item"
          :class="{ 'md-slash-item-active': i === index }"
          @mousedown.prevent="emit('pick', item)"
          @mouseenter="emit('hover', i)"
        >
          <span class="md-slash-icon" v-html="item.icon"></span>
          <span class="md-slash-levels">
            <template v-for="(b, bi) in textBadges" :key="b.id">
              <!-- 第 7 个徽章前强制换行：H1-H6 一行，列表/任务/行内代码一行 -->
              <span v-if="bi === 6" class="md-slash-row-break"></span>
              <span
                class="md-slash-level"
                :class="{ 'md-slash-level-icon': !!b.icon, 'md-slash-level-active': i === index && bi === textIndex }"
                :title="b.label"
                @mousedown.prevent.stop="emit('pick', item, bi)"
                @mouseenter="emit('text-index', bi)"
              ><span v-if="b.icon" v-html="b.icon"></span><template v-else>{{ b.text }}</template></span>
            </template>
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
          <span class="md-slash-icon" v-html="item.icon"></span>
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
        <div
          v-else
          class="md-slash-item"
          :class="{ 'md-slash-item-active': i === index }"
          @mousedown.prevent="emit('pick', item)"
          @mouseenter="emit('hover', i)"
        >
          <span class="md-slash-icon" v-html="item.icon"></span>
          <span>{{ item.label }}</span>
        </div>
      </template>
    </template>
    <div v-else class="md-slash-empty">无匹配语法</div>
  </div>
</template>
