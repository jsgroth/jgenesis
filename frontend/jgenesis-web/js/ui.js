export function showUi() {
    document.getElementById("loading-text").remove();
    document.getElementById("header-text").classList.remove("hidden");
    document.getElementById("jgenesis").classList.remove("hidden");
    document.getElementById("footer").hidden = false;
}

export function focusCanvas() {
    document.querySelector("canvas").focus();
}

export function showSmsGgConfig() {
    document.getElementById("smsgg-config").hidden = false;

    document.getElementById("genesis-config").hidden = true;
    document.getElementById("snes-config").hidden = true;
}

export function showGenesisConfig() {
    document.getElementById("genesis-config").hidden = false;

    document.getElementById("smsgg-config").hidden = true;
    document.getElementById("snes-config").hidden = true;
}

export function showSnesConfig() {
    document.getElementById("snes-config").hidden = false;

    document.getElementById("smsgg-config").hidden = true;
    document.getElementById("genesis-config").hidden = true;
}

/**
 * @param visible {boolean}
 */
export function setCursorVisible(visible) {
    let canvas = document.querySelector("canvas");
    if (visible) {
        canvas.classList.remove("cursor-hidden");
    } else {
        canvas.classList.add("cursor-hidden");
    }
}

/**
 * @param romTitle {string}
 */
export function setRomTitle(romTitle) {
    document.getElementById("jgenesis-rom-title").innerText = romTitle;
}