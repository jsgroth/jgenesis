/**
 * @param key {string}
 * @returns {Promise<Object.<string, Uint8Array>>}
 */
export function loadSaveFiles(key) {
    return new Promise((resolve, reject) => {
        const openReq = indexedDB.open("saves");

        openReq.onerror = () => reject(`Failed to open IndexedDB for read, key ${key}`);

        openReq.onupgradeneeded = (event) => {
            const db = event.target.result;
            db.createObjectStore("files", { keyPath: "key" });
        };

        openReq.onsuccess = (event) => {
            const db = event.target.result;
            const tx = db.transaction(["files"]);

            tx.onerror = () => reject(`IndexedDB transaction error for key ${key}: ${tx.error}`);

            const objectStore = tx.objectStore("files");
            const objectReq = objectStore.get(key);

            objectReq.onerror = () => reject(`Failed to read from IndexedDB for key ${key}`);

            objectReq.onsuccess = (event) => {
                const value = event.target.result;
                if (!value || !value["data"]) {
                    resolve({});
                    return;
                }

                const files = {};
                Object.keys(value["data"]).forEach((extension) => {
                    files[extension] = value["data"][extension];
                });

                resolve(files);
            };
        };
    });
}

/**
 * @param key {string}
 * @param extension {string}
 * @param bytes {Uint8Array}
 * @returns {Promise<void>}
 */
export function writeSaveFile(key, extension, bytes) {
    return new Promise((resolve, reject) => {
        const openReq = indexedDB.open("saves");

        openReq.onerror = () => reject(`Failed to open IndexedDB for write, key ${key} extension ${extension}`);

        openReq.onupgradeneeded = (event) => {
            const db = event.target.result;
            db.createObjectStore("files", { keyPath: "key" });
        };

        openReq.onsuccess = (event) => {
            const db = event.target.result;
            const tx = db.transaction(["files"], "readwrite");

            tx.onerror = () => reject(`IndexedDB transaction error for key ${key} extension ${extension}: ${tx.error}`);

            const objectStore = tx.objectStore("files");

            const getReq = objectStore.get(key);

            getReq.onerror = () => reject(`Failed to read from IndexedDB for key ${key} extension ${extension}`);

            getReq.onsuccess = (event) => {
                let value = event.target.result;
                if (!value || !value["data"]) {
                    value = { key, data: {} };
                }

                value["data"][extension] = bytes;

                const putReq = objectStore.put(value);

                putReq.onerror = () => reject(`Failed to write to IndexedDB for key ${key} extension ${extension}`);

                putReq.onsuccess = () => resolve();
            };
        };
    });
}

/**
 * @param key {string}
 * @returns {Promise<Uint8Array | null>}
 */
export function loadBios(key) {
    return new Promise((resolve, reject) => {
        const openReq = indexedDB.open("bios_roms");

        openReq.onerror = () => reject(`Failed to open IndexedDB for read, key ${key}`);

        openReq.onupgradeneeded = (event) => {
            const db = event.target.result;
            db.createObjectStore("files", { keyPath: "key" });
        };

        openReq.onsuccess = (event) => {
            const db = event.target.result;
            const tx = db.transaction(["files"]);

            tx.onerror = () => reject(`IndexedDB transaction error for key ${key}: ${tx.error}`);

            const objectStore = tx.objectStore("files");
            const getReq = objectStore.get(key);

            getReq.onerror = () => reject(`Failed to read from IndexedDB for key ${key}`);

            getReq.onsuccess = (event) => {
                const value = event.target.result;
                if (value && value["data"]) {
                    resolve(value["data"]);
                } else {
                    resolve(null);
                }
            };
        };
    });
}

/**
 * @param key {string}
 * @param bytes {Uint8Array}
 * @returns {Promise<void>}
 */
export function writeBios(key, bytes) {
    return new Promise((resolve, reject) => {
        const openReq = indexedDB.open("bios_roms");

        openReq.onerror = () => reject(`Failed to open IndexedDB for read, key ${key}`);

        openReq.onupgradeneeded = (event) => {
            const db = event.target.result;
            db.createObjectStore("files", { keyPath: "key" });
        };

        openReq.onsuccess = (event) => {
            const db = event.target.result;
            const tx = db.transaction(["files"], "readwrite");

            tx.onerror = () => reject(`IndexedDB transaction error for key ${key}: ${tx.error}`);

            const objectStore = tx.objectStore("files");
            const putReq = objectStore.put({ key, data: bytes });

            putReq.onerror = () => reject(`Failed to write to IndexedDB for key ${key}`);

            putReq.onsuccess = () => resolve();
        };
    });
}