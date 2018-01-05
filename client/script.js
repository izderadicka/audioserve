$(function() {
    const baseUrl =`${window.location.protocol}//${window.location.hostname}:3000`;
    let currentFolder = "";
    function loadFolder(path) {
        $.ajax(baseUrl+"/folder/"+ path)
        .fail( err => console.log("Server error", err))
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
            let files = $("#files");
            let fcount = $("#files-count");
            files.empty();
            fcount.text(data.files.length);
            for (let file of  data.files) {
                let item = $('<a class="list-group-item list-group-item-action">')
                    .attr("href", file.path)
                    .text(file.name)
                files.append(item);
            }
            update_breadcrumb(path);
            currentFolder = path;
            clearPlayer();
            

        });
    }

    function update_breadcrumb(path) {
        bc = $("#breadcrumb");
        let segments = path.split("/");
        bc.empty();
        bc.append($('<li class="breadcrumb-item"><a href="">Home</a></li>'));
        for (let i=0;  i< segments.length; i++) {
            let item = $('<li class="breadcrumb-item">');
            if (i == segments.length-1) {
                item.addClass("active");
            }
            let partPath = segments.slice(0,i+1).join('/');
            item.append($(`<a href="${partPath}">${segments[i]}</a></li>`));
            bc.append(item);
        }

    }

    function playFile(target) {
        $("#files a").removeClass("active");
        target.addClass("active");
        let path = target.attr("href");
        let fullUrl = baseUrl+"/audio/"+path;
        let player = $("#player audio").get()[0];
        player.src= fullUrl;
        player.play();
    }

    function clearPlayer() {
        let player = $("#player audio").get()[0];
        player.pause()
        player.src = "";
        $("#files a").removeClass("active");
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

    $("#player audio").on("ended", evt => {
        let nextTarget = $("#files a.active + a");
        if (nextTarget.length) {
            nextTarget.get(0).scrollIntoView({block: "center", 
                inline: "nearest",
                behaviour: "smooth"
            });
            playFile(nextTarget);
        } else {
            clearPlayer();
            console.log("Playback of folder finished");
        }
    })

    loadFolder(currentFolder);
    
})