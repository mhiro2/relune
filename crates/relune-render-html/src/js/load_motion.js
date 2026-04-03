"use strict";
(() => {
  // ts/load_motion.ts
  {
    const svg = document.querySelector(".canvas svg");
    if (svg) {
      svg.querySelectorAll(".edge").forEach((edge, index) => {
        edge.style.setProperty("--enter-index", String(index));
      });
      svg.querySelectorAll(".node").forEach((node, index) => {
        node.style.setProperty("--enter-index", String(index + 6));
      });
    }
  }
})();
