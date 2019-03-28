import $ from "jquery";
import "bootstrap";
import "bootstrap/dist/css/bootstrap.min.css";
import "./styles.css";
import base64js from "base64-js";
import { sha256 } from "js-sha256";
import { AudioPlayer, formatTime } from "./player.js";
import showdown from "showdown";
import { debug } from "./debug.js";

$(function () {
    const RECENT_QUERY = "__RECENT__";
    let baseUrl;
    if (AUDIOSERVE_DEVELOPMENT) {
        baseUrl = `${window.location.protocol}//${window.location.hostname}:3000`;
    } else {
        baseUrl = `${window.location.protocol}//${window.location.host}${window.location.pathname.length > 1 ? window.location.pathname : ""}`;
    }


    let collectionUrl = baseUrl;
    let collections = [];
    let pendingCall = null;
    let pendingSpinner = null;
    let transcodingLimit = 58;
    let transcoding = "m";
    let ordering ="a";
    let transcodingLimits = { l: 38, m: 56, h: 76 };

    function isRecent(q) {
        return q == RECENT_QUERY;
    }

    function showSpinner() {
        pendingSpinner = window.setTimeout(() =>
            $("#splash").show(), 1000);
    }

    function hideSpinner() {
        if (pendingSpinner) {
            window.clearTimeout(pendingSpinner);
            pendingSpinner = null;
        }
        $("#splash").hide();
    }

    $(document).ajaxStart(showSpinner);
    $(document).ajaxStop(hideSpinner);

    function ajax(params) {
        params.xhrFields = {
            withCredentials: true
        };
        if (pendingCall) {
            pendingCall.abort();
        }
        let res = $.ajax(params);
        pendingCall = res;
        res.always(() => {
            pendingCall = null;
        });
        return res;
    }

    function loadCollections() {
        return ajax({ url: baseUrl + "/collections" })
            .then(data => {
                debug("Collections", data);
                collections = data.names;
                console.assert(data.names.length > 0);
                console.assert(collections.length == data.count, "Invalid collections response - count does not fit");
                let cselect = $("#collections select").empty();
                for (let i = 0; i < data.names.length; i++) {
                    $("<option>").attr("value", i).text(data.names[i]).appendTo(cselect);
                }
                if (data.names.length > 1) {
                    $("#collections").show();
                    let storedIndex = parseInt(window.localStorage.getItem("audioserve_collection") || 0);
                    let collIndex = storedIndex < data.names.length ? storedIndex : 0;
                    cselect.val(collIndex);
                    setCollection(collIndex);
                    window.localStorage.setItem("audioserve_collection", collIndex);

                } else {
                    $("#collections").hide();
                    setCollection(0);
                    window.localStorage.removeItem("audioserve_collection");

                }
                // hide folder download is disabled
                if (!data.folder_download) {
                    $("#folder-download-link").hide();
                }
            })
            .then(() => {
                return ajax({ url: baseUrl + "/transcodings" })
                    .then((t) => {
                        let c = 1.2;
                        $("#bitrate-trans-low").text( t.low.bitrate + "kbps");
                        transcodingLimits.l = t.low.bitrate * c;
                        $("#bitrate-trans-medium").text( t.medium.bitrate + "kbps");
                        transcodingLimits.m = t.medium.bitrate * c;
                        $("#bitrate-trans-high").text( t.high.bitrate + "kbps");
                        transcodingLimits.h = t.high.bitrate * c;

                        if (window.localStorage.getItem("audioserver_transcoding")) {
                            setTranscoding(window.localStorage.getItem("audioserver_transcoding"));
                        }
                        transSelect.filter(`[value="${transcoding}"]`).prop('checked', true);
                    });
            })
            .catch(err => {
                if (err.status == 401) {
                    $("#login-dialog").modal();
                    throw new Error("Unauthorised");
                } else {
                    console.log("Cannot load collections", err);
                    alert("Server error when loading collections");
                    throw new Error("Server Error");
                }
            });
    }

    function scrollMain(to) {
        $("#main").scrollTop(to || 0);
    }

    function forcedTranscode(file) {
        //FF does not support matroska
        if(navigator.userAgent.indexOf("Firefox") != -1 && file.mime.indexOf("matroska")>0) return true;

        return false;
    }

    function calcTranscoding(file) {
        let mustTranscode = forcedTranscode(file);
        let bitrate = file.meta?parseInt(file.meta.bitrate):-1;

        if (mustTranscode || bitrate >= 0 && transcodingLimit >= 0 && bitrate > transcodingLimit) {
            file.trans = true;
            file.path = file.path + `?trans=${mustTranscode && 
                (! transcoding ||transcoding=='0')?'h': transcoding}`;
        } else {
            file.trans = false;
            file.path = file.path + `?trans=0`;
        }
    }

    function loadFolder(path, fromHistory, scrollTo) {
        $("#info-container").hide();
        ajax({
            url: collectionUrl + "/folder/" + path + (ordering != "a"?`?ord=${ordering}`:""),
        }
        )
            .fail(err => {
                console.log("Server error", err);
                if (err.status == 404 && path.length) {
                    loadFolder("");
                } else if (err.status == 401) {
                    $("#login-dialog").modal();
                } else {
                    alert("Cannot contact server");
                }
            })
            .then(data => {
                $("#search-form input").val("");
                if (data.cover) {
                    $("#info-cover").show().attr('src', collectionUrl + "/cover/" + data.cover.path);
                    $("#info-container").show();
                } else {
                    $("#info-cover").hide();
                }

                $("#info-desc").empty();
                if (data.description) {
                    $.ajax({
                        url: collectionUrl + "/desc/" + data.description.path,
                        xhrFields: {
                            withCredentials: true
                        }
                    })
                        .then((text, status, response) => {
                            let mime = response.getResponseHeader("Content-Type");
                            if (mime == "text/html") {
                                $("#info-desc").html(text);
                            } else if (mime == "text/x-markdown") {
                                let converter = new showdown.Converter();
                                $("#info-desc").html(converter.makeHtml(text));
                            } else if (mime == "text/plain") {
                                let lines = text.split(/\r?\n/);
                                for (let line of lines) {
                                    let para = $("<p>").text(line);
                                    $("#info-desc").append(para);
                                }
                            } else {
                                $("#info-desc").text(text);
                            }
                            $("#info-container").show();
                        })
                        .catch((e) => console.log("Cannot load description", e));
                }

                let subfolders = $('#subfolders');
                let count = $('#subfolders-count');
                subfolders.empty();
                count.text(data.subfolders.length);
                for (let subfolder of data.subfolders) {
                    let item = $('<a class="list-group-item list-group-item-action">')
                        .attr("href", subfolder.path)
                        .text(subfolder.name);
                    subfolders.append(item);
                }
                if (data.subfolders.length) {
                    $("#subfolders-container").show();
                } else {
                    $("#subfolders-container").hide();
                }
                let files = $("#files");
                let fcount = $("#files-count");
                files.empty();
                fcount.text(data.files.length);
                for (let file of data.files) {
                    calcTranscoding(file);
                    let item = $('<a class="list-group-item list-group-item-action">')
                        .attr("href", file.path)
                        .data("duration", file.meta?file.meta.duration:0)
                        .data("transcoded", file.trans)
                        .text(file.name);

                    files.append(item);
                    if (file.meta && file.meta.duration) {
                        item.append(" ");
                        item.append($(`<span class="duration">(${formatTime(file.meta.duration)})</span>`));
                    }
                    if (file.meta && file.meta.bitrate) {
                        item.append(" ");
                        item.append($(`<span class="bitrate">${file.meta.bitrate} kbps</span>`));
                    }
                    if (file.trans) {
                        item.append($("<span>").addClass("transcoded"));
                    }
                }
                if (data.files.length) {
                    $("#files-container").show();
                    $("#folder-download-link").attr("href", collectionUrl + "/download/" + path);
                } else {
                    $("#files-container").hide();
                }

                $(".collapse").collapse('show');

                updateBreadcrumb(path);
                let prevFolder = window.localStorage.getItem("audioserve_folder");
                window.localStorage.setItem("audioserve_folder", path);
                if (!fromHistory) {
                    window.history.pushState({
                        "audioserve_folder": path,
                        "audioserve_collection": currentCollection()
                    },
                        `Audioserve - folder ${path}`);
                }

                scrollMain(scrollTo);

                if (prevFolder !== path) {
                    clearPlayer();
                }
                let lastFile = window.localStorage.getItem("audioserve_file");
                if (lastFile) {
                    let target = $(`#files a[href="${lastFile}"]`);
                    if (target.length) {
                        let time = window.localStorage.getItem("audioserve_time");
                        showInView(target);
                        playFile(target, true, time);
                    }
                }
            });
    }

    function dirname(p) {
        let m = p.match(/(.*)\//);
        return m?m[1]:m;

    }

    function search(query, fromHistory, scrollTo) {
        ajax(isRecent(query)?
            {
                url: collectionUrl +"/recent",
                type: "GET"
            }
            :{
                url: collectionUrl + "/search" + (ordering != "a"?`?ord=${ordering}`:""),
                type: "GET",
                data: { q: query }
            }
        )
            .fail(err => {
                console.log("Search error", err);
                if (err.status == 401) {
                    $("#login-dialog").modal();
                } else {
                    alert("Server error");
                }
            })
            .then(data => {
                $("#info-container").hide();
                let subfolders = $('#subfolders');
                let count = $('#subfolders-count');
                subfolders.empty();
                count.text(data.subfolders.length);
                for (let subfolder of data.subfolders) {
                    let item = $('<a class="list-group-item list-group-item-action">')
                        .attr("href", subfolder.path);
                        
                    let title = $("<span>").addClass("title").text(subfolder.name);
                    item.append(title);

                    let dir = dirname(subfolder.path);
                    if (dir) {
                    let path = $("<span>").addClass("subtitle").text(dir);
                    item.append(path);
                    }
                    subfolders.append(item);
                }
                if (data.subfolders.length) {
                    $("#subfolders-container").show();
                } else {
                    $("#subfolders-container").hide();
                }
                let files = $("#files");
                let fcount = $("#files-count");
                files.empty();
                fcount.text("");
                files.empty();
                $("#files-container").hide();
                updateBreadcrumbSearch(query);
                scrollMain(scrollTo);
                clearPlayer();
                if (!fromHistory) {
                    window.history.pushState({
                        "audioserve_search": query,
                        "audioserve_collection": currentCollection()
                    }, `Audioserve - search ${query}`);
                }
            });
    }

    function updateBreadcrumb(path) {
        let bc = $("#breadcrumb");
        let segments = path.split("/");
        bc.empty();
        bc.append($('<li class="breadcrumb-item"><a href="">Home</a></li>'));
        for (let i = 0; i < segments.length; i++) {
            let item = $('<li class="breadcrumb-item">');
            if (i == segments.length - 1) {
                item.addClass("active");
                item.text(segments[i]);
            } else {
                let partPath = segments.slice(0, i + 1).join('/');
                item.append($(`<a href="${partPath}">${segments[i]}</a></li>`));
            }
            bc.append(item);
        }

    }

    function updateBreadcrumbSearch(query) {
        let bc = $("#breadcrumb");
        let recent = isRecent(query);
        let name = recent?"Recent":"Search";
        bc.empty();
        bc.append($('<li class="breadcrumb-item"><a href="">Home</a></li>'));
        bc.append($(`<li class="breadcrumb-item">${name}</li>`));
        if (!recent) {
            let item = $('<li class="breadcrumb-item"></li>').text(query);
            bc.append(item);
        }
    }

    let player = new AudioPlayer();

    function playFile(target, paused, startTime) {

        $("#files a").removeClass("active");
        target.addClass("active");
        let path = target.attr("href");
        window.localStorage.setItem("audioserve_file", path);
        let fullUrl = collectionUrl + "/audio/" + path;
        player.setUrl(fullUrl, {
            duration: target.data("duration"),
            transcoded: target.data("transcoded")
        });
        player.src = fullUrl;
        if (startTime) {
            player.jumpToTime(startTime);
        }
        if (!paused) {
            let res = player.play();
            if (res.catch) {
                res.catch(e => console.log("Play failed", e));
            }
        }
    }

    function clearPlayer() {
        window.localStorage.removeItem("audioserve_file");
        window.localStorage.removeItem("audioserve_time");

        player.pause();
        player.setUrl("");
        $("#files a").removeClass("active");
    }

    function showInView(nextTarget) {
        try {
            nextTarget.get(0).scrollIntoView({
                block: "center",
                inline: "nearest",
                behaviour: "smooth"
            });
        } catch (e) {
            nextTarget.get(0).scrollIntoView();
        }
    }

    $("#subfolders").on("click", "a.list-group-item-action", evt => {
        let target = $(evt.target).closest("a");
        loadFolder(target.attr("href"));
        evt.preventDefault();
    });

    $("#breadcrumb").on("click", "li.breadcrumb-item a", evt => {
        loadFolder($(evt.target).attr("href"));
        evt.preventDefault();
    });

    $("#files").on("click", "a.list-group-item-action", evt => {
        let target = $(evt.target).closest("a");
        debug("Click to play:", target);
        playFile(target);
        evt.preventDefault();
    });

    $("#player .audio-player").on("ended", evt => {
        let nextTarget = $("#files a.active + a");
        if (nextTarget.length) {
            showInView(nextTarget);
            playFile(nextTarget);
        } else {
            clearPlayer();
            debug("Playback of folder finished");
        }
    });

    $("#player .audio-player").on("timeupdate", evt => {
        window.localStorage.setItem("audioserve_time", evt.detail.currentTime);
    });

    function login(secret) {
        let secretBytes = new (TextEncoder)("utf-8").encode(secret);
        let randomBytes = new Uint8Array(32);
        window.crypto.getRandomValues(randomBytes);
        let concatedBytes = new Uint8Array(secretBytes.length + randomBytes.length);
        concatedBytes.set(secretBytes);
        concatedBytes.set(randomBytes, secretBytes.length);
        let digestPromise;
        if (!window.crypto.subtle) {
            digestPromise = Promise.resolve(sha256.arrayBuffer(concatedBytes));
        } else {
            digestPromise = window.crypto.subtle.digest('SHA-256', concatedBytes);
        }
        return digestPromise
            .then(s => {
                let secret = base64js.fromByteArray(randomBytes) + "|" + base64js.fromByteArray(new Uint8Array(s));
                return ajax({
                    url: baseUrl + "/authenticate",
                    type: "POST",
                    data: { secret: secret }

                });
            });
    }

    $("#login-form").on("submit", evt => {
        evt.preventDefault();
        let secret = $("#secret-input").val();
        login(secret)
            .then(data => {
                loadCollections().then(() => {
                    loadFolder(window.localStorage.getItem("audioserve_folder") || "");
                    $("#login-dialog").modal("hide");
                });
            })
            .catch(err => console.log("Login failed", err));

    });


    function setCollection(collIndex) {
        collIndex = parseInt(collIndex);
        if (collIndex > 0) {
            collectionUrl = baseUrl + "/" + collIndex;
        } else {
            collectionUrl = baseUrl;
        }

    }

    function currentCollection() {
        return $("#collections select").val();
    }

    $("#search-form").on("submit", evt => {
        let query = $("#search-input").blur().val();
        evt.preventDefault();
        if (query.length) {
            search(query);
        }
    });

    $('#recent-btn').on('click', evt => {
        search(RECENT_QUERY);
    });

    $("#main").on("scroll", (evt) => {
        //console.log(`Scroll ${$("#main").scrollTop()}`, evt.detail);
        if (window.history.state) {
            let s = window.history.state;
            s.audioserve_scroll = $("#main").scrollTop();
            window.history.replaceState(s, "");

        }
    });

    window.onpopstate = evt => {
        if (evt.state) {
            debug(`History state: ${JSON.stringify(evt.state)}`);
            if ("audioserve_collection" in evt.state) {
                let collIndex = parseInt(evt.state.audioserve_collection);
                setCollection(collIndex);
                $("#collections select").val(collIndex);
                window.localStorage.setItem("audioserve_collection", collIndex);
            }
            if ("audioserve_folder" in evt.state) {
                debug("Going back to folder ", evt.state.audioserve_folder);
                loadFolder(evt.state.audioserve_folder, true, evt.state.audioserve_scroll);
            } else if ("audioserve_search" in evt.state) {
                debug("Going back to search ", evt.state.audioserve_search);
                search(evt.state.audioserve_search, true, evt.state.audioserve_scroll);
            }
        }
    };

    $("#collections select").on("change", (evt) => {
        let collIndex = $("#collections select").val();
        setCollection(collIndex);
        window.localStorage.setItem("audioserve_collection", collIndex);
        loadFolder("");
    });

    $("#player .controls .current-time, #player .controls .total-time").on('click', evt => {
        let activeFile = $("#files a.active");
        if (activeFile.length > 0) {
            showInView(activeFile);
        }
    });

    $("#logout-link").on('click', (evt) => {
        document.cookie = 'audioserve_token=; expires=Thu, 01 Jan 1970 00:00:01 GMT;';
        document.location.reload();
    });

    let transSelect = $('input[name="transcoding"]');

    function setTranscoding(val) {
        if (val in transcodingLimits) {
            transcodingLimit = transcodingLimits[val];
            transcoding = val;
        } else {
            transcodingLimit = -1;
            transcoding = "0";
        }
        window.localStorage.setItem("audioserver_transcoding", transcoding);

    }

    transSelect.on("change", (evt) => {
        let val = transSelect.filter(":checked").val();
        setTranscoding(val);
        reloadCurrentFolder(true);

    });

    let orderSelect = $('input[name="ordering"]');
    orderSelect.on("change", (evt) => {
        let val = orderSelect.filter(":checked").val();
        window.localStorage.setItem("audioserver_ordering", val);
        ordering = val;
        reloadCurrentFolder(true);
    });

    // Intial value of ordering
    ordering = window.localStorage.getItem("audioserver_ordering") || "a";
    orderSelect.filter(`[value="${ordering}"]`).prop("checked", true);
    

    $("#folder-download-link").on('click', (evt) => {
        evt.stopPropagation();
    });

    const reloadCurrentFolder = (fromHistory) => loadFolder(window.localStorage.getItem("audioserve_folder") || 
        "", fromHistory);

    loadCollections().then(() => {
        reloadCurrentFolder();
        $("#splash").hide().addClass("transparent");
    });
});