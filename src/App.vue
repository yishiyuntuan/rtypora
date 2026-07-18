<script setup>
import { ref } from "vue";
import TitleBar from "./component/TitleBar.vue";
import StatusBar from "./component/StatusBar.vue";
import Sidebar from "./component/Sidebar.vue";
import Editor from "./component/Editor.vue";

const sidebarVisible = ref(true);
const sourceMode = ref(false);
const editorStats = ref({ wordCount: 0, charCount: 0, lineCount: 1, cursorLine: 1, cursorColumn: 1 });
</script>

<template>

  <div class="flex h-screen flex-col">
    <div class="flex flex-1 overflow-hidden">
      <Sidebar v-model:visible="sidebarVisible"/>
      <div class="flex flex-1 flex-col overflow-hidden">
        <TitleBar/>
        <main class="flex-1 overflow-hidden">
          <Editor :source-mode="sourceMode" @update:stats="editorStats = $event"/>
        </main>
        <StatusBar
          :sidebar-visible="sidebarVisible"
          :source-mode="sourceMode"
          :word-count="editorStats.wordCount"
          :char-count="editorStats.charCount"
          :line-count="editorStats.lineCount"
          :cursor-line="editorStats.cursorLine"
          :cursor-column="editorStats.cursorColumn"
          @toggle-sidebar="sidebarVisible = !sidebarVisible"
          @toggle-source="sourceMode = !sourceMode"
        />
      </div>
    </div>
  </div>
</template>
