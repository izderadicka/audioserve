export function debug(...args) {
    if (AUDIOSERVE_DEVELOPMENT) {
        console.log(...args);
    }
}

export function ifDebug(fn, ...args) {
    if (AUDIOSERVE_DEVELOPMENT) {
        fn(...args);
    }
}
