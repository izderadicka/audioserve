import $ from "jquery";
import "bootstrap";
import "bootstrap/dist/css/bootstrap.min.css";
import "./styles.css";
import base64js from "base64-js";
import {AudioPlayer, formatTime} from "./player.js";

$(function() {
    const baseUrl =`${window.location.protocol}//${window.location.hostname}:3000`;

    let pendingCall = null;

    function ajax(params) {
        params.xhrFields = {
            withCredentials: true
         };
        if (pendingCall) {
            pendingCall.abort();
        }
        let res = $.ajax(params);
        pendingCall = res;
        res.always( () => {pendingCall=null});
        return res
    }

    function loadFolder(path, fromHistory) {
        ajax({
            url: baseUrl+"/folder/"+ path,
            }
            )
        .fail( err => { 
            console.log("Server error", err);
            if (err.status == 404 && path.length) {
                loadFolder("");
            } else if (err.status == 401) {
                $("#login-dialog").modal();
            }
        })
        .then(data => {
            //console.log(data);
            let subfolders = $('#subfolders');
            let count = $('#subfolders-count');
            subfolders.empty();
            count.text(data.subfolders.length);
            for (let subfolder of  data.subfolders) {
                //console.log(subfolder);
                let item = $('<a class="list-group-item list-group-item-action">')
                    .attr("href", subfolder.path)
                    .text(subfolder.name)
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
            for (let file of  data.files) {
                let item = $('<a class="list-group-item list-group-item-action">')
                    .attr("href", file.path)
                    .data("duration", file.meta.duration)
                    .data("transcoded", file.trans)
                    .text(file.name)
                files.append(item);
                if (file.meta) {
                    item.append($(`<span class="duration">${formatTime(file.meta.duration)}</span>`))
                }
            }
            if (data.files.length) {
                $("#files-container").show();
            } else {
                $("#files-container").hide();
            }

            updateBreadcrumb(path);
            let prevFolder = window.localStorage.getItem("audioserve_folder");
            window.localStorage.setItem("audioserve_folder", path);
            if (! fromHistory) {
                window.history.pushState({"audioserve_folder": path}, `Audioserve - folder ${path}`);
            }

            if (prevFolder !== path) {
                clearPlayer();
                }
            let lastFile = window.localStorage.getItem("audioserve_file");
            if (lastFile) {
                let target=$(`#files a[href="${lastFile}"]`);
                if (target.length) {
                    let time = window.localStorage.getItem("audioserve_time");
                    showInView(target);
                    playFile(target, true, time);
                }
            }
        });
    }

    function search(query, fromHistory) {
        ajax({
            url: baseUrl+"/search",
            type: "GET",
            data: {q: query}
            }
            )
        .fail( err => { 
            console.log("Search error", err);
            if (err.status == 401) {
                $("#login-dialog").modal();
            }
        })
        .then(data => {
            //console.log(data);
            let subfolders = $('#subfolders');
            let count = $('#subfolders-count');
            subfolders.empty();
            count.text(data.subfolders.length);
            for (let subfolder of  data.subfolders) {
                //console.log(subfolder);
                let item = $('<a class="list-group-item list-group-item-action">')
                    .attr("href", subfolder.path)
                    .text(subfolder.name)
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
            clearPlayer(); 
            if (! fromHistory) {
                window.history.pushState({"audioserve_search": query}, `Audioserve - search ${query}`); 
            } 
        });
    }

    function updateBreadcrumb(path) {
        let bc = $("#breadcrumb");
        let segments = path.split("/");
        bc.empty();
        bc.append($('<li class="breadcrumb-item"><a href="">Home</a></li>'));
        for (let i=0;  i< segments.length; i++) {
            let item = $('<li class="breadcrumb-item">');
            if (i == segments.length-1) {
                item.addClass("active");
                item.text(segments[i]);
            } else {
                let partPath = segments.slice(0,i+1).join('/');
                item.append($(`<a href="${partPath}">${segments[i]}</a></li>`));
            }
            bc.append(item);
        }

    }

    function updateBreadcrumbSearch(query) {
        let bc = $("#breadcrumb");
        bc.empty();
        bc.append($('<li class="breadcrumb-item"><a href="">Home</a></li>'));
        bc.append($('<li class="breadcrumb-item">Search</li>'));
        let item = $('<li class="breadcrumb-item"></li>').text(query);
        bc.append(item); 
    }

    let player = new AudioPlayer();

    function playFile(target, paused, startTime) {
       
        $("#files a").removeClass("active");
        target.addClass("active");
        let path = target.attr("href");
        window.localStorage.setItem("audioserve_file", path);
        let fullUrl = baseUrl+"/audio/"+path;
        player.setUrl(fullUrl, {
            duration: target.data("duration"),
            transcoded: target.data("transcoded")
        });
        player.src= fullUrl;
        if (startTime) {
            player.jumpToTime(startTime)
        }
        if (! paused) {
            let res=player.play();
            if (res.catch) {
                res.catch(e => console.log("Play failed", e))
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
            nextTarget.get(0).scrollIntoView({block: "center", 
                inline: "nearest",
                behaviour: "smooth"
            });
            }  catch(e) {
                nextTarget.get(0).scrollIntoView();
            } 
    }

    $("#subfolders").on("click", "a.list-group-item-action", evt => {
        loadFolder($(evt.target).attr("href"));
        evt.preventDefault();
    });

    $("#breadcrumb").on("click", "li.breadcrumb-item a", evt => {
        loadFolder($(evt.target).attr("href"));
        evt.preventDefault();
    });

    $("#files").on("click", "a.list-group-item-action", evt => {
        let target = $(evt.target);
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
            console.log("Playback of folder finished");
        }
    });

    $("#player .audio-player").on("timeupdate", evt => {
        window.localStorage.setItem("audioserve_time", evt.detail.currentTime);
    });

    function login(secret) {
        let  secretBytes = new (TextEncoder || TextEncoderLite)("utf-8").encode(secret); 
        let randomBytes = new Uint8Array(32);
        window.crypto.getRandomValues(randomBytes);
        let concatedBytes = new Uint8Array(secretBytes.length+randomBytes.length);
        concatedBytes.set(secretBytes);
        concatedBytes.set(randomBytes, secretBytes.length);
        return window.crypto.subtle.digest('SHA-256', concatedBytes)
         .then( s => {
            let secret = base64js.fromByteArray(randomBytes)+"|"+base64js.fromByteArray(new Uint8Array(s));
            return ajax({
                url:baseUrl+"/authenticate",
                type: "POST",
                data: {secret: secret}
                
            });
        });
    }

    $("#login-form").on("submit", evt => {
        evt.preventDefault();
        let secret = $("#secret-input").val();
        login(secret)
        .then(data => {
            loadFolder(window.localStorage.getItem("audioserve_folder")|| "");
            $("#login-dialog").modal("hide");
        })
        .catch( err => console.log("Login failed", err));
        
    });

    $("#search-btn").on("click", evt => {
        $("#search-area").toggle();
        $(".app-name").toggle();

        if ($("#search-area").is(':visible')) {
            $("#search-area input").focus();
        }
    })

    function showSearch() {
        if ($(window).width() > 600) {
            $("#search-area").show();
            $(".app-name").show();
            $("#search-btn").hide();
        } else {
            $("#search-area").hide();
            $(".app-name").show();
            $("#search-btn").show();
        }
    }

    $(window).on("resize", showSearch);

    $("#search-form").on("submit", evt => {
        let query = $("#search-input").val();
        evt.preventDefault();
        if (query.length) {
            search(query)
        }
    })

    window.onpopstate = evt => {
        if (evt.state) {
        if ("audioserve_folder" in evt.state) {
            console.log("Going back to folder ", evt.state.audioserve_folder);
            loadFolder(evt.state.audioserve_folder, true);
        } else if ("audioserve_search" in evt.state) {
            console.log("Going back to search ", evt.state.audioserve_search);
            search(evt.state.audioserve_search, true);
        }
    }
    };

    showSearch();
    loadFolder(window.localStorage.getItem("audioserve_folder")|| "");
})