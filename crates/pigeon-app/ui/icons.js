// Line icons drawn for Pigeon (24px grid). Simple shapes in each app's colour,
// not the apps' official logos.
"use strict";

const ICONS = {
  whatsapp:
    '<path d="M12 3.6a8.4 8.4 0 0 0-7.3 12.6L3.6 20.4l4.3-1.1A8.4 8.4 0 1 0 12 3.6z"/>' +
    '<path class="fill" d="M9.1 8.5c.2-.4.5-.4.7-.4h.5c.2 0 .4.1.5.4l.6 1.4c.1.2 0 .4-.1.6l-.5.6c.6 1.1 1.5 2 2.6 2.6l.6-.5c.2-.1.4-.2.6-.1l1.4.6c.3.1.4.3.4.5v.5c0 .2 0 .5-.4.7-.5.3-1.3.5-2.1.2-2.1-.7-3.8-2.4-4.5-4.5-.3-.8-.1-1.6.3-2.1z"/>',
  telegram:
    '<path d="M20.6 4.6 3.9 11.2c-.6.3-.6 1.1 0 1.3l4.1 1.4 1.6 4.8c.2.6.9.7 1.3.3l2.3-2.4 4.2 3.1c.5.4 1.2.1 1.3-.5l2.8-12.9c.1-.6-.4-1-.9-.7z"/>' +
    '<path d="M8 13.9 16.6 8l-6.1 7"/>',
  instagram:
    '<rect x="3.5" y="3.5" width="17" height="17" rx="5"/>' +
    '<circle cx="12" cy="12" r="4"/>' +
    '<circle class="fill" cx="16.9" cy="7.1" r="1"/>',
  messenger:
    '<path d="M12 3.3c-4.9 0-8.7 3.6-8.7 8.2 0 2.5 1.1 4.7 3 6.2v3l2.8-1.5c.9.2 1.9.4 2.9.4 4.9 0 8.7-3.6 8.7-8.2S16.9 3.3 12 3.3z"/>' +
    '<path d="M7.6 13.6l3-3.3 2.4 2.2 3.4-3.3"/>',
  discord:
    '<path d="M8.6 5.6c-1.5.3-2.9.8-4.1 1.6-1.6 2.8-2.2 5.7-1.9 8.6 1.4 1 2.8 1.8 4.4 2.3l1-1.7"/>' +
    '<path d="M15.4 5.6c1.5.3 2.9.8 4.1 1.6 1.6 2.8 2.2 5.7 1.9 8.6-1.4 1-2.8 1.8-4.4 2.3l-1-1.7"/>' +
    '<path d="M7.2 16c3.1 1.3 6.5 1.3 9.6 0M8.6 5.6l.6 1.2c1.8-.4 3.8-.4 5.6 0l.6-1.2"/>' +
    '<circle class="fill" cx="9.2" cy="12.2" r="1.3"/><circle class="fill" cx="14.8" cy="12.2" r="1.3"/>',
  email:
    '<rect x="3.5" y="5.5" width="17" height="13" rx="2.5"/>' + '<path d="M4.3 7.2 12 12.8l7.7-5.6"/>',
  x: '<path d="M5 4.5h3.6l10.4 15H15.4z"/>' + '<path d="M18.6 4.5 13 10.9M5.4 19.5l5.6-6.4"/>',
  copy:
    '<rect x="8.5" y="8.5" width="11.5" height="11.5" rx="2.5"/>' +
    '<path d="M15.5 8.5V6.6A2.6 2.6 0 0 0 12.9 4H6.6A2.6 2.6 0 0 0 4 6.6v6.3a2.6 2.6 0 0 0 2.6 2.6h1.9"/>',
  anyone:
    '<circle cx="9" cy="9" r="3"/>' +
    '<path d="M3.8 19c.6-2.9 2.6-4.4 5.2-4.4s4.6 1.5 5.2 4.4"/>' +
    '<circle cx="16.6" cy="8.6" r="2.4"/>' +
    '<path d="M16.4 13.6c2.2 0 3.7 1.3 4.2 3.8"/>',
  add: '<path d="M12 5.5v13M5.5 12h13"/>',
  back: '<path d="M14.5 6 8.5 12l6 6"/>',
  check: '<path d="M5.5 12.6 9.8 17 18.5 7.6"/>',
  alert: '<circle cx="12" cy="12" r="8.5"/><path d="M12 7.8v5M12 16.1v.2"/>',
  spark: '<path d="M12 4v4M12 16v4M4 12h4M16 12h4M6.6 6.6l2.2 2.2M15.2 15.2l2.2 2.2M6.6 17.4l2.2-2.2M15.2 8.8l2.2-2.2"/>',
};

function icon(name) {
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("aria-hidden", "true");
  svg.classList.add("icon");
  // Only ever fed the fixed strings above, never anything from outside.
  svg.innerHTML = ICONS[name] || "";
  return svg;
}
