import "./player.css";
import { debug, ifDebug } from "./debug.js";

const JUMP_STEP_SHORT = 10;
const JUMP_STEP_LONG = 60;

export function formatTime(dur) {
    if (!isFinite(dur)) return "?";
    let hours = 0;
    let mins = Math.floor(dur / 60);
    let secs = Math.floor(dur % 60);
    secs = ("0" + secs).slice(-2);
    if (mins >= 60) {
        hours = Math.floor(mins / 60);
        mins = mins - hours * 60;
        mins = ("0" + mins).slice(-2);
    }
    if (hours) {
        return `${hours}:${mins}:${secs}`;
    } else {
        return `${mins}:${secs}`;
    }
}

const SPEED_1 = "rotate(-129.65699,249.48283,324.49355)";
const SPEED_2 = "rotate(-83.345203,249.48283,324.49355)";
const SPEED_3 = "rotate(-44.27031,249.48283,324.49355)";
const SPEED_4 = "";
const SPEED_5 = "rotate(39.786055,249.48283,324.49355)";

const VOLUME_FULL = 'M14.667 0v2.747c3.853 1.146 6.666 4.72 6.666 8.946 0 4.227-2.813 7.787-6.666 8.934v2.76C20 22.173 24 17.4 24 11.693 24 5.987 20 1.213 14.667 0zM18 11.693c0-2.36-1.333-4.386-3.333-5.373v10.707c2-.947 3.333-2.987 3.333-5.334zm-18-4v8h5.333L12 22.36V1.027L5.333 7.693H0z';
const VOLUME_MED = 'M0 7.667v8h5.333L12 22.333V1L5.333 7.667M17.333 11.373C17.333 9.013 16 6.987 14 6v10.707c2-.947 3.333-2.987 3.333-5.334z';
const VOLUME_LOW = 'M0 7.667v8h5.333L12 22.333V1L5.333 7.667';
const PLAY = "M18 12L0 24V0";
const PAUSE = "M0 0h6v24H0zM12 0h6v24h-6z";
const NO_RELOAD_JUMP_BACK = 300;
const NO_RELOAD_JUMP_FWD = 120;
const MEDIA_ERRORS = ["MEDIA_ERR_ABORTED", "MEDIA_ERR_NETWORK", "MEDIA_ERR_DECODE", "MEDIA_ERR_SRC_NOT_SUPPORTED"];

function isHidden(el) {
    var style = window.getComputedStyle(el);
    return (style.display === 'none')
}

function  touchToEvent (touch, type) {
    return {
        target: touch.target,
        clientX: touch.clientX,
        clientY: touch.clientY,
        type: type
    };
};


function getCoefficient(event, dragged) {
    const getRangeBox = () => {
        let rangeBox = event.target;
        if (event.type == 'click' && event.target.classList.contains('pin')) {
            rangeBox = event.target.parentElement.parentElement;
        }
        if (dragged && (event.type == 'mousemove' || event.type == 'mouseup')) {
            rangeBox = dragged.parentElement.parentElement;
        }
        return rangeBox;
    };

    let slider = getRangeBox();
    let rect = slider.getBoundingClientRect();
    let K = 0;
    if (slider.dataset.direction == 'horizontal') {

        let offsetX = event.clientX - slider.offsetLeft;
        let width = slider.clientWidth;
        K = offsetX / width;
        K = K < 0 ? 0 : K > 1 ? 1 : K;

    } else if (slider.dataset.direction == 'vertical') {

        let height = slider.clientHeight;
        let offsetY = event.clientY - rect.top;
        K = 1 - offsetY / height;
        K = K < 0 ? 0 : K > 1 ? 1 : K;

    }
    return K;
}

class ControlButton {
    constructor(rootElem, changeListener) {
        this.rootElem = rootElem;
        this._value = null;
        this._changeListener = changeListener;
        this._currentlyDragged = null;
        let volumeControls = rootElem.querySelector('.volume-controls');
        this._volumeProgress = volumeControls.querySelector('.slider .progress');
        let volumeBtn = rootElem.querySelector('.volume-btn');
        let sliderVolume = rootElem.querySelector(".volume .slider");
        let pinVolume = sliderVolume.querySelector(".pin");
        this._icon = rootElem.querySelector('#speaker');
        this._digits = volumeControls.querySelector(".digits");

        this._digits.addEventListener('click', () => {
            this._doChange(1.0);
        });

        pinVolume.addEventListener('mousedown', (event) => {

            this._currentlyDragged = event.target;
            let handler = this._onChange.bind(this);
            window.addEventListener('mousemove', handler, false);

            window.addEventListener('mouseup', () => {
                this._currentlyDragged = false;
                window.removeEventListener('mousemove', handler, false);
            }, { once: true });
        });

        pinVolume.addEventListener("touchstart", (event) => {
            if (event.changedTouches.length == 1 && event.targetTouches.length == 1) {
                let touch = event.changedTouches[0];
                this._currentlyDragged = touch.target;
                let touchId = touch.identifier;

                let myTouch = (event) => {
                    for (let i = 0; i < event.changedTouches.length; i++) {
                        let t = event.changedTouches.item(i);
                        if (t.identifier === touch.identifier) return t;
                    }
                };

                let handler = (event) => {
                    event.preventDefault();
                    let t = myTouch(event);
                    if (t) {
                        let evt = touchToEvent(t, "mousemove");
                        this._onChange(evt);
                    }
                };
                window.addEventListener("touchmove", handler, {"passive": false});
                window.addEventListener("touchend", (event) => {
                    let t = myTouch(event);
                    if (t) {
                        this._currentlyDragged = false;
                        window.removeEventListener("touchmove", handler);
                    }

                }, { once: true });
            }
        }, { passive: true });

        sliderVolume.addEventListener('click', this._onChange.bind(this));

        volumeBtn.addEventListener('click', () => {
            volumeBtn.classList.toggle('open');
            volumeControls.classList.toggle('hidden');
        }
        );
    }

    _onChange(event) {
        let newValue = getCoefficient(event, this._currentlyDragged);
        let scaledValue = this.scaleValue(newValue);
        this._doChange(scaledValue);
    }

    _doChange(scaledValue) {
        if (scaledValue != this.value) {
            this._value = scaledValue;
            this._changeListener(scaledValue);
        }
    }

    formatDigits(v) {
        return v.toFixed(1);
    }

    scaleValue(v) {
        return v;
    }

    scaleValueInverse(v) {
        return v;
    }

    get value() {
        return this._value
    }

    set value(v) {
        const progress = this.scaleValueInverse(v) * 100;
        this._volumeProgress.style.height = progress + '%';
        this._digits.innerText = this.formatDigits(v);
        this._value = v; 
    }

}

class VolumeButton extends ControlButton {

    updateVolume(volume) {
        this.value =volume;
        if (volume >= 0.5) {
            this._icon.attributes.d.value = VOLUME_FULL;
        } else if (volume < 0.5 && volume > 0.05) {
            this._icon.attributes.d.value = VOLUME_MED;
        } else if (volume <= 0.05) {
            this._icon.attributes.d.value = VOLUME_LOW;
        }
    }

    formatDigits(v) {
        return (100*v).toFixed(0)
    }

    scaleValue(v) {
        return Math.pow(v,2)
    }

    scaleValueInverse(v) {
        return Math.sqrt(v)
    }

}

class SpeedButton extends ControlButton {

    updateSpeed(speed) {
        this.value = speed;
        const spd = (s) => {
            this._icon.attributes.transform.value = s;
        };
        if (speed < 0.5) {
            spd(SPEED_1);
        }
        else if (speed >= 0.5 && speed < 0.9) {
            spd(SPEED_2);
        } else if ( speed >= 0.9 && speed < 1.2) {
            spd(SPEED_3);
        } else if ( speed >= 1.2 && speed < 2.1) {
            spd(SPEED_4);
        } else {
            spd(SPEED_5);
        }
    }

    scaleValue(v) {
        return Math.round(10 * (v * 2.7 + 0.3))/ 10;
    }

    scaleValueInverse(v) {
        return (v - 0.3) / 2.7;
    }

}

export class AudioPlayer {
    // Most of code copied from https://codepen.io/gregh/pen/NdVvbm

    constructor() {

        this.transcoded = false; // true if stream does not have size - e.g. is chunked encoded
        this.knownDuration = null; // duration of media provided in external metadata - sent in options to this player
        this._timeOffset = 0; // offset of current steam in case time seeking was used
        this._sizedContent = false;  // If clip size is known - e.g. HTTP response has content-length 

        let audioPlayer = document.querySelector('.audio-player');
        this._rootElem = audioPlayer;
        this._player = audioPlayer.querySelector('audio');

        this._playPause = audioPlayer.querySelector('#playPause');
        this._playpauseBtn = audioPlayer.querySelector('.play-pause-btn');
        this._loading = audioPlayer.querySelector('.loading');
        this._progress = audioPlayer.querySelector('.progress');
        this._volumeBtn = new VolumeButton(audioPlayer.querySelector("#volume-btn"), (volume) => {
            this._player.volume = volume;
            localStorage.setItem('audioserve_volume', volume);
        });
        let vol = parseFloat(localStorage.getItem('audioserve_volume')) || 1.0;
        this._volumeBtn.updateVolume(vol);
        this._player.volume = vol;
        this._speedBtn = new SpeedButton(audioPlayer.querySelector("#speed-btn"), (speed) => {
            this._player.playbackRate = speed;
            localStorage.setItem('audioserve_speed', speed);
        });
        this._speedBtn.updateSpeed(parseFloat(localStorage.getItem('audioserve_speed')) || 1.0);
    
        
        this._currentTime = audioPlayer.querySelector('.current-time');
        this._totalTime = audioPlayer.querySelector('.total-time');
        this._cacheIndicator = audioPlayer.querySelector('.player-cache');
        this._currentlyDragged = null;
        this._isChrome = !!window.chrome; // Chrome requires some tweaks
        this._rewind_step = JUMP_STEP_SHORT;

        let sliderTime = audioPlayer.querySelector(".controls .slider");
        let pinTime = sliderTime.querySelector(".pin");

        pinTime.addEventListener('mousedown', (event) => {

            this._currentlyDragged = event.target;
            let handler = this._onMoveSlider.bind(this);
            window.addEventListener('mousemove', handler, false);
            window.addEventListener('mouseup', (evt) => {
                window.setTimeout(() => this._currentlyDragged = false, 200);
                this._onMoveSlider(evt, true);
                window.removeEventListener('mousemove', handler, false);
                evt.stopImmediatePropagation();
            }, { once: true });
        });

        window.addEventListener("touchcancel", () => {
            debug("touch canceled");
        });

        pinTime.addEventListener("touchstart", (event) => {
            if (event.changedTouches.length == 1 && event.targetTouches.length == 1) {
                let touch = event.changedTouches[0];
                this._currentlyDragged = touch.target;
                let touchId = touch.identifier;
                let clientX, clientY;

                let myTouch = (event) => {
                    for (let i = 0; i < event.changedTouches.length; i++) {
                        let t = event.changedTouches.item(i);
                        if (t.identifier === touch.identifier) return t;
                    }
                };

                let handler = (event) => {
                    let t = myTouch(event);
                    if (t) {
                        let evt = touchToEvent(t, "mousemove");
                        clientX = evt.clientX;
                        clientY = evt.clientY;
                        this._onMoveSlider(evt);
                    }
                };
                window.addEventListener("touchmove", handler);
                window.addEventListener("touchend", (event) => {
                    let t = myTouch(event);
                    if (t) {
                        window.setTimeout(() => { this._currentlyDragged = false; }, 200);
                        window.removeEventListener("touchmove", handler);
                        let evt = touchToEvent(event, "mouseup");
                        evt.clientX = clientX;
                        evt.clientY = clientY;
                        this._onMoveSlider(evt, true);
                    }

                }, { once: true });
            }
        }, { passive: true });

        
        sliderTime.addEventListener('click', (evt) => {
            if (!this._currentlyDragged && !evt.target.className.includes("cache")) {
                this._onMoveSlider(evt, true);
            }
        });

        this._playpauseBtn.addEventListener('click', this.togglePlay.bind(this));

        this.initPlayer();

        window.addEventListener('keyup', (evt) => {
            if (evt.keyCode == 16) {
                this._rewind_step = JUMP_STEP_SHORT;
            }
        });
        // play and forward and rewind by keys
        window.addEventListener('keydown', (evt) => {
            if (evt.keyCode == 16) {
                this._rewind_step = JUMP_STEP_LONG;
            }
            // disable for certain elements where keys plays a role
            if (!['INPUT', 'SELECT', 'TEXTAREA'].includes(evt.target.tagName) && [32, 37, 39].includes(evt.keyCode)) {
                evt.preventDefault();
                evt.stopPropagation();

                // ArrowRight 39
                if (evt.keyCode == 39) {
                    this._player.currentTime = this._player.currentTime + this._rewind_step;
                }

                // ArrowLeft 37
                if (evt.keyCode == 37) {
                    var time = this._timeOffset + this._player.currentTime - this._rewind_step;
                    time = Math.max(time, 0);
                    this.jumpToTime(time);
                }

                // Space
                if (evt.keyCode == 32) {
                    debug("space press", evt.target.tagName);

                    //only if play_pause is visible
                    if (!isHidden(this._playpauseBtn)) {
                        debug("Click play- pause");
                        this._playpauseBtn.dispatchEvent(new Event('click'));
                    }
                }
                return false;

            }
        });

        window.addEventListener("resize", () => {
            debug("window resized");
            this._updateCacheIndicator();
        });
    }

    initPlayer() {
        ifDebug(() => {
            this._player.addEventListener('abort', (evt) => console.log("Player aborted"));
            this._player.addEventListener('emptied', (evt) => console.log("Player emptied"));
            this._player.addEventListener('stalled', (evt) => console.log("Player stalled"));
            this._player.addEventListener('suspend', (evt) => console.log("Player suspend"));
        });

        this._player.addEventListener('error', (evt) => {
            let codeName = (code) => {
                if (code > 0 && code <= MEDIA_ERRORS.length)
                    return MEDIA_ERRORS[code - 1];
                else
                    return `UNKNOWN_${code}`;
            };
            let e = this._player.error;
            let msg = `Player errror: ${codeName(e.code)} : ${e.message}`;
            console.log(msg);
            alert("Player error - check connection");
        });

        this._player.addEventListener('timeupdate', this._updateProgress.bind(this));
        this._player.addEventListener('volumechange', this._updateVolume.bind(this));
        this._player.addEventListener('ratechange', this._updateSpeed.bind(this));
        this._player.addEventListener('durationchange', this._updateTotal.bind(this));
        this._player.addEventListener('loadedmetadata', (evt) => {
            if (isFinite(this._player.duration) && this._player.duration > 0) {
                debug(`Known duration ${this._player.duration}`);
                this._sizedContent = true;
                this._updateTotal();
            } else {
                this._sizedContent = false;
            }

        });
        this._player.addEventListener('canplay', () => {
            this._showPlay();
        });
        this._player.addEventListener('ended', () => {
            this._displayPlay();
            let event = new Event("ended");
            this._rootElem.dispatchEvent(event);
            debug("Track ended");
        });
        this._player.addEventListener('pause', (evt) => this._displayPlay());
        this._player.addEventListener('playing', (evt) => {
            this._displayPause();
            // set stored playback speed
            this._player.playbackRate = parseFloat(localStorage.getItem('audioserve_speed')) || 1.0;
        });

        let state = this._player.readyState;
        if (state > 1) this._updateTotal();
        if (state > 2) this._showPlay();
    }

    _updateTotal() {
        this._totalTime.textContent = formatTime(this.getTotalTime());
    }

    get _cacheRanges() {
        if (this._isChrome) {
            // in chrome seekable is whole range of media even for chunked streams
            // which is not good for transcoded audio - as seek their means reloading from 0 !
            // so here is better to look for .buffered, which basically says what can be seeked
            // without reloading media
            return this._player.buffered;
        } else {
            // in FF for chunked streams is what is in buffers
            return this._player.seekable;
        }
    }

    _updateCacheIndicator() {
        let ranges = this._cacheRanges;
        let totalTime = this.getTotalTime();
        let totalLength = this._cacheIndicator.offsetWidth;
        let offset = totalLength * this._timeOffset / totalTime;
        let indicator = this._cacheIndicator;
        while (indicator.firstChild) {
            indicator.removeChild(indicator.firstChild);
        }

        for (let i = 0; i < ranges.length; i++) {
            let start = ranges.start(i);
            let end = ranges.end(i);
            start = offset + totalLength * start / totalTime;
            end = offset + totalLength * end / totalTime;
            end = Math.min(totalLength, end);

            let bar = document.createElement("div");
            bar.setAttribute("class", "cache-bar");
            bar.style.left = `${start}px`;
            bar.style.width = `${end - start}px`;
            indicator.appendChild(bar);
        }
    }

    _isCached(time) {
        let t = time - this._timeOffset;
        let ranges = this._cacheRanges;
        let remainsToLoad = this.getTotalTime - time;
        for (let i = 0; i < ranges.length; i++) {
            let start = ranges.start(i);
            let end = ranges.end(i);
            //                            V-- 10 secs will be loaded soon - it's like it would be cached
            if (t >= start && t <= end + 10) return { isCached: true, remainsToLoad: 0 };
            let fromEnd = time - end;
            if (fromEnd >= 0 && fromEnd < remainsToLoad) {
                remainsToLoad = fromEnd;
            }
        }
        return { isCached: false, remainsToLoad: remainsToLoad };

    }

    _updateProgress() {
        let event = new CustomEvent('timeupdate', {
            detail: {
                currentTime: this._player.currentTime + this._timeOffset,
                totalTime: this.getTotalTime()
            }
        });
        this._rootElem.dispatchEvent(event);
        if (!this._currentlyDragged) {
            this._updateCacheIndicator();
            let current = this._player.currentTime + this._timeOffset;
            let percent = (current / this.getTotalTime()) * 100;
            if (percent > 100) percent = 100;
            if (isNaN(percent)) percent = 0;
            this._progress.style.width = percent + '%';
            this._currentTime.textContent = formatTime(current);
        }
    }

    _updateVolume() {
        this._volumeBtn.updateVolume(this._player.volume);
    }

    _updateSpeed() {
        this._speedBtn.updateSpeed(this._player.playbackRate);
    }

    getTotalTime() {
        if (this.transcoded && this.knownDuration) {
            return this.knownDuration;
        } else {
            return this._player.duration;
        }
    }

    _onMoveSlider(event, jump = false) {

        let k = getCoefficient(event, this._currentlyDragged);
        let currentTime = this.getTotalTime() * k;
        let percent = k * 100;
        this._progress.style.width = percent + '%';
        this._currentTime.textContent = formatTime(currentTime);
        if (jump) {
            this.jumpToTime(currentTime);
        }
    }

    _showPlay() {
        this._playpauseBtn.style.display = 'block';
        this._displayPlay();
        this._loading.style.display = 'none';
    }

    _hidePlay() {
        this._playpauseBtn.style.display = 'none';
        this._loading.style.display = 'block';
    }

    _jumpWithSeek(time) {
        debug(`Reloading media by server seek with time ${time}`);
        //This is a hack - it's just not good to jump directly to the end 
        if (time >= this.getTotalTime() && time >= 1) time = this.getTotalTime() - 0.51;
        let queryIndex = this._player.src.indexOf("&seek=");
        let baseUrl = queryIndex > 0 ? this._player.src.substr(0, queryIndex) : this._player.src;
        let wasPlaying = !this._player.paused;
        let url = baseUrl + `&seek=${Math.round(time * 1000) / 1000}`;
        this._timeOffset = time;
        this._player.src = url;
        this._player.currentTime = 0;
        if (wasPlaying) {
            this._player.play();
        } else {
            this._updateProgress();
        }
    }

    jumpToTime(time) {
        time = parseFloat(time);
        debug(`Jumping to time ${time}, duration: ${this._player.duration}`);

        let currentTime = this._player.currentTime + this._timeOffset;
        let diff = time - currentTime;

        //do not jump less then 1 sec
        if (Math.abs(diff) > 1 && isFinite(time)) {

            // if streamed without previously known size we need special treatment
            if (this.transcoded) {
                let isCached = this._isCached(time);
                debug("Is cached:", isCached.isCached);
                if (isCached.isCached) {
                    //safe to move player time there
                    this._player.currentTime = time - this._timeOffset;
                } else if (this._isChrome) {

                    //Chrome tweak -  just look at delta if it is smaller then something or jumping before current offset

                    if (diff > NO_RELOAD_JUMP_FWD) {
                        // jump with seek
                        this._jumpWithSeek(time);
                    } else if (diff < 0 && (time - this._timeOffset < 0 || -diff > NO_RELOAD_JUMP_BACK)) {
                        // jump back can work in FF, but Chrome does not seem to cache whole file so 
                        // jumping back only limited
                        this._jumpWithSeek(time);
                    } else {
                        this._player.currentTime = time - this._timeOffset;
                    }
                } else {

                    this._jumpWithSeek(time);
                }

            } else {
                this._player.currentTime = time;
            }
        }
    }

    togglePlay() {
        if (this._player.paused) {
            this.play(this.beforePlay);
        } else {
            this.pause();
        }
    }

    setUrl(url, options) {
        this._timeOffset = 0;
        if (options && "duration" in options) {
            this.knownDuration = options.duration;
        } else {
            this.knownDuration = null;
        }
        if (options && options.transcoded) this.transcoded = true;
        else if (options && options.unsized) this.transcoded = true;
        else if (/\$\$[\d\-]+\$\$/.test(url)) this.transcoded = true;
        else this.transcoded = false;
        if (!url) {
            this._player.removeAttribute("src");
            this._player.load();
            this._updateTotal();
            this._updateProgress();
            this._hidePlay();
            this._loading.style.display = 'none';
        } else {
            this._player.src = url;
            this._hidePlay();
        }
    }

    _displayPause() {
        this._playPause.attributes.d.value = PAUSE;
    }

    _displayPlay() {
        this._playPause.attributes.d.value = PLAY;
    }



    play(beforePlay) {
        return (beforePlay ? beforePlay() : Promise.resolve())
            .then((cont) => {
                if (cont === false) return Promise.resolve();
                return this._player.play()
                    .catch((e) => {
                        this.pause();
                        console.log("Play error", e);
                    }
                    );
            });
    }

    pause() {
        this._player.pause();

    }
}