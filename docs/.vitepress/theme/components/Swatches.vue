<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from "vue";

/**
 * A row of colour chips rendered from live theme tokens.
 *
 * Pass `value` as a token (`var(--accent)`) rather than a hex and the chip
 * follows the theme — including the light/dark switch — because the value it
 * prints is read back off the element with `getComputedStyle`. The brand page
 * therefore cannot quote a colour the product no longer uses.
 */
type Swatch = {
  /** Label under the chip. */
  name: string;
  /** Any CSS colour. Prefer a token so the docs track the library. */
  value: string;
  /** Optional second line, for a note the colour itself cannot say. */
  note?: string;
};

const props = defineProps<{ items: Swatch[] }>();

const chips = ref<HTMLElement[]>([]);
const resolved = ref<string[]>([]);
let observer: MutationObserver | undefined;

function toHex(color: string): string {
  const match = color.match(/\d+(\.\d+)?/g);
  if (!match || match.length < 3) {
    return color;
  }
  const [r, g, b] = match.slice(0, 3).map((n) => Math.round(Number(n)));
  return (
    "#" +
    [r, g, b]
      .map((n) => n.toString(16).padStart(2, "0"))
      .join("")
      .toUpperCase()
  );
}

function read() {
  resolved.value = chips.value.map((el) =>
    el ? toHex(getComputedStyle(el).backgroundColor) : "",
  );
}

onMounted(() => {
  read();
  // The theme toggle swaps a class on <html>; re-read so the printed hex never
  // disagrees with the chip beside it.
  observer = new MutationObserver(read);
  observer.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ["class"],
  });
});

onBeforeUnmount(() => observer?.disconnect());
</script>

<template>
  <ul class="cc-swatches">
    <li v-for="(item, i) in props.items" :key="item.name" class="cc-swatch">
      <span
        class="cc-swatch-chip"
        :ref="(el) => (chips[i] = el as HTMLElement)"
        :style="{ background: item.value }"
      />
      <span class="cc-swatch-name">{{ item.name }}</span>
      <span class="cc-swatch-value">{{ resolved[i] ?? "" }}</span>
      <span v-if="item.note" class="cc-swatch-note">{{ item.note }}</span>
    </li>
  </ul>
</template>

<style scoped>
.cc-swatches {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(112px, 1fr));
  gap: var(--spacing-md);
  margin: var(--spacing-xl) 0;
  padding: 0;
  list-style: none;
}

.cc-swatch {
  display: flex;
  flex-direction: column;
  gap: 2px;
  margin: 0;
  padding: var(--spacing-sm);
  border-radius: var(--radius-lg);
  background: var(--color-bg-primary);
}

.cc-swatch-chip {
  height: 52px;
  margin-bottom: var(--spacing-xs);
  border-radius: var(--radius-md);
  /* The one place a hairline earns its keep: without it a near-white swatch
     is invisible against the card. */
  box-shadow: inset 0 0 0 1px
    color-mix(in oklab, var(--color-text-primary) 12%, transparent);
}

.cc-swatch-name {
  color: var(--color-text-primary);
  font-size: var(--font-size-sm);
  font-weight: var(--font-weight-medium);
  font-variation-settings: "wght" var(--font-weight-medium);
}

.cc-swatch-value {
  min-height: 1.4em;
  color: var(--color-text-tertiary);
  font-family: var(--font-family-mono);
  font-size: var(--font-size-2xs);
}

.cc-swatch-note {
  color: var(--color-text-tertiary);
  font-size: var(--font-size-2xs);
}
</style>
