import { h } from "vue";
import type { Theme } from "vitepress";
import DefaultTheme from "vitepress/theme";
import Banner from "./Banner.vue";
import Swatches from "./components/Swatches.vue";
// The Cold Crabby design language, bridged onto VitePress's own variables.
// Imported after the default theme so its `:root` block wins on order.
import "./styles/index.scss";
import "./banner.css";

export default {
  extends: DefaultTheme,
  Layout() {
    return h(DefaultTheme.Layout, null, {
      "layout-top": () => h(Banner),
    });
  },
  enhanceApp({ app }) {
    // Usable in any markdown page without an import.
    app.component("Swatches", Swatches);
  },
} satisfies Theme;
