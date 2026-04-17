// Placeholder until M5 fills in the five panels. Just wires the tab bar.
const tabs = document.querySelectorAll("#tabs .tab");
const panels = document.querySelectorAll(".panel");
tabs.forEach((t) => {
  t.addEventListener("click", () => {
    tabs.forEach((x) => x.classList.remove("active"));
    panels.forEach((x) => x.classList.remove("active"));
    t.classList.add("active");
    const target = document.getElementById(`panel-${t.dataset.panel}`);
    if (target) target.classList.add("active");
  });
});
const phase = document.getElementById("phase");
if (phase) phase.textContent = "Phase: awaiting data…";
