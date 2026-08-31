<script setup lang="ts"></script>

<template>
  <div class="cc-banner" role="note">
    <span class="cc-banner-tag">Early docs</span>
    <span class="cc-banner-text">
      These pages are very early. Structure and tone are still being worked out,
      so anything here may change drastically.
    </span>
  </div>
</template>

<style scoped>
.cc-banner {
  position: fixed;
  top: 0;
  right: 0;
  left: 0;
  z-index: var(--vp-z-index-layout-top);

  display: flex;
  align-items: center;
  justify-content: center;
  gap: 10px;

  box-sizing: border-box;
  height: var(--vp-layout-top-height);
  padding: 0 var(--spacing-xl);

  background: var(--cc-banner-bg);
  color: var(--cc-banner-fg);
  font-size: var(--font-size-sm);
  font-weight: var(--font-weight-medium);
  font-variation-settings: "wght" var(--font-weight-medium);
  line-height: 1.4;
  text-align: center;
}

.cc-banner-tag {
  flex: none;
  padding: 2px 8px;
  border-radius: var(--radius-sm);
  background: rgb(0 0 0 / 0.18);
  font-size: var(--font-size-2xs);
  font-weight: 700;
  font-variation-settings: "wght" 700;
  letter-spacing: 0.04em;
  text-transform: uppercase;
}

@media (max-width: 719px) {
  .cc-banner {
    flex-direction: column;
    justify-content: center;
    gap: var(--spacing-xs);
    padding: 0 var(--spacing-lg);
    font-size: var(--font-size-xs);
  }
}
</style>
