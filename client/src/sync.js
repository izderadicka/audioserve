import { debug } from "./debug";
import config from"./config";

function mapProtocol(p) {
    if (p == "http:") {
        return "ws:";
    } else if (p== "https:") {
        return "wss:";
    }
}

class PlaybackSync {
    constructor() {
        const baseUrl = AUDIOSERVE_DEVELOPMENT?
            `${mapProtocol(window.location.protocol)}//${window.location.hostname}:${config.DEVELOPMENT_PORT}`:
            `${mapProtocol(window.location.protocol)}//${window.location.host}${window.location.pathname.length > 1 ? window.location.pathname : ""}`;
        
        this.socketUrl = baseUrl+"/position";
        this.closed = false;
        this.filePath = null;
        this.groupPrefix = null;
    }

    open() {
        this.closed = false;
        debug("Opening ws on url "+this.socketUrl);
        const webSocket = new WebSocket(this.socketUrl);
        webSocket.addEventListener("error", err => {
            console.error("WS Error", err);
        });
        webSocket.addEventListener("close", err => {
            this.resetOnClose();
            debug("WS Close", err);
            // reopen
            if (! this.closed) window.setTimeout(() => this.open(), 1000);

        });
        webSocket.addEventListener("open", ev => {
            debug("WS is ready");
        });
        webSocket.addEventListener("message", evt => {
            debug("Got message " + evt.data);
            const data = JSON.parse(evt.data);
            const parseGroup  = (item) => {
                if (item && item.folder) {
                    const [prefix, collection] = /^\w+\/(\d+)\//.exec(item.folder);
                    item.folder = item.folder.substr(prefix.length);
                    item.collection = parseInt(collection);
                }
            };
            parseGroup(data.folder);
            parseGroup(data.last);
            if (this.pendingAnswer) {
                if (this.pendingTimeout) clearInterval(this.pendingTimeout);
                this.pendingTimeout = null;
                this.pendingAnswer(data);
                this.pendingAnswer = null;
                this.pendingQuery = null;
            }
        });

        this.socket = webSocket;
    }

    close() {
        this.closed = true;
        this.socket.close();
        this.resetOnClose();
    }

    resetOnClose() {
        this.socket = null;
        this.filePath = null;
        this.lastSend = null;
    }

    enqueuePosition(filePath, position, force=false) {
        if (this.pendingMessage) window.clearTimeout(this.pendingMessage);
        if (!this.active) return;
        position = Math.round(position*1000)/1000;
        filePath = this.groupPrefix+filePath;
        if (this.filePath && this.lastSend && filePath == this.filePath) {

            if (force || Date.now() - this.lastSend > config.POSITION_REPORTING_PERIOD) {
                this.sendMessage(`${position}|`);
            } else {
                this.pendingMessage = window.setTimeout(() => {
                    this.sendMessage(`${position}|`);
                    this.pendingMessage = null;
                },
                config.POSITION_REPORTING_PERIOD
                );
            }
        } else {
            this.filePath = filePath;
            this.sendMessage(`${position}|${filePath}`);
        }
    }

    sendMessage(msg) {
        if (this.active) {
            this.socket.send(msg);
            this.lastSend = Date.now();
        } else {
            console.error("Cannot send message, socket not ready");
        }

    }

    queryPosition(folderPath) {
        if (this.pendingQuery) {
            if (this.pendingTimeout) clearInterval(this.pendingTimeout);
            pendingQuery(new Error("Canceled by next query"));

        }
        if (this.active) {
            const p = new Promise((resolve, reject) => {
                this.pendingAnswer = resolve;
                this.pendingQuery = reject;
                this.pendingTimeout = setTimeout(() => {
                    reject(new Error("Timeout"));
                }, 3000);
            });
            this.socket.send(folderPath?this.groupPrefix+folderPath:"?");
            return p;

        } else if (this.groupPrefix){
            return Promise.reject(new Error("No websocket connection "));
        } else {
            return Promise.resolve(null);
        }

    }

    get active() {
        return !this.closed && this.socket && this.socket.readyState == WebSocket.OPEN;
    }

}

export const sync = new PlaybackSync();