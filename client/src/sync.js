class PlaybackSync {
    constructor() {
        this.socketUrl = "ws://localhost:3001/myws";
        this.hold = false;
    }

    open() {

        const webSocket = new WebSocket(this.socketUrl);
        webSocket.addEventListener("error", err => {
            console.log("WS Error", err);
        });
        webSocket.addEventListener("close", err => {
            console.log("WS Close", err);
            // reopen
            window.setTimeout(() => this.open(), 1000);

        });
        webSocket.addEventListener("open", ev => {
            console.log("WS is ready");
        });
        webSocket.addEventListener("message", evt => {
            console.log("Got message " + evt.data);
            const data = JSON.parse(evt.data);
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
        this.socket.close();
        this.socket = null;
    }

    sendPosition(filePath, position) {
        if (this.active && !this.hold) {
            if (this.socket.filePath && filePath == this.socket.filePath) {
                this.socket.send(`${position}|`);
            } else {
                this.socket.filePath = filePath;
                this.socket.send(`${position}|${filePath}`);
            }
        }

    }

    queryPosition(filePath) {
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
            this.socket.send(filePath?filePath:"?");
            return p;

        } else {
            return Promise.reject(new Error("No connection"));
        }

    }

    get active() {
        return this.socket && this.socket.readyState == WebSocket.OPEN;
    }

    pause() {
        this.hold = true;
    }

    unPause() {
        this.hold = false;
    }

}

export const sync = new PlaybackSync();