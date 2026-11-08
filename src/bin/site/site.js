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

if (window.PagefindUI) {
  new window.PagefindUI({
    element: "#search",
    showSubResults: true,
    showImages: false,
    resetStyles: false,
    translations: { placeholder: "Search the docs" },
  });
  document.addEventListener("keydown", (event) => {
    const typing = event.target.closest("input, textarea, [contenteditable]");
    if (event.key === "/" && !typing) {
      event.preventDefault();
      document.querySelector("#search input")?.focus();
    }
  });
} else {
  document.getElementById("search").hidden = true;
}
