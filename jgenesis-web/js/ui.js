export function showUi() {
    document.getElementById("loading-text").remove();
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