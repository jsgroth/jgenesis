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
    document.getElementById("genesis-config").hidden = true;
    document.getElementById("smsgg-config").hidden = false;
}

export function showGenesisConfig() {
    document.getElementById("smsgg-config").hidden = true;
    document.getElementById("genesis-config").hidden = false;
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