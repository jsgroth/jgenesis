export function showUi() {
    document.getElementById("loading-text").remove();
    document.getElementById("header-text").classList.remove("hidden");
    document.getElementById("jgenesis").classList.remove("hidden");
    document.getElementById("footer").hidden = false;
}

/**
 * @param fullscreen {boolean}
 */
export function setFullscreen(fullscreen) {
    document.querySelectorAll(".hide-fullscreen").forEach((element) => {
        element.hidden = fullscreen;
    });
}

export function focusCanvas() {
    document.querySelector("canvas").focus();
}

const configIds = ["smsgg-config", "genesis-config", "snes-config", "gba-config"];

/**
 * @param id {string}
 */
function hideAllConfigsExcept(id) {
    for (const configId of configIds) {
        document.getElementById(configId).hidden = configId !== id;
    }

    document.getElementById("supported-files-info").hidden = true;
    document.getElementById("input-config").hidden = false;
}

/**
 * @param inputNames {string[]}
 * @param inputKeys {string[]}
 */
function renderInputs(inputNames, inputKeys) {
    const listNode = document.createElement("ul");

    for (const [i, name] of inputNames.entries()) {
        const key = inputKeys[i];

        const span = document.createElement("span");
        span.innerText = `${name}: `;

        const button = document.createElement("input");
        button.classList.add("input-configure");
        button.setAttribute("name", "input-configure");
        button.setAttribute("type", "button");
        button.setAttribute("value", key);
        button.setAttribute("data-name", name);

        if (window.inputClickListener) {
            button.addEventListener("click", window.inputClickListener);
        }

        const listItem = document.createElement("li");
        listItem.appendChild(span);
        listItem.appendChild(button);
        listNode.appendChild(listItem);
    }

    const controls = document.getElementById("controls");
    controls.innerHTML = "";
    controls.appendChild(listNode);
}

/**
 * @param inputNames {string[]}
 * @param inputKeys {string[]}
 */
export function showSmsGgConfig(inputNames, inputKeys) {
    hideAllConfigsExcept("smsgg-config");
    renderInputs(inputNames, inputKeys);
}

/**
 * @param inputNames {string[]}
 * @param inputKeys {string[]}
 */
export function showGenesisConfig(inputNames, inputKeys) {
    hideAllConfigsExcept("genesis-config");
    renderInputs(inputNames, inputKeys);
}

/**
 * @param inputNames {string[]}
 * @param inputKeys {string[]}
 */
export function showSnesConfig(inputNames, inputKeys) {
    hideAllConfigsExcept("snes-config");
    renderInputs(inputNames, inputKeys);
}

/**
 * @param inputNames {string[]}
 * @param inputKeys {string[]}
 */
export function showGbaConfig(inputNames, inputKeys) {
    hideAllConfigsExcept("gba-config");
    renderInputs(inputNames, inputKeys);
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

/**
 * @param saveUiEnabled {boolean}
 */
export function setSaveUiEnabled(saveUiEnabled) {
    let saveButtons = document.getElementsByClassName("save-button");
    if (saveUiEnabled) {
        for (let i = 0; i < saveButtons.length; i++) {
            saveButtons[i].removeAttribute("disabled");
        }
    } else {
        for (let i = 0; i < saveButtons.length; i++) {
            saveButtons[i].setAttribute("disabled", "");
        }
    }
}

export function beforeInputConfigure() {
    for (const element of document.getElementsByClassName("input-configure")) {
        element.disabled = true;
    }

    document.getElementById("jgenesis-wasm").classList.add("darken");
}

/**
 * @param name {string}
 * @param key {string}
 */
export function afterInputConfigure(name, key) {
    for (const element of document.getElementsByClassName("input-configure")) {
        element.disabled = false;

        if (element.getAttribute("data-name") === name) {
            element.setAttribute("value", key);
        }
    }

    document.getElementById("jgenesis-wasm").classList.remove("darken");
}

/**
 * @param key {string}
 * @return {string | null}
 */
export function localStorageGet(key) {
    return localStorage.getItem(key);
}

/**
 * @param key {string}
 * @param value {string}
 */
export function localStorageSet(key, value) {
    localStorage.setItem(key, value);
}