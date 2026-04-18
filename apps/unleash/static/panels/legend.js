// Legend strip: class chips + link-profile chips. Used under Mesh and Map.
// Clicking a class chip toggles a dim-filter on non-matching dots (hooks
// exposed via `onFilterChange`).

import { CLASSES, LINK_PROFILES } from "/app/panels/classes.js";

export function initLegend(root, { onFilterChange } = {}) {
  root.classList.add("legend-strip");
  root.innerHTML = "";

  const active = new Set(Object.keys(CLASSES));

  const classRow = document.createElement("div");
  classRow.className = "legend-row";
  classRow.innerHTML = "<span class='legend-label'>classes</span>";
  Object.entries(CLASSES).forEach(([key, meta]) => {
    const chip = document.createElement("button");
    chip.className = "legend-chip class-chip active";
    chip.dataset.class = key;
    chip.title = meta.role;
    chip.innerHTML = `<i style="background:${meta.color}"></i>${meta.label}`;
    chip.addEventListener("click", () => {
      if (active.has(key)) {
        active.delete(key);
        chip.classList.remove("active");
      } else {
        active.add(key);
        chip.classList.add("active");
      }
      onFilterChange?.(active);
    });
    classRow.appendChild(chip);
  });

  const linkRow = document.createElement("div");
  linkRow.className = "legend-row";
  linkRow.innerHTML = "<span class='legend-label'>links</span>";
  Object.entries(LINK_PROFILES).forEach(([key, meta]) => {
    const chip = document.createElement("span");
    chip.className = "legend-chip link-chip";
    chip.dataset.profile = key;
    chip.title = meta.description;
    chip.innerHTML = `<i style="background:${meta.color}"></i>${meta.label}`;
    linkRow.appendChild(chip);
  });

  root.append(classRow, linkRow);
  return {
    isActive(cls) {
      return active.has(cls);
    },
  };
}
