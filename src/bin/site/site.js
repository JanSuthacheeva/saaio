"use strict";

const root = document.documentElement;
const darkQuery = window.matchMedia("(prefers-color-scheme: dark)");

function shownTheme() {
  return root.dataset.theme || (darkQuery.matches ? "dark" : "light");
}

document.querySelector(".theme").addEventListener("click", () => {
  const theme = shownTheme() === "dark" ? "light" : "dark";
  root.dataset.theme = theme;
  try {
    localStorage.setItem("theme", theme);
  } catch {
    // Without storage the choice lasts until the page changes.
  }
});

// Touch screens have no keyboard shortcut to show. Runs before Pagefind's module upgrades
// the trigger, since a deferred script runs before a module placed after it.
if (window.matchMedia("(hover: none)").matches) {
  document.querySelector("pagefind-modal-trigger")?.setAttribute("hide-shortcut", "");
}

const menu = document.querySelector(".menu");
const nav = document.getElementById("nav");

function setMenu(open) {
  menu.setAttribute("aria-expanded", String(open));
  nav.classList.toggle("open", open);
}

menu.addEventListener("click", () => setMenu(!nav.classList.contains("open")));
document.addEventListener("keydown", (event) => {
  if (event.key === "Escape") setMenu(false);
});
nav.addEventListener("click", (event) => {
  if (event.target.closest("a")) setMenu(false);
});
