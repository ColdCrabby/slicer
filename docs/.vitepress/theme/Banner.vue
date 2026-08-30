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
  padding: 0 24px;

  background: var(--cc-banner-bg);
  color: var(--cc-banner-fg);
  font-size: 13px;
  font-weight: 500;
  line-height: 1.4;
  text-align: center;
}

.cc-banner-tag {
  flex: none;
  padding: 2px 8px;
  border-radius: 10px;
  background: rgb(0 0 0 / 0.18);
  font-size: 11px;
  font-weight: 700;
  letter-spacing: 0.02em;
  text-transform: uppercase;
}

@media (max-width: 719px) {
  .cc-banner {
    flex-direction: column;
    justify-content: center;
    gap: 4px;
    padding: 0 16px;
    font-size: 12px;
  }
}
</style>
